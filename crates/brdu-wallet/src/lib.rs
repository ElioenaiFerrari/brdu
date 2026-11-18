//! BRDU wallet (ADR-0005, 0021): BIP-39 phrase, SLIP-10 Ed25519 derivation and accounts.
//!
//! - **The phrase** has 24 words (12 is also accepted on import). The **passphrase** is the "25th word": the same phrase
//!   with a different passphrase yields a **different wallet**, valid and empty, with no error at all. That is why import asks
//!   for confirmation of the address (see [`Wallet::address`]).
//! - **Derivation** is SLIP-10 for Ed25519, with all indices hardened: `m/44'/1112687701'/account'/0'/index'`.
//!   `1112687701` is "BRDU" in ASCII (0x42524455) and is **not registered in SLIP-44**.
//! - **Secrets** live in `Zeroizing` buffers, with no `Debug` or `Clone`. The phrase only leaves here to be shown to
//!   the owner, once. **Not audited.**
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::fmt;

use bip39::{Language, Mnemonic};
use brdu_crypto::{Address, PublicKey, SecretKey};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use thiserror::Error;
use zeroize::Zeroizing;

/// "BRDU" in ASCII, used as the coin type in the derivation path.
pub const COIN_TYPE: u32 = 0x4252_4455;
/// Longest accepted passphrase (printable ASCII only, so as not to depend on Unicode normalization).
pub const MAX_PASSPHRASE: usize = 64;
const HARDENED: u32 = 0x8000_0000;

/// Wallet failures. No message carries a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WalletError {
    /// Could not obtain system entropy.
    #[error("no system entropy")]
    Entropy,
    /// Wrong number of words, a word outside the list, or an invalid checksum.
    #[error("invalid phrase")]
    InvalidPhrase,
    /// Passphrase with a character outside printable ASCII, or too long.
    #[error("invalid passphrase")]
    InvalidPassphrase,
    /// Malformed derivation path or a non-hardened index.
    #[error("invalid derivation path")]
    BadPath,
}

/// The recovery phrase. `Debug` is redacted; it is never `Display`ed or logged.
pub struct Phrase {
    mnemonic: Zeroizing<String>,
}

impl fmt::Debug for Phrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Phrase(<redacted>)")
    }
}

impl Phrase {
    /// Generates a new 24-word phrase with 32 bytes of system entropy.
    pub fn generate() -> Result<Self, WalletError> {
        let mut entropy = Zeroizing::new([0u8; 32]);
        getrandom::fill(entropy.as_mut_slice()).map_err(|_| WalletError::Entropy)?;
        Self::from_entropy(&entropy)
    }

    /// A 24-word phrase from 32 bytes of entropy (for tests and vectors).
    pub fn from_entropy(entropy: &[u8; 32]) -> Result<Self, WalletError> {
        let m = Mnemonic::from_entropy_in(Language::English, entropy)
            .map_err(|_| WalletError::InvalidPhrase)?;
        Ok(Self {
            mnemonic: Zeroizing::new(m.to_string()),
        })
    }

    /// Parses typed words: 12 or 24, all from the English BIP-39 list, with a valid checksum. Forgives upper case and
    /// extra spaces.
    pub fn parse(text: &str) -> Result<Self, WalletError> {
        let words: Vec<String> = text
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        if words.len() != 12 && words.len() != 24 {
            return Err(WalletError::InvalidPhrase);
        }
        let joined = Zeroizing::new(words.join(" "));
        Mnemonic::parse_in_normalized(Language::English, joined.as_str())
            .map_err(|_| WalletError::InvalidPhrase)?;
        Ok(Self { mnemonic: joined })
    }

    /// The words, to show to the owner once (on the creation screen). Whoever calls this must not log them.
    #[must_use]
    pub fn words(&self) -> Vec<&str> {
        self.mnemonic.split(' ').collect()
    }

    /// How many words.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words().len()
    }

    /// No words? (never happens in a valid phrase)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mnemonic.is_empty()
    }

    /// The 64-byte BIP-39 seed (PBKDF2-HMAC-SHA512, 2048 rounds, salt `"mnemonic" + passphrase`).
    fn seed(&self, passphrase: &str) -> Result<Zeroizing<[u8; 64]>, WalletError> {
        check_passphrase(passphrase)?;
        let m = Mnemonic::parse_in_normalized(Language::English, self.mnemonic.as_str())
            .map_err(|_| WalletError::InvalidPhrase)?;
        Ok(Zeroizing::new(m.to_seed_normalized(passphrase)))
    }

    /// The wallet of this phrase with this passphrase (empty if there is none).
    pub fn wallet(&self, passphrase: &str) -> Result<Wallet, WalletError> {
        Ok(Wallet {
            seed: self.seed(passphrase)?,
        })
    }
}

