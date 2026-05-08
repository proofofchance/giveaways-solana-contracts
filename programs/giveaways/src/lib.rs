//! # ProofOfChance Giveaways Program
//!
//! A transparent, decentralized giveaway system built on Solana that uses
//! participant-provided entropy (proof-of-chance) for fair winner selection.
//!
//! ## Core Concepts
//!
//! ### Proof-of-Chance Model
//! - Participants provide a secret phrase that gets hashed on-chain
//! - During upload window, participants upload plaintext to service provider
//! - Provider uploads all reveals on-chain for transparent entropy generation
//! - Winners are selected deterministically using aggregated entropy
//!
//! ### Anti-Censorship Design
//! - Participants can attest on-chain that they've uploaded their reveal
//! - Provider must include all attested participants in reveal batch
//! - If any attested participant is missing, settlement is blocked
//!
//! ### Manual Review System
//! - Giveaway creators can disqualify participants after winner computation
//! - All disqualifications are logged on-chain for transparency
//! - Winners can be recomputed until locked by creator
//!
//! ## Account Architecture
//!
//! All accounts use Program Derived Addresses (PDAs) for security:
//! - **Config**: `["config"]` - Global system configuration
//! - **Giveaway**: `["giveaway", config_pubkey, giveaway_id_le_bytes]` - Individual giveaway state
//! - **Participant**: `["participant", giveaway_pubkey, wallet_pubkey]` - Participant data
//! - **Vault**: `["vault", giveaway_pubkey]` - Fund custody account
//! - **WinnersLedger**: `["winners_ledger", giveaway_pubkey]` - Winner settlement data
//!
//! ## Instruction Flow
//!
//! 1. **Initialize**: Set up global configuration (once per deployment)
//! 2. **CreateGiveaway**: Creator sets up giveaway with locked funds
//! 3. **Participate**: Participants submit proof text and proof-of-chance hash
//! 4. **AttestUploaded**: Participants attest to off-chain reveal upload
//! 5. **UploadReveals**: Provider uploads batch of reveals for settlement
//! 6. **FinalizeWinners**: Compute winners and store merkle commitment
//! 7. **DisqualifyParticipant**: Creator can remove invalid entries (with audit trail)
//! 8. **RecomputeWinners**: Recompute after disqualifications
//! 9. **LockWinners**: Freeze winner set and enable settlement
//! 10. **SettlePayoutBatch**: Pay winners in batches using merkle proofs
//!
//! ## Security Features
//!
//! - **PDA-based accounts**: All accounts use program-derived addresses
//! - **Locked funds**: Creator cannot withdraw; funds only go to winners
//! - **Transparent operations**: All actions emit events for audit trail
//! - **Anti-censorship**: Attestation system prevents provider manipulation
//! - **Deterministic selection**: Same entropy always produces same winners

#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;
pub mod utils;

use instructions::{
    attest_uploaded::*, begin_upload_phase::*, create_giveaway::*, disqualify_participant::*,
    extend_active_deadline::*, finalize_winners::*, initialize::*, lock_winners::*, participate::*,
    recompute_winners::*, settle_giveaway::*, settle_payout_batch::*, update_service_charge::*,
    upload_reveals::*,
};

declare_id!("DUMRJ15A2ivmUNDK6EX7wfRQ1cYw4vw5ewSyT8xSJuRG");

#[program]
pub mod giveaways {
    use super::*;

    /// Initialize the giveaways system configuration
    ///
    /// Sets up the global config account with authority and default parameters.
    /// Can only be called once per deployment.
    ///
    /// Accounts expected:
    /// 0. `[writable]` Config account (PDA: ["config"])
    /// 1. `[signer]` Authority (becomes config.authority)
    /// 2. `[]` System program
    pub fn initialize(
        ctx: Context<Initialize>,
        service_fee_bps: u16,
        default_active_duration_secs: u32,
        default_upload_duration_secs: u32,
    ) -> Result<()> {
        instructions::initialize::process(
            ctx,
            service_fee_bps,
            default_active_duration_secs,
            default_upload_duration_secs,
        )
    }

    /// Create a new giveaway with locked funds
    ///
    /// Creator deposits the configured total payout amount. Service fee is
    /// taken from that amount during successful payout settlement.
    /// Giveaway parameters are stored on-chain for transparency.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account (PDA: ["giveaway", config, id])
    /// 2. `[writable]` Vault account (PDA: ["vault", giveaway])
    /// 3. `[signer, writable]` Creator (funds source)
    /// 4. `[]` Instructions sysvar
    pub fn create_giveaway(
        ctx: Context<CreateGiveaway>,
        giveaway_id: u64,
        total_payout_lamports: u64,
        number_of_winners: u32,
        active_start_unix: i64,
        active_deadline_unix: i64,
        upload_duration_secs: u32,
    ) -> Result<()> {
        instructions::create_giveaway::process(
            ctx,
            giveaway_id,
            total_payout_lamports,
            number_of_winners,
            active_start_unix,
            active_deadline_unix,
            upload_duration_secs,
        )
    }

    /// Participate in a giveaway
    ///
    /// Submit proof text and proof-of-chance commitment.
    /// Can be called multiple times to update entry before deadline.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Participant account (PDA: ["participant", giveaway, wallet])
    /// 3. `[signer]` Participant wallet
    /// 4. `[]` System program
    pub fn participate(
        ctx: Context<Participate>,
        commitment_hash: [u8; 32],
        proof_text: String,
    ) -> Result<()> {
        instructions::participate::process(ctx, commitment_hash, proof_text)
    }

