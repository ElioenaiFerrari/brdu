//! Encrypted vault for the wallet recovery phrase.
//!
//! - The phrase is sealed with **ChaCha20-Poly1305** under a key derived from the password with **Argon2id**
//!   (memory-hard). The KDF parameters and the salt travel in the file, so they can be raised later.
//! - The header (version, KDF parameters, salt, **account address**) is the AEAD associated data: tampering with
//!   any of it fails authentication.
//! - The BIP-39 **passphrase (25th word) is never stored**. The vault keeps the public address of the account it was
//!   made for, so that unlocking with the wrong passphrase is detected by the caller (derived address != stored).
//! - Secrets are zeroized on drop. Not audited.
#![forbid(unsafe_code)]

use argon2::{Algorithm, Argon2, Params, Version};
use brdu_crypto::hex;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

/// Current file format version.
pub const VERSION: u32 = 1;
/// Default Argon2id memory cost (KiB): 64 MiB.
pub const DEFAULT_M_KIB: u32 = 64 * 1024;
/// Default Argon2id time cost (passes).
pub const DEFAULT_T: u32 = 3;
/// Default Argon2id parallelism.
pub const DEFAULT_P: u32 = 1;
/// Smallest password accepted when sealing.
pub const MIN_PASSWORD_LEN: usize = 8;
/// Upper bounds accepted when OPENING a file (a hostile file must not make us allocate gigabytes).
const MAX_M_KIB: u32 = 1024 * 1024;
const MAX_T: u32 = 16;
const MAX_P: u32 = 16;

/// Vault errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    /// The password is too short.
    #[error("password must have at least {MIN_PASSWORD_LEN} characters")]
    WeakPassword,
    /// Wrong password, or the file was tampered with (the two are indistinguishable by design).
    #[error("wrong password or corrupted vault")]
    Decrypt,
    /// The file is not a valid vault.
    #[error("malformed vault file: {0}")]
    Malformed(&'static str),
    /// A version this build does not know.
    #[error("unsupported vault version {0}")]
    Version(u32),
    /// Key derivation failed (invalid parameters).
    #[error("key derivation failed")]
    Kdf,
    /// No randomness from the OS.
    #[error("no system randomness")]
    Random,
}

/// Argon2id parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_kib: DEFAULT_M_KIB,
            t: DEFAULT_T,
            p: DEFAULT_P,
        }
    }
}

/// The sealed vault as stored on disk (JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vault {
    /// Format version.
    pub version: u32,
    /// KDF name (always `argon2id`).
    pub kdf: String,
    /// KDF parameters.
    pub params: KdfParams,
    /// Salt (hex).
    pub salt: String,
    /// The `brdu1…` address of the account this vault belongs to (public).
    pub address: String,
    /// AEAD nonce (hex).
    pub nonce: String,
    /// Ciphertext + tag (hex).
    pub ciphertext: String,
}

fn random<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| VaultError::Random)?;
    Ok(b)
}

fn derive(password: &str, salt: &[u8], p: &KdfParams) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    if p.m_kib > MAX_M_KIB || p.t > MAX_T || p.p > MAX_P {
        return Err(VaultError::Malformed("KDF parameters out of range"));
    }
    let params = Params::new(p.m_kib, p.t, p.p, Some(32)).map_err(|_| VaultError::Kdf)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| VaultError::Kdf)?;
    Ok(key)
}

/// The bytes authenticated alongside the ciphertext: everything in the header.
fn aad(v: &Vault) -> Vec<u8> {
    format!(
        "brdu-vault/{}/{}/{}/{}/{}/{}/{}",
        v.version, v.kdf, v.params.m_kib, v.params.t, v.params.p, v.salt, v.address
    )
    .into_bytes()
}

impl Vault {
    /// Seals `secret` (the recovery phrase) for the account at `address`, with the default KDF cost.
    pub fn seal(secret: &str, password: &str, address: &str) -> Result<Self, VaultError> {
        Self::seal_with(secret, password, address, KdfParams::default())
    }

    /// Like [`Vault::seal`] with explicit KDF parameters (tests use cheap ones).
    pub fn seal_with(
        secret: &str,
        password: &str,
        address: &str,
        params: KdfParams,
    ) -> Result<Self, VaultError> {
        if password.chars().count() < MIN_PASSWORD_LEN {
            return Err(VaultError::WeakPassword);
        }
        let salt = random::<16>()?;
        let nonce = random::<12>()?;
        let mut v = Self {
            version: VERSION,
            kdf: "argon2id".into(),
            params,
            salt: hex::encode(&salt),
            address: address.to_string(),
            nonce: hex::encode(&nonce),
            ciphertext: String::new(),
        };
        let key = derive(password, &salt, &params)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: secret.as_bytes(),
                    aad: &aad(&v),
                },
            )
            .map_err(|_| VaultError::Decrypt)?;
        v.ciphertext = hex::encode(&ct);
        Ok(v)
    }

    /// Opens the vault: the recovery phrase, zeroized on drop.
    pub fn open(&self, password: &str) -> Result<Zeroizing<String>, VaultError> {
        if self.version != VERSION {
            return Err(VaultError::Version(self.version));
        }
        if self.kdf != "argon2id" {
            return Err(VaultError::Malformed("unknown KDF"));
        }
        let salt = hex::decode(&self.salt).ok_or(VaultError::Malformed("salt"))?;
        if salt.len() < 8 || salt.len() > 64 {
            return Err(VaultError::Malformed("salt length"));
        }
        let nonce = hex::decode_array::<12>(&self.nonce).ok_or(VaultError::Malformed("nonce"))?;
        let ct = hex::decode(&self.ciphertext).ok_or(VaultError::Malformed("ciphertext"))?;
        let key = derive(password, &salt, &self.params)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
        let mut plain = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ct,
                    aad: &aad(self),
                },
            )
            .map_err(|_| VaultError::Decrypt)?;
        let text = String::from_utf8(plain.clone())
            .map_err(|_| VaultError::Malformed("plaintext is not UTF-8"));
        plain.zeroize();
        text.map(Zeroizing::new)
    }

    /// Serializes to the on-disk JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Parses the on-disk JSON.
    pub fn from_json(text: &str) -> Result<Self, VaultError> {
        serde_json::from_str(text).map_err(|_| VaultError::Malformed("not a vault JSON"))
    }
}
