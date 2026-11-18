use brdu_bft::Certificate;
use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey};
use brdu_ledger::{Block, BlockReceipt, Executor, State};

use crate::error::ChainError;
use crate::genesis::Genesis;

/// A finalized block: the block and the commit certificate (precommits from more than 2/3 of the validators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecidedBlock {
    /// The block.
    pub block: Block,
    /// The certificate.
    pub cert: Certificate,
}

impl DecidedBlock {
    /// Deterministic encoding.
    pub fn encode(&self) -> Result<Vec<u8>, ChainError> {
        let mut e = Encoder::new();
        e.bytes(&self.block.encode()?).bytes(&self.cert.encode());
        Ok(e.finish()?)
    }

    /// Reads a finalized block (the block has at most `max_txs` transactions).
    pub fn decode(
        bytes: &[u8],
        max_txs: usize,
        max_block_bytes: usize,
    ) -> Result<Self, ChainError> {
        let mut d = Decoder::new(bytes);
        let block = Block::decode(d.bytes(max_block_bytes)?, max_txs)?;
        let cert = Certificate::decode(d.bytes(256 * 256)?)?;
        d.finish()?;
        Ok(Self { block, cert })
    }
}

/// Verified replica: only advances through `apply`, which checks **everything** (certificate, proposer, set, execution).
pub struct Chain {
    genesis: Genesis,
    genesis_hash: Hash32,
    exec: Executor,
    state: State,
    blocks: Vec<DecidedBlock>,
    validated: Option<Hash32>,
}

impl Chain {
    /// Creates the empty chain from the genesis.
    pub fn new(genesis: Genesis) -> Result<Self, ChainError> {
        Self::with_executor(genesis, Executor::new)
    }

    /// Like `new`, with a custom executor (modules, verification threads).
    pub fn with_executor(
        genesis: Genesis,
        make: impl FnOnce(brdu_ledger::Rules) -> Executor,
    ) -> Result<Self, ChainError> {
        let rules = genesis.rules()?;
        Ok(Self {
            genesis_hash: genesis.hash(),
            exec: make(rules)
                .with_module(Box::new(brdu_ledger::gov::GovModule))
                .with_module(Box::new(brdu_credentials::CredentialsModule))
                .with_module(Box::new(brdu_assets::AssetsModule)),
            state: seed_state(&genesis),
            blocks: Vec::new(),
            validated: None,
            genesis,
        })
    }

    /// The genesis.
    #[must_use]
    pub const fn genesis(&self) -> &Genesis {
        &self.genesis
    }

    /// The genesis hash.
    #[must_use]
    pub const fn genesis_hash(&self) -> &Hash32 {
        &self.genesis_hash
    }

    /// The executor (rules, minimum fee).
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.exec
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    /// Height (number of blocks). The next one has height `height() + 1`.
    #[must_use]
    pub const fn height(&self) -> u64 {
        self.blocks.len() as u64
    }

    /// The finalized blocks.
    #[must_use]
    pub fn blocks(&self) -> &[DecidedBlock] {
        &self.blocks
    }

    /// Blocks with height greater than `after`.
    #[must_use]
    pub fn blocks_after(&self, after: u64) -> &[DecidedBlock] {
        let i = usize::try_from(after)
            .unwrap_or(usize::MAX)
            .min(self.blocks.len());
        self.blocks.get(i..).unwrap_or(&[])
    }

    /// The validators, in rotation order.
    #[must_use]
    pub fn validators(&self) -> Vec<PublicKey> {
        brdu_ledger::gov::validators(&self.state)
    }

    /// The validator set epoch (0 = genesis; increases with each change that takes effect).
    #[must_use]
    pub fn epoch(&self) -> u64 {
        brdu_ledger::gov::epoch(&self.state)
    }

    /// The pending validator change, if any.
    #[must_use]
    pub fn pending_change(&self) -> Option<brdu_ledger::gov::Pending> {
        brdu_ledger::gov::pending(&self.state)
    }

    /// The validators' addresses, sorted (the header's `signers`).
    #[must_use]
    pub fn signers(&self) -> Vec<Address> {
        let mut v: Vec<Address> = self
            .validators()
            .iter()
            .map(Address::from_public_key)
            .collect();
        v.sort();
        v
    }

    /// The proposer of `round` at the next block's height.
    #[must_use]
    pub fn proposer(&self, round: u64) -> Option<PublicKey> {
        proposer_of(&self.validators(), self.height() + 1, round)
    }

