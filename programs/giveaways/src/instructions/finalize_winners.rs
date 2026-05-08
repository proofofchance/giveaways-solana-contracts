//! # Finalize Winners Instruction
//!
//! Computes winners using uploaded reveals and stores merkle commitment
//! for batch settlement verification.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant, WinnersLedger},
    utils::crypto::{build_merkle_tree, compute_winner_seed, select_winners, WinnerEntry},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct FinalizeWinners<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = !giveaway.winners_locked @ GiveawayError::WinnersLocked,
    )]
    pub giveaway: Account<'info, Giveaway>,

    /// CHECK: Vault account for validation
    #[account(
        constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount
    )]
    pub vault: AccountInfo<'info>,

    #[account(
        init_if_needed,
        payer = authority,
        space = WinnersLedger::calculate_size(giveaway.number_of_winners),
        seeds = [WINNERS_LEDGER_SEED, giveaway.key().as_ref()],
        bump
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    #[account(
        mut,
        constraint = authority.key() == config.authority @ GiveawayError::Unauthorized
    )]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn process(ctx: Context<FinalizeWinners>) -> Result<()> {
    let accounts = ctx.accounts;
    let _config = &accounts.config;
    let giveaway = &mut accounts.giveaway;
    let winners_ledger = &mut accounts.winners_ledger;
    let clock = Clock::get()?;

    // Validate settlement is ready
    require!(
        giveaway.is_ready_for_settlement(clock.unix_timestamp),
        GiveawayError::SettlementNotReady
    );

    require!(
        giveaway.attested_count > 0,
        GiveawayError::NoEligibleParticipants
    );
    require!(
        giveaway.provider_uploaded_count >= giveaway.attested_count && giveaway.uploads_complete,
        GiveawayError::MissingAttestedParticipants
    );

    // Collect all reveal-included participants. The count check prevents a
    // caller from finalizing with only a subset of uploaded accounts.
    let mut eligible_participants: Vec<Pubkey> = Vec::new();
    let mut included_participants_count = 0u32;

    for account_info in ctx.remaining_accounts.iter() {
        let mut participant_data = &account_info.data.borrow()[..];
        if let Ok(participant) = Participant::try_deserialize(&mut participant_data) {
            require_keys_eq!(
                participant.giveaway,
                giveaway.key(),
                GiveawayError::InvalidAccount
            );

            if participant.is_eligible() {
                eligible_participants.push(participant.wallet);
            }
            if participant.reveal_included {
                included_participants_count = included_participants_count
                    .checked_add(1)
                    .ok_or(GiveawayError::MathOverflow)?;
            }
        }
    }

    require_eq!(
        included_participants_count,
        giveaway.provider_uploaded_count,
        GiveawayError::MissingAttestedParticipants
    );

    eligible_participants.sort_by_key(|pubkey| pubkey.to_bytes());

    let seed = compute_winner_seed(
        giveaway.id,
        giveaway.poc_aggregate_hash,
        &eligible_participants,
    );

    if eligible_participants.is_empty() {
        crate::events::GiveawayEvent::NoWinners {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            reason: "no_eligible_participants".to_string(),
            total_participants: giveaway.participants_count,
            total_attested: giveaway.attested_count,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    let winners_vec = select_winners(&seed, &eligible_participants, giveaway.number_of_winners);
    let actual_winners_count = winners_vec.len() as u32;
    let next_recompute_version = giveaway.recompute_version + 1;

    let winners_pool = giveaway.calculate_winners_pool();
    let per_winner_payout = if actual_winners_count > 0 {
        winners_pool / (actual_winners_count as u64)
    } else {
        0
    };
    let total_winners_payout = actual_winners_count as u64 * per_winner_payout;

    let winners: Vec<WinnerEntry> = winners_vec
        .iter()
        .enumerate()
        .map(|(index, recipient)| WinnerEntry {
            index: index as u32,
            recipient: *recipient,
            amount: per_winner_payout,
        })
        .collect();

    let merkle_root = if !winners.is_empty() {
        build_merkle_tree(&winners)?
    } else {
        [0u8; 32]
    };

    // Initialize or update winners ledger
    if winners_ledger.giveaway == Pubkey::default() {
        // First time - initialize
        winners_ledger.initialize(
            giveaway.key(),
            merkle_root,
            actual_winners_count,
            total_winners_payout,
            per_winner_payout,
            winners_vec.clone(),
            next_recompute_version,
            clock.unix_timestamp,
        );
    } else {
        // Recompute
        winners_ledger.recompute(
            merkle_root,
            actual_winners_count,
            total_winners_payout,
            per_winner_payout,
            winners_vec.clone(),
            next_recompute_version,
            clock.unix_timestamp,
        );
    }

    // Update giveaway state
    giveaway.mark_winners_computed();

    // Emit event
    crate::events::GiveawayEvent::WinnersComputed {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        winners_ledger: winners_ledger.key().to_string(),
        merkle_root: hex::encode(merkle_root),
        seed: hex::encode(seed),
        rule_version: GIVEAWAY_RULE_VERSION_V1.to_string(),
        total_eligible: eligible_participants.len() as u32,
        winners_count: actual_winners_count,
        total_payout_lamports: total_winners_payout,
        per_winner_lamports: per_winner_payout,
        recompute_version: giveaway.recompute_version,
        winners: winners_vec
            .iter()
            .map(|winner| winner.to_string())
            .collect(),
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Winners computed for giveaway {}: {} winners, {} lamports each",
        giveaway.id,
        actual_winners_count,
        per_winner_payout
    );

    Ok(())
}
