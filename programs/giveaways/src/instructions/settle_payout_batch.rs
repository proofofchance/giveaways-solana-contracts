//! # Settle Payout Batch Instruction
//!
//! Processes winner payouts in batches using merkle proof verification.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, WinnersLedger},
    utils::{account::transfer_lamports, crypto::verify_merkle_proof},
};
use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct WinnerProof {
    pub winner: Pubkey,
    pub amount: u64,
    pub winner_index: u32,
    pub merkle_proof: Vec<Vec<u8>>,
}

#[derive(Accounts)]
pub struct SettlePayoutBatch<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = giveaway.winners_computed @ GiveawayError::WinnersNotComputed,
        constraint = giveaway.winners_locked @ GiveawayError::WinnersNotLocked,
    )]
    pub giveaway: Account<'info, Giveaway>,

    /// CHECK: Vault account validated by constraint
    #[account(
        mut,
        constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount
    )]
    pub vault: AccountInfo<'info>,

    #[account(
        mut,
        constraint = winners_ledger.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
        constraint = winners_ledger.locked @ GiveawayError::WinnersNotLocked,
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    #[account(
        mut,
        constraint = authority.key() == config.authority @ GiveawayError::Unauthorized
    )]
    pub authority: Signer<'info>,

    /// CHECK: Creator receives leftover payout/refund and is validated against giveaway state.
    #[account(
        mut,
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: AccountInfo<'info>,
}

pub fn process<'info>(
    ctx: Context<'_, '_, '_, 'info, SettlePayoutBatch<'info>>,
    batch_index: u32,
    winners: Vec<WinnerProof>,
) -> Result<()> {
    let _config = &ctx.accounts.config;
    let giveaway = &mut ctx.accounts.giveaway;
    let vault = &ctx.accounts.vault;
    let winners_ledger = &mut ctx.accounts.winners_ledger;
    let authority = &ctx.accounts.authority;
    let creator = &ctx.accounts.creator;
    let clock = Clock::get()?;

    // Validate batch size
    require!(
        !winners.is_empty() && winners.len() <= MAX_WINNERS_PER_BATCH,
        GiveawayError::InvalidBatch
    );
    require!(
        winners_ledger.winners_count > 0,
        GiveawayError::NoEligibleParticipants
    );

    // Start settlement if not started
    winners_ledger.start_settlement(clock.unix_timestamp);
    giveaway.begin_settlement(clock.unix_timestamp);

    let mut winners_paid_in_batch = 0u32;

    // Process each winner in the batch
    for (winner_idx_in_batch, winner_proof) in winners.iter().enumerate() {
        // Validate winner index
        require!(
            winners_ledger.validate_winner_index(winner_proof.winner_index),
            GiveawayError::InvalidBatch
        );

        require!(
            winner_proof.amount == winners_ledger.per_winner_lamports,
            GiveawayError::InvalidBatch
        );
        require!(
            winners_ledger
                .winner_wallets
                .get(winner_proof.winner_index as usize)
                .copied()
                == Some(winner_proof.winner),
            GiveawayError::InvalidBatch
        );
        require!(
            !winners_ledger.is_winner_paid(winner_proof.winner_index),
            GiveawayError::AlreadyPaid
        );

        // Verify merkle proof
        let proof_valid = verify_merkle_proof(
            winner_proof.winner_index,
            &winner_proof.winner,
            winner_proof.amount,
            &winner_proof.merkle_proof,
            &winners_ledger.merkle_root,
        );

        require!(proof_valid, GiveawayError::InvalidMerkleProof);

        // Get winner account from remaining accounts
        let winner_account = ctx
            .remaining_accounts
            .get(winner_idx_in_batch)
            .ok_or(GiveawayError::InvalidAccount)?;

        require_keys_eq!(
            winner_account.key(),
            winner_proof.winner,
            GiveawayError::InvalidAccount
        );
        require!(
            winner_account.is_writable,
            GiveawayError::AccountNotWritable
        );

        transfer_lamports(vault, winner_account, winner_proof.amount)?;

        // Mark as paid
        require!(
            winners_ledger.mark_winner_paid(winner_proof.winner_index),
            GiveawayError::AlreadyPaid
        );
        winners_paid_in_batch += 1;

        // Emit winner paid event
        crate::events::GiveawayEvent::WinnerPaid {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            winner: winner_proof.winner.to_string(),
            amount_lamports: winner_proof.amount,
            batch_index,
            winner_index: winner_proof.winner_index,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    // Mark batch as processed
    winners_ledger.mark_batch_processed();

    // Check if settlement is complete
    if winners_ledger.is_settlement_complete() {
        winners_ledger.complete_settlement(clock.unix_timestamp);

        let service_fee = giveaway.calculate_service_fee();

        if service_fee > 0 {
            transfer_lamports(vault, &authority.to_account_info(), service_fee)?;

            crate::events::GiveawayEvent::ServiceFeePaid {
                giveaway_id: giveaway.id,
                giveaway: giveaway.key().to_string(),
                service_fee_lamports: service_fee,
                authority: authority.key().to_string(),
                timestamp: clock.unix_timestamp,
            }
            .emit();
        }

        let refund = vault.lamports();
        if refund > 0 {
            transfer_lamports(vault, creator, refund)?;

            crate::events::GiveawayEvent::CreatorRefunded {
                giveaway_id: giveaway.id,
                giveaway: giveaway.key().to_string(),
                creator: creator.key().to_string(),
                amount_lamports: refund,
                timestamp: clock.unix_timestamp,
            }
            .emit();
        }

        // Mark giveaway as settled
        giveaway.settle();

        crate::events::GiveawayEvent::GiveawaySettled {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            total_winners: winners_ledger.winners_count,
            total_paid_lamports: winners_ledger.total_payout_lamports,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    msg!(
        "Processed batch {} for giveaway {}: {} winners paid",
        batch_index,
        giveaway.id,
        winners_paid_in_batch
    );

    Ok(())
}
