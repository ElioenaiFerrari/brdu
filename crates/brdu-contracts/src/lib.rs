//! Contracts between parties (ADR-0004 and ADR-0017 of the BRDC, adapted): **declarative templates** (data: states,
//! transitions, who acts, time guards, effects on money) run by a small interpreter with a **closed vocabulary**.
//! There is no VM and no third-party code: a template cannot do anything outside what is listed here.
//!
//! Kinds: 20 create a contract · 21 act (one transition of the template). Parties: `A` (creator) and `B`.
//! Templates: `compra-venda` (sale), `aluguel` (rent), `emprestimo` (loan, no automatic interest).
//! Invariant: the escrow of each contract belongs to the supply (`balances + escrows = minted - burned`) and a
//! contract in a final state has zero escrow. The chain never decides who is right: it only runs transitions.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32};
use brdu_ledger::{LedgerError, Module, Rules, State, Tx, TxCtx};

/// Transaction kinds of this module.
pub mod kind {
    /// Create a contract from a template.
    pub const CREATE: u8 = 20;
    /// Perform one transition of the template.
    pub const ACT: u8 = 21;
}

const C_PREFIX: &[u8] = b"ctr/c/";
const ESCROW_KEY: &[u8] = b"ctr/escrow";
/// Final states: the contract holds no escrow.
pub const TERMINAL: [&str; 6] = [
    "done",
    "refunded",
    "cancelled",
    "closed",
    "repaid",
    "defaulted",
];

// --- the closed vocabulary ----------------------------------------------------------------------------------------

/// A party of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    /// The creator.
    A,
    /// The counterparty.
    B,
}

/// Who may perform a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// Only party A.
    OnlyA,
    /// Only party B.
    OnlyB,
    /// Anyone.
    Any,
}

