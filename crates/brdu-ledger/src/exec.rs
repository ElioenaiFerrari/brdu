//! The executor: applies transactions and blocks to the state, **deterministically and atomically**.
//!
//! Order of a transaction's checks: shape and signature → network → minimum fee → nonce → fee debit →
//! kind rule. If any step fails, nothing the transaction changed remains.
//!
//! **Block:** it is valid or not as a whole. One invalid transaction brings the block down. Each transaction's fee is
//! charged to the sender and, at the end of the block, **split equally among the validators** in the header; the remainder
//! of the division goes to the proposer (ADR-0027).

use std::collections::HashSet;

use brdu_crypto::sign::verify_all;
use brdu_crypto::{Address, Hash32, PublicKey, Signature};

use crate::block::{Block, MAX_SIGNERS};
use crate::coin;
use crate::error::LedgerError;
use crate::rules::Rules;
use crate::state::State;
use crate::tx::{Tx, kind};

/// Context a module receives when executing a transaction.
#[derive(Debug, Clone, Copy)]
pub struct TxCtx {
    /// The sender's address.
    pub sender: Address,
    /// The block time (or the state's, when validating for the mempool).
    pub time: u64,
    /// The height of the block being built.
    pub height: u64,
}

/// A module (assets, credentials, contracts) handles a range of transaction kinds (5 to 21).
pub trait Module: Send + Sync {
    /// The kinds this module executes.
    fn kinds(&self) -> core::ops::RangeInclusive<u8>;

    /// Minimum fee of a module transaction (default: zero).
    fn min_fee(&self, _rules: &Rules, _tx: &Tx) -> u64 {
        0
    }

    /// Hook at the **end of each block** (after the transactions and the fee split): this is where a module
    /// applies scheduled changes (for example, swapping the validator set). Default: nothing.
    fn end_block(
        &self,
        _state: &mut State,
        _rules: &Rules,
        _height: u64,
    ) -> Result<(), LedgerError> {
        Ok(())
    }

    /// Executes. The signature, network, fee and nonce have already been handled.
    fn execute(
        &self,
        state: &mut State,
        rules: &Rules,
        ctx: &TxCtx,
        tx: &Tx,
    ) -> Result<(), LedgerError>;
}

/// The result of applying a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockReceipt {
    /// Block hash.
    pub hash: Hash32,
    /// How many transactions.
    pub tx_count: usize,
    /// Total fees split.
    pub fees: u64,
}

/// Applies transactions and blocks according to the network [`Rules`].
pub struct Executor {
    rules: Rules,
    modules: Vec<Box<dyn Module>>,
    verify_threads: usize,
}

impl Executor {
    /// An executor with the coin only.
    #[must_use]
    pub fn new(rules: Rules) -> Self {
        Self {
            rules,
            modules: Vec::new(),
            verify_threads: 1,
        }
    }

    /// Registers a module.
    #[must_use]
    pub fn with_module(mut self, module: Box<dyn Module>) -> Self {
        self.modules.push(module);
        self
    }

    /// How many threads verify a block's signatures (default 1; only worthwhile with many transactions and CPU).
    #[must_use]
    pub fn with_verify_threads(mut self, threads: usize) -> Self {
        self.verify_threads = threads.max(1);
        self
    }

    /// The network rules.
    #[must_use]
    pub const fn rules(&self) -> &Rules {
        &self.rules
    }

    fn module_for(&self, k: u8) -> Option<&dyn Module> {
        self.modules
            .iter()
            .find(|m| m.kinds().contains(&k))
            .map(AsRef::as_ref)
    }

    /// Minimum fee of `tx`: transfer = base + per_byte × size; mint, burn and attestation = 0 (they require M
    /// issuers); modules define their own. Unknown kind is an error.
    pub fn min_fee(&self, tx: &Tx) -> Result<u64, LedgerError> {
        match tx.kind {
            kind::TRANSFER => {
                let size = u64::try_from(tx.size()).map_err(|_| LedgerError::Overflow)?;
                self.rules
                    .per_byte
                    .checked_mul(size)
                    .and_then(|v| v.checked_add(self.rules.transfer_base))
                    .ok_or(LedgerError::Overflow)
            }
            kind::MINT | kind::BURN | kind::ATTESTATION => Ok(0),
            other => self
                .module_for(other)
                .map(|m| m.min_fee(&self.rules, tx))
                .ok_or(LedgerError::UnknownKind(other)),
        }
    }

