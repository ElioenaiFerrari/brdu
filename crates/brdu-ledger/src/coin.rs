//! A moeda (tipos 1 a 4): transferência, mint, burn e atestado de reserva (ADR-0002, 0006).
//!
//! - **Mint e burn** exigem **M assinaturas distintas de emissores** sobre um documento amarrado à rede, à conta, ao
//!   valor e à referência. Cada referência de depósito vale **uma vez** (por tipo).
//! - **Burn** só do dono: quem assina a transação é o titular da conta que queima.
//! - **Atestado**: M emissores declaram o total em reais em caixa, com número de sequência `último + 1`.
//! - Ninguém congela, estorna ou recupera nada (ADR-0006).

use std::collections::BTreeSet;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, SecretKey, Signature, tagged, tags};

use crate::error::LedgerError;
use crate::rules::Rules;
use crate::state::{Attestation, State};
use crate::tx::kind;

/// Maior referência de depósito, em bytes.
pub const MAX_REF: usize = 64;
/// Mais aprovações que isto numa transação é abuso.
pub const MAX_APPROVALS: usize = 32;
/// Tamanho de uma aprovação: chave pública (32) + assinatura (64).
pub const APPROVAL_LEN: usize = 96;

/// Uma aprovação de emissor sobre um documento: `chave pública ‖ assinatura`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Approval {
    /// O emissor.
    pub issuer: PublicKey,
    /// A assinatura sobre o documento (os 32 bytes do hash).
    pub signature: Signature,
}

impl Approval {
    /// O emissor `key` aprova o documento `doc`.
    #[must_use]
    pub fn new(doc: &Hash32, key: &SecretKey) -> Self {
        Self {
            issuer: key.public(),
            signature: key.sign(doc.as_bytes()),
        }
    }

    /// `chave pública ‖ assinatura` (96 bytes).
    #[must_use]
    pub fn to_bytes(&self) -> [u8; APPROVAL_LEN] {
        let mut out = [0u8; APPROVAL_LEN];
        out[..32].copy_from_slice(self.issuer.as_bytes());
        out[32..].copy_from_slice(self.signature.as_bytes());
        out
    }

    /// Lê 96 bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (pk, sig) = bytes.split_at_checked(32)?;
        Some(Self {
            issuer: PublicKey(pk.try_into().ok()?),
            signature: Signature(sig.try_into().ok()?),
        })
    }
}

// --- documentos que os emissores assinam -------------------------------------------------------------------

/// Documento de um mint (`burn = false`) ou burn (`burn = true`).
#[must_use]
pub fn issuance_doc(
    burn: bool,
    chain_id: &str,
    account: &Address,
    amount: u64,
    reference: &str,
) -> Hash32 {
    issuance_doc_with_tag(
        if burn {
            tags::ISSUANCE_BURN
        } else {
            tags::ISSUANCE_MINT
        },
        chain_id,
        account,
        amount,
        reference,
    )
}

/// Como [`issuance_doc`], com outro prefixo de domínio (os vetores do BRDC usam `brdc/issuance/…`).
#[must_use]
pub fn issuance_doc_with_tag(
    tag: &str,
    chain_id: &str,
    account: &Address,
    amount: u64,
    reference: &str,
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain_id)
        .fixed(account.hash().as_bytes())
        .u64(amount)
        .string(reference);
    tagged(tag, &[&e.finish().unwrap_or_default()])
}

/// Documento de um atestado de reserva.
#[must_use]
pub fn attestation_doc(chain_id: &str, seq: u64, reserve: u64, reference: &str) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain_id).u64(seq).u64(reserve).string(reference);
    tagged(tags::ATTESTATION, &[&e.finish().unwrap_or_default()])
}

// --- cargas -----------------------------------------------------------------------------------------------

/// Carga de transferência: `destino (32) ‖ valor (u64)`.
#[must_use]
pub fn transfer_payload(to: &Address, amount: u64) -> Vec<u8> {
    let mut e = Encoder::new();
    e.fixed(to.hash().as_bytes()).u64(amount);
    e.finish().unwrap_or_default()
}

/// Carga de mint ou burn: conta, valor, referência única e aprovações.
#[must_use]
pub fn issuance_payload(
    account: &Address,
    amount: u64,
    reference: &str,
    approvals: &[Approval],
) -> Vec<u8> {
    let mut e = Encoder::new();
    e.fixed(account.hash().as_bytes())
        .u64(amount)
        .string(reference)
        .count(approvals.len());
    for a in approvals {
        e.bytes(&a.to_bytes());
    }
    e.finish().unwrap_or_default()
}

/// Carga de atestado: sequência, reserva, referência e aprovações.
#[must_use]
pub fn attestation_payload(
    seq: u64,
    reserve: u64,
    reference: &str,
    approvals: &[Approval],
) -> Vec<u8> {
    let mut e = Encoder::new();
    e.u64(seq)
        .u64(reserve)
        .string(reference)
        .count(approvals.len());
    for a in approvals {
        e.bytes(&a.to_bytes());
    }
    e.finish().unwrap_or_default()
}

fn read_approvals(d: &mut Decoder<'_>) -> Result<Vec<Approval>, LedgerError> {
    let n = d.count(MAX_APPROVALS).map_err(|_| LedgerError::Malformed)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let raw = d.bytes(APPROVAL_LEN).map_err(|_| LedgerError::Malformed)?;
        out.push(Approval::from_bytes(raw).ok_or(LedgerError::Malformed)?);
    }
    Ok(out)
}