/// A numeric expression (no loops, no calls).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A term fixed at creation.
    Term(&'static str),
    /// A variable (0 if unset).
    Var(&'static str),
    /// A constant.
    Const(u64),
    /// The block time.
    Time,
    /// The contract's escrow.
    Escrow,
    /// Sum.
    Add(Box<Expr>, Box<Expr>),
    /// Subtraction, never below zero.
    Sub(Box<Expr>, Box<Expr>),
    /// Product.
    Mul(Box<Expr>, Box<Expr>),
    /// Minimum.
    Min(Box<Expr>, Box<Expr>),
}

/// A condition that must hold for a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// Block time < variable.
    Before(&'static str),
    /// Block time >= variable.
    AtOrAfter(&'static str),
    /// Variable < term.
    VarLtTerm(&'static str, &'static str),
    /// Variable == term.
    VarEqTerm(&'static str, &'static str),
}

/// An effect on money or variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// From the party's account to the contract's escrow.
    Lock(Party, Expr),
    /// From the escrow to the party's account (at most the escrow).
    Release(Party, Expr),
    /// Directly between parties, no escrow.
    Pay(Party, Party, Expr),
    /// Set a variable.
    SetVar(&'static str, Expr),
    /// Increment a variable.
    IncVar(&'static str),
}

/// One transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// Name used in the act payload.
    pub name: &'static str,
    /// State it starts from.
    pub from: &'static str,
    /// Who may do it.
    pub actor: Actor,
    /// Conditions.
    pub guards: Vec<Guard>,
    /// Effects, applied in order, all or nothing.
    pub effects: Vec<Effect>,
    /// State it ends in.
    pub to: &'static str,
}

/// A template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Id.
    pub id: &'static str,
    /// Names of the terms, in the order of the create payload.
    pub terms: Vec<&'static str>,
    /// Initial state.
    pub initial_state: &'static str,
    /// Effects at creation (party A is the creator).
    pub initial_effects: Vec<Effect>,
    /// Transitions.
    pub transitions: Vec<Transition>,
}

fn e_term(k: &'static str) -> Expr {
    Expr::Term(k)
}
fn add(a: Expr, b: Expr) -> Expr {
    Expr::Add(Box::new(a), Box::new(b))
}
fn mul(a: Expr, b: Expr) -> Expr {
    Expr::Mul(Box::new(a), Box::new(b))
}
fn sub(a: Expr, b: Expr) -> Expr {
    Expr::Sub(Box::new(a), Box::new(b))
}
fn min(a: Expr, b: Expr) -> Expr {
    Expr::Min(Box::new(a), Box::new(b))
}
fn tr(
    name: &'static str,
    from: &'static str,
    actor: Actor,
    guards: Vec<Guard>,
    effects: Vec<Effect>,
    to: &'static str,
) -> Transition {
    Transition {
        name,
        from,
        actor,
        guards,
        effects,
        to,
    }
}

/// Ids of the available templates.
pub const TEMPLATE_IDS: [&str; 3] = ["compra-venda", "aluguel", "emprestimo"];

/// A template by id.
#[must_use]
pub fn template(id: &str) -> Option<Template> {
    use Actor::{Any, OnlyA, OnlyB};
    use Effect::{IncVar, Lock, Pay, Release, SetVar};
    use Party::{A, B};
    Some(match id {
        "compra-venda" => Template {
            id: "compra-venda",
            terms: vec!["price", "duration"],
            initial_state: "open",
            initial_effects: vec![],
            transitions: vec![
                tr(
                    "accept",
                    "open",
                    OnlyB,
                    vec![],
                    vec![
                        Lock(B, e_term("price")),
                        SetVar("deadline", add(Expr::Time, e_term("duration"))),
                    ],
                    "funded",
                ),
                tr(
                    "confirm",
                    "funded",
                    OnlyB,
                    vec![],
                    vec![Release(A, Expr::Escrow)],
                    "done",
                ),
                tr(
                    "refund",
                    "funded",
                    OnlyA,
                    vec![],
                    vec![Release(B, Expr::Escrow)],
                    "refunded",
                ),
                tr(
                    "refund_expired",
                    "funded",
                    Any,
                    vec![Guard::AtOrAfter("deadline")],
                    vec![Release(B, Expr::Escrow)],
                    "refunded",
                ),
                tr("cancel", "open", OnlyA, vec![], vec![], "cancelled"),
            ],
        },
        "aluguel" => Template {
            id: "aluguel",
            terms: vec!["rent", "period", "periods", "deposit", "grace"],
            initial_state: "open",
            initial_effects: vec![],
            transitions: vec![
                tr(
                    "accept",
                    "open",
                    OnlyB,
                    vec![],
                    vec![
                        Lock(B, e_term("deposit")),
                        SetVar("paid", Expr::Const(0)),
                        SetVar(
                            "settle_at",
                            add(
                                Expr::Time,
                                add(mul(e_term("period"), e_term("periods")), e_term("grace")),
                            ),
                        ),
                    ],
                    "active",
                ),
                tr(
                    "pay_rent",
                    "active",
                    OnlyB,
                    vec![Guard::VarLtTerm("paid", "periods")],
                    vec![Pay(B, A, e_term("rent")), IncVar("paid")],
                    "active",
                ),
                tr(
                    "settle",
                    "active",
                    Any,
                    vec![Guard::AtOrAfter("settle_at")],
                    vec![
                        Release(
                            A,
                            min(
                                Expr::Escrow,
                                mul(sub(e_term("periods"), Expr::Var("paid")), e_term("rent")),
                            ),
                        ),
                        Release(B, Expr::Escrow),
                    ],
                    "closed",
                ),
                tr(
                    "settle_paid",
                    "active",
                    OnlyA,
                    vec![Guard::VarEqTerm("paid", "periods")],
                    vec![Release(B, Expr::Escrow)],
                    "closed",
                ),
                tr("cancel", "open", OnlyA, vec![], vec![], "cancelled"),
            ],
        },
        "emprestimo" => Template {
            id: "emprestimo",
            terms: vec!["principal", "repay", "duration"],
            initial_state: "open",
            initial_effects: vec![Lock(A, e_term("principal"))],
            transitions: vec![
                tr(
                    "accept",
                    "open",
                    OnlyB,
                    vec![],
                    vec![
                        Release(B, Expr::Escrow),
                        SetVar("deadline", add(Expr::Time, e_term("duration"))),
                    ],
                    "active",
                ),
                tr(
                    "repay",
                    "active",
                    OnlyB,
                    vec![],
                    vec![Pay(B, A, e_term("repay"))],
                    "repaid",
                ),
                tr(
                    "mark_default",
                    "active",
                    Any,
                    vec![Guard::AtOrAfter("deadline")],
                    vec![],
                    "defaulted",
                ),
                tr(
                    "cancel",
                    "open",
                    OnlyA,
                    vec![],
                    vec![Release(A, Expr::Escrow)],
                    "cancelled",
                ),
            ],
        },
        _ => return None,
    })
}

// --- stored contract -----------------------------------------------------------------------------------------------

/// A contract instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// Id (1 to 64 printable bytes).
    pub id: String,
    /// Template id.
    pub template: String,
    /// The creator.
    pub a: Address,
    /// The counterparty.
    pub b: Address,
    /// Terms by name.
    pub terms: BTreeMap<String, u64>,
    /// Current state.
    pub state: String,
    /// Money held for this contract.
    pub escrow: u64,
    /// Variables (deadlines and counters).
    pub vars: BTreeMap<String, u64>,
}

