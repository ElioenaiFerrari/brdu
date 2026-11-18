use brdu_bft::CertError;
use brdu_crypto::DecodeError;
use brdu_ledger::LedgerError;
use thiserror::Error;

/// Chain errors.
#[derive(Debug, Error, PartialEq)]
pub enum ChainError {
    /// Invalid genesis.
    #[error("invalid genesis: {0}")]
    Genesis(&'static str),
    /// Malformed block or one that breaks the rules.
    #[error("bad block: {0}")]
    Block(&'static str),
    /// Invalid certificate.
    #[error("bad certificate: {0}")]
    Certificate(#[from] CertError),
    /// The ledger rejected the block.
    #[error("ledger: {0}")]
    Ledger(#[from] LedgerError),
    /// Malformed bytes.
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
}
