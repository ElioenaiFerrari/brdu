//! BRDU ledger: the state machine that consensus replicates.
//!
//! - [`Tx`]: signed transaction (same format as BRDC), with fee and nonce.
//! - [`State`]: accounts, supply, references, reserve attestation and module storage, with a **Merkle
//!   root** and atomic undo.
//! - [`Block`] and [`Executor`]: a whole block or nothing; fee split among the validators; state root checked.
//! - [`coin`]: transfer, mint, burn and reserve attestation (M-of-N issuers); coverage as information only.
//! - [`Module`]: extension point for assets, credentials and contracts (milestone M9).
//!
//! Pure: no I/O, no network, no clock. **Not audited.**
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod block;
pub mod coin;
pub mod error;
pub mod exec;
pub mod gov;
pub mod rules;
pub mod state;
pub mod tx;

pub use block::Block;
pub use error::LedgerError;
pub use exec::{BlockReceipt, Executor, Module, TxCtx};
pub use rules::Rules;
pub use state::{Account, Attestation, Coverage, State};
pub use tx::{Tx, kind};
