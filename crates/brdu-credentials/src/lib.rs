//! Native credentials (ADR-0019 of the BRDC, adapted): declarations by an **organ** about a **subject**. The chain
//! does not know whether they are true: it keeps **who declared what, when, and whether it still holds**.
//!
//! - A **type** (created by the root authorities, M of N) fixes the organ (keys + M of N), the **contest window**,
//!   the **validity**, the **privacy** (public data, or only a 32-byte commitment) and an optional **reference** to
//!   another type (a divorce references a marriage and, when activated, revokes it).
//! - An **issue** (the organ, M of N) is born **pending** and only holds after the window. The **subject** may
//!   **contest**, the organ may **veto** (M of N), anyone may **activate** after the window, the organ may
//!   **revoke** (M of N). The `cred_id` is burned forever: an approval can never issue twice.
//! - Transaction kinds: 14 type, 15 issue, 16 contest, 17 veto, 18 activate, 19 revoke.
//!
//! Pure: "now" is the time of the block being executed.
#![forbid(unsafe_code)]

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, tagged};
use brdu_ledger::approvals;
use brdu_ledger::coin::Approval;
use brdu_ledger::{LedgerError, Module, Rules, State, Tx, TxCtx};

/// Transaction kinds of this module.
pub mod kind {
    /// Create a credential type (root authorities, M of N).
    pub const TYPE: u8 = 14;
    /// Issue a credential (the type's organ, M of N).
    pub const ISSUE: u8 = 15;
    /// The subject contests a pending credential.
    pub const CONTEST: u8 = 16;
    /// The organ vetoes a pending credential (M of N).
    pub const VETO: u8 = 17;
    /// Anyone activates a pending credential after its window.
    pub const ACTIVATE: u8 = 18;
    /// The organ revokes a pending or active credential (M of N).
    pub const REVOKE: u8 = 19;
}

/// Prefix of the documents the organs sign.
pub const DOC_PREFIX: &str = "brdu/cred";
const T_PREFIX: &[u8] = b"cred/t/";
const C_PREFIX: &[u8] = b"cred/c/";
/// Most bytes of public data in a credential.
pub const MAX_PUBLIC_DATA: usize = 512;
/// Longest revocation reason.
pub const MAX_REASON: usize = 128;

/// Whether the data of a credential is on the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// The data (up to 512 bytes) is on the chain.
    Public,
    /// Only a 32-byte commitment is on the chain; the holder reveals the data off-chain.
    Private,
}

/// A credential type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSpec {
    /// Unique id (1 to 32 printable bytes).
    pub id: String,
    /// The organ: its keys (1 to 16, distinct).
    pub organ: Vec<PublicKey>,
    /// How many organ approvals an issue, veto or revoke needs.
    pub threshold: u32,
    /// Contest window in seconds.
    pub contest: u64,
    /// Validity in seconds (0 = never expires).
    pub validity: u64,
    /// Public data or private commitment.
    pub privacy: Privacy,
    /// The type a credential of this type must reference ("" = none).
    pub ref_type: String,
    /// When a credential of this type is activated, the referenced one is revoked.
    pub revokes: bool,
}

/// Where a credential is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Issued; the window is open.
    Pending,
    /// Holds (until it expires or is revoked).
    Active,
    /// The subject refused it.
    Contested,
    /// The organ vetoed it.
    Vetoed,
    /// The organ revoked it.
    Revoked,
}

impl Status {
    const fn byte(self) -> u8 {
        match self {
            Self::Pending => 0,
            Self::Active => 1,
            Self::Contested => 2,
            Self::Vetoed => 3,
            Self::Revoked => 4,
        }
    }

    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Pending,
            1 => Self::Active,
            2 => Self::Contested,
            3 => Self::Vetoed,
            4 => Self::Revoked,
            _ => return None,
        })
    }

    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Contested => "contested",
            Self::Vetoed => "vetoed",
            Self::Revoked => "revoked",
        }
    }
}

/// A credential entry. It is never deleted: the id stays burned with its final status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// Unique id (1 to 64 printable bytes).
    pub id: String,
    /// Its type.
    pub type_id: String,
    /// The holder.
    pub subject: Address,
    /// Public data, or the 32-byte commitment.
    pub data: Vec<u8>,
    /// Referenced credential ("" = none).
    pub reference: String,
    /// Status.
    pub status: Status,
    /// When it was issued (block time).
    pub issued_at: u64,
    /// When the window ends (it can be activated from then on).
    pub active_at: u64,
    /// When it expires (0 = never).
    pub expires_at: u64,
}

