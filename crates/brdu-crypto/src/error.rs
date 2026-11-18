//! Primitive errors. No error message carries a secret.

use thiserror::Error;

/// Cryptography failures (signature, key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CryptoError {
    /// The signature does not match the key and the message (or is malformed/non-canonical).
    #[error("invalid signature")]
    BadSignature,
    /// The bytes are not a valid public key.
    #[error("invalid public key")]
    BadKey,
}

/// Failures when parsing a `brdu1…` address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AddressError {
    /// Not valid bech32m (wrong checksum, character outside the alphabet, mixed upper and lower case…).
    #[error("malformed address or typo")]
    Malformed,
    /// Valid bech32m, but for another currency (different prefix).
    #[error("address prefix differs from the expected one")]
    WrongPrefix,
    /// The content is not 32 bytes long.
    #[error("invalid address size")]
    BadSize,
}

/// Failures when reading deterministically encoded bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// The bytes ran out before the end of the field.
    #[error("truncated data")]
    Truncated,
    /// Bytes were left over after the last field.
    #[error("trailing bytes after the end")]
    Trailing,
    /// The text field is not valid UTF-8.
    #[error("text is not valid UTF-8")]
    Utf8,
    /// The declared size exceeds the accepted limit (protection against huge allocations).
    #[error("field larger than the limit")]
    TooLong,
}

/// Failures when parsing or building a BRDU amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AmountError {
    /// Empty text or a character that is not part of a number.
    #[error("malformed amount")]
    Malformed,
    /// More than 6 decimal places.
    #[error("at most 6 decimal places")]
    TooManyDecimals,
    /// The value does not fit in 64 bits.
    #[error("amount too large")]
    Overflow,
}
