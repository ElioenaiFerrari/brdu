//! Native assets (ADR-0018 of the BRDC, adapted): property of things (real estate, vehicles…) and its **transfer with
//! payment in BRDU**, with no judge on the chain. Who guarantees that the asset exists and who owns it is an
//! **organ** (a registry office…), not the chain.
//!
//! - A **class** (created by the root authorities, M of N) fixes the organ (keys + M of N) and the **veto window**.
//! - **Register**: the class's organ approves (M of N) an asset: unique id, first owner, metadata (<= 256 bytes).
//! - **Direct trade**, in steps, each one a transaction: **offer** (the owner: buyer and price) → **accept** (the
//!   buyer; the price leaves their balance into **escrow** and the window starts) → **veto** (the organ, M of N, only
//!   BEFORE the deadline: escrow goes back to the buyer) or **finalize** (anyone, only AFTER the deadline: ownership
//!   passes to the buyer and escrow goes to the seller) · **cancel** (the owner, only before the accept).
//! - **Liens**: the organ adds and removes them (M of N). An asset with a lien cannot be offered, and a trade in its
//!   window cannot finalize while a lien exists.
//! - Every organ approval signs a document containing the asset **revision**, which rises with each organ action:
//!   an approval is never valid twice. A veto also pins buyer, price and deadline.
//! - **Conservation:** escrow belongs to the supply: `balances + escrow = minted - burned`.
//!
//! Kinds: 5 class · 6 register · 7 offer · 8 accept · 9 cancel · 10 veto · 11 finalize · 12 lien add · 13 lien remove.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, tagged};
use brdu_ledger::approvals;
use brdu_ledger::coin::Approval;
use brdu_ledger::{LedgerError, Module, Rules, State, Tx, TxCtx};

/// Transaction kinds of this module.
pub mod kind {
    /// Create an asset class (root authorities, M of N).
    pub const CLASS: u8 = 5;
    /// Register an asset (the class's organ, M of N).
    pub const REGISTER: u8 = 6;
    /// The owner offers the asset to a buyer for a price.
    pub const OFFER: u8 = 7;
    /// The buyer accepts: the price goes to escrow and the veto window starts.
    pub const ACCEPT: u8 = 8;
    /// The owner cancels an offer that was not accepted yet.
    pub const CANCEL: u8 = 9;
    /// The organ vetoes an accepted trade before its deadline (M of N).
    pub const VETO: u8 = 10;
    /// Anyone finalizes an accepted trade after its deadline.
    pub const FINALIZE: u8 = 11;
    /// The organ adds a lien (M of N).
    pub const LIEN_ADD: u8 = 12;
    /// The organ removes a lien (M of N).
    pub const LIEN_REMOVE: u8 = 13;
}

/// Prefix of the documents the organs sign.
pub const DOC_PREFIX: &str = "brdu/asset";
const C_PREFIX: &[u8] = b"asset/c/";
const A_PREFIX: &[u8] = b"asset/a/";
const D_PREFIX: &[u8] = b"asset/d/";
const ESCROW_KEY: &[u8] = b"asset/escrow";
/// Most bytes of asset metadata.
pub const MAX_META: usize = 256;
/// Longest lien reference.
pub const MAX_LIEN_REF: usize = 128;

/// An asset class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSpec {
    /// Unique id (1 to 32 printable bytes).
    pub id: String,
    /// The organ keys (1 to 16, distinct).
    pub organ: Vec<PublicKey>,
    /// How many organ approvals an organ action needs.
    pub threshold: u32,
    /// Veto window in seconds.
    pub veto_window: u64,
}

/// An asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Unique id (1 to 64 printable bytes).
    pub id: String,
    /// Its class.
    pub class: String,
    /// The owner.
    pub owner: Address,
    /// Metadata (normally a hash of the title document).
    pub meta: Vec<u8>,
    /// Revision: rises with every organ action and every ownership change.
    pub rev: u64,
    /// Liens: id → reference.
    pub liens: BTreeMap<String, String>,
}

/// Where a trade is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealStatus {
    /// Offered; the buyer has not accepted.
    Offered,
    /// Accepted; the price is in escrow until the veto window ends.
    Accepted,
}

