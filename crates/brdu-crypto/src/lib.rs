//! Primitivas criptográficas do BRDU (ADR-0005, 0010, 0011).
//!
//! - [`hash`]: SHA3-256 com prefixo de domínio.
//! - [`sign`]: Ed25519 estrito, chave privada sem `Debug` e zerada ao sair da memória.
//! - [`address`] e [`bech32m`]: endereços `brdu1…` com checksum.
//! - [`encoding`]: codificação determinística de mensagens.
//! - [`amount`]: valores em unidades mínimas (6 casas), sem ponto flutuante.
//! - [`merkle`]: árvore de Merkle com domínios separados.
//!
//! `no_std` com `alloc`: serve ao nó, à carteira desktop e, no futuro, ao navegador (WASM). **Não auditado.**
#![cfg_attr(not(test), no_std)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

extern crate alloc;

pub mod address;
pub mod amount;
pub mod bech32m;
pub mod encoding;
pub mod error;
pub mod hash;
pub mod hex;
pub mod merkle;
pub mod sign;

pub use address::Address;
pub use amount::Amount;
pub use error::{AddressError, AmountError, CryptoError, DecodeError};
pub use hash::{Hash32, sha3, tagged, tags};
pub use sign::{PublicKey, SecretKey, Signature};
