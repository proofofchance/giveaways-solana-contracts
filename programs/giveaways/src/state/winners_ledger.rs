//! # Winners Ledger Account State
//!
//! The WinnersLedger account stores winner settlement data including
//! merkle root, payout tracking, and batch processing state.

use anchor_lang::prelude::*;

/// Winners settlement ledger
///
/// Stores merkle tree root and tracks payout status for winner settlement.
/// Uses a bitmap to prevent double payments and enable batch processing.
///
/// ## PDA Seeds
/// `["winners_ledger", giveaway_pubkey]`
#[account]
pub struct WinnersLedger {
    /// Giveaway this ledger belongs to
    pub giveaway: Pubkey,

    /// Merkle root of indexed (winner_index, winner, amount) leaves
    pub merkle_root: [u8; 32],

    /// Number of winners in the merkle tree
    pub winners_count: u32,

    /// Total payout amount for all winners
    pub total_payout_lamports: u64,

    /// Per-winner payout amount
    pub per_winner_lamports: u64,

    /// Bitmap tracking which winners have been paid (packed bits)
    /// Each bit represents one winner by their index in the merkle tree
    pub paid_bitmap: Vec<u8>,

    /// Ordered winner wallets for the current recompute version
    pub winner_wallets: Vec<Pubkey>,

    /// Number of winners paid so far
    pub paid_count: u32,

    /// Number of batches processed
    pub batches_processed: u32,

    /// Recompute version when winners were finalized
    pub recompute_version: u32,

    /// Whether winners are locked (no more recomputes allowed)
    pub locked: bool,

    /// When winners were first computed
    pub computed_at_unix: i64,

    /// When winners were locked
    pub locked_at_unix: i64,

    /// When settlement began
    pub settlement_started_at_unix: i64,

    /// When settlement completed (all winners paid)
    pub settlement_completed_at_unix: i64,

    /// Reserved space for future fields
    pub reserved: [u8; 128],
}

impl WinnersLedger {
    /// Maximum size of WinnersLedger account in bytes
    /// Note: This is a base size - actual size depends on bitmap length
    pub const BASE_SIZE: usize = 8 + // discriminator
        32 +  // giveaway
        32 +  // merkle_root
        4 +   // winners_count
        8 +   // total_payout_lamports
        8 +   // per_winner_lamports
        4 +   // paid_bitmap vec length prefix
        4 +   // winner_wallets vec length prefix
        4 +   // paid_count
        4 +   // batches_processed
        4 +   // recompute_version
        1 +   // locked
        8 +   // computed_at_unix
        8 +   // locked_at_unix
        8 +   // settlement_started_at_unix
        8 +   // settlement_completed_at_unix
        128; // reserved

    /// Calculate size needed for a given number of winners
    pub fn calculate_size(winners_count: u32) -> usize {
        let bitmap_bytes = winners_count.div_ceil(8) as usize; // Round up to nearest byte
        let winner_wallet_bytes = winners_count as usize * 32;
        Self::BASE_SIZE + bitmap_bytes + winner_wallet_bytes
    }

    /// Initialize a new winners ledger
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        &mut self,
        giveaway: Pubkey,
        merkle_root: [u8; 32],
        winners_count: u32,
        total_payout_lamports: u64,
        per_winner_lamports: u64,
        winner_wallets: Vec<Pubkey>,
        recompute_version: u32,
        current_time: i64,
    ) {
        self.giveaway = giveaway;
        self.merkle_root = merkle_root;
        self.winners_count = winners_count;
        self.total_payout_lamports = total_payout_lamports;
        self.per_winner_lamports = per_winner_lamports;

        // Initialize bitmap with all zeros (no one paid yet)
        let bitmap_bytes = winners_count.div_ceil(8) as usize;
        self.paid_bitmap = vec![0u8; bitmap_bytes];
        self.winner_wallets = winner_wallets;

        self.paid_count = 0;
        self.batches_processed = 0;
        self.recompute_version = recompute_version;
        self.locked = false;
        self.computed_at_unix = current_time;
        self.locked_at_unix = 0;
        self.settlement_started_at_unix = 0;
        self.settlement_completed_at_unix = 0;
        self.reserved = [0; 128];
    }

    /// Update for recompute
    #[allow(clippy::too_many_arguments)]
    pub fn recompute(
        &mut self,
        merkle_root: [u8; 32],
        winners_count: u32,
        total_payout_lamports: u64,
        per_winner_lamports: u64,
        winner_wallets: Vec<Pubkey>,
        recompute_version: u32,
        current_time: i64,
    ) {
        self.merkle_root = merkle_root;
        self.winners_count = winners_count;
        self.total_payout_lamports = total_payout_lamports;
        self.per_winner_lamports = per_winner_lamports;
        self.recompute_version = recompute_version;
        self.computed_at_unix = current_time;

        // Reset payment tracking
        let bitmap_bytes = winners_count.div_ceil(8) as usize;
        self.paid_bitmap = vec![0u8; bitmap_bytes];
        self.winner_wallets = winner_wallets;
        self.paid_count = 0;
        self.batches_processed = 0;
        self.settlement_started_at_unix = 0;
        self.settlement_completed_at_unix = 0;
    }

    /// Lock winners (prevent further recomputes)
    pub fn lock(&mut self, current_time: i64) {
        self.locked = true;
        self.locked_at_unix = current_time;
    }

    /// Mark settlement as started
    pub fn start_settlement(&mut self, current_time: i64) {
        if self.settlement_started_at_unix == 0 {
            self.settlement_started_at_unix = current_time;
        }
    }

    /// Check if a winner has been paid
    pub fn is_winner_paid(&self, winner_index: u32) -> bool {
        if winner_index >= self.winners_count {
            return false;
        }

        let byte_index = (winner_index / 8) as usize;
        let bit_index = winner_index % 8;

        if byte_index >= self.paid_bitmap.len() {
            return false;
        }

        (self.paid_bitmap[byte_index] & (1 << bit_index)) != 0
    }

    /// Mark a winner as paid
    pub fn mark_winner_paid(&mut self, winner_index: u32) -> bool {
        if winner_index >= self.winners_count {
            return false;
        }

        let byte_index = (winner_index / 8) as usize;
        let bit_index = winner_index % 8;

        if byte_index >= self.paid_bitmap.len() {
            return false;
        }

        // Check if already paid
        if (self.paid_bitmap[byte_index] & (1 << bit_index)) != 0 {
            return false; // Already paid
        }

        // Mark as paid
        self.paid_bitmap[byte_index] |= 1 << bit_index;
        self.paid_count += 1;

        true
    }

    /// Mark a batch as processed
    pub fn mark_batch_processed(&mut self) {
        self.batches_processed += 1;
    }

    /// Check if settlement is complete
    pub fn is_settlement_complete(&self) -> bool {
        self.paid_count == self.winners_count
    }

    /// Mark settlement as complete
    pub fn complete_settlement(&mut self, current_time: i64) {
        if self.is_settlement_complete() {
            self.settlement_completed_at_unix = current_time;
        }
    }

    /// Get number of unpaid winners
    pub fn get_unpaid_count(&self) -> u32 {
        self.winners_count - self.paid_count
    }

    /// Validate winner index
    pub fn validate_winner_index(&self, winner_index: u32) -> bool {
        winner_index < self.winners_count
    }

    /// Get bitmap size in bytes
    pub fn get_bitmap_size(&self) -> usize {
        self.paid_bitmap.len()
    }
}
