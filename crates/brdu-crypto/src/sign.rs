//! Ed25519 (RFC 8032) with strict verification.
//!
//! - The private key never implements `Debug`, `Display`, `Clone` or `Serialize`, and is zeroed when dropped from memory.
//! - Verification is **strict** (`verify_strict`): it rejects non-canonical signatures and small-order keys,
//!   so the same message does not have two valid signatures.

use core::fmt;
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey, VerifyingKey};
use zeroize::Zeroizing;

use crate::error::CryptoError;
use crate::hash::tagged;
use crate::hex;

/// Ed25519 private key (32-byte seed). Secret: `Debug` is redacted.
pub struct SecretKey(Zeroizing<[u8; 32]>);

impl SecretKey {
    /// Builds the key from a 32-byte seed (for example, from a SLIP-10 derivation).
    #[must_use]
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        Self(Zeroizing::new(seed))
    }

    /// The corresponding public key.
    #[must_use]
    pub fn public(&self) -> PublicKey {
        PublicKey(SigningKey::from_bytes(&self.0).verifying_key().to_bytes())
    }

    /// Signs `message` (deterministic).
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(SigningKey::from_bytes(&self.0).sign(message).to_bytes())
    }

    /// Signs the tagged hash of `data` with the domain prefix `tag` (this is how BRDU signs everything).
    #[must_use]
    pub fn sign_tagged(&self, tag: &str, data: &[u8]) -> Signature {
        self.sign(tagged(tag, &[data]).as_bytes())
    }

    /// Exposes the 32 seed bytes. **Only** for whoever needs to store the encrypted secret (the wallet vault).
    #[must_use]
    pub fn expose_seed(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// Ed25519 public key (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hexadecimal.
    #[must_use]
    pub fn to_hex(&self) -> alloc::string::String {
        hex::encode(&self.0)
    }

    /// Verifies `signature` over `message` (strict).
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), CryptoError> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| CryptoError::BadKey)?;
        let sig = DalekSignature::from_bytes(&signature.0);
        key.verify_strict(message, &sig)
            .map_err(|_| CryptoError::BadSignature)
    }

    /// Verifies a signature made with [`SecretKey::sign_tagged`].
    pub fn verify_tagged(
        &self,
        tag: &str,
        data: &[u8],
        signature: &Signature,
    ) -> Result<(), CryptoError> {
        self.verify(tagged(tag, &[data]).as_bytes(), signature)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.to_hex())
    }
}

/// Ed25519 signature (64 bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", hex::encode(&self.0))
    }
}

/// Verifies several signatures at once. With the `batch` feature it uses dalek's batch verification (much
/// faster for a full block); without it, it checks them one by one. In any case, **all** must be valid.
///
/// Note: dalek's batch verification is **not strict** (it accepts what the strict one would reject). So, when
/// the batch passes, BRDU still checks each item in strict mode if `strict` is `true` (the node default).
pub fn verify_all(
    items: &[(&PublicKey, &[u8], &Signature)],
    strict: bool,
) -> Result<(), CryptoError> {
    #[cfg(feature = "batch")]
    {
        if !strict {
            return verify_batch(items);
        }
    }
    #[cfg(not(feature = "batch"))]
    let _ = strict;
    items
        .iter()
        .try_for_each(|(pk, msg, sig)| pk.verify(msg, sig))
}

#[cfg(feature = "batch")]
fn verify_batch(items: &[(&PublicKey, &[u8], &Signature)]) -> Result<(), CryptoError> {
    let mut keys = alloc::vec::Vec::with_capacity(items.len());
    let mut sigs = alloc::vec::Vec::with_capacity(items.len());
    let mut msgs = alloc::vec::Vec::with_capacity(items.len());
    for (pk, msg, sig) in items {
        keys.push(VerifyingKey::from_bytes(&pk.0).map_err(|_| CryptoError::BadKey)?);
        sigs.push(DalekSignature::from_bytes(&sig.0));
        msgs.push(*msg);
    }
    ed25519_dalek::verify_batch(&msgs, &sigs, &keys).map_err(|_| CryptoError::BadSignature)
}
