//! Consenso BFT do BRDU (desenho Tendermint, validadores de poder igual), **sem E/S**: o motor recebe eventos e
//! devolve ações; quem o roda (o nó, ou o simulador) entrega mensagens, dispara prazos e executa as ações.
#![forbid(unsafe_code)]

pub mod engine;
pub mod types;

pub use engine::{Action, Engine, EngineConfig, Event, Step};
pub use types::{CertError, Certificate, Message, Proposal, Vote, VoteType, quorum};
