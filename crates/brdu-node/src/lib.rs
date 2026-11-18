//! BRDU validator runtime. A **core** thread owns consensus, the mempool and the chain; network threads
//! (one per peer), the HTTP API and accept threads only exchange messages with it over channels. The BFT engine
//! (`brdu-bft`) does no I/O: the clock, disk, network and journal live here.
#![forbid(unsafe_code)]

mod api;
mod core;
mod mempool;
mod net;
mod node;
mod wire;

pub use mempool::{Mempool, PoolError};
pub use node::{Config, Node, NodeError, Timeouts};
pub use wire::{Limits, Wire, kind};
