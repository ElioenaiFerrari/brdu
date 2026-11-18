//! Mempool with **O(1) dedupe** (a set of ids; the BRDC lesson: re-hashing every pending transaction on each
//! submission cost 0.5 → 15 ms with 7,500 pending), a limit in transactions **and in bytes**, and a contiguous
//! nonce per sender.

use std::collections::{BTreeMap, HashMap, HashSet};

/// How many nonces ahead of the expected one the pool agrees to hold (fast submission to different nodes arrives out of order).
pub const MAX_NONCE_GAP: u64 = 16;

use brdu_crypto::{Address, Hash32};
use brdu_ledger::Tx;

/// Why a transaction was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    /// Already in the pool.
    Duplicate,
    /// The pool is full (transactions or bytes).
    Full,
    /// The nonce was already used (or is lower than expected).
    StaleNonce { expected: u64, got: u64 },
    /// Gap too large: many earlier transactions from the sender are missing.
    NonceGap { expected: u64, got: u64 },
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate => write!(f, "duplicate transaction"),
            Self::Full => write!(f, "mempool full"),
            Self::StaleNonce { expected, got } => {
                write!(f, "stale nonce: expected {expected}, got {got}")
            }
            Self::NonceGap { expected, got } => {
                write!(f, "nonce gap: expected {expected}, got {got}")
            }
        }
    }
}

/// The pool.
pub struct Mempool {
    /// Transactions with a nonce gap (waiting for the earlier ones).
    queued: BTreeMap<(Address, u64), Tx>,
    queued_ids: HashSet<Hash32>,
    max_txs: usize,
    max_bytes: usize,
    bytes: usize,
    next_seq: u64,
    by_seq: BTreeMap<u64, Tx>,
    ids: HashMap<Hash32, u64>,
    /// Next expected nonce per sender, counting what is already in the pool.
    next_nonce: HashMap<Address, u64>,
}

impl Mempool {
    /// Empty pool with the given limits.
    #[must_use]
    pub fn new(max_txs: usize, max_bytes: usize) -> Self {
        Self {
            queued: BTreeMap::new(),
            queued_ids: HashSet::new(),
            max_txs,
            max_bytes,
            bytes: 0,
            next_seq: 0,
            by_seq: BTreeMap::new(),
            ids: HashMap::new(),
            next_nonce: HashMap::new(),
        }
    }

