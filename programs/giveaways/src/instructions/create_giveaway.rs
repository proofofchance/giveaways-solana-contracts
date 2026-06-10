//! # Create Giveaway Instruction
//!
//! Creates a new giveaway with locked funds and metadata.
//! Creator deposits the configured total payout; service fee is taken from
//! that amount only on successful payout settlement.

use crate::{
    constants::*,
    error::GiveawayError,
    state::{Config, Giveaway},
    utils::{account::create_account_with_pda, validation::*},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
#[instruction(giveaway_id: u64)]
pub struct CreateGiveaway<'info> {
    #[account(mut)]
    pub config: Account<'info, Config>,

    #[account(
        init,
        payer = creator,
        space = Giveaway::MAX_SIZE,
        seeds = [GIVEAWAY_SEED, config.key().as_ref(), &giveaway_id.to_le_bytes()],
        bump
    )]
    pub giveaway: Account<'info, Giveaway>,

    /// CHECK: Vault PDA will be created and validated
    #[account(
        mut,
        seeds = [VAULT_SEED, giveaway.key().as_ref()],
        bump
    )]
    pub vault: AccountInfo<'info>,

    #[account(mut)]
    pub creator: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[allow(clippy::too_many_arguments)]
pub fn process(
    ctx: Context<CreateGiveaway>,
    giveaway_id: u64,
    total_payout_lamports: u64,
    number_of_winners: u32,
    active_start_unix: i64,
    active_deadline_unix: i64,
    upload_duration_secs: u32,
) -> Result<()> {
    let config = &ctx.accounts.config;
    let giveaway = &mut ctx.accounts.giveaway;
    let vault = &ctx.accounts.vault;
    let creator = &ctx.accounts.creator;
    let system_program = &ctx.accounts.system_program;
    let clock = Clock::get()?;

    // Enforce on-chain sequential ID
    require!(
        giveaway_id == config.next_giveaway_id,
        GiveawayError::InvalidGiveawayId
    );

    // Validate parameters
    validate_winner_count(number_of_winners)?;
    validate_duration(
        upload_duration_secs,
        MIN_UPLOAD_DURATION_SECS,
        MAX_UPLOAD_DURATION_SECS,
    )?;
    validate_timing_sequence(active_start_unix, active_deadline_unix)?;
    require!(
        Giveaway::validate_timing(
            active_start_unix,
            active_deadline_unix,
            upload_duration_secs
        ),
        GiveawayError::InvalidTiming
    );
    let upload_deadline_unix = active_deadline_unix
        .checked_add(i64::from(upload_duration_secs))
        .ok_or(GiveawayError::MathOverflow)?;

    // Validate minimum payout
    require!(total_payout_lamports > 0, GiveawayError::InsufficientFunds);
    validate_minimum_winners_pool(
        total_payout_lamports,
        config.service_fee_bps,
        number_of_winners,
    )?;

    // Funding model: host deposits ONLY total_payout; service fee is taken at settlement
    // from the winners pool actually paid. Any leftover after winners+fee returns to host.
    require_sufficient_lamports(creator, total_payout_lamports)?;

    // Create vault account (owned by this program)
    let vault_bump = ctx.bumps.vault;
    create_account_with_pda(
        &creator.to_account_info(),
        &vault.to_account_info(),
        &system_program.to_account_info(),
        ctx.program_id,
        &[VAULT_SEED, giveaway.key().as_ref()],
        vault_bump,
        0,
    )?;

    // Transfer funds to vault (only the winners pool)
    anchor_lang::solana_program::program::invoke(
        &anchor_lang::solana_program::system_instruction::transfer(
            creator.key,
            vault.key,
            total_payout_lamports,
        ),
        &[creator.to_account_info(), vault.to_account_info()],
    )?;

    // Initialize giveaway
    giveaway.initialize(
        giveaway_id,
        config.key(),
        creator.key(),
        vault.key(),
        total_payout_lamports,
        number_of_winners,
        config.service_fee_bps,
        active_start_unix,
        active_deadline_unix,
        upload_duration_secs,
        clock.unix_timestamp,
    );

    // Emit event using custom event system
    crate::events::GiveawayEvent::GiveawayCreated {
        giveaway_id,
        giveaway: giveaway.key().to_string(),
        creator: creator.key().to_string(),
        config: config.key().to_string(),
        vault: vault.key().to_string(),
        total_payout_lamports,
        number_of_winners,
        active_start_unix,
        active_deadline_unix,
        upload_start_unix: active_deadline_unix,
        upload_deadline_unix,
        service_fee_bps: config.service_fee_bps,
        timestamp: clock.unix_timestamp,
    }
    .emit();

    msg!(
        "Giveaway {} created by {} with {} lamports payout for {} winners",
        giveaway_id,
        creator.key(),
        total_payout_lamports,
        number_of_winners
    );

    // Increment the next id on success
    let next = config
        .next_giveaway_id
        .checked_add(1)
        .ok_or(GiveawayError::MathOverflow)?;
    ctx.accounts.config.next_giveaway_id = next;

    Ok(())
}
