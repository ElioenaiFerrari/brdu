//! A cadeia do BRDU: **gênese** (rede, validadores, emissores, regras), **blocos finais** (bloco + certificado de
//! commit) e uma **réplica verificada** que só aceita um bloco depois de conferir o certificado (mais de 2/3 dos
//! validadores), o proponente da rodada, o conjunto que reparte a taxa e a execução exata do ledger. Quem
//! sincroniza (nó, carteira, explorador) não precisa confiar em quem entregou o bloco.
#![forbid(unsafe_code)]

mod chain;
mod error;
mod genesis;
mod store;

pub use chain::{Chain, DecidedBlock};
pub use error::ChainError;
pub use genesis::Genesis;
pub use store::RecordLog;
