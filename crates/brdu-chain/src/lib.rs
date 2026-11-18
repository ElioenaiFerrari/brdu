//! The BRDU chain: **genesis** (network, validators, issuers, rules), **finalized blocks** (block + commit
//! certificate) and a **verified replica** that only accepts a block after checking the certificate (more than 2/3 of the
//! validators), the round proposer, the set that splits the fee and the exact ledger execution. Whoever
//! syncs (node, wallet, explorer) does not need to trust whoever delivered the block.
#![forbid(unsafe_code)]

mod chain;
mod error;
mod genesis;
mod store;

pub use chain::{Chain, DecidedBlock};
pub use error::ChainError;
pub use genesis::Genesis;
pub use store::RecordLog;