    /// Builds the next block with `txs` (without altering the chain). `proposer` must be the round's proposer, which
    /// `apply` checks later.
    pub fn build(
        &mut self,
        proposer: Address,
        time: u64,
        txs: Vec<brdu_ledger::Tx>,
    ) -> Result<Block, ChainError> {
        let signers = self.signers();
        Ok(self
            .exec
            .build_block(&mut self.state, proposer, signers, time, txs)?)
    }

    /// Chooses, **in order**, the transactions from `candidates` that are valid together in the next block (up to the
    /// transaction and byte limits), trying each one against the state and undoing everything at the end. Returns the chosen ones and
    /// the ones that failed (the caller discards them).
    pub fn select_txs<'a>(
        &mut self,
        candidates: impl Iterator<Item = &'a brdu_ledger::Tx>,
    ) -> (Vec<brdu_ledger::Tx>, Vec<Hash32>) {
        let (max_txs, max_bytes) = (
            self.exec.rules().max_block_txs,
            self.exec.rules().max_block_bytes,
        );
        let exec = &self.exec;
        self.state.speculate(|s| {
            let (mut chosen, mut bad, mut bytes) = (Vec::new(), Vec::new(), 0usize);
            for tx in candidates {
                if chosen.len() >= max_txs {
                    break;
                }
                let size = tx.size();
                if bytes + size > max_bytes {
                    break;
                }
                if exec.apply_tx(s, tx).is_ok() {
                    bytes += size;
                    chosen.push(tx.clone());
                } else {
                    bad.push(tx.id());
                }
            }
            (chosen, bad)
        })
    }

    /// Checks **without applying**: certificate, proposer, signer set and (via the ledger) the rest.
    fn check(&self, d: &DecidedBlock) -> Result<(), ChainError> {
        let b = &d.block;
        if d.cert.height != b.height || b.height != self.height() + 1 {
            return Err(ChainError::Block("height"));
        }
        if d.cert.block_hash != b.hash() {
            return Err(ChainError::Block("certificate is for another block"));
        }
        let validators = self.validators();
        d.cert.verify(&self.genesis.chain_id, &validators)?;
        let want =
            proposer_of(&validators, b.height, d.cert.round).map(|k| Address::from_public_key(&k));
        if want != Some(b.proposer) {
            return Err(ChainError::Block("not the proposer of the round"));
        }
        if b.signers != self.signers() {
            return Err(ChainError::Block("signers are not the validator set"));
        }
        Ok(())
    }

    /// Applies a finalized block. **All or nothing.** If the block has just gone through `validate`, the transaction
    /// signatures are not checked again.
    pub fn apply(&mut self, d: DecidedBlock) -> Result<BlockReceipt, ChainError> {
        self.check(&d)?;
        let hash = d.block.hash();
        let receipt = if self.validated == Some(hash) {
            self.exec.apply_block_presigned(&mut self.state, &d.block)?
        } else {
            self.exec.apply_block(&mut self.state, &d.block)?
        };
        self.validated = None;
        self.blocks.push(d);
        Ok(receipt)
    }

    /// What a validator checks before voting on a proposal: the round's proposer, the signer set
    /// and the full ledger dry run (without altering the chain). Remembers the last approved block for `apply`.
    pub fn validate(&mut self, block: &Block, round: u64) -> Result<(), ChainError> {
        let want = proposer_of(&self.validators(), block.height, round)
            .map(|k| Address::from_public_key(&k));
        if want != Some(block.proposer) {
            return Err(ChainError::Block("not the proposer of the round"));
        }
        if block.signers != self.signers() {
            return Err(ChainError::Block("signers are not the validator set"));
        }
        self.exec.dry_run_block(&mut self.state, block)?;
        self.validated = Some(block.hash());
        Ok(())
    }
}

/// `validators[(height + round) % n]`, the same rule as the BFT engine.
#[must_use]
pub fn proposer_of(validators: &[PublicKey], height: u64, round: u64) -> Option<PublicKey> {
    let n = u64::try_from(validators.len()).ok().filter(|n| *n > 0)?;
    let i = usize::try_from(height.wrapping_add(round) % n).ok()?;
    validators.get(i).copied()
}

/// The state before block 1: just the initial validator set and its limits (rotation, ADR-0015).
fn seed_state(g: &Genesis) -> State {
    let mut s = State::new();
    brdu_ledger::gov::init(
        &mut s,
        &g.validators,
        g.min_validators as usize,
        g.max_validators as usize,
    );
    s
}
