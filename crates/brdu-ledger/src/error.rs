//! Ledger errors. No message carries a secret.

use brdu_crypto::{DecodeError, Hash32};
use thiserror::Error;

/// Why a transaction or a block was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LedgerError {
    // --- transaction (shape and signature)
    /// Bytes that are not a transaction.
    #[error("malformed transaction")]
    Malformed,
    /// The payload exceeds the limit.
    #[error("payload too large")]
    PayloadTooLarge,
    /// The signature does not match.
    #[error("invalid signature")]
    BadSignature,
    /// The transaction is for another network.
    #[error("wrong network")]
    WrongChain,
    /// Transaction kind this network does not know.
    #[error("unknown transaction kind: {0}")]
    UnknownKind(u8),
    // --- accounts and fees
    /// The nonce is not the expected one (replay or out-of-order transaction).
    #[error("wrong nonce: expected {expected}, got {got}")]
    BadNonce {
        /// The account's next nonce.
        expected: u64,
        /// What came in the transaction.
        got: u64,
    },
    /// The fee is lower than the minimum.
    #[error("fee below minimum: minimum {min}, got {got}")]
    FeeTooLow {
        /// The minimum fee.
        min: u64,
        /// The one that came.
        got: u64,
    },
    /// Balance lower than required.
    #[error("insufficient funds")]
    InsufficientFunds,
    /// The account would exceed 64 bits.
    #[error("64-bit overflow")]
    Overflow,
    // --- coin
    /// Zero amount.
    #[error("zero amount")]
    ZeroAmount,
    /// The deposit reference was already used.
    #[error("reference already used")]
    RefUsed,
    /// Empty or too-long reference.
    #[error("invalid reference")]
    BadRef,
    /// Fewer than M distinct issuer approvals.
    #[error("insufficient issuer approvals: {valid} of {needed}")]
    NotEnoughApprovals {
        /// How many approvals are valid and distinct.
        valid: usize,
        /// How many are required.
        needed: usize,
    },
    /// Only the account owner can burn their own balance.
    #[error("only the owner burns their own balance")]
    NotOwner,
    /// The attestation sequence number is not the next one.
    #[error("wrong attestation sequence: expected {expected}, got {got}")]
    BadSequence {
        /// The expected sequence.
        expected: u64,
        /// The one that came.
        got: u64,
    },
    // --- block
    /// The block is not the next one (height or previous hash).
    #[error("block out of place")]
    BadLinkage,
    /// The block time goes backwards.
    #[error("block time earlier than the state time")]
    TimeWentBack,
    /// More transactions or bytes than the limit.
    #[error("block too large")]
    BlockTooLarge,
    /// The same transaction twice in the block.
    #[error("duplicate transaction in block")]
    DuplicateTx(Hash32),
    /// The header's state root does not match the execution.
    #[error("state root differs from the computed one")]
    StateRootMismatch,
    /// A transaction in the block is invalid (the whole block is rejected).
    #[error("transaction {id} invalid: {reason}")]
    InvalidTx {
        /// Which one.
        id: Hash32,
        /// Why.
        reason: Box<LedgerError>,
    },
    /// Failure reading encoded bytes.
    #[error("decoding: {0}")]
    Decode(#[from] DecodeError),
    /// A module rule rejected it (assets, credentials, contracts modules).
    #[error("module: {0}")]
    Module(&'static str),
    /// Invalid network rules (threshold outside 1..=issuers, for example).
    #[error("invalid network rules")]
    BadRules,
}

impl LedgerError {
    /// A short, stable label, to count rejections by reason (gossip, metrics).
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::PayloadTooLarge => "payload_too_large",
            Self::BadSignature => "bad_signature",
            Self::WrongChain => "wrong_chain",
            Self::UnknownKind(_) => "unknown_kind",
            Self::BadNonce { .. } => "bad_nonce",
            Self::FeeTooLow { .. } => "fee_too_low",
            Self::InsufficientFunds => "insufficient_funds",
            Self::Overflow => "overflow",
            Self::ZeroAmount => "zero_amount",
            Self::RefUsed => "ref_used",
            Self::BadRef => "bad_ref",
            Self::NotEnoughApprovals { .. } => "not_enough_approvals",
            Self::NotOwner => "not_owner",
            Self::BadSequence { .. } => "bad_sequence",
            Self::BadLinkage => "bad_linkage",
            Self::TimeWentBack => "time_went_back",
            Self::BlockTooLarge => "block_too_large",
            Self::DuplicateTx(_) => "duplicate_tx",
            Self::StateRootMismatch => "state_root_mismatch",
            Self::InvalidTx { .. } => "invalid_tx",
            Self::Decode(_) => "decode",
            Self::Module(_) => "module",
            Self::BadRules => "bad_rules",
        }
    }
}