    /// Validates and applies **one** transaction to `state` (used in the mempool). The fee is debited from the sender (in a block's
    /// state it is split among the validators; in a "pending" state it simply leaves the account).
    pub fn apply_tx(&self, state: &mut State, tx: &Tx) -> Result<(), LedgerError> {
        tx.verify()?;
        state.atomically(|s| self.run_verified(s, tx))
    }

    fn run_verified(&self, s: &mut State, tx: &Tx) -> Result<(), LedgerError> {
        if tx.chain_id != self.rules.chain_id {
            return Err(LedgerError::WrongChain);
        }
        let min = self.min_fee(tx)?;
        if tx.fee < min {
            return Err(LedgerError::FeeTooLow { min, got: tx.fee });
        }
        let sender = tx.sender_address();
        let mut acc = s.account(&sender);
        if acc.nonce != tx.nonce {
            return Err(LedgerError::BadNonce {
                expected: acc.nonce,
                got: tx.nonce,
            });
        }
        acc.balance = acc
            .balance
            .checked_sub(tx.fee)
            .ok_or(LedgerError::InsufficientFunds)?;
        acc.nonce = acc.nonce.checked_add(1).ok_or(LedgerError::Overflow)?;
        s.set_account(sender, acc);
        match tx.kind {
            kind::TRANSFER..=kind::ATTESTATION => {
                coin::execute(s, &self.rules, tx.kind, &sender, &tx.payload)
            }
            other => {
                let module = self
                    .module_for(other)
                    .ok_or(LedgerError::UnknownKind(other))?;
                let ctx = TxCtx {
                    sender,
                    time: s.time(),
                    height: s.height().saturating_add(1),
                };
                module.execute(s, &self.rules, &ctx, tx)
            }
        }
    }

    // --- blocks --------------------------------------------------------------------------------

    fn check_block_shape(&self, state: &State, block: &Block) -> Result<(), LedgerError> {
        if block.height != state.height().saturating_add(1) || block.prev_hash != state.last_hash()
        {
            return Err(LedgerError::BadLinkage);
        }
        if block.time < state.time() {
            return Err(LedgerError::TimeWentBack);
        }
        let bytes: usize = block.txs.iter().map(Tx::size).sum();
        if block.txs.len() > self.rules.max_block_txs || bytes > self.rules.max_block_bytes {
            return Err(LedgerError::BlockTooLarge);
        }
        let mut seen = HashSet::with_capacity(block.txs.len());
        for tx in &block.txs {
            if !seen.insert(tx.id()) {
                return Err(LedgerError::DuplicateTx(tx.id()));
            }
        }
        // signers: non-empty, strictly increasing (no repeats) and including the proposer
        let sorted = block.signers.windows(2).all(|w| w[0] < w[1]);
        if block.signers.is_empty()
            || block.signers.len() > MAX_SIGNERS
            || !sorted
            || !block.signers.contains(&block.proposer)
        {
            return Err(LedgerError::BadLinkage);
        }
        Ok(())
    }

    /// Checks shape and signature of all transactions (the most expensive part of a block).
    pub fn verify_signatures(&self, txs: &[Tx]) -> Result<(), LedgerError> {
        let mut msgs: Vec<Hash32> = Vec::with_capacity(txs.len());
        for tx in txs {
            tx.check_shape()?;
            msgs.push(tx.signing_message()?);
        }
        let items: Vec<(&PublicKey, &[u8], &Signature)> = txs
            .iter()
            .zip(&msgs)
            .map(|(tx, m)| (&tx.sender, m.as_bytes().as_slice(), &tx.signature))
            .collect();
        let threads = self.verify_threads.min(items.len().max(1));
        if threads <= 1 || items.len() < 64 {
            return verify_all(&items, true).map_err(|_| LedgerError::BadSignature);
        }
        let chunk = items.len().div_ceil(threads);
        let ok = std::thread::scope(|sc| {
            let handles: Vec<_> = items
                .chunks(chunk)
                .map(|part| sc.spawn(move || verify_all(part, true).is_ok()))
                .collect();
            handles.into_iter().all(|h| h.join().unwrap_or(false))
        });
        if ok {
            Ok(())
        } else {
            Err(LedgerError::BadSignature)
        }
    }

