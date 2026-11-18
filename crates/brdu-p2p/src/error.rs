use thiserror::Error;

/// Transport errors.
#[derive(Debug, Error)]
pub enum NetError {
    /// I/O.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// The handshake failed: bad message, bad proof, another network or genesis, key not accepted.
    #[error("handshake refused: {0}")]
    Handshake(String),
    /// A frame failed to decrypt (tampered, replayed, out of order).
    #[error("bad frame")]
    BadFrame,
    /// Message larger than the limit for its type.
    #[error("message too large")]
    TooLarge,
    /// Deadline exceeded.
    #[error("timed out")]
    Timeout,
    /// Refused (limits, policy).
    #[error("refused: {0}")]
    Refused(String),
}
