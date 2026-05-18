//! # Settle Giveaway Instruction
//!
//! Single instruction that handles giveaway settlement across all scenarios:
//! - Zero participants: immediate refund to creator
//! - With participants: compute winners, lock set, handle refunds for unused slots

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway, WinnersLedger},
    utils::account::transfer_lamports,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct SettleGiveaway<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
    )]
    pub giveaway: Account<'info, Giveaway>,

    /// CHECK: Vault account validated by constraint
    #[account(
        mut,
        constraint = vault.key() == giveaway.vault @ GiveawayError::InvalidAccount
    )]
    pub vault: AccountInfo<'info>,

    #[account(
        init_if_needed,
        payer = creator,
        space = WinnersLedger::calculate_size(giveaway.number_of_winners),
        seeds = [WINNERS_LEDGER_SEED, giveaway.key().as_ref()],
        bump
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    #[account(
        mut,
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: Signer<'info>,

    /// CHECK: Authority account - can be same as creator
    #[account(
        constraint = authority.key() == config.authority || authority.key() == giveaway.creator @ GiveawayError::Unauthorized
    )]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn process(ctx: Context<SettleGiveaway>) -> Result<()> {
    let accounts = ctx.accounts;
    let _config = &accounts.config;
    let giveaway_key = accounts.giveaway.key();
    let giveaway = &mut accounts.giveaway;
    let vault = &accounts.vault;
    let creator = &accounts.creator;
    let _authority = &accounts.authority;
    let clock = Clock::get()?;

    let reason = if giveaway.participants_count == 0 {
        require!(
            clock.unix_timestamp >= giveaway.active_deadline_unix,
            GiveawayError::SettlementNotReady
        );
        "no_participants"
    } else if giveaway.attested_count == 0 {
        require!(
            clock.unix_timestamp >= giveaway.upload_deadline_unix,
            GiveawayError::SettlementNotReady
        );
        "no_attesters"
    } else if giveaway.remediation_expired(clock.unix_timestamp) {
        "accepted_reveals_omitted_after_remediation"
    } else if giveaway.winners_computed
        && accounts.winners_ledger.giveaway == giveaway_key
        && accounts.winners_ledger.winners_count == 0
    {
        "no_eligible_participants"
    } else {
        return Err(GiveawayError::InvalidInstruction.into());
    };

    crate::events::GiveawayEvent::NoWinners {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        reason: reason.to_string(),
        total_participants: giveaway.participants_count,
        total_attested: giveaway.attested_count,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    let refund = vault.lamports();
    if refund > 0 {
        transfer_lamports(vault, &creator.to_account_info(), refund)?;

        crate::events::GiveawayEvent::CreatorRefunded {
            giveaway_id: giveaway.id,
            giveaway: giveaway_key.to_string(),
            creator: creator.key().to_string(),
            amount_lamports: refund,
            timestamp: clock.unix_timestamp,
        }
        .emit();
    }

    giveaway.settle();

    crate::events::GiveawayEvent::GiveawaySettled {
        giveaway_id: giveaway.id,
        giveaway: giveaway_key.to_string(),
        total_winners: 0,
        total_paid_lamports: 0,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Giveaway {} settled via {} refund path",
        giveaway.id,
        reason
    );

    Ok(())
}
