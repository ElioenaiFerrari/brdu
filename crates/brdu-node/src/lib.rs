//! Runtime do validador do BRDU. Uma thread de **núcleo** é dona do consenso, do mempool e da cadeia; threads de
//! rede (uma por par), de API HTTP e de aceitação só trocam mensagens com ela por canais. O motor BFT
//! (`brdu-bft`) não faz E/S: aqui ficam o relógio, o disco, a rede e o diário.
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