    /// Attest that proof-of-chance has been uploaded to provider
    ///
    /// Anti-censorship mechanism - provider must include all attested participants.
    /// Can only be called during upload/attestation window.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[]` Giveaway account
    /// 2. `[writable]` Participant account
    /// 3. `[signer]` Participant wallet
    pub fn attest_uploaded(ctx: Context<AttestUploaded>) -> Result<()> {
        instructions::attest_uploaded::process(ctx)
    }

    /// Upload batch of participant reveals
    ///
    /// Provider uploads plaintext proof-of-chance data for entropy generation.
    /// Must include all attested participants or settlement will be blocked.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[signer]` Authority
    /// 3..N. `[writable]` Participant accounts
    pub fn upload_reveals(ctx: Context<UploadReveals>, reveals: Vec<RevealData>) -> Result<()> {
        instructions::upload_reveals::process(ctx, reveals)
    }

    /// Finalize winners and store merkle commitment
    ///
    /// Determines winners using uploaded reveals and stores the merkle root
    /// on-chain for batch settlement verification.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[writable]` WinnersLedger account (PDA: ["winners_ledger", giveaway])
    /// 4. `[signer]` Authority
    /// 5. `[]` System program
    /// 6..N. `[]` All participant accounts
    pub fn finalize_winners(ctx: Context<FinalizeWinners>) -> Result<()> {
        instructions::finalize_winners::process(ctx)
    }

    /// Disqualify a participant entry
    ///
    /// Only giveaway creator can disqualify participants.
    /// All disqualifications are logged for transparency.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Participant account
    /// 3. `[signer]` Creator
    pub fn disqualify_participant(
        ctx: Context<DisqualifyParticipant>,
        reason_code: u8,
    ) -> Result<()> {
        instructions::disqualify_participant::process(ctx, reason_code)
    }

    /// Recompute winners after disqualifications
    ///
    /// Can be called multiple times until winners are locked.
    /// Uses same deterministic algorithm with current eligible participants.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` WinnersLedger account
    /// 3. `[signer]` Creator or Authority
    /// 4..N. `[]` All participant accounts
    pub fn recompute_winners(ctx: Context<RecomputeWinners>) -> Result<()> {
        instructions::recompute_winners::process(ctx)
    }

    /// Lock winners to prevent further changes
    ///
    /// Irreversible action that enables settlement.
    /// No more disqualifications or recomputes allowed after this.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` WinnersLedger account
    /// 3. `[signer]` Creator
    pub fn lock_winners(ctx: Context<LockWinners>) -> Result<()> {
        instructions::lock_winners::process(ctx)
    }

    /// Settle giveaway across all scenarios
    ///
    /// Single instruction that handles settlement for both zero-participant
    /// and winner scenarios. Computes winners, locks set, and handles immediate
    /// refunds for zero-winner cases.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[writable]` WinnersLedger account (PDA: ["winners_ledger", giveaway])
    /// 4. `[signer]` Creator
    /// 5. `[signer]` Authority (can be same as creator)
    /// 6. `[]` System program
    /// 7..N. `[]` All participant accounts
    pub fn settle_giveaway(ctx: Context<SettleGiveaway>) -> Result<()> {
        instructions::settle_giveaway::process(ctx)
    }

    /// Process a batch of winner payouts
    ///
    /// Verifies merkle proofs and transfers funds directly to winners.
    /// Can be called multiple times to process all winners in batches.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[writable]` Vault account
    /// 3. `[writable]` WinnersLedger account
    /// 4. `[signer, writable]` Authority
    /// 5. `[writable]` Creator refund account
    /// 6..N. `[writable]` Winner wallet accounts
    pub fn settle_payout_batch<'info>(
        ctx: Context<'_, '_, '_, 'info, SettlePayoutBatch<'info>>,
        batch_index: u32,
        winners: Vec<WinnerProof>,
    ) -> Result<()> {
        instructions::settle_payout_batch::process(ctx, batch_index, winners)
    }

    /// Extend active deadline (only before current deadline)
    ///
    /// Allows creator to extend participation window.
    /// Cannot shorten or extend after deadline has passed.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[signer]` Creator
    pub fn extend_active_deadline(
        ctx: Context<ExtendActiveDeadline>,
        new_deadline_unix: i64,
    ) -> Result<()> {
        instructions::extend_active_deadline::process(ctx, new_deadline_unix)
    }

    /// Begin upload phase immediately
    ///
    /// Authority-only instruction to start upload/attestation phase now.
    ///
    /// Accounts expected:
    /// 0. `[]` Config account
    /// 1. `[writable]` Giveaway account
    /// 2. `[signer]` Authority
    pub fn begin_upload_phase(ctx: Context<BeginUploadPhase>) -> Result<()> {
        instructions::begin_upload_phase::process(ctx)
    }

    /// Update service charge rate (admin only)
    ///
    /// Updates the global service fee rate.
    /// Only affects new giveaways created after this change.
    ///
    /// Accounts expected:
    /// 0. `[writable]` Config account
    /// 1. `[signer]` Authority
    pub fn update_service_charge(
        ctx: Context<UpdateServiceCharge>,
        new_service_fee_bps: u16,
    ) -> Result<()> {
        instructions::update_service_charge::process(ctx, new_service_fee_bps)
    }
}
