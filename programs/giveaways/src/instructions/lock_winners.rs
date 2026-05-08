//! # Lock Winners Instruction
//!
//! Irreversibly locks the winner set to prevent further changes.
//! After this, no more disqualifications or recomputes are allowed.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway, WinnersLedger},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct LockWinners<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
        constraint = giveaway.winners_computed @ GiveawayError::WinnersNotComputed,
        constraint = !giveaway.winners_locked @ GiveawayError::WinnersLocked,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        mut,
        constraint = winners_ledger.giveaway == giveaway.key() @ GiveawayError::InvalidAccount,
    )]
    pub winners_ledger: Account<'info, WinnersLedger>,

    #[account(
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: Signer<'info>,
}

pub fn process(ctx: Context<LockWinners>) -> Result<()> {
    let giveaway = &mut ctx.accounts.giveaway;
    let winners_ledger = &mut ctx.accounts.winners_ledger;
    let _creator = &ctx.accounts.creator;
    let clock = Clock::get()?;

    // Lock winners in both accounts
    giveaway.lock_winners();
    winners_ledger.lock(clock.unix_timestamp);

    // Emit event
    crate::events::GiveawayEvent::WinnersLocked {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        winners_ledger: winners_ledger.key().to_string(),
        final_recompute_version: giveaway.recompute_version,
        final_winners_count: winners_ledger.winners_count,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Winners locked for giveaway {} with {} winners (version {})",
        giveaway.id,
        winners_ledger.winners_count,
        giveaway.recompute_version
    );

    Ok(())
}