// --- storage ------------------------------------------------------------------------------------------------------

fn type_key(id: &str) -> Vec<u8> {
    [T_PREFIX, id.as_bytes()].concat()
}

fn cred_key(id: &str) -> Vec<u8> {
    [C_PREFIX, id.as_bytes()].concat()
}

fn encode_type(t: &TypeSpec) -> Vec<u8> {
    let mut e = Encoder::new();
    e.count(t.organ.len());
    for k in &t.organ {
        e.fixed(k.as_bytes());
    }
    e.u32(t.threshold)
        .u64(t.contest)
        .u64(t.validity)
        .u8(u8::from(t.privacy == Privacy::Private))
        .string(&t.ref_type)
        .u8(u8::from(t.revokes));
    e.finish().unwrap_or_default()
}

fn decode_type(id: &str, raw: &[u8]) -> Option<TypeSpec> {
    let mut d = Decoder::new(raw);
    let n = d.count(16).ok()?;
    let mut organ = Vec::with_capacity(n);
    for _ in 0..n {
        organ.push(PublicKey(d.fixed::<32>().ok()?));
    }
    let t = TypeSpec {
        id: id.to_string(),
        organ,
        threshold: d.u32().ok()?,
        contest: d.u64().ok()?,
        validity: d.u64().ok()?,
        privacy: if d.u8().ok()? == 1 {
            Privacy::Private
        } else {
            Privacy::Public
        },
        ref_type: d.string(32).ok()?.to_string(),
        revokes: d.u8().ok()? == 1,
    };
    d.finish().ok()?;
    Some(t)
}

fn encode_cred(c: &Credential) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(&c.type_id)
        .fixed(c.subject.hash().as_bytes())
        .bytes(&c.data)
        .string(&c.reference)
        .u8(c.status.byte())
        .u64(c.issued_at)
        .u64(c.active_at)
        .u64(c.expires_at);
    e.finish().unwrap_or_default()
}

fn decode_cred(id: &str, raw: &[u8]) -> Option<Credential> {
    let mut d = Decoder::new(raw);
    let c = Credential {
        id: id.to_string(),
        type_id: d.string(32).ok()?.to_string(),
        subject: Address(Hash32(d.fixed::<32>().ok()?)),
        data: d.bytes(MAX_PUBLIC_DATA).ok()?.to_vec(),
        reference: d.string(64).ok()?.to_string(),
        status: Status::from_byte(d.u8().ok()?)?,
        issued_at: d.u64().ok()?,
        active_at: d.u64().ok()?,
        expires_at: d.u64().ok()?,
    };
    d.finish().ok()?;
    Some(c)
}

/// A credential type by id.
#[must_use]
pub fn get_type(state: &State, id: &str) -> Option<TypeSpec> {
    decode_type(id, state.ext_get(&type_key(id))?)
}

/// A credential by id.
#[must_use]
pub fn get_credential(state: &State, id: &str) -> Option<Credential> {
    decode_cred(id, state.ext_get(&cred_key(id))?)
}

/// All credential types (ordered by id).
#[must_use]
pub fn list_types(state: &State) -> Vec<TypeSpec> {
    state
        .ext_scan(T_PREFIX)
        .filter_map(|(k, v)| decode_type(std::str::from_utf8(k.get(T_PREFIX.len()..)?).ok()?, v))
        .collect()
}

/// The credentials of a holder (ordered by id). O(number of credentials): an index belongs to the explorer.
#[must_use]
pub fn credentials_of(state: &State, subject: &Address) -> Vec<Credential> {
    state
        .ext_scan(C_PREFIX)
        .filter_map(|(k, v)| decode_cred(std::str::from_utf8(k.get(C_PREFIX.len()..)?).ok()?, v))
        .filter(|c| c.subject == *subject)
        .collect()
}

/// The credential is active and has not expired at `now` (seconds).
#[must_use]
pub fn valid(state: &State, id: &str, now: u64) -> bool {
    get_credential(state, id)
        .is_some_and(|c| c.status == Status::Active && (c.expires_at == 0 || now < c.expires_at))
}

// --- documents the organs sign ----------------------------------------------------------------------------------------

fn doc(prefix: &str, op: &str, body: &[u8]) -> Hash32 {
    tagged(&format!("{prefix}/{op}"), &[body])
}

