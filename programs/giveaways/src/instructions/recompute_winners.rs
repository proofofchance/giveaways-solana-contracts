//! # Recompute Winners Instruction
//!
//! Recomputes winners after disqualifications using the same deterministic algorithm.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, Participant, WinnersLedger},
    utils::crypto::{build_merkle_tree, compute_winner_seed, select_winners, WinnerEntry},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct RecomputeWinners<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = !giveaway.winners_locked @ GiveawayError::WinnersLocked,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        mut,
        constraint = winners_ledger.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    #[account(
        constraint = signer.key() == giveaway.creator || signer.key() == config.authority @ GiveawayError::Unauthorized
    )]
    pub signer: Signer<'info>,
}

pub fn process(ctx: Context<RecomputeWinners>) -> Result<()> {
    let giveaway = &mut ctx.accounts.giveaway;
    let winners_ledger = &mut ctx.accounts.winners_ledger;
    let clock = Clock::get()?;

    require!(giveaway.winners_computed, GiveawayError::WinnersNotComputed);
    require!(
        giveaway.attested_count > 0,
        GiveawayError::NoEligibleParticipants
    );

    if giveaway.has_missing_attested_reveals() {
        return Err(GiveawayError::MissingAttestedParticipants.into());
    }

    if giveaway.attested_reveals_complete() && !giveaway.uploads_complete {
        giveaway.uploads_complete = true;
    }

    // Collect all reveal-included participants. The count check prevents a
    // caller from recomputing with only a subset of uploaded accounts.
    let mut eligible_participants = Vec::new();
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

    // Update winners ledger
    winners_ledger.recompute(
        merkle_root,
        actual_winners_count,
        total_winners_payout,
        per_winner_payout,
        winners_vec.clone(),
        next_recompute_version,
        clock.unix_timestamp,
    );

    // Update giveaway
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

    Ok(())
}
