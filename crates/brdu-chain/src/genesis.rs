use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, tagged};
use brdu_ledger::Rules;

use crate::error::ChainError;

/// Prefixo de domínio do hash da gênese.
pub const GENESIS_TAG: &str = "brdu/genesis";
const MAX_SET: usize = 256;

/// O ato de fundação de uma rede: tudo o que os nós precisam combinar antes do bloco 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genesis {
    /// Nome da rede (entra em toda assinatura).
    pub chain_id: String,
    /// Validadores, **na ordem** que decide o proponente: `validators[(altura + rodada) % n]`.
    pub validators: Vec<PublicKey>,
    /// Emissores (mint, burn, atestado), conjunto separado dos validadores (ADR-0010).
    pub issuers: Vec<PublicKey>,
    /// Quantos emissores têm de aprovar (M de N).
    pub issuer_threshold: u32,
    /// Taxa base de transferência.
    pub transfer_base: u64,
    /// Taxa por byte.
    pub per_byte: u64,
    /// Máximo de transações por bloco.
    pub max_block_txs: u32,
    /// Máximo de bytes por bloco.
    pub max_block_bytes: u32,
    /// Menor tamanho que o conjunto de validadores pode ter (rotação).
    pub min_validators: u32,
    /// Maior tamanho do conjunto (no máximo 32).
    pub max_validators: u32,
}

impl Genesis {
    /// Gênese com as regras padrão.
    #[must_use]
    pub fn new(
        chain_id: &str,
        validators: Vec<PublicKey>,
        issuers: Vec<PublicKey>,
        issuer_threshold: u32,
    ) -> Self {
        Self {
            chain_id: chain_id.to_string(),
            validators,
            issuers,
            issuer_threshold,
            transfer_base: Rules::DEFAULT_TRANSFER_BASE,
            per_byte: Rules::DEFAULT_PER_BYTE,
            max_block_txs: u32::try_from(Rules::DEFAULT_MAX_BLOCK_TXS).unwrap_or(u32::MAX),
            max_block_bytes: u32::try_from(Rules::DEFAULT_MAX_BLOCK_BYTES).unwrap_or(u32::MAX),
            min_validators: 1,
            max_validators: 21,
        }
    }

    /// Confere a gênese e monta as regras do ledger.
    pub fn rules(&self) -> Result<Rules, ChainError> {
        if self.validators.is_empty() || self.validators.len() > MAX_SET {
            return Err(ChainError::Genesis("validator count"));
        }
        let (min, max) = (self.min_validators as usize, self.max_validators as usize);
        if min == 0 || max > 32 || min > self.validators.len() || self.validators.len() > max {
            return Err(ChainError::Genesis("validator limits"));
        }
        let mut seen = self.validators.clone();
        seen.sort();
        seen.dedup();
        if seen.len() != self.validators.len() {
            return Err(ChainError::Genesis("duplicate validator"));
        }
        let mut rules = Rules::new(
            &self.chain_id,
            &self.issuers,
            self.issuer_threshold as usize,
        )
        .map_err(|_| ChainError::Genesis("issuers or threshold"))?;
        rules.transfer_base = self.transfer_base;
        rules.per_byte = self.per_byte;
        rules.max_block_txs = self.max_block_txs as usize;
        rules.max_block_bytes = self.max_block_bytes as usize;
        Ok(rules)
    }

    /// Os endereços dos validadores, **ordenados**: o `signers` de todo bloco.
    #[must_use]
    pub fn signer_addresses(&self) -> Vec<Address> {
        let mut v: Vec<Address> = self
            .validators
            .iter()
            .map(Address::from_public_key)
            .collect();
        v.sort();
        v
    }

    /// Codificação determinística.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.string(&self.chain_id).count(self.validators.len());
        for v in &self.validators {
            e.fixed(v.as_bytes());
        }
        e.count(self.issuers.len());
        for i in &self.issuers {
            e.fixed(i.as_bytes());
        }
        e.u32(self.issuer_threshold)
            .u64(self.transfer_base)
            .u64(self.per_byte)
            .u32(self.max_block_txs)
            .u32(self.max_block_bytes)
            .u32(self.min_validators)
            .u32(self.max_validators);
        e.finish().unwrap_or_default()
    }

    /// Lê uma gênese.
    pub fn decode(bytes: &[u8]) -> Result<Self, ChainError> {
        let mut d = Decoder::new(bytes);
        let chain_id = d.string(64)?.to_string();
        let nv = d.count(MAX_SET)?;
        let mut validators = Vec::with_capacity(nv);
        for _ in 0..nv {
            validators.push(PublicKey(d.fixed::<32>()?));
        }
        let ni = d.count(MAX_SET)?;
        let mut issuers = Vec::with_capacity(ni);
        for _ in 0..ni {
            issuers.push(PublicKey(d.fixed::<32>()?));
        }
        let g = Self {
            chain_id,
            validators,
            issuers,
            issuer_threshold: d.u32()?,
            transfer_base: d.u64()?,
            per_byte: d.u64()?,
            max_block_txs: d.u32()?,
            max_block_bytes: d.u32()?,
            min_validators: d.u32()?,
            max_validators: d.u32()?,
        };
        d.finish()?;
        Ok(g)
    }

    /// O hash que identifica a rede: dois nós só se falam se concordam nele.
    #[must_use]
    pub fn hash(&self) -> Hash32 {
        tagged(GENESIS_TAG, &[&self.encode()])
    }
}