fn read_ref<'a>(d: &mut Decoder<'a>) -> Result<&'a str, LedgerError> {
    let r = d.string(MAX_REF).map_err(|_| LedgerError::Malformed)?;
    if r.is_empty() {
        return Err(LedgerError::BadRef);
    }
    Ok(r)
}

/// Quantas aprovações **válidas e de emissores distintos** o documento tem.
fn valid_approvals(rules: &Rules, doc: &Hash32, approvals: &[Approval]) -> usize {
    let distinct: BTreeSet<PublicKey> = approvals
        .iter()
        .filter(|a| {
            rules.is_issuer(&a.issuer) && a.issuer.verify(doc.as_bytes(), &a.signature).is_ok()
        })
        .map(|a| a.issuer)
        .collect();
    distinct.len()
}

fn need_approvals(rules: &Rules, doc: &Hash32, approvals: &[Approval]) -> Result<(), LedgerError> {
    let valid = valid_approvals(rules, doc, approvals);
    if valid < rules.issuer_threshold() {
        return Err(LedgerError::NotEnoughApprovals {
            valid,
            needed: rules.issuer_threshold(),
        });
    }
    Ok(())
}

// --- execução ---------------------------------------------------------------------------------------------

/// Executa uma transação de moeda (tipos 1 a 4). A taxa, o nonce e a assinatura já foram tratados.
pub(crate) fn execute(
    state: &mut State,
    rules: &Rules,
    tx_kind: u8,
    sender: &Address,
    payload: &[u8],
) -> Result<(), LedgerError> {
    match tx_kind {
        kind::TRANSFER => transfer(state, sender, payload),
        kind::MINT => mint(state, rules, payload),
        kind::BURN => burn(state, rules, sender, payload),
        kind::ATTESTATION => attest(state, rules, payload),
        other => Err(LedgerError::UnknownKind(other)),
    }
}

fn transfer(state: &mut State, sender: &Address, payload: &[u8]) -> Result<(), LedgerError> {
    let mut d = Decoder::new(payload);
    let to = Address(Hash32(d.fixed::<32>().map_err(|_| LedgerError::Malformed)?));
    let amount = d.u64().map_err(|_| LedgerError::Malformed)?;
    d.finish().map_err(|_| LedgerError::Malformed)?;
    if amount == 0 {
        return Err(LedgerError::ZeroAmount);
    }
    state.debit(sender, amount)?;
    state.credit(&to, amount)
}

fn mint(state: &mut State, rules: &Rules, payload: &[u8]) -> Result<(), LedgerError> {
    let mut d = Decoder::new(payload);
    let account = Address(Hash32(d.fixed::<32>().map_err(|_| LedgerError::Malformed)?));
    let amount = d.u64().map_err(|_| LedgerError::Malformed)?;
    let reference = read_ref(&mut d)?.to_string();
    let approvals = read_approvals(&mut d)?;
    d.finish().map_err(|_| LedgerError::Malformed)?;
    if amount == 0 {
        return Err(LedgerError::ZeroAmount);
    }
    if state.ref_used(kind::MINT, &reference) {
        return Err(LedgerError::RefUsed);
    }
    need_approvals(
        rules,
        &issuance_doc(false, &rules.chain_id, &account, amount, &reference),
        &approvals,
    )?;
    state.credit(&account, amount)?;
    state.add_minted(amount)?;
    state.use_ref(kind::MINT, &reference);
    Ok(())
}

fn burn(
    state: &mut State,
    rules: &Rules,
    sender: &Address,
    payload: &[u8],
) -> Result<(), LedgerError> {
    let mut d = Decoder::new(payload);
    let account = Address(Hash32(d.fixed::<32>().map_err(|_| LedgerError::Malformed)?));
    let amount = d.u64().map_err(|_| LedgerError::Malformed)?;
    let reference = read_ref(&mut d)?.to_string();
    let approvals = read_approvals(&mut d)?;
    d.finish().map_err(|_| LedgerError::Malformed)?;
    if account != *sender {
        return Err(LedgerError::NotOwner);
    }
    if amount == 0 {
        return Err(LedgerError::ZeroAmount);
    }
    if state.ref_used(kind::BURN, &reference) {
        return Err(LedgerError::RefUsed);
    }
    need_approvals(
        rules,
        &issuance_doc(true, &rules.chain_id, &account, amount, &reference),
        &approvals,
    )?;
    state.debit(&account, amount)?;
    state.add_burned(amount)?;
    state.use_ref(kind::BURN, &reference);
    Ok(())
}

fn attest(state: &mut State, rules: &Rules, payload: &[u8]) -> Result<(), LedgerError> {
    let mut d = Decoder::new(payload);
    let seq = d.u64().map_err(|_| LedgerError::Malformed)?;
    let reserve = d.u64().map_err(|_| LedgerError::Malformed)?;
    let reference = read_ref(&mut d)?.to_string();
    let approvals = read_approvals(&mut d)?;
    d.finish().map_err(|_| LedgerError::Malformed)?;
    let expected = state.attestation().map_or(1, |a| a.seq.saturating_add(1));
    if seq != expected {
        return Err(LedgerError::BadSequence { expected, got: seq });
    }
    need_approvals(
        rules,
        &attestation_doc(&rules.chain_id, seq, reserve, &reference),
        &approvals,
    )?;
    state.set_attestation(Attestation {
        seq,
        reserve,
        reference,
    });
    Ok(())
}