/// The document that authorizes creating a type (signed by the root authorities).
#[must_use]
pub fn type_doc(chain: &str, t: &TypeSpec) -> Hash32 {
    type_doc_with_prefix(DOC_PREFIX, chain, t)
}

/// [`type_doc`] with another domain prefix (the BRDC vectors use `brdc/cred`).
#[must_use]
pub fn type_doc_with_prefix(prefix: &str, chain: &str, t: &TypeSpec) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain).string(&t.id).count(t.organ.len());
    for k in &t.organ {
        e.fixed(k.as_bytes());
    }
    e.u32(t.threshold)
        .u64(t.contest)
        .u64(t.validity)
        .u8(u8::from(t.privacy == Privacy::Private))
        .string(&t.ref_type)
        .u8(u8::from(t.revokes));
    doc(prefix, "type", &e.finish().unwrap_or_default())
}

/// The document that authorizes issuing a credential (signed by the type's organ).
#[must_use]
pub fn issue_doc(
    chain: &str,
    type_id: &str,
    id: &str,
    subject: &Address,
    data: &[u8],
    reference: &str,
) -> Hash32 {
    issue_doc_with_prefix(DOC_PREFIX, chain, type_id, id, subject, data, reference)
}

/// [`issue_doc`] with another domain prefix.
#[must_use]
pub fn issue_doc_with_prefix(
    prefix: &str,
    chain: &str,
    type_id: &str,
    id: &str,
    subject: &Address,
    data: &[u8],
    reference: &str,
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain)
        .string(type_id)
        .string(id)
        .fixed(subject.hash().as_bytes())
        .bytes(data)
        .string(reference);
    doc(prefix, "issue", &e.finish().unwrap_or_default())
}

/// The document that authorizes a veto. It carries the **issue time**, so a veto approved for one issue is not
/// valid for another credential reusing the id (ids are unique, but the time pins the exact entry).
#[must_use]
pub fn veto_doc(chain: &str, id: &str, issued_at: u64) -> Hash32 {
    veto_doc_with_prefix(DOC_PREFIX, chain, id, issued_at)
}

/// [`veto_doc`] with another domain prefix.
#[must_use]
pub fn veto_doc_with_prefix(prefix: &str, chain: &str, id: &str, issued_at: u64) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain).string(id).u64(issued_at);
    doc(prefix, "veto", &e.finish().unwrap_or_default())
}

/// The document that authorizes a revocation.
#[must_use]
pub fn revoke_doc(chain: &str, id: &str, reason: &str) -> Hash32 {
    revoke_doc_with_prefix(DOC_PREFIX, chain, id, reason)
}

/// [`revoke_doc`] with another domain prefix.
#[must_use]
pub fn revoke_doc_with_prefix(prefix: &str, chain: &str, id: &str, reason: &str) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain).string(id).string(reason);
    doc(prefix, "revoke", &e.finish().unwrap_or_default())
}

// --- payloads -----------------------------------------------------------------------------------------------------------

/// Payload of a type creation (kind 14).
#[must_use]
pub fn type_payload(t: &TypeSpec, approvals_list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(&t.id).count(t.organ.len());
    for k in &t.organ {
        e.fixed(k.as_bytes());
    }
    e.u32(t.threshold)
        .u64(t.contest)
        .u64(t.validity)
        .u8(u8::from(t.privacy == Privacy::Private))
        .string(&t.ref_type)
        .u8(u8::from(t.revokes));
    approvals::write_list(&mut e, approvals_list);
    e.finish().unwrap_or_default()
}

/// Payload of an issue (kind 15).
#[must_use]
pub fn issue_payload(
    type_id: &str,
    id: &str,
    subject: &Address,
    data: &[u8],
    reference: &str,
    approvals_list: &[Approval],
) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(type_id)
        .string(id)
        .fixed(subject.hash().as_bytes())
        .bytes(data)
        .string(reference);
    approvals::write_list(&mut e, approvals_list);
    e.finish().unwrap_or_default()
}

/// Payload that is only an id (contest, kind 16, and activate, kind 18).
#[must_use]
pub fn id_payload(id: &str) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(id);
    e.finish().unwrap_or_default()
}

/// Payload of a veto (kind 17).
#[must_use]
pub fn veto_payload(id: &str, approvals_list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(id);
    approvals::write_list(&mut e, approvals_list);
    e.finish().unwrap_or_default()
}