impl DealStatus {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::Accepted => "accepted",
        }
    }
}

/// A trade in progress (at most one per asset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deal {
    /// The asset.
    pub asset_id: String,
    /// The seller (the owner who offered).
    pub seller: Address,
    /// The buyer.
    pub buyer: Address,
    /// The price in base units.
    pub price: u64,
    /// Status.
    pub status: DealStatus,
    /// End of the veto window (0 until accepted).
    pub deadline: u64,
}

// --- storage ------------------------------------------------------------------------------------------------------

fn key(prefix: &[u8], id: &str) -> Vec<u8> {
    [prefix, id.as_bytes()].concat()
}

fn encode_class(c: &ClassSpec) -> Vec<u8> {
    let mut e = Encoder::new();
    e.count(c.organ.len());
    for k in &c.organ {
        e.fixed(k.as_bytes());
    }
    e.u32(c.threshold).u64(c.veto_window);
    e.finish().unwrap_or_default()
}

fn decode_class(id: &str, raw: &[u8]) -> Option<ClassSpec> {
    let mut d = Decoder::new(raw);
    let n = d.count(16).ok()?;
    let mut organ = Vec::with_capacity(n);
    for _ in 0..n {
        organ.push(PublicKey(d.fixed::<32>().ok()?));
    }
    let c = ClassSpec {
        id: id.to_string(),
        organ,
        threshold: d.u32().ok()?,
        veto_window: d.u64().ok()?,
    };
    d.finish().ok()?;
    Some(c)
}

fn encode_asset(a: &Asset) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(&a.class)
        .fixed(a.owner.hash().as_bytes())
        .bytes(&a.meta)
        .u64(a.rev)
        .count(a.liens.len());
    for (lien, reference) in &a.liens {
        e.string(lien).string(reference);
    }
    e.finish().unwrap_or_default()
}

fn decode_asset(id: &str, raw: &[u8]) -> Option<Asset> {
    let mut d = Decoder::new(raw);
    let class = d.string(32).ok()?.to_string();
    let owner = Address(Hash32(d.fixed::<32>().ok()?));
    let meta = d.bytes(MAX_META).ok()?.to_vec();
    let rev = d.u64().ok()?;
    let n = d.count(1024).ok()?;
    let mut liens = BTreeMap::new();
    for _ in 0..n {
        liens.insert(
            d.string(32).ok()?.to_string(),
            d.string(MAX_LIEN_REF).ok()?.to_string(),
        );
    }
    d.finish().ok()?;
    Some(Asset {
        id: id.to_string(),
        class,
        owner,
        meta,
        rev,
        liens,
    })
}

fn encode_deal(x: &Deal) -> Vec<u8> {
    let mut e = Encoder::new();
    e.fixed(x.seller.hash().as_bytes())
        .fixed(x.buyer.hash().as_bytes())
        .u64(x.price)
        .u8(u8::from(x.status == DealStatus::Accepted))
        .u64(x.deadline);
    e.finish().unwrap_or_default()
}

fn decode_deal(id: &str, raw: &[u8]) -> Option<Deal> {
    let mut d = Decoder::new(raw);
    let x = Deal {
        asset_id: id.to_string(),
        seller: Address(Hash32(d.fixed::<32>().ok()?)),
        buyer: Address(Hash32(d.fixed::<32>().ok()?)),
        price: d.u64().ok()?,
        status: if d.u8().ok()? == 1 {
            DealStatus::Accepted
        } else {
            DealStatus::Offered
        },
        deadline: d.u64().ok()?,
    };
    d.finish().ok()?;
    Some(x)
}

/// A class by id.
#[must_use]
pub fn get_class(state: &State, id: &str) -> Option<ClassSpec> {
    decode_class(id, state.ext_get(&key(C_PREFIX, id))?)
}

/// All classes (ordered by id).
#[must_use]
pub fn list_classes(state: &State) -> Vec<ClassSpec> {
    state
        .ext_scan(C_PREFIX)
        .filter_map(|(k, v)| decode_class(std::str::from_utf8(k.get(C_PREFIX.len()..)?).ok()?, v))
        .collect()
}

