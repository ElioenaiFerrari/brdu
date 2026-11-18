//! BRDU BFT consensus (Tendermint design, equal-power validators), **with no I/O**: the engine receives events and
//! returns actions; whoever runs it (the node, or the simulator) delivers messages, fires timeouts and executes the actions.
#![forbid(unsafe_code)]

pub mod engine;
pub mod types;

pub use engine::{Action, Engine, EngineConfig, Event, Step};
pub use types::{CertError, Certificate, Message, Proposal, Vote, VoteType, quorum};