fn put_map(e: &mut Encoder, m: &BTreeMap<String, u64>) {
    e.count(m.len());
    for (k, v) in m {
        e.string(k).u64(*v);
    }
}

fn get_map(d: &mut Decoder<'_>) -> Option<BTreeMap<String, u64>> {
    let n = d.count(64).ok()?;
    let mut m = BTreeMap::new();
    for _ in 0..n {
        m.insert(d.string(32).ok()?.to_string(), d.u64().ok()?);
    }
    Some(m)
}

fn encode(c: &Contract) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(&c.template)
        .fixed(c.a.hash().as_bytes())
        .fixed(c.b.hash().as_bytes());
    put_map(&mut e, &c.terms);
    e.string(&c.state).u64(c.escrow);
    put_map(&mut e, &c.vars);
    e.finish().unwrap_or_default()
}

fn decode(id: &str, raw: &[u8]) -> Option<Contract> {
    let mut d = Decoder::new(raw);
    let template = d.string(32).ok()?.to_string();
    let a = Address(Hash32(d.fixed::<32>().ok()?));
    let b = Address(Hash32(d.fixed::<32>().ok()?));
    let terms = get_map(&mut d)?;
    let state = d.string(32).ok()?.to_string();
    let escrow = d.u64().ok()?;
    let vars = get_map(&mut d)?;
    d.finish().ok()?;
    Some(Contract {
        id: id.to_string(),
        template,
        a,
        b,
        terms,
        state,
        escrow,
        vars,
    })
}

/// A contract by id.
#[must_use]
pub fn get_contract(state: &State, id: &str) -> Option<Contract> {
    decode(id, state.ext_get(&[C_PREFIX, id.as_bytes()].concat())?)
}

/// All contracts (ordered by id).
#[must_use]
pub fn list_contracts(state: &State) -> Vec<Contract> {
    state
        .ext_scan(C_PREFIX)
        .filter_map(|(k, v)| decode(std::str::from_utf8(k.get(C_PREFIX.len()..)?).ok()?, v))
        .collect()
}

/// The contracts where an account is party A or B. O(number of contracts).
#[must_use]
pub fn contracts_of(state: &State, who: &Address) -> Vec<Contract> {
    list_contracts(state)
        .into_iter()
        .filter(|c| c.a == *who || c.b == *who)
        .collect()
}

/// Total money held in escrow by all contracts.
#[must_use]
pub fn escrow_total(state: &State) -> u64 {
    state
        .ext_get(ESCROW_KEY)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map_or(0, u64::from_be_bytes)
}

// --- payloads -----------------------------------------------------------------------------------------------------------

/// Payload of a creation (kind 20): template, id, party B, terms in the template's order.
#[must_use]
pub fn create_payload(template: &str, id: &str, party_b: &Address, terms: &[u64]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(template)
        .string(id)
        .fixed(party_b.hash().as_bytes())
        .count(terms.len());
    for t in terms {
        e.u64(*t);
    }
    e.finish().unwrap_or_default()
}