/// An asset by id.
#[must_use]
pub fn get_asset(state: &State, id: &str) -> Option<Asset> {
    decode_asset(id, state.ext_get(&key(A_PREFIX, id))?)
}

/// The assets of an owner (ordered by id). O(number of assets).
#[must_use]
pub fn assets_of(state: &State, owner: &Address) -> Vec<Asset> {
    state
        .ext_scan(A_PREFIX)
        .filter_map(|(k, v)| decode_asset(std::str::from_utf8(k.get(A_PREFIX.len()..)?).ok()?, v))
        .filter(|a| a.owner == *owner)
        .collect()
}

/// The trade of an asset, if any.
#[must_use]
pub fn get_deal(state: &State, asset_id: &str) -> Option<Deal> {
    decode_deal(asset_id, state.ext_get(&key(D_PREFIX, asset_id))?)
}

/// All trades in progress (ordered by asset id).
#[must_use]
pub fn list_deals(state: &State) -> Vec<Deal> {
    state
        .ext_scan(D_PREFIX)
        .filter_map(|(k, v)| decode_deal(std::str::from_utf8(k.get(D_PREFIX.len()..)?).ok()?, v))
        .collect()
}

/// Total BRDU held in escrow by accepted trades. `balances + escrow = minted - burned`.
#[must_use]
pub fn escrow_total(state: &State) -> u64 {
    state
        .ext_get(ESCROW_KEY)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map_or(0, u64::from_be_bytes)
}

fn set_escrow(state: &mut State, v: u64) {
    state.ext_put(ESCROW_KEY.to_vec(), v.to_be_bytes().to_vec());
}

// --- documents the organs sign ----------------------------------------------------------------------------------------

fn doc(prefix: &str, op: &str, body: &[u8]) -> Hash32 {
    tagged(&format!("{prefix}/{op}"), &[body])
}

/// Document that authorizes creating a class (signed by the root authorities).
#[must_use]
pub fn class_doc(
    chain: &str,
    id: &str,
    organ: &[PublicKey],
    threshold: u32,
    veto_window: u64,
) -> Hash32 {
    class_doc_with_prefix(DOC_PREFIX, chain, id, organ, threshold, veto_window)
}

/// [`class_doc`] with another domain prefix (the BRDC vectors use `brdc/asset`).
#[must_use]
pub fn class_doc_with_prefix(
    prefix: &str,
    chain: &str,
    id: &str,
    organ: &[PublicKey],
    threshold: u32,
    veto_window: u64,
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain).string(id).count(organ.len());
    for k in organ {
        e.fixed(k.as_bytes());
    }
    e.u32(threshold).u64(veto_window);
    doc(prefix, "class", &e.finish().unwrap_or_default())
}

/// Document that authorizes registering an asset.
#[must_use]
pub fn register_doc(chain: &str, class: &str, asset: &str, owner: &Address, meta: &[u8]) -> Hash32 {
    register_doc_with_prefix(DOC_PREFIX, chain, class, asset, owner, meta)
}

/// [`register_doc`] with another domain prefix.
#[must_use]
pub fn register_doc_with_prefix(
    prefix: &str,
    chain: &str,
    class: &str,
    asset: &str,
    owner: &Address,
    meta: &[u8],
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain)
        .string(class)
        .string(asset)
        .fixed(owner.hash().as_bytes())
        .bytes(meta);
    doc(prefix, "register", &e.finish().unwrap_or_default())
}

/// Document that authorizes a veto: it pins the asset revision, buyer, price and deadline.
#[must_use]
pub fn veto_doc(
    chain: &str,
    asset: &str,
    rev: u64,
    buyer: &Address,
    price: u64,
    deadline: u64,
) -> Hash32 {
    veto_doc_with_prefix(DOC_PREFIX, chain, asset, rev, buyer, price, deadline)
}

