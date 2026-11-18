//! The verified replica and the lookup indexes.

use std::collections::HashMap;
use std::time::Instant;

use brdu_chain::{Chain, DecidedBlock, Genesis};
use brdu_crypto::{Address, Hash32};
use brdu_ledger::Tx;
use serde_json::Value;

use crate::view;

/// Health of the sync with the node.
#[derive(Debug, Default)]
pub struct SyncStatus {
    /// Height the node claims to have.
    pub node_height: u64,
    /// Last success.
    pub last_ok: Option<Instant>,
    /// Last error (node down, or a node that lied).
    pub last_error: Option<String>,
    /// Blocks the node delivered that verification **rejected**.
    pub rejected_blocks: u64,
}

/// The verified chain and its indexes.
pub struct Explorer {
    /// The replica: only advances through blocks that pass all verification.
    pub chain: Chain,
    /// transaction id -> (height, position in the block).
    txs: HashMap<Hash32, (u64, usize)>,
    /// account -> where it appears (as sender or counterparty).
    by_account: HashMap<Address, Vec<(u64, usize)>>,
    /// block hash -> height.
    by_hash: HashMap<Hash32, u64>,
    /// API address of the node feeding this explorer.
    pub node: String,
    /// Sync health.
    pub status: SyncStatus,
    /// Equivocation evidence **reported by the node** (not verifiable here).
    pub evidence: Value,
}

impl Explorer {
    /// Empty explorer over the genesis (pinned by whoever runs it; the node does not choose).
    pub fn new(genesis: Genesis, node: &str) -> Result<Self, String> {
        Ok(Self {
            chain: Chain::new(genesis).map_err(|e| e.to_string())?,
            txs: HashMap::new(),
            by_account: HashMap::new(),
            by_hash: HashMap::new(),
            node: node.to_string(),
            status: SyncStatus::default(),
            evidence: Value::Null,
        })
    }

    /// Height of the verified replica.
    #[must_use]
    pub fn height(&self) -> u64 {
        self.chain.height()
    }

    /// Verifies and applies a final block received from the node. **Rejects** anything that fails any verification.
    pub fn ingest(&mut self, d: DecidedBlock) -> Result<(), String> {
        let height = d.block.height;
        if height != self.chain.height() + 1 {
            return Err(format!(
                "block {height} out of order (expected {})",
                self.chain.height() + 1
            ));
        }
        let txs: Vec<Tx> = d.block.txs.clone();
        let hash = d.block.hash();
        self.chain
            .apply(d)
            .map_err(|e| format!("block {height} rejected: {e}"))?;
        self.by_hash.insert(hash, height);
        for (i, tx) in txs.iter().enumerate() {
            self.txs.insert(tx.id(), (height, i));
            self.by_account
                .entry(tx.sender_address())
                .or_default()
                .push((height, i));
            if let Some(other) = view::counterparty(tx)
                && other != tx.sender_address()
            {
                self.by_account.entry(other).or_default().push((height, i));
            }
        }
        Ok(())
    }

    /// Where the transaction is.
    #[must_use]
    pub fn find_tx(&self, id: &Hash32) -> Option<(u64, usize)> {
        self.txs.get(id).copied()
    }

    /// The height of a block by hash.
    #[must_use]
    pub fn find_block(&self, hash: &Hash32) -> Option<u64> {
        self.by_hash.get(hash).copied()
    }

    /// An account's occurrences (newest first), with `skip` and `limit`.
    #[must_use]
    pub fn account_txs(&self, a: &Address, skip: usize, limit: usize) -> Vec<(u64, usize)> {
        self.by_account
            .get(a)
            .map(|v| v.iter().rev().skip(skip).take(limit).copied().collect())
            .unwrap_or_default()
    }

    /// How many occurrences the account has.
    #[must_use]
    pub fn account_tx_count(&self, a: &Address) -> usize {
        self.by_account.get(a).map_or(0, Vec::len)
    }

    /// Total indexed transactions.
    #[must_use]
    pub fn tx_total(&self) -> usize {
        self.txs.len()
    }
}
