//! O executor: aplica transações e blocos ao estado, de forma **determinística e atômica**.
//!
//! Ordem das conferências de uma transação: forma e assinatura → rede → taxa mínima → nonce → débito da taxa →
//! regra do tipo. Se qualquer passo falha, nada do que a transação mudou fica.
//!
//! **Bloco:** vale ou não vale por inteiro. Uma transação inválida derruba o bloco. A taxa de cada transação é
//! cobrada do remetente e, no fim do bloco, **repartida igualmente entre os validadores** do cabeçalho; o resto
//! da divisão vai ao proponente (ADR-0027).

use std::collections::HashSet;

use brdu_crypto::sign::verify_all;
use brdu_crypto::{Address, Hash32, PublicKey, Signature};

use crate::block::{Block, MAX_SIGNERS};
use crate::coin;
use crate::error::LedgerError;
use crate::rules::Rules;
use crate::state::State;
use crate::tx::{Tx, kind};

/// Contexto que um módulo recebe ao executar uma transação.
#[derive(Debug, Clone, Copy)]
pub struct TxCtx {
    /// O endereço do remetente.
    pub sender: Address,
    /// A hora do bloco (ou a do estado, ao validar para o mempool).
    pub time: u64,
    /// A altura do bloco em construção.
    pub height: u64,
}

/// Um módulo (bens, credenciais, contratos) trata um intervalo de tipos de transação (5 a 21).
pub trait Module: Send + Sync {
    /// Os tipos que este módulo executa.
    fn kinds(&self) -> core::ops::RangeInclusive<u8>;

    /// Taxa mínima de uma transação do módulo (padrão: zero).
    fn min_fee(&self, _rules: &Rules, _tx: &Tx) -> u64 {
        0
    }

    /// Gancho ao **fim de cada bloco** (depois das transações e da repartição das taxas): é onde um módulo
    /// aplica mudanças agendadas (por exemplo, a troca do conjunto de validadores). Padrão: nada.
    fn end_block(
        &self,
        _state: &mut State,
        _rules: &Rules,
        _height: u64,
    ) -> Result<(), LedgerError> {
        Ok(())
    }

    /// Executa. A assinatura, a rede, a taxa e o nonce já foram tratados.
    fn execute(
        &self,
        state: &mut State,
        rules: &Rules,
        ctx: &TxCtx,
        tx: &Tx,
    ) -> Result<(), LedgerError>;
}

/// O resultado de aplicar um bloco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockReceipt {
    /// Hash do bloco.
    pub hash: Hash32,
    /// Quantas transações.
    pub tx_count: usize,
    /// Total de taxas repartidas.
    pub fees: u64,
}

/// Aplica transações e blocos conforme as [`Rules`] da rede.
pub struct Executor {
    rules: Rules,
    modules: Vec<Box<dyn Module>>,
    verify_threads: usize,
}

impl Executor {
    /// Um executor só com a moeda.
    #[must_use]
    pub fn new(rules: Rules) -> Self {
        Self {
            rules,
            modules: Vec::new(),
            verify_threads: 1,
        }
    }

    /// Registra um módulo.
    #[must_use]
    pub fn with_module(mut self, module: Box<dyn Module>) -> Self {
        self.modules.push(module);
        self
    }

    /// Quantas threads conferem as assinaturas de um bloco (padrão 1; só vale a pena com muitas transações e CPU).
    #[must_use]
    pub fn with_verify_threads(mut self, threads: usize) -> Self {
        self.verify_threads = threads.max(1);
        self
    }

    /// As regras da rede.
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

    /// Taxa mínima de `tx`: transferência = base + por_byte × tamanho; mint, burn e atestado = 0 (exigem M
    /// emissores); módulos definem a sua. Tipo desconhecido é erro.
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

    /// Valida e aplica **uma** transação ao `state` (usado no mempool). A taxa é debitada do remetente (no estado
    /// de um bloco ela é repartida aos validadores; num estado "pendente" ela simplesmente sai da conta).
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

    // --- blocos --------------------------------------------------------------------------------

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
        // signatários: não vazio, estritamente crescente (sem repetição) e com o proponente
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

    /// Confere forma e assinatura de todas as transações (a parte mais cara de um bloco).
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

    /// Executa as transações do bloco, reparte as taxas e posiciona o estado. Não confere a raiz.
    fn run_block(&self, s: &mut State, block: &Block) -> Result<u64, LedgerError> {
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

    /// Aplica um bloco **inteiro ou nada**. Confere ligação, hora, limites, assinaturas, cada transação e a raiz de
    /// estado do cabeçalho.
    pub fn apply_block(
        &self,
        state: &mut State,
        block: &Block,
    ) -> Result<BlockReceipt, LedgerError> {
        self.check_block_shape(state, block)?;
        self.verify_signatures(&block.txs)?;
        self.apply_block_presigned(state, block)
    }

    /// Ensaio de um bloco: confere **tudo** (inclusive a raiz) e desfaz. É o que um validador faz antes de votar.
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

    /// Como `apply_block`, mas **sem reconferir as assinaturas**: só para quem acabou de conferi-las (o ensaio de
    /// `dry_run_block` do mesmo bloco). Forma, ligação e execução continuam conferidas.
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

    /// Monta o bloco seguinte com `txs` (todas têm de valer, em ordem) e calcula a raiz de estado **sem alterar**
    /// `state` (tudo é desfeito ao fim).
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
