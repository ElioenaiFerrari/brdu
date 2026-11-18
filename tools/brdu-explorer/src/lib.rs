//! BRDU explorer. Keeps a **verified replica** of the chain (every block checked: certificate of more than
//! 2/3, proposer, signers, execution and state root), fed by any node, and serves it as a web page and a
//! **read-only** JSON API. It does not trust the node: a lying node is rejected and reported on screen.
#![forbid(unsafe_code)]

pub mod http;
pub mod index;
pub mod server;
pub mod sync;
pub mod view;

pub use index::{Explorer, SyncStatus};
pub use server::{Shared, serve};
pub use sync::{spawn_sync, sync_once};
