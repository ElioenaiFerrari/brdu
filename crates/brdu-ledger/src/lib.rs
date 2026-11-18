//! Livro-razão do BRDU: a máquina de estados que o consenso replica.
//!
//! - [`Tx`]: transação assinada (mesmo formato do BRDC), com taxa e nonce.
//! - [`State`]: contas, oferta, referências, atestado de reserva e armazenamento dos módulos, com **raiz de
//!   Merkle** e desfazer atômico.
//! - [`Block`] e [`Executor`]: bloco inteiro ou nada; taxa repartida entre os validadores; raiz de estado conferida.
//! - [`coin`]: transferência, mint, burn e atestado de reserva (M-de-N emissores); cobertura só como informação.
//! - [`Module`]: ponto de extensão para bens, credenciais e contratos (marco M9).
//!
//! Puro: sem E/S, sem rede, sem relógio. **Não auditado.**
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod block;
pub mod coin;
pub mod error;
pub mod exec;
pub mod gov;
pub mod rules;
pub mod state;
pub mod tx;

pub use block::Block;
pub use error::LedgerError;
pub use exec::{BlockReceipt, Executor, Module, TxCtx};
pub use rules::Rules;
pub use state::{Account, Attestation, Coverage, State};
pub use tx::{Tx, kind};
