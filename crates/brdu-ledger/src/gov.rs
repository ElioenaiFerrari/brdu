//! Governance of the validator set (rotation, ADR-0015). The set in force is **chain state**
//! (`val/set`, `val/epoch`, `val/pending` in module storage), so it enters the state root and every node
//! reaches the same set at every height.
//!
//! A change (`add` or `remove` of **one** key) takes effect with **more than 2/3 of the set in force** approving the
//! exact document; only one change can be pending; the set stays between `min` and `max`; the effective height is
//! in the future. At the end of block `effective − 1` the new set takes effect and the epoch goes up.

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Hash32, PublicKey, tagged};

use crate::coin::{APPROVAL_LEN, Approval, MAX_APPROVALS};
use crate::error::LedgerError;
use crate::exec::{Module, TxCtx};
use crate::rules::Rules;
use crate::state::State;
use crate::tx::{Tx, kind};

/// Domain prefix of the document the validators sign.
pub const CHANGE_TAG: &str = "brdu/validators/change";
/// How many blocks in advance a change must be scheduled (minimum).
pub const MIN_LEAD: u64 = 2;

const K_SET: &[u8] = b"val/set";
const K_EPOCH: &[u8] = b"val/epoch";
const K_PENDING: &[u8] = b"val/pending";
const K_MIN: &[u8] = b"val/min";
const K_MAX: &[u8] = b"val/max";

/// What the change does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A validator joins (goes to the end of the rotation order).
    Add,
    /// A validator leaves.
    Remove,
}

impl Action {
    const fn byte(self) -> u8 {
        match self {
            Self::Add => 1,
            Self::Remove => 2,
        }
    }

    const fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Add),
            2 => Some(Self::Remove),
            _ => None,
        }
    }
}

/// A scheduled change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    /// What it does.
    pub action: Action,
    /// Who joins or leaves.
    pub key: PublicKey,
    /// The first height at which the new set takes effect.
    pub effective_height: u64,
}

// --- state reads -------------------------------------------------------------------------------------------

/// The validator set in force, **in rotation order**.
#[must_use]
pub fn validators(state: &State) -> Vec<PublicKey> {
    state
        .ext_get(K_SET)
        .and_then(decode_set)
        .unwrap_or_default()
}

/// The epoch (goes up with each change that takes effect; 0 is genesis).
#[must_use]
pub fn epoch(state: &State) -> u64 {
    num(state, K_EPOCH)
}

/// The pending change, if any.
#[must_use]
pub fn pending(state: &State) -> Option<Pending> {
    let raw = state.ext_get(K_PENDING)?;
    let mut d = Decoder::new(raw);
    let p = Pending {
        action: Action::from_byte(d.u8().ok()?)?,
        key: PublicKey(d.fixed::<32>().ok()?),
        effective_height: d.u64().ok()?,
    };
    d.finish().ok()?;
    Some(p)
}

/// Size limits of the set: `(minimum, maximum)`.
#[must_use]
pub fn limits(state: &State) -> (usize, usize) {
    (num(state, K_MIN) as usize, num(state, K_MAX) as usize)
}

fn num(state: &State, key: &[u8]) -> u64 {
    state
        .ext_get(key)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map_or(0, u64::from_be_bytes)
}

fn put_num(state: &mut State, key: &[u8], v: u64) {
    state.ext_put(key.to_vec(), v.to_be_bytes().to_vec());
}

fn encode_set(set: &[PublicKey]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.count(set.len());
    for k in set {
        e.fixed(k.as_bytes());
    }
    e.finish().unwrap_or_default()
}

fn decode_set(raw: &[u8]) -> Option<Vec<PublicKey>> {
    let mut d = Decoder::new(raw);
    let n = d.count(256).ok()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(PublicKey(d.fixed::<32>().ok()?));
    }
    d.finish().ok()?;
    Some(out)
}

/// Writes the initial set (genesis). Only `Chain` calls this, once, before block 1.
pub fn init(state: &mut State, set: &[PublicKey], min: usize, max: usize) {
    state.ext_put(K_SET.to_vec(), encode_set(set));
    put_num(state, K_EPOCH, 0);
    put_num(state, K_MIN, min as u64);
    put_num(state, K_MAX, max as u64);
}

// --- document and payload ----------------------------------------------------------------------------------

