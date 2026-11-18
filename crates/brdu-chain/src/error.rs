use brdu_bft::CertError;
use brdu_crypto::DecodeError;
use brdu_ledger::LedgerError;
use thiserror::Error;

/// Erros da cadeia.
#[derive(Debug, Error, PartialEq)]
pub enum ChainError {
    /// Gênese inválida.
    #[error("invalid genesis: {0}")]
    Genesis(&'static str),
    /// Bloco malformado ou fora de regra.
    #[error("bad block: {0}")]
    Block(&'static str),
    /// Certificado inválido.
    #[error("bad certificate: {0}")]
    Certificate(#[from] CertError),
    /// O ledger recusou o bloco.
    #[error("ledger: {0}")]
    Ledger(#[from] LedgerError),
    /// Bytes malformados.
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
}