    /// How many transactions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_seq.len() + self.queued.len()
    }

    /// Empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_seq.is_empty() && self.queued.is_empty()
    }

    /// How many can already go into a block (no nonce gap).
    #[must_use]
    pub fn ready_len(&self) -> usize {
        self.by_seq.len()
    }

    /// Bytes held.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Is it in the pool? O(1).
    #[must_use]
    pub fn contains(&self, id: &Hash32) -> bool {
        self.ids.contains_key(id) || self.queued_ids.contains(id)
    }

    /// Tries to admit a transaction. `committed_nonce` is the account's nonce in the final state. The signature has already been checked.
    pub fn add(&mut self, tx: Tx, id: Hash32, committed_nonce: u64) -> Result<(), PoolError> {
        if self.contains(&id) {
            return Err(PoolError::Duplicate);
        }
        let sender = tx.sender_address();
        let expected = self
            .next_nonce
            .get(&sender)
            .copied()
            .unwrap_or(committed_nonce)
            .max(committed_nonce);
        if tx.nonce < expected {
            return Err(PoolError::StaleNonce {
                expected,
                got: tx.nonce,
            });
        }
        let size = tx.size();
        if self.len() >= self.max_txs || self.bytes + size > self.max_bytes {
            return Err(PoolError::Full);
        }
        if tx.nonce > expected {
            if tx.nonce - expected > MAX_NONCE_GAP {
                return Err(PoolError::NonceGap {
                    expected,
                    got: tx.nonce,
                });
            }
            self.bytes += size;
            self.queued_ids.insert(id);
            self.queued.insert((sender, tx.nonce), tx);
            return Ok(());
        }
        self.push_ready(tx, id);
        self.promote(&sender);
        Ok(())
    }

    fn push_ready(&mut self, tx: Tx, id: Hash32) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.next_nonce.insert(tx.sender_address(), tx.nonce + 1);
        self.bytes += tx.size();
        self.ids.insert(id, seq);
        self.by_seq.insert(seq, tx);
    }

    /// Moves the sender's transactions that no longer have a gap to the ready queue.
    fn promote(&mut self, sender: &Address) {
        while let Some(next) = self.next_nonce.get(sender).copied()
            && let Some(tx) = self.queued.remove(&(*sender, next))
        {
            let id = tx.id();
            self.queued_ids.remove(&id);
            self.bytes = self.bytes.saturating_sub(tx.size()); // push_ready adds it back
            self.push_ready(tx, id);
        }
    }

    /// The transactions in arrival order (the per-sender nonce is already in order).
    pub fn iter(&self) -> impl Iterator<Item = &Tx> {
        self.by_seq.values()
    }

    /// The **ready** transactions that are the account's next one (nonce equal to the chain's): the head of each
    /// queue, the only one that can enter the next block.
    pub fn heads(&self, committed_nonce: &dyn Fn(&Address) -> u64) -> Vec<Tx> {
        self.by_seq
            .values()
            .filter(|t| t.nonce == committed_nonce(&t.sender_address()))
            .cloned()
            .collect()
    }

    /// Removes the sender's transactions with nonce ≥ `from` (ready and queued): if the one at `from` is not valid,
    /// the following ones are not either.
    pub fn remove_sender_from(&mut self, sender: &Address, from: u64) {
        let ids: Vec<Hash32> = self
            .by_seq
            .values()
            .chain(self.queued.values())
            .filter(|t| t.nonce >= from && t.sender_address() == *sender)
            .map(Tx::id)
            .collect();
        for id in ids {
            self.remove(&id);
        }
    }

    /// Removes by id (O(log n)).
    pub fn remove(&mut self, id: &Hash32) {
        if let Some(seq) = self.ids.remove(id)
            && let Some(tx) = self.by_seq.remove(&seq)
        {
            self.bytes = self.bytes.saturating_sub(tx.size());
            // a transaction that leaves (failed while building the block) gives its nonce back: the sender's next
            // one can use it (otherwise the pool would reject a valid transaction as a "stale nonce")
            self.next_nonce
                .entry(tx.sender_address())
                .and_modify(|n| *n = (*n).min(tx.nonce));
        } else if self.queued_ids.remove(id) {
            let key = self
                .queued
                .iter()
                .find(|(_, t)| t.id() == *id)
                .map(|(k, _)| *k);
            if let Some(tx) = key.and_then(|k| self.queued.remove(&k)) {
                self.bytes = self.bytes.saturating_sub(tx.size());
            }
        }
    }

    /// After a block: drops what is already past the account nonce and rebuilds the "next nonces". O(pool).
    pub fn purge(&mut self, committed_nonce: &dyn Fn(&Address) -> u64) {
        let stale: Vec<Hash32> = self
            .by_seq
            .values()
            .filter(|t| t.nonce < committed_nonce(&t.sender_address()))
            .map(Tx::id)
            .collect();
        for id in stale {
            self.remove(&id);
        }
        self.next_nonce.clear();
        for tx in self.by_seq.values() {
            let e = self.next_nonce.entry(tx.sender_address()).or_insert(0);
            *e = (*e).max(tx.nonce + 1);
        }
        // the queued ones: discard the stale ones and promote those that now fit
        let stale_q: Vec<(Address, u64)> = self
            .queued
            .iter()
            .filter(|((a, n), _)| *n < committed_nonce(a))
            .map(|(k, _)| *k)
            .collect();
        for k in stale_q {
            if let Some(tx) = self.queued.remove(&k) {
                self.queued_ids.remove(&tx.id());
                self.bytes = self.bytes.saturating_sub(tx.size());
            }
        }
        let senders: Vec<Address> = self
            .queued
            .keys()
            .map(|(a, _)| *a)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        for a in senders {
            let c = committed_nonce(&a);
            self.next_nonce
                .entry(a)
                .and_modify(|n| *n = (*n).max(c))
                .or_insert(c);
            self.promote(&a);
        }
    }
}
