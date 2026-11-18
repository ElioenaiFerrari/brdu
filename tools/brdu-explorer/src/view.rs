//! What the explorer API returns: JSON built only from the **verified** chain.

use brdu_chain::DecidedBlock;
use brdu_crypto::encoding::Decoder;
use brdu_crypto::{Address, Hash32, PublicKey};
use brdu_ledger::{Coverage, Tx, kind};
use serde_json::{Value, json};

use crate::index::Explorer;

/// Human-readable transaction kind name (an English code; the UI translates it).
#[must_use]
pub const fn kind_name(k: u8) -> &'static str {
    match k {
        kind::TRANSFER => "transfer",
        kind::MINT => "mint",
        kind::BURN => "burn",
        kind::ATTESTATION => "reserve attestation",
        kind::VALIDATOR_CHANGE => "validator change",
        5..=13 => "assets",
        14..=19 => "credentials",
        20..=21 => "contracts",
        _ => "unknown",
    }
}

/// The other end of the transaction (transfer destination, mint/burn account).
#[must_use]
pub fn counterparty(tx: &Tx) -> Option<Address> {
    match tx.kind {
        kind::TRANSFER | kind::MINT | kind::BURN => {
            let mut d = Decoder::new(&tx.payload);
            Some(Address(Hash32(d.fixed::<32>().ok()?)))
        }
        _ => None,
    }
}

/// A readable summary of the payload (what the transaction does).
fn summary(tx: &Tx) -> Value {
    let mut d = Decoder::new(&tx.payload);
    match tx.kind {
        kind::TRANSFER => match (d.fixed::<32>(), d.u64()) {
            (Ok(to), Ok(amount)) => json!({"to": Address(Hash32(to)).encode(), "amount": amount}),
            _ => Value::Null,
        },
        kind::MINT | kind::BURN => match (d.fixed::<32>(), d.u64(), d.string(64)) {
            (Ok(acc), Ok(amount), Ok(reference)) => {
                json!({"account": Address(Hash32(acc)).encode(), "amount": amount, "reference": reference})
            }
            _ => Value::Null,
        },
        kind::ATTESTATION => match (d.u64(), d.u64(), d.string(64)) {
            (Ok(seq), Ok(reserve), Ok(reference)) => {
                json!({"seq": seq, "reserve": reserve, "reference": reference})
            }
            _ => Value::Null,
        },
        kind::VALIDATOR_CHANGE => match (d.u8(), d.fixed::<32>(), d.u64()) {
            (Ok(action), Ok(key), Ok(eff)) => json!({
                "action": if action == 1 { "add" } else { "remove" },
                "validator": Address::from_public_key(&PublicKey(key)).encode(),
                "effective_height": eff,
            }),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

/// A transaction.
#[must_use]
pub fn tx(t: &Tx, height: u64, index: usize) -> Value {
    json!({
        "id": t.id().to_hex(), "height": height, "index": index,
        "kind": t.kind, "kind_name": kind_name(t.kind),
        "sender": t.sender_address().encode(), "nonce": t.nonce, "fee": t.fee, "size": t.size(),
        "summary": summary(t),
    })
}

/// A block (with or without the transaction list).
#[must_use]
pub fn block(d: &DecidedBlock, validators: usize, full: bool) -> Value {
    let b = &d.block;
    let mut v = json!({
        "height": b.height, "time": b.time, "hash": b.hash().to_hex(),
        "prev_hash": b.prev_hash.map(|h| h.to_hex()), "state_root": b.state_root.to_hex(),
        "proposer": b.proposer.encode(), "round": d.cert.round, "tx_count": b.txs.len(),
        "signed_by": d.cert.signers().iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>(),
        "validators": validators,
        // the explorer only stores blocks it verified itself
        "verified": true,
    });
    if full && let Some(o) = v.as_object_mut() {
        o.insert(
            "txs".into(),
            Value::Array(
                b.txs
                    .iter()
                    .enumerate()
                    .map(|(i, t)| tx(t, b.height, i))
                    .collect(),
            ),
        );
    }
    v
}

fn coverage(x: Coverage) -> Value {
    match x {
        Coverage::None => json!({"state": "none"}),
        Coverage::Covered(s) => json!({"state": "covered", "surplus": s}),
        Coverage::Under(s) => json!({"state": "under", "shortfall": s}),
    }
}

/// The network overview.
#[must_use]
pub fn overview(e: &Explorer) -> Value {
    let c = &e.chain;
    let s = c.state();
    let validators = c.validators();
    let recent: Vec<Value> = c
        .blocks()
        .iter()
        .rev()
        .take(8)
        .map(|d| block(d, validators.len(), false))
        .collect();
    let pending = c.pending_change().map(|p| {
        json!({
            "action": if p.action == brdu_ledger::gov::Action::Add { "add" } else { "remove" },
            "validator": Address::from_public_key(&p.key).encode(),
            "effective_height": p.effective_height,
        })
    });
    json!({
        "chain_id": c.genesis().chain_id,
        "genesis_hash": c.genesis_hash().to_hex(),
        "height": c.height(),
        "epoch": c.epoch(),
        "state_root": s.root().to_hex(),
        "tx_total": e.tx_total(),
        "accounts": s.accounts_len(),
        "supply": {
            "minted": s.minted(), "burned": s.burned(), "supply": s.supply(),
            "reserve": s.attestation().map(|a| a.reserve), "attestation_seq": s.attestation().map(|a| a.seq),
            "coverage": coverage(s.coverage()),
        },
        "validators": validators.iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>(),
        "pending_change": pending,
        "recent_blocks": recent,
        "node": {
            "address": e.node, "height": e.status.node_height,
            "behind": e.status.node_height.saturating_sub(c.height()),
            "last_error": e.status.last_error, "rejected_blocks": e.status.rejected_blocks,
            "synced_secs_ago": e.status.last_ok.map(|t| t.elapsed().as_secs()),
        },
    })
}

/// An account: balance, nonce and the transactions it appears in.
#[must_use]
pub fn account(e: &Explorer, a: &Address, skip: usize, limit: usize) -> Value {
    let st = e.chain.state().account(a);
    let txs: Vec<Value> = e
        .account_txs(a, skip, limit)
        .iter()
        .filter_map(|(h, i)| {
            let d = e.chain.blocks().get(usize::try_from(h - 1).ok()?)?;
            Some(tx(d.block.txs.get(*i)?, *h, *i))
        })
        .collect();
    json!({
        "address": a.encode(), "balance": st.balance, "nonce": st.nonce,
        "is_validator": e.chain.validators().iter().any(|k| Address::from_public_key(k) == *a),
        "tx_count": e.account_tx_count(a), "txs": txs,
    })
}
