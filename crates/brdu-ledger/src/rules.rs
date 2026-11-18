//! Network rules, fixed at genesis: fees, limits and issuers (ADR-0002, 0008, 0014).

use brdu_crypto::PublicKey;

use crate::error::LedgerError;

/// Immutable rules of a network. State changes; rules do not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// Network identifier (goes into every signature: prevents replaying a transaction on another network).
    pub chain_id: String,
    /// Base minimum fee of a transfer, in minimum units (default 1,000 = 0.001 BRDU).
    pub transfer_base: u64,
    /// Minimum fee per byte of the encoded transaction (default 1).
    pub per_byte: u64,
    /// Transactions per block (default 1,000).
    pub max_block_txs: usize,
    /// Bytes of transactions per block (default 1,000,000).
    pub max_block_bytes: usize,
    /// The issuers (public keys, no repeats, in order).
    issuers: Vec<PublicKey>,
    /// How many distinct issuer approvals are required (M of N).
    issuer_threshold: usize,
}

impl Rules {
    /// Default base fee (0.001 BRDU).
    pub const DEFAULT_TRANSFER_BASE: u64 = 1_000;
    /// Default fee per byte.
    pub const DEFAULT_PER_BYTE: u64 = 1;
    /// Default transactions per block.
    pub const DEFAULT_MAX_BLOCK_TXS: usize = 1_000;
    /// Default bytes per block.
    pub const DEFAULT_MAX_BLOCK_BYTES: usize = 1_000_000;

    /// Rules with the defaults, for `chain_id`, `issuers` and the threshold M. Fails if the threshold is not in
    /// `1..=N` or if there are no issuers.
    pub fn new(
        chain_id: &str,
        issuers: &[PublicKey],
        issuer_threshold: usize,
    ) -> Result<Self, LedgerError> {
        let mut sorted: Vec<PublicKey> = issuers.to_vec();
        sorted.sort();
        sorted.dedup();
        if sorted.is_empty()
            || issuer_threshold == 0
            || issuer_threshold > sorted.len()
            || chain_id.is_empty()
        {
            return Err(LedgerError::BadRules);
        }
        Ok(Self {
            chain_id: chain_id.to_string(),
            transfer_base: Self::DEFAULT_TRANSFER_BASE,
            per_byte: Self::DEFAULT_PER_BYTE,
            max_block_txs: Self::DEFAULT_MAX_BLOCK_TXS,
            max_block_bytes: Self::DEFAULT_MAX_BLOCK_BYTES,
            issuers: sorted,
            issuer_threshold,
        })
    }

    /// The issuers.
    #[must_use]
    pub fn issuers(&self) -> &[PublicKey] {
        &self.issuers
    }

    /// The threshold M.
    #[must_use]
    pub const fn issuer_threshold(&self) -> usize {
        self.issuer_threshold
    }

    /// Is `key` an issuer?
    #[must_use]
    pub fn is_issuer(&self, key: &PublicKey) -> bool {
        self.issuers.binary_search(key).is_ok()
    }
}