/// Payload of an action (kind 21).
#[must_use]
pub fn act_payload(id: &str, action: &str) -> Vec<u8> {
    let mut e = Encoder::new();
    e.string(id).string(action);
    e.finish().unwrap_or_default()
}

// --- interpreter ------------------------------------------------------------------------------------------------------

fn err(reason: &'static str) -> LedgerError {
    LedgerError::Module(reason)
}

fn check(ok: bool, reason: &'static str) -> Result<(), LedgerError> {
    if ok { Ok(()) } else { Err(err(reason)) }
}

fn eval(x: &Expr, now: u64, c: &Contract) -> Result<u64, LedgerError> {
    let v: u128 = eval128(x, now, c)?;
    u64::try_from(v).map_err(|_| LedgerError::Overflow)
}

fn eval128(x: &Expr, now: u64, c: &Contract) -> Result<u128, LedgerError> {
    Ok(match x {
        Expr::Term(k) => u128::from(*c.terms.get(*k).ok_or(err("bad_terms"))?),
        Expr::Var(k) => u128::from(c.vars.get(*k).copied().unwrap_or(0)),
        Expr::Const(n) => u128::from(*n),
        Expr::Time => u128::from(now),
        Expr::Escrow => u128::from(c.escrow),
        Expr::Add(a, b) => eval128(a, now, c)?
            .checked_add(eval128(b, now, c)?)
            .ok_or(LedgerError::Overflow)?,
        Expr::Sub(a, b) => eval128(a, now, c)?.saturating_sub(eval128(b, now, c)?),
        Expr::Mul(a, b) => eval128(a, now, c)?
            .checked_mul(eval128(b, now, c)?)
            .ok_or(LedgerError::Overflow)?,
        Expr::Min(a, b) => eval128(a, now, c)?.min(eval128(b, now, c)?),
    })
}

fn who(c: &Contract, p: Party) -> Address {
    match p {
        Party::A => c.a,
        Party::B => c.b,
    }
}

fn set_escrow_total(state: &mut State, v: u64) {
    state.ext_put(ESCROW_KEY.to_vec(), v.to_be_bytes().to_vec());
}

fn guard_ok(g: &Guard, now: u64, c: &Contract) -> Result<(), LedgerError> {
    let var = |k: &str| c.vars.get(k).copied().unwrap_or(0);
    let term = |k: &str| c.terms.get(k).copied().unwrap_or(0);
    match g {
        Guard::Before(v) => check(now < var(v), "too_late"),
        Guard::AtOrAfter(v) => check(now >= var(v), "too_early"),
        Guard::VarLtTerm(v, t) => check(var(v) < term(t), "not_ready"),
        Guard::VarEqTerm(v, t) => check(var(v) == term(t), "not_ready"),
    }
}

fn apply(
    effects: &[Effect],
    state: &mut State,
    now: u64,
    c: &mut Contract,
) -> Result<(), LedgerError> {
    for effect in effects {
        match effect {
            Effect::Lock(p, x) => {
                let amount = eval(x, now, c)?;
                state.debit(&who(c, *p), amount)?;
                c.escrow = c.escrow.checked_add(amount).ok_or(LedgerError::Overflow)?;
                let total = escrow_total(state)
                    .checked_add(amount)
                    .ok_or(LedgerError::Overflow)?;
                set_escrow_total(state, total);
            }
            Effect::Release(p, x) => {
                let amount = eval(x, now, c)?.min(c.escrow);
                state.credit(&who(c, *p), amount)?;
                c.escrow -= amount;
                let total = escrow_total(state)
                    .checked_sub(amount)
                    .ok_or(LedgerError::Overflow)?;
                set_escrow_total(state, total);
            }
            Effect::Pay(from, to, x) => {
                let amount = eval(x, now, c)?;
                state.debit(&who(c, *from), amount)?;
                state.credit(&who(c, *to), amount)?;
            }
            Effect::SetVar(k, x) => {
                let v = eval(x, now, c)?;
                c.vars.insert((*k).to_string(), v);
            }
            Effect::IncVar(k) => {
                let e = c.vars.entry((*k).to_string()).or_insert(0);
                *e = e.checked_add(1).ok_or(LedgerError::Overflow)?;
            }
        }
    }
    Ok(())
}

