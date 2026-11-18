//! BRDU address (ADR-0010): `brdu1…` in bech32m.
//!
//! `address = bech32m("brdu", tagged_hash("brdu/addr", public_key))`. The address holds the **hash** of the key,
//! so it does not reveal the public key until the first signature.

use alloc::string::String;
use core::fmt;

use crate::bech32m;
use crate::error::AddressError;
use crate::hash::{Hash32, tagged, tags};
use crate::sign::PublicKey;

/// Human-readable prefix of BRDU addresses.
pub const HRP: &str = "brdu";

/// The hash that identifies an account in the ledger.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address(pub Hash32);

impl Address {
    /// The address of a public key.
    #[must_use]
    pub fn from_public_key(key: &PublicKey) -> Self {
        Self(tagged(tags::ADDR, &[key.as_bytes()]))
    }

    /// The 32-byte hash the ledger stores.
    #[must_use]
    pub const fn hash(&self) -> &Hash32 {
        &self.0
    }

    /// The `brdu1…` text.
    #[must_use]
    pub fn encode(&self) -> String {
        // the prefix is constant and valid, and the content is 32 bytes: it cannot fail
        bech32m::encode(HRP, self.0.as_bytes()).unwrap_or_default()
    }

    /// Parses and validates a `brdu1…` address.
    pub fn decode(text: &str) -> Result<Self, AddressError> {
        let (hrp, data) = bech32m::decode(text)?;
        if hrp != HRP {
            return Err(AddressError::WrongPrefix);
        }
        let bytes: [u8; 32] = data.try_into().map_err(|_| AddressError::BadSize)?;
        Ok(Self(Hash32(bytes)))
    }

    /// Does the public key match this address?
    #[must_use]
    pub fn matches(&self, key: &PublicKey) -> bool {
        *self == Self::from_public_key(key)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.encode())
    }
}
