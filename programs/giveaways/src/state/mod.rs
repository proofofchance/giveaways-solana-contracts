//! # State Module
//!
//! Contains all account state definitions for the giveaways program.
//! Each account type is defined in its own module for better organization.

pub mod config;
pub mod giveaway;
pub mod participant;
pub mod winners_ledger;

pub use config::*;
pub use giveaway::*;
pub use participant::*;
pub use winners_ledger::*;
