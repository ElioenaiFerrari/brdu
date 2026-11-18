//! Block: header and transactions, with deterministic encoding and a tagged hash.

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, tagged, tags};

use crate::error::LedgerError;
use crate::tx::Tx;

/// More signers than this in a header is abuse.
pub const MAX_SIGNERS: usize = 256;

/// A block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Height (the first block has height 1).
    pub height: u64,
    /// Time in Unix seconds; non-decreasing.
    pub time: u64,
    /// Hash of the previous block (`None` for the first).
    pub prev_hash: Option<Hash32>,
    /// Who proposed it.
    pub proposer: Address,
    /// The validator set in force, ordered: the block fee is split among them (ADR-0027).
    pub signers: Vec<Address>,
    /// The state root **after** applying the block.
    pub state_root: Hash32,
    /// The transactions.
    pub txs: Vec<Tx>,
}

impl Block {
    /// Deterministic encoding.
    pub fn encode(&self) -> Result<Vec<u8>, LedgerError> {
        let mut e = Encoder::new();
        e.u64(self.height).u64(self.time);
        match &self.prev_hash {
            Some(h) => e.u8(1).fixed(h.as_bytes()),
            None => e.u8(0),
        };
        e.fixed(self.proposer.hash().as_bytes())
            .count(self.signers.len());
        for s in &self.signers {
            e.fixed(s.hash().as_bytes());
        }
        e.fixed(self.state_root.as_bytes()).count(self.txs.len());
        for tx in &self.txs {
            e.bytes(&tx.encode()?);
        }
        e.finish().map_err(LedgerError::Decode)
    }

    /// Reads a block, rejecting more than `max_txs` transactions and any garbage or leftovers.
    pub fn decode(bytes: &[u8], max_txs: usize) -> Result<Self, LedgerError> {
        let mut d = Decoder::new(bytes);
        let bad = |_| LedgerError::Malformed;
        let height = d.u64().map_err(bad)?;
        let time = d.u64().map_err(bad)?;
        let prev_hash = match d.u8().map_err(bad)? {
            0 => None,
            1 => Some(Hash32(d.fixed::<32>().map_err(bad)?)),
            _ => return Err(LedgerError::Malformed),
        };
        let proposer = Address(Hash32(d.fixed::<32>().map_err(bad)?));
        let n_signers = d.count(MAX_SIGNERS).map_err(bad)?;
        let mut signers = Vec::with_capacity(n_signers);
        for _ in 0..n_signers {
            signers.push(Address(Hash32(d.fixed::<32>().map_err(bad)?)));
        }
        let state_root = Hash32(d.fixed::<32>().map_err(bad)?);
        let n_txs = d.count(max_txs).map_err(bad)?;
        let mut txs = Vec::with_capacity(n_txs);
        for _ in 0..n_txs {
            txs.push(Tx::decode(
                d.bytes(crate::tx::MAX_PAYLOAD + 512).map_err(bad)?,
            )?);
        }
        d.finish().map_err(bad)?;
        Ok(Self {
            height,
            time,
            prev_hash,
            proposer,
            signers,
            state_root,
            txs,
        })
    }

    /// Block hash: tagged, over the full encoding.
    #[must_use]
    pub fn hash(&self) -> Hash32 {
        self.encode()
            .map_or(Hash32::ZERO, |b| tagged(tags::BLOCK, &[&b]))
    }
}
