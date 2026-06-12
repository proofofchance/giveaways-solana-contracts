//! # Giveaway Account State
//!
//! The Giveaway account represents an individual giveaway instance with all its
//! state data including timing, participants, funds, and settlement status.

use crate::constants::*;
use anchor_lang::prelude::*;

/// Current serialized Giveaway account layout version.
pub const GIVEAWAY_ACCOUNT_VERSION: u16 = 1;

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
    pub participants_count: u64,

    /// Number of participants who attested upload
    pub attested_count: u64,

    /// Number of reveals uploaded by provider
    pub provider_uploaded_count: u64,

    /// Aggregate hash of uploaded reveals, used as the deterministic entropy base
    pub poc_aggregate_hash: [u8; 32],

    /// Whether all attested reveals required for settlement have been uploaded
    pub uploads_complete: bool,

    /// Number of disqualified participants
    pub disqualified_count: u64,

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

    /// Serialized account layout version for controlled upgrades.
    pub account_version: u16,

    /// Reserved space for future fields
    pub reserved: [u8; 110],
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
        8 +   // participants_count
        8 +   // attested_count
        8 +   // provider_uploaded_count
        32 +  // poc_aggregate_hash
        1 +   // uploads_complete
        8 +   // disqualified_count
        1 +   // winners_computed
        1 +   // winners_locked
        4 +   // recompute_version
        1 +   // settled
        8 +   // remediation_start_unix
        8 +   // remediation_deadline_unix
        2 +   // account_version
        110; // reserved

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
        self.account_version = GIVEAWAY_ACCOUNT_VERSION;
        self.reserved = [0; 110];
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
    pub fn add_participant(&mut self) -> Result<()> {
        self.participants_count = self
            .participants_count
            .checked_add(1)
            .ok_or(crate::error::GiveawayError::MathOverflow)?;
        Ok(())
    }

    /// Remove a participant (for disqualification)
    pub fn remove_participant(&mut self) {
        if self.participants_count > 0 {
            self.participants_count -= 1;
        }
    }

    /// Add an attestation
    pub fn add_attestation(&mut self) -> Result<()> {
        self.attested_count = self
            .attested_count
            .checked_add(1)
            .ok_or(crate::error::GiveawayError::MathOverflow)?;
        Ok(())
    }

    /// Add uploaded reveals
    pub fn add_uploaded_reveals(&mut self, count: u64, aggregate_hash: [u8; 32]) -> Result<()> {
        self.provider_uploaded_count = self
            .provider_uploaded_count
            .checked_add(count)
            .ok_or(crate::error::GiveawayError::MathOverflow)?;
        self.poc_aggregate_hash = aggregate_hash;
        self.uploads_complete =
            self.attested_count > 0 && self.provider_uploaded_count >= self.attested_count;
        Ok(())
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
    pub fn disqualify_participant(&mut self) -> Result<()> {
        self.disqualified_count = self
            .disqualified_count
            .checked_add(1)
            .ok_or(crate::error::GiveawayError::MathOverflow)?;
        Ok(())
    }

    /// Mark winners as computed
    pub fn mark_winners_computed(&mut self) -> Result<()> {
        self.winners_computed = true;
        self.recompute_version = self
            .recompute_version
            .checked_add(1)
            .ok_or(crate::error::GiveawayError::MathOverflow)?;
        Ok(())
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
        ((u128::from(self.total_payout_lamports) * u128::from(self.service_fee_bps)) / 10_000u128)
            as u64
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
    pub fn get_eligible_participants_count(&self) -> u64 {
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
        let active_duration_secs = active_deadline_unix.saturating_sub(active_start_unix);
        active_start_unix < active_deadline_unix
            && active_duration_secs >= i64::from(MIN_ACTIVE_DURATION_SECS)
            && active_duration_secs <= i64::from(MAX_ACTIVE_DURATION_SECS)
            && (MIN_UPLOAD_DURATION_SECS..=MAX_UPLOAD_DURATION_SECS).contains(&upload_duration_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_giveaway() -> Giveaway {
        Giveaway {
            id: 1,
            config: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            vault: Pubkey::new_unique(),
            status: GiveawayStatus::Active,
            total_payout_lamports: 0,
            number_of_winners: 1,
            service_fee_bps: 0,
            created_at_unix: 0,
            active_start_unix: 0,
            active_deadline_unix: 0,
            upload_start_unix: 0,
            upload_deadline_unix: 0,
            settlement_start_unix: 0,
            participants_count: 0,
            attested_count: 0,
            provider_uploaded_count: 0,
            poc_aggregate_hash: [0; 32],
            uploads_complete: false,
            disqualified_count: 0,
            winners_computed: false,
            winners_locked: false,
            recompute_version: 0,
            settled: false,
            remediation_start_unix: 0,
            remediation_deadline_unix: 0,
            account_version: GIVEAWAY_ACCOUNT_VERSION,
            reserved: [0; 110],
        }
    }

    #[test]
    fn service_fee_uses_wide_arithmetic() {
        let mut giveaway = test_giveaway();
        giveaway.total_payout_lamports = u64::MAX;
        giveaway.service_fee_bps = MAX_SERVICE_FEE_BPS;

        assert_eq!(
            giveaway.calculate_service_fee(),
            ((u128::from(u64::MAX) * u128::from(MAX_SERVICE_FEE_BPS)) / 10_000u128) as u64
        );
    }

    #[test]
    fn timing_requires_active_duration_bounds() {
        let start = 1_000;

        assert!(Giveaway::validate_timing(
            start,
            start + i64::from(MIN_ACTIVE_DURATION_SECS),
            MIN_UPLOAD_DURATION_SECS
        ));
        assert!(!Giveaway::validate_timing(
            start,
            start + i64::from(MIN_ACTIVE_DURATION_SECS) - 1,
            MIN_UPLOAD_DURATION_SECS
        ));
        assert!(!Giveaway::validate_timing(
            start,
            start + i64::from(MAX_ACTIVE_DURATION_SECS) + 1,
            MIN_UPLOAD_DURATION_SECS
        ));
    }

    #[test]
    fn initialized_giveaway_has_layout_version() {
        let mut giveaway = test_giveaway();
        giveaway.initialize(
            7,
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            1_000_000,
            1,
            500,
            1_000,
            2_000,
            MIN_UPLOAD_DURATION_SECS,
            900,
        );

        assert_eq!(giveaway.account_version, GIVEAWAY_ACCOUNT_VERSION);
        assert_eq!(giveaway.reserved, [0; 110]);
    }

    #[test]
    fn participant_progress_counters_support_u64_scale() {
        let mut giveaway = test_giveaway();
        let beyond_u32 = u64::from(u32::MAX) + 1;

        giveaway.participants_count = u64::from(u32::MAX);
        giveaway.add_participant().unwrap();
        assert_eq!(giveaway.participants_count, beyond_u32);

        giveaway.attested_count = u64::from(u32::MAX);
        giveaway.add_attestation().unwrap();
        assert_eq!(giveaway.attested_count, beyond_u32);

        giveaway.provider_uploaded_count = u64::from(u32::MAX);
        giveaway
            .add_uploaded_reveals(2, [7u8; 32])
            .expect("wide reveal counts should not be capped at u32");
        assert_eq!(giveaway.provider_uploaded_count, beyond_u32 + 1);

        giveaway.disqualified_count = u64::from(u32::MAX);
        giveaway.disqualify_participant().unwrap();
        assert_eq!(giveaway.disqualified_count, beyond_u32);
        assert_eq!(giveaway.get_eligible_participants_count(), 0);
    }
}
