use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, tagged};
use brdu_ledger::Rules;

use crate::error::ChainError;

/// Domain prefix of the genesis hash.
pub const GENESIS_TAG: &str = "brdu/genesis";
const MAX_SET: usize = 256;

/// A network's founding act: everything the nodes must agree on before block 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genesis {
    /// Network name (goes into every signature).
    pub chain_id: String,
    /// Validators, **in the order** that decides the proposer: `validators[(height + round) % n]`.
    pub validators: Vec<PublicKey>,
    /// Issuers (mint, burn, attestation), a set separate from the validators (ADR-0010).
    pub issuers: Vec<PublicKey>,
    /// How many issuers must approve (M of N).
    pub issuer_threshold: u32,
    /// Base transfer fee.
    pub transfer_base: u64,
    /// Fee per byte.
    pub per_byte: u64,
    /// Maximum transactions per block.
    pub max_block_txs: u32,
    /// Maximum bytes per block.
    pub max_block_bytes: u32,
    /// Smallest size the validator set may have (rotation).
    pub min_validators: u32,
    /// Largest size of the set (at most 32).
    pub max_validators: u32,
    /// Root authorities (M of N): they create credential types and asset classes. Empty = none can be created.
    pub authorities: Vec<PublicKey>,
    /// How many authority approvals are required (1 when there are no authorities).
    pub authority_threshold: u32,
    /// Smallest contest/veto window a credential type may declare, in seconds.
    pub min_window: u64,
    /// Largest window, in seconds.
    pub max_window: u64,
}

impl Genesis {
    /// Genesis with the default rules.
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
            authorities: Vec::new(),
            authority_threshold: 1,
            min_window: Rules::DEFAULT_MIN_WINDOW,
            max_window: Rules::DEFAULT_MAX_WINDOW,
        }
    }

    /// Checks the genesis and builds the ledger rules.
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
        let (na, ta) = (self.authorities.len(), self.authority_threshold as usize);
        let mut distinct = self.authorities.clone();
        distinct.sort();
        distinct.dedup();
        if na > 16 || distinct.len() != na || ta == 0 || (na > 0 && ta > na) {
            return Err(ChainError::Genesis("authorities or threshold"));
        }
        if self.min_window == 0 || self.min_window > self.max_window {
            return Err(ChainError::Genesis("windows"));
        }
        rules.min_window = self.min_window;
        rules.max_window = self.max_window;
        rules.authorities = self.authorities.clone();
        rules.authority_threshold = ta;
        rules.transfer_base = self.transfer_base;
        rules.per_byte = self.per_byte;
        rules.max_block_txs = self.max_block_txs as usize;
        rules.max_block_bytes = self.max_block_bytes as usize;
        Ok(rules)
    }

    /// The validators' addresses, **sorted**: the `signers` of every block.
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

    /// Deterministic encoding.
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
            .u32(self.max_validators)
            .count(self.authorities.len());
        for a in &self.authorities {
            e.fixed(a.as_bytes());
        }
        e.u32(self.authority_threshold)
            .u64(self.min_window)
            .u64(self.max_window);
        e.finish().unwrap_or_default()
    }

    /// Reads a genesis.
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
            authorities: {
                let n = d.count(16)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(PublicKey(d.fixed::<32>()?));
                }
                v
            },
            authority_threshold: d.u32()?,
            min_window: d.u64()?,
            max_window: d.u64()?,
        };
        d.finish()?;
        Ok(g)
    }

    /// The hash that identifies the network: two nodes only talk if they agree on it.
    #[must_use]
    pub fn hash(&self) -> Hash32 {
        tagged(GENESIS_TAG, &[&self.encode()])
    }
}
