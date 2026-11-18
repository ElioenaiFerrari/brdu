//! Authenticated, encrypted links between validators (BRDU ADR-0013). Transport only: no consensus rule
//! lives here. A link is a Noise XX session (`snow`) whose static key is **bound to the node's Ed25519 consensus key**
//! by a signature inside the handshake: whoever is on the other end is provably the institution the
//! validator set lists, **on this network and this genesis**.

#![forbid(unsafe_code)]

mod error;
mod handshake;
mod identity;
mod limits;
mod records;
mod session;
#[cfg(test)]
mod tests;

pub use error::NetError;
pub use handshake::{accept, connect};
pub use identity::{IdentityProof, NetIdentity, PeerPolicy};
pub use limits::{ConnGuard, ConnLimiter, Reputation, TokenBucket};
pub use records::{MAX_LIFETIME_SECS, MergeOutcome, PeerBook, PeerRecord, valid_addr};
pub use session::{HANDSHAKE_TIMEOUT, Session};
