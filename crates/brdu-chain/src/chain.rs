use brdu_bft::Certificate;
use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey};
use brdu_ledger::{Block, BlockReceipt, Executor, State};

use crate::error::ChainError;
use crate::genesis::Genesis;

/// Um bloco final: o bloco e o certificado de commit (precommits de mais de 2/3 dos validadores).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecidedBlock {
    /// O bloco.
    pub block: Block,
    /// O certificado.
    pub cert: Certificate,
}

impl DecidedBlock {
    /// Codificação determinística.
    pub fn encode(&self) -> Result<Vec<u8>, ChainError> {
        let mut e = Encoder::new();
        e.bytes(&self.block.encode()?).bytes(&self.cert.encode());
        Ok(e.finish()?)
    }

    /// Lê um bloco final (o bloco tem no máximo `max_txs` transações).
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

/// Réplica verificada: só avança por `apply`, que confere **tudo** (certificado, proponente, conjunto, execução).
pub struct Chain {
    genesis: Genesis,
    genesis_hash: Hash32,
    exec: Executor,
    state: State,
    blocks: Vec<DecidedBlock>,
    validated: Option<Hash32>,
}

impl Chain {
    /// Cria a cadeia vazia a partir da gênese.
    pub fn new(genesis: Genesis) -> Result<Self, ChainError> {
        Self::with_executor(genesis, Executor::new)
    }

    /// Como `new`, com um executor customizado (módulos, threads de verificação).
    pub fn with_executor(
        genesis: Genesis,
        make: impl FnOnce(brdu_ledger::Rules) -> Executor,
    ) -> Result<Self, ChainError> {
        let rules = genesis.rules()?;
        Ok(Self {
            genesis_hash: genesis.hash(),
            exec: make(rules).with_module(Box::new(brdu_ledger::gov::GovModule)),
            state: seed_state(&genesis),
            blocks: Vec::new(),
            validated: None,
            genesis,
        })
    }

    /// A gênese.
    #[must_use]
    pub const fn genesis(&self) -> &Genesis {
        &self.genesis
    }

    /// O hash da gênese.
    #[must_use]
    pub const fn genesis_hash(&self) -> &Hash32 {
        &self.genesis_hash
    }

    /// O executor (regras, taxa mínima).
    #[must_use]
    pub const fn executor(&self) -> &Executor {
        &self.exec
    }

    /// O estado atual.
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    /// Altura (número de blocos). O próximo tem altura `height() + 1`.
    #[must_use]
    pub const fn height(&self) -> u64 {
        self.blocks.len() as u64
    }

    /// Os blocos finais.
    #[must_use]
    pub fn blocks(&self) -> &[DecidedBlock] {
        &self.blocks
    }

    /// Blocos com altura maior que `after`.
    #[must_use]
    pub fn blocks_after(&self, after: u64) -> &[DecidedBlock] {
        let i = usize::try_from(after)
            .unwrap_or(usize::MAX)
            .min(self.blocks.len());
        self.blocks.get(i..).unwrap_or(&[])
    }

    /// Os validadores, na ordem do rodízio.
    #[must_use]
    pub fn validators(&self) -> Vec<PublicKey> {
        brdu_ledger::gov::validators(&self.state)
    }

    /// A época do conjunto de validadores (0 = gênese; sobe a cada troca efetivada).
    #[must_use]
    pub fn epoch(&self) -> u64 {
        brdu_ledger::gov::epoch(&self.state)
    }

    /// A troca de validadores pendente, se houver.
    #[must_use]
    pub fn pending_change(&self) -> Option<brdu_ledger::gov::Pending> {
        brdu_ledger::gov::pending(&self.state)
    }

    /// Os endereços dos validadores, ordenados (o `signers` do cabeçalho).
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

    /// O proponente da `round` na altura do próximo bloco.
    #[must_use]
    pub fn proposer(&self, round: u64) -> Option<PublicKey> {
        proposer_of(&self.validators(), self.height() + 1, round)
    }

    /// Monta o próximo bloco com `txs` (sem alterar a cadeia). `proposer` tem de ser o da rodada, o que o
    /// `apply` confere depois.
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

    /// Escolhe, **em ordem**, as transações de `candidates` que valem juntas no próximo bloco (até os limites de
    /// transações e de bytes), ensaiando cada uma sobre o estado e desfazendo tudo ao fim. Devolve as escolhidas e
    /// as que falharam (o chamador as descarta).
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

    /// Confere **sem aplicar**: certificado, proponente, conjunto de signatários e (via ledger) o resto.
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

    /// Aplica um bloco final. **Tudo ou nada.** Se o bloco acabou de passar por `validate`, as assinaturas das
    /// transações não são conferidas de novo.
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

    /// O que um validador confere antes de votar numa proposta: o proponente da rodada, o conjunto de signatários
    /// e o ensaio completo do ledger (sem alterar a cadeia). Lembra o último bloco aprovado para `apply`.
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

/// `validators[(altura + rodada) % n]`, a mesma regra do motor BFT.
#[must_use]
pub fn proposer_of(validators: &[PublicKey], height: u64, round: u64) -> Option<PublicKey> {
    let n = u64::try_from(validators.len()).ok().filter(|n| *n > 0)?;
    let i = usize::try_from(height.wrapping_add(round) % n).ok()?;
    validators.get(i).copied()
}

/// O estado antes do bloco 1: só o conjunto inicial de validadores e seus limites (rotação, ADR-0015).
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
