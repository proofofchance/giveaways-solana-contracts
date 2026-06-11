//! # Begin Upload Phase Instruction
//!
//! Authority-only instruction to start the upload/attestation phase immediately.
//! This is only available in binaries built with the `allow-early-upload` feature.

use crate::{
    error::GiveawayError,
    state::{Config, Giveaway},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct BeginUploadPhase<'info> {
    #[account()]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        constraint = !giveaway.settled @ GiveawayError::GiveawayAlreadySettled,
    )]
    pub giveaway: Account<'info, Giveaway>,

    #[account(
        constraint = authority.key() == config.authority @ GiveawayError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

pub fn process(ctx: Context<BeginUploadPhase>) -> Result<()> {
    #[cfg(not(feature = "allow-early-upload"))]
    {
        let _ = ctx;
        err!(GiveawayError::EarlyUploadDisabled)
    }

    #[cfg(feature = "allow-early-upload")]
    {
        let config = &ctx.accounts.config;
        let giveaway = &mut ctx.accounts.giveaway;
        let _authority = &ctx.accounts.authority;
        let clock = Clock::get()?;

        let now = clock.unix_timestamp;

        // Set upload phase to start now with default duration.
        giveaway.upload_start_unix = now;
        giveaway.upload_deadline_unix = now
            .checked_add(i64::from(config.default_upload_duration_secs))
            .ok_or(GiveawayError::MathOverflow)?;

        // Also update active deadline to now if it's in the future.
        if giveaway.active_deadline_unix > now {
            giveaway.active_deadline_unix = now;
        }

        crate::events::GiveawayEvent::UploadPhaseBegan {
            giveaway_id: giveaway.id,
            giveaway: giveaway.key().to_string(),
            upload_start_unix: giveaway.upload_start_unix,
            upload_deadline_unix: giveaway.upload_deadline_unix,
            timestamp: now,
        }
        .emit();

        msg!(
            "Upload phase began for giveaway {} (ends at {})",
            giveaway.id,
            giveaway.upload_deadline_unix
        );

        Ok(())
    }
}
