//! # Giveaway Account State
//!
//! The Giveaway account represents an individual giveaway instance with all its
//! state data including timing, participants, funds, and settlement status.

use crate::constants::*;
use anchor_lang::prelude::*;

/// Status of a giveaway instance
#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GiveawayStatus {
    /// Giveaway is active (not settled). Phase is derived from timers.
    #[default]
    Active = 0,
    /// Giveaway is closed and settled
    Settled = 1,
}

/// Individual giveaway instance state
///
/// Each giveaway is a unique instance with its own participants, funds, and settlement.
/// The giveaway follows a strict lifecycle from creation through settlement.
///
/// ## PDA Seeds
/// `["giveaway", config_pubkey, giveaway_id_le_bytes]`
///
/// ## Lifecycle Phases
/// 1. **Active**: Accepting participation entries (active_start_unix to active_deadline_unix)
/// 2. **Upload**: Participants upload PoC and attest (active_deadline_unix to upload_deadline_unix)
/// 3. **Remediation**: Anyone can include signed/attested reveals omitted by the provider
/// 4. **Settlement**: Winners compute only after all attested reveals are included
/// 5. **Settled**: All winners paid and giveaway complete
#[account]
pub struct Giveaway {
    /// Unique giveaway ID (sequential)
    pub id: u64,

    /// Config account that created this giveaway
    pub config: Pubkey,

    /// Creator wallet address
    pub creator: Pubkey,

    /// Vault account holding locked funds
    pub vault: Pubkey,

    /// Current status
    pub status: GiveawayStatus,

    /// Total payout amount in lamports
    pub total_payout_lamports: u64,

    /// Number of winners to select
    pub number_of_winners: u32,

    /// Service fee rate snapshot (basis points)
    pub service_fee_bps: u16,

    /// When giveaway was created
    pub created_at_unix: i64,

    /// When active window starts
    pub active_start_unix: i64,

    /// When active window ends (participation deadline)
    pub active_deadline_unix: i64,

    /// When upload/attestation window starts (same as active_deadline_unix)
    pub upload_start_unix: i64,

    /// When upload/attestation window ends
    pub upload_deadline_unix: i64,

    /// When settlement began (0 if not started)
    pub settlement_start_unix: i64,

    /// Total number of participants
    pub participants_count: u32,

    /// Number of participants who attested upload
    pub attested_count: u32,

    /// Number of reveals uploaded by provider
    pub provider_uploaded_count: u32,

    /// Aggregate hash of uploaded reveals, used as the deterministic entropy base
    pub poc_aggregate_hash: [u8; 32],

    /// Whether all attested reveals required for settlement have been uploaded
    pub uploads_complete: bool,

    /// Number of disqualified participants
    pub disqualified_count: u32,

    /// Whether winners have been computed
    pub winners_computed: bool,

    /// Whether winners are locked (no more changes allowed)
    pub winners_locked: bool,

    /// Current recompute version
    pub recompute_version: u32,

    /// Whether settlement is complete
    pub settled: bool,

    /// When omitted-reveal remediation began (0 if not active/needed)
    pub remediation_start_unix: i64,

    /// When omitted-reveal remediation expires (0 if not active/needed)
    pub remediation_deadline_unix: i64,

    /// Reserved space for future fields
    pub reserved: [u8; 112],
}

impl Giveaway {
    /// Maximum size of Giveaway account in bytes
    pub const MAX_SIZE: usize = 8 + // discriminator
        8 +   // id
        32 +  // config
        32 +  // creator
        32 +  // vault
        1 +   // status
        8 +   // total_payout_lamports
        4 +   // number_of_winners
        2 +   // service_fee_bps
        8 +   // created_at_unix
        8 +   // active_start_unix
        8 +   // active_deadline_unix
        8 +   // upload_start_unix
        8 +   // upload_deadline_unix
        8 +   // settlement_start_unix
        4 +   // participants_count
        4 +   // attested_count
        4 +   // provider_uploaded_count
        32 +  // poc_aggregate_hash
        1 +   // uploads_complete
        4 +   // disqualified_count
        1 +   // winners_computed
        1 +   // winners_locked
        4 +   // recompute_version
        1 +   // settled
        8 +   // remediation_start_unix
        8 +   // remediation_deadline_unix
        112; // reserved

