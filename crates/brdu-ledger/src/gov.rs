//! Governança do conjunto de validadores (rotação, ADR-0015). O conjunto em vigor é **estado da cadeia**
//! (`val/set`, `val/epoch`, `val/pending` no armazenamento de módulos), então entra na raiz de estado e todo nó
//! chega ao mesmo conjunto em cada altura.
//!
//! Uma troca (`add` ou `remove` de **uma** chave) vale com **mais de 2/3 do conjunto em vigor** aprovando o
//! documento exato; só uma troca pode estar pendente; o conjunto fica entre `min` e `max`; a altura efetiva é
//! futura. Ao fim do bloco `efetiva − 1` o conjunto novo passa a valer e a época sobe.

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Hash32, PublicKey, tagged};

use crate::coin::{APPROVAL_LEN, Approval, MAX_APPROVALS};
use crate::error::LedgerError;
use crate::exec::{Module, TxCtx};
use crate::rules::Rules;
use crate::state::State;
use crate::tx::{Tx, kind};

/// Prefixo de domínio do documento que os validadores assinam.
pub const CHANGE_TAG: &str = "brdu/validators/change";
/// Com quantos blocos de antecedência uma troca precisa ser agendada (mínimo).
pub const MIN_LEAD: u64 = 2;

const K_SET: &[u8] = b"val/set";
const K_EPOCH: &[u8] = b"val/epoch";
const K_PENDING: &[u8] = b"val/pending";
const K_MIN: &[u8] = b"val/min";
const K_MAX: &[u8] = b"val/max";

/// O que a troca faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Entra um validador (vai para o fim da ordem do rodízio).
    Add,
    /// Sai um validador.
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

/// Uma troca agendada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    /// O que faz.
    pub action: Action,
    /// Quem entra ou sai.
    pub key: PublicKey,
    /// A primeira altura em que o conjunto novo vale.
    pub effective_height: u64,
}

// --- leitura do estado -------------------------------------------------------------------------------------

/// O conjunto de validadores em vigor, **na ordem do rodízio**.
#[must_use]
pub fn validators(state: &State) -> Vec<PublicKey> {
    state
        .ext_get(K_SET)
        .and_then(decode_set)
        .unwrap_or_default()
}

/// A época (sobe a cada troca efetivada; 0 é a gênese).
#[must_use]
pub fn epoch(state: &State) -> u64 {
    num(state, K_EPOCH)
}

/// A troca pendente, se houver.
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

/// Limites de tamanho do conjunto: `(mínimo, máximo)`.
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

/// Grava o conjunto inicial (a gênese). Só o `Chain` chama, uma vez, antes do bloco 1.
pub fn init(state: &mut State, set: &[PublicKey], min: usize, max: usize) {
    state.ext_put(K_SET.to_vec(), encode_set(set));
    put_num(state, K_EPOCH, 0);
    put_num(state, K_MIN, min as u64);
    put_num(state, K_MAX, max as u64);
}

// --- documento e carga --------------------------------------------------------------------------------------

/// O documento que os validadores assinam: rede, ação, chave, altura efetiva e **época atual** (impede reaproveitar
/// uma aprovação depois de outra troca).
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

/// A carga da transação `VALIDATOR_CHANGE`.
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

// --- o módulo ---------------------------------------------------------------------------------------------------

/// O módulo de governança do conjunto de validadores (tipo 22).
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
            return Err(LedgerError::Module(
                "já há uma troca de validadores pendente",
            ));
        }
        if c.effective_height < ctx.height.saturating_add(MIN_LEAD) {
            return Err(LedgerError::Module("a altura efetiva tem de ser futura"));
        }
        let present = set.contains(&c.key);
        let new_len = match c.action {
            Action::Add if present => return Err(LedgerError::Module("a chave já é validadora")),
            Action::Remove if !present => {
                return Err(LedgerError::Module("a chave não é validadora"));
            }
            Action::Add => set.len() + 1,
            Action::Remove => set.len() - 1,
        };
        if new_len < min || new_len > max {
            return Err(LedgerError::Module(
                "o conjunto sairia dos limites de tamanho",
            ));
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
