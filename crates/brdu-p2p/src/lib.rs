//! Elos autenticados e cifrados entre validadores (ADR-0013 do BRDU). Só transporte: nenhuma regra de consenso
//! mora aqui. Um elo é uma sessão Noise XX (`snow`) cuja chave estática é **ligada à chave de consenso Ed25519**
//! do nó por uma assinatura dentro do aperto de mão: quem está do outro lado é provadamente a instituição que o
//! conjunto de validadores lista, **nesta rede e nesta gênese**.

#![forbid(unsafe_code)]

mod error;
mod handshake;
mod identity;
mod limits;
mod records;
mod session;
#[cfg(test)]
mod tests;

pub use error::NetError;
pub use handshake::{accept, connect};
pub use identity::{IdentityProof, NetIdentity, PeerPolicy};
pub use limits::{ConnGuard, ConnLimiter, Reputation, TokenBucket};
pub use records::{MAX_LIFETIME_SECS, MergeOutcome, PeerBook, PeerRecord, valid_addr};
pub use session::{HANDSHAKE_TIMEOUT, Session};