fn terms_ok(rules: &Rules, template: &str, t: &BTreeMap<String, u64>) -> bool {
    let window = |s: u64| (rules.min_window..=rules.max_window).contains(&s);
    let g = |k: &str| t.get(k).copied().unwrap_or(0);
    match template {
        "compra-venda" => g("price") > 0 && window(g("duration")),
        "emprestimo" => g("principal") > 0 && g("repay") > 0 && window(g("duration")),
        "aluguel" => {
            g("rent") > 0
                && (1..=1200).contains(&g("periods"))
                && window(g("period"))
                && g("grace") <= rules.max_window
                && u128::from(g("rent")) * u128::from(g("periods")) <= u128::from(u64::MAX)
        }
        _ => false,
    }
}

fn put(state: &mut State, c: &Contract) {
    state.ext_put([C_PREFIX, c.id.as_bytes()].concat(), encode(c));
}

/// The contracts module (kinds 20 and 21). Register it with `Executor::with_module`.
pub struct ContractsModule;

impl Module for ContractsModule {
    fn kinds(&self) -> core::ops::RangeInclusive<u8> {
        kind::CREATE..=kind::ACT
    }

    fn min_fee(&self, rules: &Rules, _tx: &Tx) -> u64 {
        rules.contract_fee
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
            kind::CREATE => {
                let template_id = d.string(32).map_err(bad)?.to_string();
                let id = d.string(64).map_err(bad)?.to_string();
                let party_b = Address(Hash32(d.fixed::<32>().map_err(bad)?));
                let n = d.count(16).map_err(bad)?;
                let mut values = Vec::with_capacity(n);
                for _ in 0..n {
                    values.push(d.u64().map_err(bad)?);
                }
                d.finish().map_err(bad)?;
                let tpl = template(&template_id).ok_or(err("no_template"))?;
                check(
                    !id.is_empty() && id.chars().all(|c| !c.is_control()),
                    "bad_id",
                )?;
                check(get_contract(state, &id).is_none(), "contract_exists")?;
                check(party_b != ctx.sender, "self_deal")?;
                check(values.len() == tpl.terms.len(), "bad_terms")?;
                let terms: BTreeMap<String, u64> = tpl
                    .terms
                    .iter()
                    .map(|k| (*k).to_string())
                    .zip(values)
                    .collect();
                check(terms_ok(rules, &template_id, &terms), "bad_terms")?;
                let mut c = Contract {
                    id,
                    template: template_id,
                    a: ctx.sender,
                    b: party_b,
                    terms,
                    state: tpl.initial_state.to_string(),
                    escrow: 0,
                    vars: BTreeMap::new(),
                };
                apply(&tpl.initial_effects, state, ctx.time, &mut c)?;
                put(state, &c);
                Ok(())
            }
            kind::ACT => {
                let id = d.string(64).map_err(bad)?.to_string();
                let action = d.string(32).map_err(bad)?.to_string();
                d.finish().map_err(bad)?;
                let mut c = get_contract(state, &id).ok_or(err("no_contract"))?;
                let tpl = template(&c.template).ok_or(err("no_template"))?;
                let t = tpl
                    .transitions
                    .iter()
                    .find(|t| t.name == action)
                    .ok_or(err("no_transition"))?;
                check(c.state == t.from, "bad_state")?;
                match t.actor {
                    Actor::Any => {}
                    Actor::OnlyA => check(ctx.sender == c.a, "not_party")?,
                    Actor::OnlyB => check(ctx.sender == c.b, "not_party")?,
                }
                for g in &t.guards {
                    guard_ok(g, ctx.time, &c)?;
                }
                apply(&t.effects, state, ctx.time, &mut c)?;
                c.state = t.to.to_string();
                put(state, &c);
                Ok(())
            }
            _ => Err(LedgerError::UnknownKind(tx.kind)),
        }
    }
}
