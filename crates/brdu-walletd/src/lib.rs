//! The wallet service: everything a wallet UI needs, with the keys kept inside this process.
//!
//! Principles (ADR-0018):
//! - The recovery phrase is shown **once**, at creation; it is stored only inside the encrypted vault.
//! - The BIP-39 passphrase is **never stored**; the vault keeps the account address so a wrong passphrase is detected.
//! - The UI never receives a private key or the password back: only public data and signed results.
//! - Sending is two steps: `prepare_send` (validates and previews amount, fee and total) and `confirm_send`
//!   (signs and submits); a prepared send is single-use and expires.
//! - The wallet locks itself after a period of inactivity.
//!
//! The same [`Wallet::call`] JSON interface serves the Tauri app and the web daemon. Test keys only: not audited.
#![forbid(unsafe_code)]

mod error;
mod http;
mod service;

pub use error::WalletError;
pub use service::{Config, Wallet};