/// Payload of a revocation (kind 19).
#[must_use]
pub fn revoke_payload(id: &str, reason: &str, approvals_list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(id).string(reason);
    approvals::write_list(&mut e, approvals_list);
    e.finish().unwrap_or_default()
}

// --- execution ----------------------------------------------------------------------------------------------------------

fn err(reason: &'static str) -> LedgerError {
    LedgerError::Module(reason)
}

fn check(ok: bool, reason: &'static str) -> Result<(), LedgerError> {
    if ok { Ok(()) } else { Err(err(reason)) }
}

fn id_ok(id: &str, max: usize) -> bool {
    !id.is_empty() && id.len() <= max && id.chars().all(|c| !c.is_control())
}

fn organ_ok(organ: &[PublicKey], threshold: u32) -> bool {
    let mut distinct = organ.to_vec();
    distinct.sort();
    distinct.dedup();
    (1..=16).contains(&organ.len())
        && distinct.len() == organ.len()
        && threshold >= 1
        && threshold as usize <= organ.len()
}

fn set_status(state: &mut State, id: &str, status: Status) -> Result<(), LedgerError> {
    let mut c = get_credential(state, id).ok_or(err("no_credential"))?;
    c.status = status;
    state.ext_put(cred_key(id), encode_cred(&c));
    Ok(())
}

fn organ_approved(t: &TypeSpec, d: &Hash32, list: &[Approval]) -> Result<(), LedgerError> {
    approvals::require(d, list, &t.organ, t.threshold as usize)
}

/// Checks the fields of an issue (type exists, id free, data in the type's format, valid reference) WITHOUT the
/// approvals. Returns the type.
pub fn check_issue(
    state: &State,
    type_id: &str,
    id: &str,
    subject: &Address,
    data: &[u8],
    reference: &str,
) -> Result<TypeSpec, LedgerError> {
    let t = get_type(state, type_id).ok_or(err("no_type"))?;
    check(id_ok(id, 64), "bad_id")?;
    check(get_credential(state, id).is_none(), "credential_exists")?;
    check(
        match t.privacy {
            Privacy::Private => data.len() == 32,
            Privacy::Public => data.len() <= MAX_PUBLIC_DATA,
        },
        "bad_data",
    )?;
    if t.ref_type.is_empty() {
        check(reference.is_empty(), "bad_ref")?;
    } else {
        let ok = get_credential(state, reference).is_some_and(|r| {
            r.type_id == t.ref_type && r.subject == *subject && r.status == Status::Active
        });
        check(ok, "bad_ref")?;
    }
    Ok(t)
}

/// The credentials module (kinds 14 to 19). Register it with `Executor::with_module`.
pub struct CredentialsModule;

impl Module for CredentialsModule {
    fn kinds(&self) -> core::ops::RangeInclusive<u8> {
        kind::TYPE..=kind::REVOKE
    }

    fn min_fee(&self, rules: &Rules, tx: &Tx) -> u64 {
        match tx.kind {
            kind::TYPE => rules.registration_fee,
            kind::ISSUE | kind::REVOKE => rules.credential_fee,
            _ => 0,
        }
    }