    /// Initialize a new giveaway
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        &mut self,
        id: u64,
        config: Pubkey,
        creator: Pubkey,
        vault: Pubkey,
        total_payout_lamports: u64,
        number_of_winners: u32,
        service_fee_bps: u16,
        active_start_unix: i64,
        active_deadline_unix: i64,
        upload_duration_secs: u32,
        current_time: i64,
    ) {
        self.id = id;
        self.config = config;
        self.creator = creator;
        self.vault = vault;
        self.status = GiveawayStatus::Active;
        self.total_payout_lamports = total_payout_lamports;
        self.number_of_winners = number_of_winners;
        self.service_fee_bps = service_fee_bps;
        self.created_at_unix = current_time;
        self.active_start_unix = active_start_unix;
        self.active_deadline_unix = active_deadline_unix;
        self.upload_start_unix = active_deadline_unix;
        self.upload_deadline_unix = active_deadline_unix + upload_duration_secs as i64;
        self.settlement_start_unix = 0;
        self.participants_count = 0;
        self.attested_count = 0;
        self.provider_uploaded_count = 0;
        self.poc_aggregate_hash = [0; 32];
        self.uploads_complete = false;
        self.disqualified_count = 0;
        self.winners_computed = false;
        self.winners_locked = false;
        self.recompute_version = 0;
        self.settled = false;
        self.remediation_start_unix = 0;
        self.remediation_deadline_unix = 0;
        self.reserved = [0; 112];
    }

    /// Check if giveaway is in active phase (accepting participation)
    pub fn is_in_active_phase(&self, current_time: i64) -> bool {
        current_time >= self.active_start_unix && current_time < self.active_deadline_unix
    }

    /// Check if giveaway is in upload/attestation phase
    pub fn is_in_upload_phase(&self, current_time: i64) -> bool {
        current_time >= self.upload_start_unix && current_time < self.upload_deadline_unix
    }

    /// Check if giveaway is ready for settlement
    pub fn is_ready_for_settlement(&self, current_time: i64) -> bool {
        // Settlement ready if upload window elapsed OR all participants attested
        let upload_elapsed = current_time >= self.upload_deadline_unix;
        let all_attested =
            self.participants_count > 0 && self.attested_count == self.participants_count;
        upload_elapsed || all_attested || self.uploads_complete
    }

    /// Get current phase as string
    pub fn get_phase(&self, current_time: i64) -> &'static str {
        if self.settled {
            "settled"
        } else if self.remediation_active(current_time) {
            "remediation"
        } else if self.is_in_active_phase(current_time) {
            "active"
        } else if self.is_in_upload_phase(current_time) {
            "upload"
        } else {
            "settlement"
        }
    }

    /// Add a participant
    pub fn add_participant(&mut self) {
        self.participants_count += 1;
    }

    /// Remove a participant (for disqualification)
    pub fn remove_participant(&mut self) {
        if self.participants_count > 0 {
            self.participants_count -= 1;
        }
    }

    /// Add an attestation
    pub fn add_attestation(&mut self) {
        self.attested_count += 1;
    }

    /// Add uploaded reveals
    pub fn add_uploaded_reveals(&mut self, count: u32, aggregate_hash: [u8; 32]) {
        self.provider_uploaded_count += count;
        self.poc_aggregate_hash = aggregate_hash;
        self.uploads_complete =
            self.attested_count > 0 && self.provider_uploaded_count >= self.attested_count;
    }

    /// Returns true when some accepted/attested reveals have not yet been included.
    pub fn has_missing_attested_reveals(&self) -> bool {
        self.attested_count > 0 && self.provider_uploaded_count < self.attested_count
    }

    /// Returns true when every accepted/attested reveal has been included.
    pub fn attested_reveals_complete(&self) -> bool {
        self.attested_count > 0 && self.provider_uploaded_count >= self.attested_count
    }

    /// Starts omitted-reveal remediation if it is needed and has not already started.
    pub fn begin_remediation(&mut self, current_time: i64) {
        if self.remediation_start_unix == 0 {
            self.remediation_start_unix = current_time;
            self.remediation_deadline_unix = current_time + DEFAULT_REMEDIATION_WINDOW_SECS;
        }
    }

    /// Returns true while the remediation/challenge window is open.
    pub fn remediation_active(&self, current_time: i64) -> bool {
        self.remediation_start_unix > 0
            && self.remediation_deadline_unix > 0
            && current_time <= self.remediation_deadline_unix
            && self.has_missing_attested_reveals()
    }

    /// Returns true when omitted-reveal remediation expired without full inclusion.
    pub fn remediation_expired(&self, current_time: i64) -> bool {
        self.remediation_deadline_unix > 0
            && current_time > self.remediation_deadline_unix
            && self.has_missing_attested_reveals()
    }

    /// Disqualify a participant
    pub fn disqualify_participant(&mut self) {
        self.disqualified_count += 1;
    }

    /// Mark winners as computed
    pub fn mark_winners_computed(&mut self) {
        self.winners_computed = true;
        self.recompute_version += 1;
    }

    /// Lock winners (prevent further changes)
    pub fn lock_winners(&mut self) {
        self.winners_locked = true;
    }

    /// Begin settlement phase
    pub fn begin_settlement(&mut self, current_time: i64) {
        if self.settlement_start_unix == 0 {
            self.settlement_start_unix = current_time;
        }
    }

    /// Mark as settled
    pub fn settle(&mut self) {
        self.status = GiveawayStatus::Settled;
        self.settled = true;
    }

    /// Extend active deadline
    pub fn extend_active_deadline(&mut self, new_deadline_unix: i64, upload_duration_secs: u32) {
        self.active_deadline_unix = new_deadline_unix;
        self.upload_start_unix = new_deadline_unix;
        self.upload_deadline_unix = new_deadline_unix + upload_duration_secs as i64;
    }

    // Off-chain metadata removed from on-chain state

    /// Calculate per-winner payout amount before settlement.
    pub fn calculate_per_winner_payout(&self) -> u64 {
        if self.number_of_winners == 0 {
            0
        } else {
            self.calculate_winners_pool() / self.number_of_winners as u64
        }
    }

    /// Calculate service fee amount
    pub fn calculate_service_fee(&self) -> u64 {
        (self.total_payout_lamports * self.service_fee_bps as u64) / 10000
    }

    /// Calculate the payout pool available to winners after service fee.
    pub fn calculate_winners_pool(&self) -> u64 {
        self.total_payout_lamports
            .saturating_sub(self.calculate_service_fee())
    }

    /// Calculate total required funding. The configured total payout is
    /// inclusive of service fee; no extra fee deposit is required at creation.
    pub fn calculate_total_required_funding(&self) -> u64 {
        self.total_payout_lamports
    }

    /// Get eligible participants count (not disqualified)
    pub fn get_eligible_participants_count(&self) -> u32 {
        self.participants_count
            .saturating_sub(self.disqualified_count)
    }

    // No text length validation needed; metadata moved off-chain

    /// Validate winner count
    pub fn validate_winner_count(number_of_winners: u32) -> bool {
        (MIN_WINNERS..=MAX_WINNERS).contains(&number_of_winners)
    }

    /// Validate timing
    pub fn validate_timing(
        active_start_unix: i64,
        active_deadline_unix: i64,
        upload_duration_secs: u32,
    ) -> bool {
        active_start_unix < active_deadline_unix
            && (MIN_UPLOAD_DURATION_SECS..=MAX_UPLOAD_DURATION_SECS).contains(&upload_duration_secs)
    }
}
