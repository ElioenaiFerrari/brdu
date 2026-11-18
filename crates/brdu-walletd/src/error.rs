use thiserror::Error;

/// A wallet error with a stable machine `code` (the UI translates it) and an English message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct WalletError {
    /// Stable code: `locked`, `no_vault`, `vault_exists`, `wrong_password`, `too_many_attempts`, `wrong_passphrase`, `weak_password`,
    /// `invalid_phrase`, `address_mismatch`, `invalid_address`, `invalid_amount`, `insufficient_funds`,
    /// `unknown_prepared_send`, `node_unreachable`, `rejected`, `no_explorer`, `bad_request`, `unknown_method`,
    /// `internal`.
    pub code: &'static str,
    /// Human-readable detail (English).
    pub message: String,
}

impl WalletError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
