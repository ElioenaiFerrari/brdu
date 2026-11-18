//! Regras da rede, fixadas na gênese: taxas, limites e emissores (ADR-0002, 0008, 0014).

use brdu_crypto::PublicKey;

use crate::error::LedgerError;

/// Regras imutáveis de uma rede. O estado muda; as regras não.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// Identificador da rede (entra em toda assinatura: impede reaproveitar transação em outra rede).
    pub chain_id: String,
    /// Taxa mínima base de uma transferência, em unidades mínimas (padrão 1.000 = 0,001 BRDU).
    pub transfer_base: u64,
    /// Taxa mínima por byte da transação codificada (padrão 1).
    pub per_byte: u64,
    /// Transações por bloco (padrão 1.000).
    pub max_block_txs: usize,
    /// Bytes de transações por bloco (padrão 1.000.000).
    pub max_block_bytes: usize,
    /// Os emissores (chaves públicas, sem repetição, em ordem).
    issuers: Vec<PublicKey>,
    /// Quantas aprovações distintas de emissores são exigidas (M de N).
    issuer_threshold: usize,
}

impl Rules {
    /// Taxa base padrão (0,001 BRDU).
    pub const DEFAULT_TRANSFER_BASE: u64 = 1_000;
    /// Taxa por byte padrão.
    pub const DEFAULT_PER_BYTE: u64 = 1;
    /// Transações por bloco padrão.
    pub const DEFAULT_MAX_BLOCK_TXS: usize = 1_000;
    /// Bytes por bloco padrão.
    pub const DEFAULT_MAX_BLOCK_BYTES: usize = 1_000_000;

    /// Regras com os padrões, para `chain_id`, `issuers` e o limiar M. Falha se o limiar não estiver em
    /// `1..=N` ou se não houver emissores.
    pub fn new(
        chain_id: &str,
        issuers: &[PublicKey],
        issuer_threshold: usize,
    ) -> Result<Self, LedgerError> {
        let mut sorted: Vec<PublicKey> = issuers.to_vec();
        sorted.sort();
        sorted.dedup();
        if sorted.is_empty()
            || issuer_threshold == 0
            || issuer_threshold > sorted.len()
            || chain_id.is_empty()
        {
            return Err(LedgerError::BadRules);
        }
        Ok(Self {
            chain_id: chain_id.to_string(),
            transfer_base: Self::DEFAULT_TRANSFER_BASE,
            per_byte: Self::DEFAULT_PER_BYTE,
            max_block_txs: Self::DEFAULT_MAX_BLOCK_TXS,
            max_block_bytes: Self::DEFAULT_MAX_BLOCK_BYTES,
            issuers: sorted,
            issuer_threshold,
        })
    }

    /// Os emissores.
    #[must_use]
    pub fn issuers(&self) -> &[PublicKey] {
        &self.issuers
    }

    /// O limiar M.
    #[must_use]
    pub const fn issuer_threshold(&self) -> usize {
        self.issuer_threshold
    }

    /// `key` é um emissor?
    #[must_use]
    pub fn is_issuer(&self, key: &PublicKey) -> bool {
        self.issuers.binary_search(key).is_ok()
    }
}
