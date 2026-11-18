//! BRDU cryptographic primitives (ADR-0005, 0010, 0011).
//!
//! - [`hash`]: SHA3-256 with a domain prefix.
//! - [`sign`]: strict Ed25519, private key without `Debug` and zeroed when dropped from memory.
//! - [`address`] and [`bech32m`]: `brdu1…` addresses with a checksum.
//! - [`encoding`]: deterministic message encoding.
//! - [`amount`]: amounts in minimum units (6 decimal places), no floating point.
//! - [`merkle`]: Merkle tree with separated domains.
//!
//! `no_std` with `alloc`: serves the node, the desktop wallet and, in the future, the browser (WASM). **Not audited.**
#![cfg_attr(not(test), no_std)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

extern crate alloc;

pub mod address;
pub mod amount;
pub mod bech32m;
pub mod encoding;
pub mod error;
pub mod hash;
pub mod hex;
pub mod merkle;
pub mod sign;

pub use address::Address;
pub use amount::Amount;
pub use error::{AddressError, AmountError, CryptoError, DecodeError};
pub use hash::{Hash32, sha3, tagged, tags};
pub use sign::{PublicKey, SecretKey, Signature};
