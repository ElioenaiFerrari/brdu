//! The coin (kinds 1 to 4): transfer, mint, burn and reserve attestation (ADR-0002, 0006).
//!
//! - **Mint and burn** require **M distinct issuer signatures** over a document bound to the network, the account, the
//!   amount and the reference. Each deposit reference counts **once** (per kind).
//! - **Burn** by the owner only: whoever signs the transaction is the holder of the account that burns.
//! - **Attestation**: M issuers declare the total reais in the vault, with sequence number `last + 1`.
//! - Nobody freezes, reverses or recovers anything (ADR-0006).

use std::collections::BTreeSet;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, SecretKey, Signature, tagged, tags};

use crate::error::LedgerError;
use crate::rules::Rules;
use crate::state::{Attestation, State};
use crate::tx::kind;

/// Largest deposit reference, in bytes.
pub const MAX_REF: usize = 64;
/// More approvals than this in one transaction is abuse.
pub const MAX_APPROVALS: usize = 32;
/// Size of an approval: public key (32) + signature (64).
pub const APPROVAL_LEN: usize = 96;

/// An issuer's approval of a document: `public key ‖ signature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Approval {
    /// The issuer.
    pub issuer: PublicKey,
    /// The signature over the document (the 32 hash bytes).
    pub signature: Signature,
}

impl Approval {
    /// The issuer `key` approves the document `doc`.
    #[must_use]
    pub fn new(doc: &Hash32, key: &SecretKey) -> Self {
        Self {
            issuer: key.public(),
            signature: key.sign(doc.as_bytes()),
        }
    }

    /// `public key ‖ signature` (96 bytes).
    #[must_use]
    pub fn to_bytes(&self) -> [u8; APPROVAL_LEN] {
        let mut out = [0u8; APPROVAL_LEN];
        out[..32].copy_from_slice(self.issuer.as_bytes());
        out[32..].copy_from_slice(self.signature.as_bytes());
        out
    }

    /// Reads 96 bytes.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (pk, sig) = bytes.split_at_checked(32)?;
        Some(Self {
            issuer: PublicKey(pk.try_into().ok()?),
            signature: Signature(sig.try_into().ok()?),
        })
    }
}

// --- documents the issuers sign ----------------------------------------------------------------------------

/// Document of a mint (`burn = false`) or burn (`burn = true`).
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

/// Like [`issuance_doc`], with another domain prefix (the BRDC vectors use `brdc/issuance/…`).
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

/// Document of a reserve attestation.
#[must_use]
pub fn attestation_doc(chain_id: &str, seq: u64, reserve: u64, reference: &str) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain_id).u64(seq).u64(reserve).string(reference);
    tagged(tags::ATTESTATION, &[&e.finish().unwrap_or_default()])
}

// --- payloads ----------------------------------------------------------------------------------------------

/// Transfer payload: `destination (32) ‖ amount (u64)`.
#[must_use]
pub fn transfer_payload(to: &Address, amount: u64) -> Vec<u8> {
    let mut e = Encoder::new();
    e.fixed(to.hash().as_bytes()).u64(amount);
    e.finish().unwrap_or_default()
}

/// Mint or burn payload: account, amount, unique reference and approvals.
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

/// Attestation payload: sequence, reserve, reference and approvals.
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

/// How many approvals are **valid and from distinct issuers** the document has.
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

// --- execution ---------------------------------------------------------------------------------------------

/// Executes a coin transaction (kinds 1 to 4). The fee, nonce and signature have already been handled.
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
