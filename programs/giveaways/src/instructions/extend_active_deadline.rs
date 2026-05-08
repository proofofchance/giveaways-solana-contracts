//! # Extend Active Deadline Instruction
//!
//! Allows creator to extend the participation deadline before it passes.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ExtendActiveDeadline<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        constraint = creator.key() == giveaway.creator @ GiveawayError::CreatorMismatch
    )]
    pub creator: Signer<'info>,
}

pub fn process(ctx: Context<ExtendActiveDeadline>, new_deadline_unix: i64) -> Result<()> {
    let giveaway = &mut ctx.accounts.giveaway;
    let _creator = &ctx.accounts.creator;
    let clock = Clock::get()?;

    // Validate deadline hasn't passed
    require!(
        clock.unix_timestamp < giveaway.active_deadline_unix,
        GiveawayError::DeadlinePassed
    );

    // Validate new deadline is later than current
    require!(
        new_deadline_unix > giveaway.active_deadline_unix,
        GiveawayError::CannotShorten
    );

    // Calculate upload duration to maintain the same interval.
    let upload_duration_secs = (giveaway.upload_deadline_unix - giveaway.upload_start_unix) as u32;

    // Update deadlines
    giveaway.extend_active_deadline(new_deadline_unix, upload_duration_secs);

    // Emit event
    crate::events::GiveawayEvent::GiveawayUpdated {
        giveaway_id: giveaway.id,
        giveaway: giveaway.key().to_string(),
        active_deadline_unix: Some(new_deadline_unix),
        upload_deadline_unix: Some(giveaway.upload_deadline_unix),
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Extended deadline for giveaway {} to {}",
        giveaway.id,
        new_deadline_unix
    );

    Ok(())
}