/// The document the validators sign: network, action, key, effective height and **current epoch** (prevents replaying
/// an approval after another change).
#[must_use]
pub fn change_doc(
    chain_id: &str,
    action: Action,
    key: &PublicKey,
    effective_height: u64,
    epoch: u64,
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain_id)
        .u8(action.byte())
        .fixed(key.as_bytes())
        .u64(effective_height)
        .u64(epoch);
    tagged(CHANGE_TAG, &[&e.finish().unwrap_or_default()])
}

/// The `VALIDATOR_CHANGE` transaction payload.
#[must_use]
pub fn change_payload(
    action: Action,
    key: &PublicKey,
    effective_height: u64,
    approvals: &[Approval],
) -> Vec<u8> {
    let mut e = Encoder::new();
    e.u8(action.byte())
        .fixed(key.as_bytes())
        .u64(effective_height)
        .count(approvals.len());
    for a in approvals {
        e.fixed(&a.to_bytes());
    }
    e.finish().unwrap_or_default()
}

struct Change {
    action: Action,
    key: PublicKey,
    effective_height: u64,
    approvals: Vec<Approval>,
}

fn decode_change(payload: &[u8]) -> Result<Change, LedgerError> {
    let mut d = Decoder::new(payload);
    let action = Action::from_byte(d.u8()?).ok_or(LedgerError::Malformed)?;
    let key = PublicKey(d.fixed::<32>()?);
    let effective_height = d.u64()?;
    let n = d.count(MAX_APPROVALS)?;
    let mut approvals = Vec::with_capacity(n);
    for _ in 0..n {
        approvals
            .push(Approval::from_bytes(&d.fixed::<APPROVAL_LEN>()?).ok_or(LedgerError::Malformed)?);
    }
    d.finish()?;
    Ok(Change {
        action,
        key,
        effective_height,
        approvals,
    })
}

// --- the module --------------------------------------------------------------------------------------------

/// The module for governance of the validator set (kind 22).
pub struct GovModule;

impl Module for GovModule {
    fn kinds(&self) -> core::ops::RangeInclusive<u8> {
        kind::VALIDATOR_CHANGE..=kind::VALIDATOR_CHANGE
    }

    fn execute(
        &self,
        state: &mut State,
        rules: &Rules,
        ctx: &TxCtx,
        tx: &Tx,
    ) -> Result<(), LedgerError> {
        let c = decode_change(&tx.payload)?;
        let set = validators(state);
        let (min, max) = limits(state);
        if pending(state).is_some() {
            return Err(LedgerError::Module("a validator change is already pending"));
        }
        if c.effective_height < ctx.height.saturating_add(MIN_LEAD) {
            return Err(LedgerError::Module(
                "the effective height must be in the future",
            ));
        }
        let present = set.contains(&c.key);
        let new_len = match c.action {
            Action::Add if present => {
                return Err(LedgerError::Module("the key is already a validator"));
            }
            Action::Remove if !present => {
                return Err(LedgerError::Module("the key is not a validator"));
            }
            Action::Add => set.len() + 1,
            Action::Remove => set.len() - 1,
        };
        if new_len < min || new_len > max {
            return Err(LedgerError::Module("the set would leave the size limits"));
        }
        let doc = change_doc(
            &rules.chain_id,
            c.action,
            &c.key,
            c.effective_height,
            epoch(state),
        );
        let mut seen: Vec<PublicKey> = Vec::new();
        for a in &c.approvals {
            if set.contains(&a.issuer)
                && !seen.contains(&a.issuer)
                && a.issuer.verify(doc.as_bytes(), &a.signature).is_ok()
            {
                seen.push(a.issuer);
            }
        }
        if seen.len() * 3 <= set.len() * 2 {
            return Err(LedgerError::NotEnoughApprovals {
                valid: seen.len(),
                needed: set.len() * 2 / 3 + 1,
            });
        }
        let mut e = Encoder::new();
        e.u8(c.action.byte())
            .fixed(c.key.as_bytes())
            .u64(c.effective_height);
        state.ext_put(K_PENDING.to_vec(), e.finish().unwrap_or_default());
        Ok(())
    }

    fn end_block(&self, state: &mut State, _rules: &Rules, height: u64) -> Result<(), LedgerError> {
        let Some(p) = pending(state) else {
            return Ok(());
        };
        if p.effective_height != height.saturating_add(1) {
            return Ok(());
        }
        let mut set = validators(state);
        match p.action {
            Action::Add => set.push(p.key),
            Action::Remove => set.retain(|k| *k != p.key),
        }
        state.ext_put(K_SET.to_vec(), encode_set(&set));
        put_num(state, K_EPOCH, epoch(state) + 1);
        state.ext_remove(K_PENDING);
        Ok(())
    }
}
