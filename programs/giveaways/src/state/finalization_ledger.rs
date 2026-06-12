//! # Finalization Ledger Account State
//!
//! Tracks chunked winner finalization for large giveaways. The ledger stores the
//! best ranked candidates seen so far, so finalization never needs every
//! participant account in one transaction.

use crate::error::GiveawayError;
use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankedCandidate {
    pub rank: [u8; 32],
    pub wallet: Pubkey,
}

impl RankedCandidate {
    pub fn new(rank: [u8; 32], wallet: Pubkey) -> Self {
        Self { rank, wallet }
    }
}

/// Chunked finalization state for one giveaway.
///
/// PDA seeds: `["finalization_ledger", giveaway_pubkey]`
#[account]
pub struct FinalizationLedger {
    pub giveaway: Pubkey,
    pub recompute_version: u32,
    pub target_winners: u32,
    pub processed_count: u64,
    pub eligible_count: u64,
    pub seed: [u8; 32],
    pub candidates: Vec<RankedCandidate>,
    pub completed: bool,
    pub started_at_unix: i64,
    pub completed_at_unix: i64,
    pub reserved: [u8; 64],
}

impl FinalizationLedger {
    pub const BASE_SIZE: usize = 8 + // discriminator
        32 + // giveaway
        4 + // recompute_version
        4 + // target_winners
        8 + // processed_count
        8 + // eligible_count
        32 + // seed
        4 + // candidates vec length prefix
        1 + // completed
        8 + // started_at_unix
        8 + // completed_at_unix
        64; // reserved

    pub const RANKED_CANDIDATE_SIZE: usize = 32 + 32;

    pub fn calculate_size(target_winners: u32) -> usize {
        Self::BASE_SIZE + target_winners as usize * Self::RANKED_CANDIDATE_SIZE
    }

    pub fn initialize(
        &mut self,
        giveaway: Pubkey,
        recompute_version: u32,
        target_winners: u32,
        seed: [u8; 32],
        current_time: i64,
    ) {
        self.giveaway = giveaway;
        self.recompute_version = recompute_version;
        self.target_winners = target_winners;
        self.processed_count = 0;
        self.eligible_count = 0;
        self.seed = seed;
        self.candidates = Vec::new();
        self.completed = false;
        self.started_at_unix = current_time;
        self.completed_at_unix = 0;
        self.reserved = [0; 64];
    }

    pub fn record_processed(&mut self) -> Result<()> {
        self.processed_count = self
            .processed_count
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;
        Ok(())
    }

    pub fn record_eligible(&mut self, wallet: Pubkey, rank: [u8; 32]) -> Result<()> {
        self.eligible_count = self
            .eligible_count
            .checked_add(1)
            .ok_or(GiveawayError::MathOverflow)?;

        if self.target_winners == 0 {
            return Ok(());
        }

        if self
            .candidates
            .iter()
            .any(|candidate| candidate.wallet == wallet)
        {
            return Err(GiveawayError::InvalidAccount.into());
        }

        let candidate = RankedCandidate::new(rank, wallet);
        let target = self.target_winners as usize;
        if self.candidates.len() < target {
            self.candidates.push(candidate);
            self.sort_candidates();
            return Ok(());
        }

        if let Some(worst_index) = self.worst_candidate_index() {
            if candidate_precedes(&candidate, &self.candidates[worst_index]) {
                self.candidates[worst_index] = candidate;
                self.sort_candidates();
            }
        }

        Ok(())
    }

    pub fn complete(&mut self, current_time: i64) {
        self.completed = true;
        self.completed_at_unix = current_time;
        self.sort_candidates();
    }

    pub fn winner_wallets(&self) -> Vec<Pubkey> {
        let mut candidates = self.candidates.clone();
        candidates.sort_by(candidate_ordering);
        candidates
            .into_iter()
            .take(self.target_winners as usize)
            .map(|candidate| candidate.wallet)
            .collect()
    }

    fn sort_candidates(&mut self) {
        self.candidates.sort_by(candidate_ordering);
    }

    fn worst_candidate_index(&self) -> Option<usize> {
        self.candidates
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| candidate_ordering(left, right))
            .map(|(index, _)| index)
    }
}

fn candidate_precedes(left: &RankedCandidate, right: &RankedCandidate) -> bool {
    candidate_ordering(left, right).is_lt()
}

fn candidate_ordering(left: &RankedCandidate, right: &RankedCandidate) -> core::cmp::Ordering {
    left.rank
        .cmp(&right.rank)
        .then_with(|| left.wallet.to_bytes().cmp(&right.wallet.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_lowest_ranked_top_k_candidates() {
        let giveaway = Pubkey::new_unique();
        let mut ledger = FinalizationLedger {
            giveaway: Pubkey::default(),
            recompute_version: 0,
            target_winners: 0,
            processed_count: 0,
            eligible_count: 0,
            seed: [0; 32],
            candidates: Vec::new(),
            completed: false,
            started_at_unix: 0,
            completed_at_unix: 0,
            reserved: [0; 64],
        };
        ledger.initialize(giveaway, 1, 2, [9; 32], 100);

        let wallet_high = Pubkey::new_unique();
        let wallet_low = Pubkey::new_unique();
        let wallet_mid = Pubkey::new_unique();
        ledger.record_eligible(wallet_high, [9; 32]).unwrap();
        ledger.record_eligible(wallet_low, [1; 32]).unwrap();
        ledger.record_eligible(wallet_mid, [5; 32]).unwrap();

        let winners = ledger.winner_wallets();
        assert_eq!(winners.len(), 2);
        assert_eq!(winners[0], wallet_low);
        assert_eq!(winners[1], wallet_mid);
        assert!(!winners.contains(&wallet_high));
    }

    #[test]
    fn finalization_progress_counters_support_u64_scale() {
        let giveaway = Pubkey::new_unique();
        let mut ledger = FinalizationLedger {
            giveaway: Pubkey::default(),
            recompute_version: 0,
            target_winners: 0,
            processed_count: u64::from(u32::MAX),
            eligible_count: u64::from(u32::MAX),
            seed: [0; 32],
            candidates: Vec::new(),
            completed: false,
            started_at_unix: 0,
            completed_at_unix: 0,
            reserved: [0; 64],
        };
        ledger.initialize(giveaway, 1, 2, [9; 32], 100);
        ledger.processed_count = u64::from(u32::MAX);
        ledger.eligible_count = u64::from(u32::MAX);

        ledger.record_processed().unwrap();
        ledger
            .record_eligible(Pubkey::new_unique(), [1; 32])
            .unwrap();

        assert_eq!(ledger.processed_count, u64::from(u32::MAX) + 1);
        assert_eq!(ledger.eligible_count, u64::from(u32::MAX) + 1);
    }
}
