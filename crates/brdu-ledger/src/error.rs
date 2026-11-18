//! Erros do livro-razão. Nenhuma mensagem carrega segredo.

use brdu_crypto::{DecodeError, Hash32};
use thiserror::Error;

/// Por que uma transação ou um bloco foi recusado.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LedgerError {
    // --- transação (forma e assinatura)
    /// Bytes que não são uma transação.
    #[error("transação malformada")]
    Malformed,
    /// A carga passa do limite.
    #[error("carga grande demais")]
    PayloadTooLarge,
    /// A assinatura não confere.
    #[error("assinatura inválida")]
    BadSignature,
    /// A transação é de outra rede.
    #[error("rede errada")]
    WrongChain,
    /// Tipo de transação que esta rede não conhece.
    #[error("tipo de transação desconhecido: {0}")]
    UnknownKind(u8),
    // --- contas e taxas
    /// O nonce não é o esperado (replay ou transação fora de ordem).
    #[error("nonce errado: esperado {expected}, veio {got}")]
    BadNonce {
        /// O próximo nonce da conta.
        expected: u64,
        /// O que veio na transação.
        got: u64,
    },
    /// A taxa é menor que a mínima.
    #[error("taxa abaixo do mínimo: mínimo {min}, veio {got}")]
    FeeTooLow {
        /// A taxa mínima.
        min: u64,
        /// A que veio.
        got: u64,
    },
    /// Saldo menor que o necessário.
    #[error("saldo insuficiente")]
    InsufficientFunds,
    /// A conta passaria de 64 bits.
    #[error("estouro de 64 bits")]
    Overflow,
    // --- moeda
    /// Valor zero.
    #[error("valor zero")]
    ZeroAmount,
    /// A referência de depósito já foi usada.
    #[error("referência já usada")]
    RefUsed,
    /// Referência vazia ou grande demais.
    #[error("referência inválida")]
    BadRef,
    /// Menos de M aprovações distintas de emissores.
    #[error("aprovações de emissores insuficientes: {valid} de {needed}")]
    NotEnoughApprovals {
        /// Quantas aprovações válidas e distintas.
        valid: usize,
        /// Quantas são exigidas.
        needed: usize,
    },
    /// Só o dono da conta pode queimar o próprio saldo.
    #[error("só o dono queima o próprio saldo")]
    NotOwner,
    /// O número de sequência do atestado não é o seguinte.
    #[error("sequência do atestado errada: esperada {expected}, veio {got}")]
    BadSequence {
        /// A sequência esperada.
        expected: u64,
        /// A que veio.
        got: u64,
    },
    // --- bloco
    /// O bloco não é o seguinte (altura ou hash anterior).
    #[error("bloco fora de lugar")]
    BadLinkage,
    /// A hora do bloco volta no tempo.
    #[error("hora do bloco anterior à do estado")]
    TimeWentBack,
    /// Mais transações ou bytes que o limite.
    #[error("bloco grande demais")]
    BlockTooLarge,
    /// A mesma transação duas vezes no bloco.
    #[error("transação repetida no bloco")]
    DuplicateTx(Hash32),
    /// A raiz de estado do cabeçalho não bate com a execução.
    #[error("raiz de estado diferente da calculada")]
    StateRootMismatch,
    /// Uma transação do bloco é inválida (o bloco todo é recusado).
    #[error("transação {id} inválida: {reason}")]
    InvalidTx {
        /// Qual.
        id: Hash32,
        /// Por quê.
        reason: Box<LedgerError>,
    },
    /// Falha ao ler bytes codificados.
    #[error("decodificação: {0}")]
    Decode(#[from] DecodeError),
    /// Uma regra do módulo recusou (módulos de bens, credenciais, contratos).
    #[error("módulo: {0}")]
    Module(&'static str),
    /// Regras da rede inválidas (limiar fora de 1..=emissores, por exemplo).
    #[error("regras da rede inválidas")]
    BadRules,
}

impl LedgerError {
    /// Um rótulo estável e curto, para contar recusas por motivo (fofoca, métricas).
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::PayloadTooLarge => "payload_too_large",
            Self::BadSignature => "bad_signature",
            Self::WrongChain => "wrong_chain",
            Self::UnknownKind(_) => "unknown_kind",
            Self::BadNonce { .. } => "bad_nonce",
            Self::FeeTooLow { .. } => "fee_too_low",
            Self::InsufficientFunds => "insufficient_funds",
            Self::Overflow => "overflow",
            Self::ZeroAmount => "zero_amount",
            Self::RefUsed => "ref_used",
            Self::BadRef => "bad_ref",
            Self::NotEnoughApprovals { .. } => "not_enough_approvals",
            Self::NotOwner => "not_owner",
            Self::BadSequence { .. } => "bad_sequence",
            Self::BadLinkage => "bad_linkage",
            Self::TimeWentBack => "time_went_back",
            Self::BlockTooLarge => "block_too_large",
            Self::DuplicateTx(_) => "duplicate_tx",
            Self::StateRootMismatch => "state_root_mismatch",
            Self::InvalidTx { .. } => "invalid_tx",
            Self::Decode(_) => "decode",
            Self::Module(_) => "module",
            Self::BadRules => "bad_rules",
        }
    }
}