/// [`veto_doc`] with another domain prefix.
#[must_use]
pub fn veto_doc_with_prefix(
    prefix: &str,
    chain: &str,
    asset: &str,
    rev: u64,
    buyer: &Address,
    price: u64,
    deadline: u64,
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain)
        .string(asset)
        .u64(rev)
        .fixed(buyer.hash().as_bytes())
        .u64(price)
        .u64(deadline);
    doc(prefix, "veto", &e.finish().unwrap_or_default())
}

/// Document that authorizes adding a lien.
#[must_use]
pub fn lien_add_doc(chain: &str, asset: &str, rev: u64, lien: &str, reference: &str) -> Hash32 {
    lien_doc_with_prefix(DOC_PREFIX, "lien_add", chain, asset, rev, lien, reference)
}

/// Document that authorizes removing a lien (`reference` is the one stored when it was added).
#[must_use]
pub fn lien_remove_doc(chain: &str, asset: &str, rev: u64, lien: &str, reference: &str) -> Hash32 {
    lien_doc_with_prefix(
        DOC_PREFIX,
        "lien_remove",
        chain,
        asset,
        rev,
        lien,
        reference,
    )
}

/// Lien documents with another domain prefix; `op` is `lien_add` or `lien_remove`.
#[must_use]
pub fn lien_doc_with_prefix(
    prefix: &str,
    op: &str,
    chain: &str,
    asset: &str,
    rev: u64,
    lien: &str,
    reference: &str,
) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain)
        .string(asset)
        .u64(rev)
        .string(lien)
        .string(reference);
    doc(prefix, op, &e.finish().unwrap_or_default())
}

// --- payloads -----------------------------------------------------------------------------------------------------------

/// Payload of a class creation (kind 5).
#[must_use]
pub fn class_payload(c: &ClassSpec, list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(&c.id).count(c.organ.len());
    for k in &c.organ {
        e.fixed(k.as_bytes());
    }
    e.u32(c.threshold).u64(c.veto_window);
    approvals::write_list(&mut e, list);
    e.finish().unwrap_or_default()
}

/// Payload of a registration (kind 6).
#[must_use]
pub fn register_payload(
    class: &str,
    asset: &str,
    owner: &Address,
    meta: &[u8],
    list: &[Approval],
) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(class)
        .string(asset)
        .fixed(owner.hash().as_bytes())
        .bytes(meta);
    approvals::write_list(&mut e, list);
    e.finish().unwrap_or_default()
}

/// Payload of an offer (kind 7).
#[must_use]
pub fn offer_payload(asset: &str, buyer: &Address, price: u64) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(asset).fixed(buyer.hash().as_bytes()).u64(price);
    e.finish().unwrap_or_default()
}

/// Payload that is only the asset id (accept 8, cancel 9, finalize 11).
#[must_use]
pub fn asset_payload(asset: &str) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(asset);
    e.finish().unwrap_or_default()
}

/// Payload of a veto (kind 10).
#[must_use]
pub fn veto_payload(asset: &str, list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(asset);
    approvals::write_list(&mut e, list);
    e.finish().unwrap_or_default()
}

/// Payload of a lien addition (kind 12).
#[must_use]
pub fn lien_add_payload(asset: &str, lien: &str, reference: &str, list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(asset).string(lien).string(reference);
    approvals::write_list(&mut e, list);
    e.finish().unwrap_or_default()
}