    fn execute(
        &self,
        state: &mut State,
        rules: &Rules,
        ctx: &TxCtx,
        tx: &Tx,
    ) -> Result<(), LedgerError> {
        let mut d = Decoder::new(&tx.payload);
        let bad = |_| LedgerError::Malformed;
        match tx.kind {
            kind::TYPE => {
                let id = d.string(32).map_err(bad)?.to_string();
                let n = d.count(16).map_err(bad)?;
                let mut organ = Vec::with_capacity(n);
                for _ in 0..n {
                    organ.push(PublicKey(d.fixed::<32>().map_err(bad)?));
                }
                let spec = TypeSpec {
                    id,
                    organ,
                    threshold: d.u32().map_err(bad)?,
                    contest: d.u64().map_err(bad)?,
                    validity: d.u64().map_err(bad)?,
                    privacy: match d.u8().map_err(bad)? {
                        0 => Privacy::Public,
                        1 => Privacy::Private,
                        _ => return Err(err("bad_type")),
                    },
                    ref_type: d.string(32).map_err(bad)?.to_string(),
                    revokes: match d.u8().map_err(bad)? {
                        0 => false,
                        1 => true,
                        _ => return Err(err("bad_type")),
                    },
                };
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                check(id_ok(&spec.id, 32), "bad_id")?;
                check(get_type(state, &spec.id).is_none(), "type_exists")?;
                check(organ_ok(&spec.organ, spec.threshold), "bad_type")?;
                check(
                    (rules.min_window..=rules.max_window).contains(&spec.contest),
                    "bad_type",
                )?;
                let ref_ok = (spec.ref_type.is_empty() && !spec.revokes)
                    || (!spec.ref_type.is_empty() && get_type(state, &spec.ref_type).is_some());
                check(ref_ok, "bad_type")?;
                check(!rules.authorities.is_empty(), "no_authority")?;
                approvals::require(
                    &type_doc(&rules.chain_id, &spec),
                    &list,
                    &rules.authorities,
                    rules.authority_threshold,
                )?;
                state.ext_put(type_key(&spec.id), encode_type(&spec));
                Ok(())
            }
            kind::ISSUE => {
                let type_id = d.string(32).map_err(bad)?.to_string();
                let id = d.string(64).map_err(bad)?.to_string();
                let subject = Address(Hash32(d.fixed::<32>().map_err(bad)?));
                let data = d.bytes(MAX_PUBLIC_DATA).map_err(bad)?.to_vec();
                let reference = d.string(64).map_err(bad)?.to_string();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let t = check_issue(state, &type_id, &id, &subject, &data, &reference)?;
                organ_approved(
                    &t,
                    &issue_doc(&rules.chain_id, &type_id, &id, &subject, &data, &reference),
                    &list,
                )?;
                let cred = Credential {
                    id: id.clone(),
                    type_id,
                    subject,
                    data,
                    reference,
                    status: Status::Pending,
                    issued_at: ctx.time,
                    active_at: ctx.time.saturating_add(t.contest),
                    expires_at: if t.validity == 0 {
                        0
                    } else {
                        ctx.time.saturating_add(t.validity)
                    },
                };
                state.ext_put(cred_key(&id), encode_cred(&cred));
                Ok(())
            }
            kind::CONTEST => {
                let id = d.string(64).map_err(bad)?.to_string();
                d.finish().map_err(bad)?;
                let c = get_credential(state, &id).ok_or(err("no_credential"))?;
                check(c.subject == ctx.sender, "not_subject")?;
                check(c.status == Status::Pending, "bad_status")?;
                check(ctx.time < c.active_at, "window_closed")?;
                set_status(state, &id, Status::Contested)
            }
            kind::VETO => {
                let id = d.string(64).map_err(bad)?.to_string();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let c = get_credential(state, &id).ok_or(err("no_credential"))?;
                check(c.status == Status::Pending, "bad_status")?;
                check(ctx.time < c.active_at, "window_closed")?;
                let t = get_type(state, &c.type_id).ok_or(err("no_type"))?;
                organ_approved(&t, &veto_doc(&rules.chain_id, &id, c.issued_at), &list)?;
                set_status(state, &id, Status::Vetoed)
            }
            kind::ACTIVATE => {
                let id = d.string(64).map_err(bad)?.to_string();
                d.finish().map_err(bad)?;
                let c = get_credential(state, &id).ok_or(err("no_credential"))?;
                check(c.status == Status::Pending, "bad_status")?;
                check(ctx.time >= c.active_at, "window_open")?;
                set_status(state, &id, Status::Active)?;
                // a type that "revokes the reference" (divorce) retires the referenced credential when activated
                if get_type(state, &c.type_id).is_some_and(|t| t.revokes)
                    && get_credential(state, &c.reference)
                        .is_some_and(|r| r.status == Status::Active)
                {
                    set_status(state, &c.reference, Status::Revoked)?;
                }
                Ok(())
            }
            kind::REVOKE => {
                let id = d.string(64).map_err(bad)?.to_string();
                let reason = d.string(MAX_REASON).map_err(bad)?.to_string();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let c = get_credential(state, &id).ok_or(err("no_credential"))?;
                check(
                    matches!(c.status, Status::Pending | Status::Active),
                    "bad_status",
                )?;
                let t = get_type(state, &c.type_id).ok_or(err("no_type"))?;
                organ_approved(&t, &revoke_doc(&rules.chain_id, &id, &reason), &list)?;
                set_status(state, &id, Status::Revoked)
            }
            _ => Err(LedgerError::UnknownKind(tx.kind)),
        }
    }
}
