//! SHA3-256 hash with a domain prefix (ADR-0011).
//!
//! `tagged(tag, data) = SHA3-256(tag_length (4 bytes, big-endian) ‖ tag ‖ data)`.
//! The prefix guarantees that the hash of one thing (a transaction) is never accepted as that of another (a block).

use alloc::string::String;
use core::fmt;
use sha3::{Digest, Sha3_256};

use crate::hex;

/// Hash length in bytes.
pub const HASH_LEN: usize = 32;

/// BRDU domain prefixes. Each use of a hash has its own.
pub mod tags {
    /// Address: hash of the public key.
    pub const ADDR: &str = "brdu/addr";
    /// Transaction signature (what the sender signs).
    pub const TX: &str = "brdu/tx";
    /// Transaction identifier (hash of the full encoding, including the signature).
    pub const TXID: &str = "brdu/txid";
    /// Document the issuers sign in a mint (`brdu/issuance/mint`) or burn (`brdu/issuance/burn`).
    pub const ISSUANCE_MINT: &str = "brdu/issuance/mint";
    /// Document the issuers sign in a burn.
    pub const ISSUANCE_BURN: &str = "brdu/issuance/burn";
    /// Document the issuers sign in a reserve attestation.
    pub const ATTESTATION: &str = "brdu/attestation";
    /// Leaf of an account in the state root.
    pub const STATE_ACCOUNT: &str = "brdu/state/account";
    /// Leaf of an already-used reference (mint/burn) in the state root.
    pub const STATE_REF: &str = "brdu/state/ref";
    /// Leaf of a module's storage in the state root.
    pub const STATE_EXT: &str = "brdu/state/ext";
    /// State scalars (supply, attestation, height, time).
    pub const STATE_META: &str = "brdu/state/meta";
    /// Block hash.
    pub const BLOCK: &str = "brdu/block";
    /// State root.
    pub const STATE: &str = "brdu/state";
    /// Merkle leaf.
    pub const MERKLE_LEAF: &str = "brdu/merkle/leaf";
    /// Merkle internal node.
    pub const MERKLE_NODE: &str = "brdu/merkle/node";
    /// Root of an empty Merkle tree.
    pub const MERKLE_EMPTY: &str = "brdu/merkle/empty";
}

/// A 32-byte hash. `Debug` and `Display` show hexadecimal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash32(pub [u8; HASH_LEN]);

impl Hash32 {
    /// 32 zero bytes (the "previous hash" of the first block).
    pub const ZERO: Self = Self([0u8; HASH_LEN]);

    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    /// Lowercase hexadecimal.
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Parses 64 hexadecimal characters.
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        hex::decode_array::<HASH_LEN>(text).map(Self)
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Plain SHA3-256.
#[must_use]
pub fn sha3(data: &[u8]) -> Hash32 {
    Hash32(Sha3_256::digest(data).into())
}

/// SHA3-256 with a domain prefix, over the concatenation of `parts` (without copying everything into a buffer).
#[must_use]
pub fn tagged(tag: &str, parts: &[&[u8]]) -> Hash32 {
    let mut h = Sha3_256::new();
    // the tag length never exceeds u32: tags are short constants; `try_from` avoids a silent `as`
    let len = u32::try_from(tag.len()).unwrap_or(u32::MAX);
    h.update(len.to_be_bytes());
    h.update(tag.as_bytes());
    for p in parts {
        h.update(p);
    }
    Hash32(h.finalize().into())
}
