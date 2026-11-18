//! Messages between validators. Each type has a size limit checked **before** the body is read.

use brdu_bft::{Proposal, Vote};
use brdu_chain::DecidedBlock;
use brdu_crypto::Hash32;
use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_ledger::Tx;

/// Message kinds.
pub mod kind {
    /// Block proposal.
    pub const PROPOSAL: u32 = 1;
    /// Vote.
    pub const VOTE: u32 = 2;
    /// Batch of transactions (gossip).
    pub const TXS: u32 = 3;
    /// Sender's height and tip.
    pub const STATUS: u32 = 4;
    /// Request for blocks.
    pub const GET_BLOCKS: u32 = 5;
    /// Final blocks.
    pub const BLOCKS: u32 = 6;
}

/// How many final blocks fit in one reply.
pub const MAX_BLOCKS_PER_REPLY: usize = 32;
/// How many transactions fit in one gossip batch.
pub const MAX_TXS_PER_MSG: usize = 500;

/// A decoded message.
#[derive(Debug, Clone)]
pub enum Wire {
    /// Proposal.
    Proposal(Proposal),
    /// Vote.
    Vote(Vote),
    /// Transactions.
    Txs(Vec<Tx>),
    /// Height and hash of the last block.
    Status { height: u64, tip: Option<Hash32> },
    /// "Send me the blocks starting at `from`."
    GetBlocks { from: u64, max: u32 },
    /// Final blocks.
    Blocks(Vec<DecidedBlock>),
}

/// Body size limits per kind.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Bytes in a block.
    pub max_block_bytes: usize,
    /// Transactions in a block.
    pub max_block_txs: usize,
}

impl Limits {
    /// Largest body that kind `k` may have.
    #[must_use]
    pub const fn max_for(&self, k: u32) -> usize {
        match k {
            kind::PROPOSAL => self.max_block_bytes + 4096,
            kind::VOTE => 512,
            kind::TXS => 1 << 20,
            kind::STATUS => 64,
            kind::GET_BLOCKS => 32,
            kind::BLOCKS => MAX_BLOCKS_PER_REPLY * (self.max_block_bytes + 70_000) + 64,
            _ => 0,
        }
    }
}

impl Wire {
    /// `(kind, body)`.
    #[must_use]
    pub fn encode(&self) -> (u32, Vec<u8>) {
        match self {
            Self::Proposal(p) => (kind::PROPOSAL, p.encode()),
            Self::Vote(v) => (kind::VOTE, v.encode()),
            Self::Txs(txs) => {
                let mut e = Encoder::new();
                e.count(txs.len());
                for t in txs {
                    e.bytes(&t.encode().unwrap_or_default());
                }
                (kind::TXS, e.finish().unwrap_or_default())
            }
            Self::Status { height, tip } => {
                let mut e = Encoder::new();
                e.u64(*height)
                    .bytes(tip.as_ref().map_or(&[][..], |h| h.as_bytes()));
                (kind::STATUS, e.finish().unwrap_or_default())
            }
            Self::GetBlocks { from, max } => {
                let mut e = Encoder::new();
                e.u64(*from).u32(*max);
                (kind::GET_BLOCKS, e.finish().unwrap_or_default())
            }
            Self::Blocks(bs) => {
                let mut e = Encoder::new();
                e.count(bs.len());
                for b in bs {
                    e.bytes(&b.encode().unwrap_or_default());
                }
                (kind::BLOCKS, e.finish().unwrap_or_default())
            }
        }
    }

    /// Parses a message; `None` if malformed.
    #[must_use]
    pub fn decode(k: u32, body: &[u8], lim: &Limits) -> Option<Self> {
        match k {
            kind::PROPOSAL => Proposal::decode(body, lim.max_block_bytes)
                .ok()
                .map(Self::Proposal),
            kind::VOTE => Vote::decode(body).ok().map(Self::Vote),
            kind::TXS => {
                let mut d = Decoder::new(body);
                let n = d.count(MAX_TXS_PER_MSG).ok()?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(Tx::decode(d.bytes(70_000).ok()?).ok()?);
                }
                d.finish().ok()?;
                Some(Self::Txs(out))
            }
            kind::STATUS => {
                let mut d = Decoder::new(body);
                let height = d.u64().ok()?;
                let tip = match d.bytes(32).ok()? {
                    [] => None,
                    h => Some(Hash32(h.try_into().ok()?)),
                };
                d.finish().ok()?;
                Some(Self::Status { height, tip })
            }
            kind::GET_BLOCKS => {
                let mut d = Decoder::new(body);
                let from = d.u64().ok()?;
                let max = d.u32().ok()?;
                d.finish().ok()?;
                Some(Self::GetBlocks { from, max })
            }
            kind::BLOCKS => {
                let mut d = Decoder::new(body);
                let n = d.count(MAX_BLOCKS_PER_REPLY).ok()?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(
                        DecidedBlock::decode(
                            d.bytes(lim.max_block_bytes + 70_000).ok()?,
                            lim.max_block_txs,
                            lim.max_block_bytes,
                        )
                        .ok()?,
                    );
                }
                d.finish().ok()?;
                Some(Self::Blocks(out))
            }
            _ => None,
        }
    }
}