/// Payload of a lien removal (kind 13).
#[must_use]
pub fn lien_remove_payload(asset: &str, lien: &str, list: &[Approval]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(asset).string(lien);
    approvals::write_list(&mut e, list);
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

fn class_of(state: &State, asset: &Asset) -> Result<ClassSpec, LedgerError> {
    get_class(state, &asset.class).ok_or(err("no_class"))
}

fn organ_approved(c: &ClassSpec, d: &Hash32, list: &[Approval]) -> Result<(), LedgerError> {
    approvals::require(d, list, &c.organ, c.threshold as usize)
}

fn put_asset(state: &mut State, a: &Asset) {
    state.ext_put(key(A_PREFIX, &a.id), encode_asset(a));
}

fn drop_deal(state: &mut State, id: &str) {
    state.ext_remove(&key(D_PREFIX, id));
}

/// The assets module (kinds 5 to 13). Register it with `Executor::with_module`.
pub struct AssetsModule;

impl Module for AssetsModule {
    fn kinds(&self) -> core::ops::RangeInclusive<u8> {
        kind::CLASS..=kind::LIEN_REMOVE
    }

    fn min_fee(&self, rules: &Rules, tx: &Tx) -> u64 {
        match tx.kind {
            kind::CLASS | kind::REGISTER => rules.registration_fee,
            kind::OFFER | kind::ACCEPT | kind::CANCEL | kind::FINALIZE => rules.step_fee,
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
            kind::CLASS => {
                let id = d.string(32).map_err(bad)?.to_string();
                let n = d.count(16).map_err(bad)?;
                let mut organ = Vec::with_capacity(n);
                for _ in 0..n {
                    organ.push(PublicKey(d.fixed::<32>().map_err(bad)?));
                }
                let spec = ClassSpec {
                    id,
                    organ,
                    threshold: d.u32().map_err(bad)?,
                    veto_window: d.u64().map_err(bad)?,
                };
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                check(id_ok(&spec.id, 32), "bad_id")?;
                check(get_class(state, &spec.id).is_none(), "class_exists")?;
                check(organ_ok(&spec.organ, spec.threshold), "bad_class")?;
                check(
                    (rules.min_window..=rules.max_window).contains(&spec.veto_window),
                    "bad_class",
                )?;
                check(!rules.authorities.is_empty(), "no_authority")?;
                let doc = class_doc(
                    &rules.chain_id,
                    &spec.id,
                    &spec.organ,
                    spec.threshold,
                    spec.veto_window,
                );
                approvals::require(&doc, &list, &rules.authorities, rules.authority_threshold)?;
                state.ext_put(key(C_PREFIX, &spec.id), encode_class(&spec));
                Ok(())
            }
            kind::REGISTER => {
                let class_id = d.string(32).map_err(bad)?.to_string();
                let asset_id = d.string(64).map_err(bad)?.to_string();
                let owner = Address(Hash32(d.fixed::<32>().map_err(bad)?));
                let meta = d.bytes(MAX_META).map_err(bad)?.to_vec();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let class = get_class(state, &class_id).ok_or(err("no_class"))?;
                check(id_ok(&asset_id, 64), "bad_id")?;
                check(get_asset(state, &asset_id).is_none(), "asset_exists")?;
                organ_approved(
                    &class,
                    &register_doc(&rules.chain_id, &class_id, &asset_id, &owner, &meta),
                    &list,
                )?;
                put_asset(
                    state,
                    &Asset {
                        id: asset_id,
                        class: class_id,
                        owner,
                        meta,
                        rev: 0,
                        liens: BTreeMap::new(),
                    },
                );
                Ok(())
            }
            kind::OFFER => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                let buyer = Address(Hash32(d.fixed::<32>().map_err(bad)?));
                let price = d.u64().map_err(bad)?;
                d.finish().map_err(bad)?;
                let asset = get_asset(state, &asset_id).ok_or(err("no_asset"))?;
                check(asset.owner == ctx.sender, "not_owner")?;
                check(get_deal(state, &asset_id).is_none(), "deal_active")?;
                check(asset.liens.is_empty(), "has_lien")?;
                check(buyer != ctx.sender, "self_deal")?;
                state.ext_put(
                    key(D_PREFIX, &asset_id),
                    encode_deal(&Deal {
                        asset_id,
                        seller: ctx.sender,
                        buyer,
                        price,
                        status: DealStatus::Offered,
                        deadline: 0,
                    }),
                );
                Ok(())
            }
            kind::ACCEPT => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                d.finish().map_err(bad)?;
                let mut deal = get_deal(state, &asset_id).ok_or(err("no_deal"))?;
                check(deal.buyer == ctx.sender, "not_buyer")?;
                check(deal.status == DealStatus::Offered, "bad_status")?;
                let asset = get_asset(state, &asset_id).ok_or(err("no_asset"))?;
                let window = class_of(state, &asset)?.veto_window;
                // the price leaves the buyer's balance for escrow (an insufficient balance fails the whole transaction)
                state.debit(&ctx.sender, deal.price)?;
                let escrow = escrow_total(state)
                    .checked_add(deal.price)
                    .ok_or(LedgerError::Overflow)?;
                set_escrow(state, escrow);
                deal.status = DealStatus::Accepted;
                deal.deadline = ctx.time.saturating_add(window);
                state.ext_put(key(D_PREFIX, &asset_id), encode_deal(&deal));
                Ok(())
            }
            kind::CANCEL => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                d.finish().map_err(bad)?;
                let deal = get_deal(state, &asset_id).ok_or(err("no_deal"))?;
                check(deal.seller == ctx.sender, "not_owner")?;
                check(deal.status == DealStatus::Offered, "bad_status")?;
                drop_deal(state, &asset_id);
                Ok(())
            }
            kind::VETO => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let deal = get_deal(state, &asset_id).ok_or(err("no_deal"))?;
                check(deal.status == DealStatus::Accepted, "bad_status")?;
                check(ctx.time < deal.deadline, "window_closed")?;
                let mut asset = get_asset(state, &asset_id).ok_or(err("no_asset"))?;
                let class = class_of(state, &asset)?;
                organ_approved(
                    &class,
                    &veto_doc(
                        &rules.chain_id,
                        &asset_id,
                        asset.rev,
                        &deal.buyer,
                        deal.price,
                        deal.deadline,
                    ),
                    &list,
                )?;
                state.credit(&deal.buyer, deal.price)?;
                let escrow = escrow_total(state)
                    .checked_sub(deal.price)
                    .ok_or(LedgerError::Overflow)?;
                set_escrow(state, escrow);
                drop_deal(state, &asset_id);
                asset.rev += 1;
                put_asset(state, &asset);
                Ok(())
            }
            kind::FINALIZE => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                d.finish().map_err(bad)?;
                let deal = get_deal(state, &asset_id).ok_or(err("no_deal"))?;
                check(deal.status == DealStatus::Accepted, "bad_status")?;
                check(ctx.time >= deal.deadline, "window_open")?;
                let mut asset = get_asset(state, &asset_id).ok_or(err("no_asset"))?;
                check(asset.liens.is_empty(), "has_lien")?;
                state.credit(&deal.seller, deal.price)?;
                let escrow = escrow_total(state)
                    .checked_sub(deal.price)
                    .ok_or(LedgerError::Overflow)?;
                set_escrow(state, escrow);
                drop_deal(state, &asset_id);
                asset.owner = deal.buyer;
                asset.rev += 1;
                put_asset(state, &asset);
                Ok(())
            }
            kind::LIEN_ADD => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                let lien = d.string(32).map_err(bad)?.to_string();
                let reference = d.string(MAX_LIEN_REF).map_err(bad)?.to_string();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let mut asset = get_asset(state, &asset_id).ok_or(err("no_asset"))?;
                check(id_ok(&lien, 32), "bad_id")?;
                check(!asset.liens.contains_key(&lien), "lien_exists")?;
                let class = class_of(state, &asset)?;
                organ_approved(
                    &class,
                    &lien_add_doc(&rules.chain_id, &asset_id, asset.rev, &lien, &reference),
                    &list,
                )?;
                asset.liens.insert(lien, reference);
                asset.rev += 1;
                put_asset(state, &asset);
                Ok(())
            }
            kind::LIEN_REMOVE => {
                let asset_id = d.string(64).map_err(bad)?.to_string();
                let lien = d.string(32).map_err(bad)?.to_string();
                let list = approvals::read_list(&mut d)?;
                d.finish().map_err(bad)?;
                let mut asset = get_asset(state, &asset_id).ok_or(err("no_asset"))?;
                let reference = asset.liens.get(&lien).cloned().ok_or(err("no_lien"))?;
                let class = class_of(state, &asset)?;
                organ_approved(
                    &class,
                    &lien_remove_doc(&rules.chain_id, &asset_id, asset.rev, &lien, &reference),
                    &list,
                )?;
                asset.liens.remove(&lien);
                asset.rev += 1;
                put_asset(state, &asset);
                Ok(())
            }
            _ => Err(LedgerError::UnknownKind(tx.kind)),
        }
    }
}