    /// Executes the block's transactions, splits the fees and positions the state. Does not check the root.
    fn run_block(&self, s: &mut State, block: &Block) -> Result<u64, LedgerError> {
        // modules read the time of the block being executed (windows, validity), not the previous block's
        s.set_position(s.height(), block.time);
        let mut fees: u128 = 0;
        for tx in &block.txs {
            self.run_verified(s, tx)
                .map_err(|e| LedgerError::InvalidTx {
                    id: tx.id(),
                    reason: Box::new(e),
                })?;
            fees += u128::from(tx.fee);
        }
        let fees = u64::try_from(fees).map_err(|_| LedgerError::Overflow)?;
        let n = u64::try_from(block.signers.len()).map_err(|_| LedgerError::Overflow)?;
        let (share, rest) = (fees / n, fees % n);
        for signer in &block.signers {
            s.credit(signer, share)?;
        }
        s.credit(&block.proposer, rest)?;
        for m in &self.modules {
            m.end_block(s, &self.rules, block.height)?;
        }
        s.set_position(block.height, block.time);
        Ok(fees)
    }

    /// Applies a **whole** block or nothing. Checks linkage, time, limits, signatures, each transaction and the header's state
    /// root.
    pub fn apply_block(
        &self,
        state: &mut State,
        block: &Block,
    ) -> Result<BlockReceipt, LedgerError> {
        self.check_block_shape(state, block)?;
        self.verify_signatures(&block.txs)?;
        self.apply_block_presigned(state, block)
    }

    /// Dry run of a block: checks **everything** (including the root) and undoes it. This is what a validator does before voting.
    pub fn dry_run_block(&self, state: &mut State, block: &Block) -> Result<(), LedgerError> {
        self.check_block_shape(state, block)?;
        self.verify_signatures(&block.txs)?;
        state.speculate(|s| {
            self.run_block(s, block)?;
            if s.root() == block.state_root {
                Ok(())
            } else {
                Err(LedgerError::StateRootMismatch)
            }
        })
    }

    /// Like `apply_block`, but **without rechecking signatures**: only for whoever just verified them (the
    /// `dry_run_block` rehearsal of the same block). Shape, linkage and execution are still checked.
    pub fn apply_block_presigned(
        &self,
        state: &mut State,
        block: &Block,
    ) -> Result<BlockReceipt, LedgerError> {
        self.check_block_shape(state, block)?;
        let hash = block.hash();
        state.atomically(|s| {
            let fees = self.run_block(s, block)?;
            if s.root() != block.state_root {
                return Err(LedgerError::StateRootMismatch);
            }
            s.set_last_hash(hash);
            Ok(BlockReceipt {
                hash,
                tx_count: block.txs.len(),
                fees,
            })
        })
    }

    /// Builds the next block with `txs` (all must be valid, in order) and computes the state root **without changing**
    /// `state` (everything is undone at the end).
    pub fn build_block(
        &self,
        state: &mut State,
        proposer: Address,
        signers: Vec<Address>,
        time: u64,
        txs: Vec<Tx>,
    ) -> Result<Block, LedgerError> {
        let mut block = Block {
            height: state.height().saturating_add(1),
            time,
            prev_hash: state.last_hash(),
            proposer,
            signers,
            state_root: Hash32::ZERO,
            txs,
        };
        self.check_block_shape(state, &block)?;
        self.verify_signatures(&block.txs)?;
        block.state_root = state.speculate(|s| self.run_block(s, &block).map(|_| s.root()))?;
        Ok(block)
    }
}