fn check_passphrase(p: &str) -> Result<(), WalletError> {
    if p.len() > MAX_PASSPHRASE || !p.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err(WalletError::InvalidPassphrase);
    }
    Ok(())
}

/// The wallet: derives accounts from a seed. `Debug` is redacted and there is no `Clone`.
pub struct Wallet {
    seed: Zeroizing<[u8; 64]>,
}

impl fmt::Debug for Wallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Wallet(<redacted>)")
    }
}

impl Wallet {
    /// The derivation path of account `account` and index `index`.
    #[must_use]
    pub fn path(account: u32, index: u32) -> String {
        format!("m/44'/{COIN_TYPE}'/{account}'/0'/{index}'")
    }

    /// The account at the standard path.
    pub fn account(&self, account: u32, index: u32) -> Result<Account, WalletError> {
        self.derive(&Self::path(account, index))
    }

    /// The account at `path` (all indices hardened, e.g. `m/0'`). Also serves the SLIP-10 vectors.
    pub fn derive(&self, path: &str) -> Result<Account, WalletError> {
        let node = derive_path(self.seed.as_slice(), path)?;
        let secret = SecretKey::from_bytes(*node.key);
        let public = secret.public();
        Ok(Account {
            path: path.to_string(),
            address: Address::from_public_key(&public),
            public,
            secret,
        })
    }

    /// The wallet root (key and chain code of the master node), for the vectors.
    pub fn master(&self) -> Result<Node, WalletError> {
        master_node(self.seed.as_slice())
    }
}

/// A derived account: path, public key, address and the private key.
pub struct Account {
    path: String,
    address: Address,
    public: PublicKey,
    secret: SecretKey,
}

impl Account {
    /// The derivation path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The `brdu1…` address.
    #[must_use]
    pub const fn address(&self) -> &Address {
        &self.address
    }

    /// The public key.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// The private key (for signing). Never `Debug`.
    #[must_use]
    pub const fn secret_key(&self) -> &SecretKey {
        &self.secret
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Account({}, {})", self.path, self.address)
    }
}

/// A SLIP-10 node: a 32-byte key and a 32-byte chain code.
pub struct Node {
    /// The node's private key.
    pub key: Zeroizing<[u8; 32]>,
    /// The chain code.
    pub chain_code: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Node(<redacted>)")
    }
}

type HmacSha512 = Hmac<Sha512>;

fn split(mac: &[u8]) -> Result<Node, WalletError> {
    let (k, c) = mac.split_at_checked(32).ok_or(WalletError::BadPath)?;
    Ok(Node {
        key: Zeroizing::new(k.try_into().map_err(|_| WalletError::BadPath)?),
        chain_code: Zeroizing::new(c.try_into().map_err(|_| WalletError::BadPath)?),
    })
}

/// SLIP-10 master node for Ed25519: `HMAC-SHA512("ed25519 seed", seed)`.
fn master_node(seed: &[u8]) -> Result<Node, WalletError> {
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed").map_err(|_| WalletError::BadPath)?;
    mac.update(seed);
    split(&mac.finalize().into_bytes())
}

/// Hardened child: `HMAC-SHA512(chain_code, 0x00 ‖ key ‖ big_endian_index)`.
fn child(parent: &Node, index: u32) -> Result<Node, WalletError> {
    let mut mac = HmacSha512::new_from_slice(parent.chain_code.as_slice())
        .map_err(|_| WalletError::BadPath)?;
    mac.update(&[0x00]);
    mac.update(parent.key.as_slice());
    mac.update(&(index | HARDENED).to_be_bytes());
    split(&mac.finalize().into_bytes())
}

/// Derives a SLIP-10 Ed25519 node from **any** seed (not only 64 bytes) and a hardened path.
/// It exists for the official SLIP-10 vectors (16-byte seed); the wallet uses [`Wallet::derive`].
pub fn slip10_derive(seed: &[u8], path: &str) -> Result<Node, WalletError> {
    derive_path(seed, path)
}

fn derive_path(seed: &[u8], path: &str) -> Result<Node, WalletError> {
    let mut parts = path.split('/');
    if parts.next() != Some("m") {
        return Err(WalletError::BadPath);
    }
    let mut node = master_node(seed)?;
    for part in parts {
        // hardened indices only (`44'`): Ed25519 has no public derivation
        let digits = part.strip_suffix('\'').ok_or(WalletError::BadPath)?;
        let index: u32 = digits.parse().map_err(|_| WalletError::BadPath)?;
        if index >= HARDENED {
            return Err(WalletError::BadPath);
        }
        node = child(&node, index)?;
    }
    Ok(node)
}
