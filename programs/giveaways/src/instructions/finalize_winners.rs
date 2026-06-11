//! # Finalize Winners Instruction
//!
//! Computes winners using uploaded reveals and stores merkle commitment
//! for batch settlement verification.

use crate::{
    constants::*,
    error::GiveawayError,
    events::GiveawayEvent,
    state::{Config, FinalizationLedger, Giveaway, Participant, WinnersLedger},
    utils::{
        crypto::{
            build_merkle_tree, compute_candidate_rank, compute_finalization_seed, WinnerEntry,
        },
        pda::assert_pda_owned,
    },
};
use anchor_lang::prelude::*;
use std::io::Cursor;

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
        init_if_needed,
        payer = authority,
        space = FinalizationLedger::calculate_size(giveaway.number_of_winners),
        seeds = [FINALIZATION_LEDGER_SEED, giveaway.key().as_ref()],
        bump
    )]
    pub finalization_ledger: Account<'info, FinalizationLedger>,

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
    let finalization_ledger = &mut accounts.finalization_ledger;
    let clock = Clock::get()?;
    let giveaway_key = giveaway.key();

    // Validate settlement is ready
    require!(
        giveaway.is_ready_for_settlement(clock.unix_timestamp),
        GiveawayError::SettlementNotReady
    );

    require!(
        giveaway.attested_count > 0,
        GiveawayError::NoEligibleParticipants
    );

    if giveaway.has_missing_attested_reveals() {
        if clock.unix_timestamp >= giveaway.upload_deadline_unix
            && giveaway.remediation_start_unix == 0
        {
            giveaway.begin_remediation(clock.unix_timestamp);
            GiveawayEvent::RevealRemediationBegan {
                giveaway_id: giveaway.id,
                giveaway: giveaway.key().to_string(),
                included_reveals_count: giveaway.provider_uploaded_count,
                attested_count: giveaway.attested_count,
                remediation_start_unix: giveaway.remediation_start_unix,
                remediation_deadline_unix: giveaway.remediation_deadline_unix,
                timestamp: clock.unix_timestamp,
            }
            .emit();
            return Ok(());
        }

        return Err(GiveawayError::MissingAttestedParticipants.into());
    }

    if giveaway.attested_reveals_complete() && !giveaway.uploads_complete {
        giveaway.uploads_complete = true;
    }

    let next_recompute_version = giveaway
        .recompute_version
        .checked_add(1)
        .ok_or(GiveawayError::MathOverflow)?;

    if finalization_ledger.giveaway == Pubkey::default() {
        finalization_ledger.initialize(
            giveaway_key,
            next_recompute_version,
            giveaway.number_of_winners,
            compute_finalization_seed(giveaway.id, giveaway.poc_aggregate_hash),
            clock.unix_timestamp,
        );
    } else {
        require_keys_eq!(
            finalization_ledger.giveaway,
            giveaway_key,
            GiveawayError::InvalidAccount
        );
        require!(
            finalization_ledger.recompute_version == next_recompute_version,
            GiveawayError::InvalidInstruction
        );
        require!(!finalization_ledger.completed, GiveawayError::WinnersLocked);
    }

    require!(
        !ctx.remaining_accounts.is_empty(),
        GiveawayError::InvalidInstruction
    );

    let mut seen_accounts = std::collections::HashSet::new();
    let mut seen_wallets = std::collections::HashSet::new();
    let mut processed_in_batch = 0u32;
    for participant_info in ctx.remaining_accounts.iter() {
        require!(
            participant_info.is_writable,
            GiveawayError::AccountNotWritable
        );
        require_eq!(
            participant_info.owner,
            ctx.program_id,
            GiveawayError::InvalidProgram
        );
        require!(
            seen_accounts.insert(participant_info.key()),
            GiveawayError::InvalidAccount
        );

        let mut participant_data = &participant_info.data.borrow()[..];
        let mut participant = Participant::try_deserialize(&mut participant_data)
            .map_err(|_| GiveawayError::AccountNotInitialized)?;

        require_keys_eq!(
            participant.giveaway,
            giveaway_key,
            GiveawayError::InvalidAccount
        );
        assert_pda_owned(
            ctx.program_id,
            participant_info,
            &[
                PARTICIPANT_SEED,
                giveaway_key.as_ref(),
                participant.wallet.as_ref(),
            ],
        )?;
        require!(
            seen_wallets.insert(participant.wallet),
            GiveawayError::InvalidAccount
        );
        require!(participant.reveal_included, GiveawayError::InvalidAccount);
        require!(
            participant.finalization_version() != finalization_ledger.recompute_version,
            GiveawayError::InvalidAccount
        );

        finalization_ledger.record_processed()?;
        processed_in_batch = processed_in_batch
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;

        if participant.is_eligible() {
            let rank = compute_candidate_rank(&finalization_ledger.seed, &participant.wallet);
            finalization_ledger.record_eligible(participant.wallet, rank)?;
        }

        participant.mark_finalized_for_version(
            finalization_ledger.recompute_version,
            clock.unix_timestamp,
        );
        let mut account_data = participant_info.data.borrow_mut();
        account_data.fill(0);
        let mut writer = Cursor::new(&mut account_data[..]);
        participant.try_serialize(&mut writer)?;
    }

    require!(
        finalization_ledger.processed_count <= giveaway.provider_uploaded_count,
        GiveawayError::InvalidInstruction
    );

    let completed = finalization_ledger.processed_count == giveaway.provider_uploaded_count;
    if completed {
        finalization_ledger.complete(clock.unix_timestamp);

        let winners_vec = finalization_ledger.winner_wallets();
        let actual_winners_count = winners_vec.len() as u32;

        if winners_vec.is_empty() {
            crate::events::GiveawayEvent::NoWinners {
                giveaway_id: giveaway.id,
                giveaway: giveaway_key.to_string(),
                reason: "no_eligible_participants".to_string(),
                total_participants: giveaway.participants_count,
                total_attested: giveaway.attested_count,
                timestamp: clock.unix_timestamp,
            }
            .emit();
        }

        let winners_pool = giveaway.calculate_winners_pool();
        let per_winner_payout = if actual_winners_count > 0 {
            winners_pool / (actual_winners_count as u64)
        } else {
            0
        };
        if actual_winners_count > 0 {
            require!(
                per_winner_payout >= MIN_WINNER_PAYOUT_LAMPORTS,
                GiveawayError::InsufficientFunds
            );
        }
        let total_winners_payout = (actual_winners_count as u64)
            .checked_mul(per_winner_payout)
            .ok_or(GiveawayError::MathOverflow)?;

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

        if winners_ledger.giveaway == Pubkey::default() {
            winners_ledger.initialize(
                giveaway_key,
                merkle_root,
                actual_winners_count,
                total_winners_payout,
                per_winner_payout,
                winners_vec.clone(),
                finalization_ledger.recompute_version,
                clock.unix_timestamp,
            );
        } else {
            winners_ledger.recompute(
                merkle_root,
                actual_winners_count,
                total_winners_payout,
                per_winner_payout,
                winners_vec.clone(),
                finalization_ledger.recompute_version,
                clock.unix_timestamp,
            )?;
        }

        giveaway.mark_winners_computed()?;

        crate::events::GiveawayEvent::WinnersComputed {
            giveaway_id: giveaway.id,
            giveaway: giveaway_key.to_string(),
            winners_ledger: winners_ledger.key().to_string(),
            merkle_root: hex::encode(merkle_root),
            seed: hex::encode(finalization_ledger.seed),
            rule_version: GIVEAWAY_RULE_VERSION_V2.to_string(),
            total_eligible: finalization_ledger.eligible_count,
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
    }

    GiveawayEvent::FinalizationChunkProcessed {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        finalization_ledger: finalization_ledger.key().to_string(),
        recompute_version: finalization_ledger.recompute_version,
        batch_size: processed_in_batch,
        processed_count: finalization_ledger.processed_count,
        required_count: giveaway.provider_uploaded_count,
        eligible_count: finalization_ledger.eligible_count,
        candidate_count: finalization_ledger.candidates.len() as u32,
        completed,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Finalization chunk for giveaway {}: processed {}/{} (complete={})",
        giveaway.id,
        finalization_ledger.processed_count,
        giveaway.provider_uploaded_count,
        completed
    );

    Ok(())
}
