//! Mempool com **dedupe O(1)** (conjunto de ids; a lição do BRDC: refazer o hash de todas as pendentes a cada
//! envio custava 0,5 → 15 ms com 7.500 pendentes), limite em transações **e em bytes**, e nonce contíguo por
//! remetente.

use std::collections::{BTreeMap, HashMap, HashSet};

/// Quantos nonces à frente do esperado o pool aceita guardar (envio rápido a nós diferentes chega fora de ordem).
pub const MAX_NONCE_GAP: u64 = 16;

use brdu_crypto::{Address, Hash32};
use brdu_ledger::Tx;

/// Por que uma transação não entrou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    /// Já está no pool.
    Duplicate,
    /// O pool está cheio (transações ou bytes).
    Full,
    /// O nonce já foi usado (ou é menor que o esperado).
    StaleNonce { expected: u64, got: u64 },
    /// Buraco grande demais: faltam muitas transações anteriores do remetente.
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

/// O pool.
pub struct Mempool {
    /// Transações com buraco de nonce (esperam as anteriores).
    queued: BTreeMap<(Address, u64), Tx>,
    queued_ids: HashSet<Hash32>,
    max_txs: usize,
    max_bytes: usize,
    bytes: usize,
    next_seq: u64,
    by_seq: BTreeMap<u64, Tx>,
    ids: HashMap<Hash32, u64>,
    /// Próximo nonce esperado por remetente, contando o que já está no pool.
    next_nonce: HashMap<Address, u64>,
}

impl Mempool {
    /// Pool vazio com os limites.
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

    /// Quantas transações.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_seq.len() + self.queued.len()
    }

    /// Vazio?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_seq.is_empty() && self.queued.is_empty()
    }

    /// Quantas já podem ir num bloco (sem buraco de nonce).
    #[must_use]
    pub fn ready_len(&self) -> usize {
        self.by_seq.len()
    }

    /// Bytes retidos.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Está no pool? O(1).
    #[must_use]
    pub fn contains(&self, id: &Hash32) -> bool {
        self.ids.contains_key(id) || self.queued_ids.contains(id)
    }

    /// Tenta admitir. `committed_nonce` é o nonce da conta no estado final. A assinatura já foi conferida.
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

    /// Passa para a fila de prontas as transações do remetente que deixaram de ter buraco.
    fn promote(&mut self, sender: &Address) {
        while let Some(next) = self.next_nonce.get(sender).copied()
            && let Some(tx) = self.queued.remove(&(*sender, next))
        {
            let id = tx.id();
            self.queued_ids.remove(&id);
            self.bytes = self.bytes.saturating_sub(tx.size()); // push_ready soma de volta
            self.push_ready(tx, id);
        }
    }

    /// As transações em ordem de chegada (o nonce por remetente já vem em ordem).
    pub fn iter(&self) -> impl Iterator<Item = &Tx> {
        self.by_seq.values()
    }

    /// As transações **prontas** que são a próxima da conta (nonce igual ao da cadeia): é a cabeça de cada fila,
    /// a única que pode entrar no próximo bloco.
    pub fn heads(&self, committed_nonce: &dyn Fn(&Address) -> u64) -> Vec<Tx> {
        self.by_seq
            .values()
            .filter(|t| t.nonce == committed_nonce(&t.sender_address()))
            .cloned()
            .collect()
    }

    /// Remove as transações do remetente com nonce ≥ `from` (prontas e em espera): se a de `from` não vale, as
    /// seguintes também não valem.
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

    /// Remove por id (O(log n)).
    pub fn remove(&mut self, id: &Hash32) {
        if let Some(seq) = self.ids.remove(id)
            && let Some(tx) = self.by_seq.remove(&seq)
        {
            self.bytes = self.bytes.saturating_sub(tx.size());
            // uma transação que sai (falhou ao montar o bloco) devolve o nonce dela: a próxima do remetente pode
            // usá-lo (senão o pool recusaria como "nonce vencido" uma transação válida)
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

    /// Depois de um bloco: tira o que já passou do nonce da conta e refaz os "próximos nonces". O(pool).
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
        // as que esperavam: descarta as vencidas e promove as que agora encaixam
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
