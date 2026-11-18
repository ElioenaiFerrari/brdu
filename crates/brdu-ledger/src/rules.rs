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
    /// Root authorities: create credential types and asset classes (M of N). Empty = none can be created.
    pub authorities: Vec<PublicKey>,
    /// How many distinct authority approvals are required (default 1).
    pub authority_threshold: usize,
    /// Smallest contest/veto window a type may declare, in seconds (default 60).
    pub min_window: u64,
    /// Largest window, in seconds (default one year).
    pub max_window: u64,
    /// Minimum fee to register a credential type or an asset class (default 0.05 BRDU).
    pub registration_fee: u64,
    /// Minimum fee to issue or revoke a credential (default 0.01 BRDU).
    pub credential_fee: u64,
    /// Minimum fee of each step of an asset trade: offer, accept, cancel, finalize (default 0.02 BRDU).
    pub step_fee: u64,
    /// Minimum fee of each step of a contract between parties (default 0.01 BRDU).
    pub contract_fee: u64,
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
    /// Default smallest window (seconds).
    pub const DEFAULT_MIN_WINDOW: u64 = 60;
    /// Default largest window (seconds): one year.
    pub const DEFAULT_MAX_WINDOW: u64 = 31_536_000;
    /// Default registration fee (0.05 BRDU).
    pub const DEFAULT_REGISTRATION_FEE: u64 = 50_000;
    /// Default credential fee (0.01 BRDU).
    pub const DEFAULT_CREDENTIAL_FEE: u64 = 10_000;
    /// Default fee of an asset trade step (0.02 BRDU).
    pub const DEFAULT_STEP_FEE: u64 = 20_000;
    /// Default fee of a contract step (0.01 BRDU).
    pub const DEFAULT_CONTRACT_FEE: u64 = 10_000;

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
            authorities: Vec::new(),
            authority_threshold: 1,
            min_window: Self::DEFAULT_MIN_WINDOW,
            max_window: Self::DEFAULT_MAX_WINDOW,
            registration_fee: Self::DEFAULT_REGISTRATION_FEE,
            credential_fee: Self::DEFAULT_CREDENTIAL_FEE,
            step_fee: Self::DEFAULT_STEP_FEE,
            contract_fee: Self::DEFAULT_CONTRACT_FEE,
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
