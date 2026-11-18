//! HTTP API (JSON). Read-only, except `POST /tx`, which carries an **already signed** transaction: the node never
//! holds a user key.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use brdu_crypto::{Address, Hash32, hex};
use brdu_ledger::{Coverage, Tx};
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::core::Input;
use crate::node::Shared;

const WORKERS: usize = 4;
const MAX_BODY: usize = 128 * 1024;

/// Verifies a transaction and sends it to the core. The signature is checked **on the caller's thread** (in parallel).
pub fn submit_raw(shared: &Shared, core: &Sender<Input>, raw: &[u8]) -> Result<Hash32, String> {
    let tx = Tx::decode(raw).map_err(|e| e.to_string())?;
    tx.verify().map_err(|e| e.to_string())?;
    let (reply, rx) = channel();
    core.send(Input::Tx {
        tx,
        from: None,
        reply: Some(reply),
    })
    .map_err(|_| "node stopped".to_string())?;
    let _ = shared;
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| "timeout".to_string())?
}

/// Spawns the API threads.
pub fn spawn(
    addr: SocketAddr,
    shared: Arc<Shared>,
    core: Sender<Input>,
) -> std::io::Result<(Vec<JoinHandle<()>>, SocketAddr)> {
    let server = Arc::new(Server::http(addr).map_err(|e| std::io::Error::other(e.to_string()))?);
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| std::io::Error::other("no ip"))?;
    let handles = (0..WORKERS)
        .map(|_| {
            let (server, shared, core) = (server.clone(), shared.clone(), core.clone());
            std::thread::spawn(move || {
                while !shared.stop.load(Ordering::SeqCst) {
                    match server.recv_timeout(Duration::from_millis(200)) {
                        Ok(Some(req)) => handle(req, &shared, &core),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            })
        })
        .collect();
    Ok((handles, bound))
}

fn respond(req: Request, code: u16, body: &Value) {
    let mut r = Response::from_string(body.to_string()).with_status_code(code);
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
        r = r.with_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]) {
        r = r.with_header(h);
    }
    let _ = req.respond(r);
}

fn handle(mut req: Request, shared: &Shared, core: &Sender<Input>) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let method = req.method().clone();
    if method == Method::Post && parts == ["tx"] {
        let mut body = Vec::new();
        let _ = std::io::Read::read_to_end(
            &mut std::io::Read::take(req.as_reader(), MAX_BODY as u64 + 1),
            &mut body,
        );
        if body.len() > MAX_BODY {
            return respond(req, 413, &json!({"error": "body too large"}));
        }
        let text = String::from_utf8_lossy(&body);
        let Some(raw) = hex::decode(text.trim()) else {
            return respond(
                req,
                400,
                &json!({"error": "body must be the hex of an encoded transaction"}),
            );
        };
        return match submit_raw(shared, core, &raw) {
            Ok(id) => respond(req, 200, &json!({"id": id.to_hex()})),
            Err(e) => respond(req, 400, &json!({"error": e})),
        };
    }
    if method != Method::Get {
        return respond(req, 405, &json!({"error": "method not allowed"}));
    }
    let (code, body) = route(&parts, query, shared);
    respond(req, code, &body);
}

fn q(query: &str, key: &str) -> Option<u64> {
    query.split('&').find_map(|kv| {
        kv.strip_prefix(&format!("{key}="))
            .and_then(|v| v.parse().ok())
    })
}

fn route(parts: &[&str], query: &str, shared: &Shared) -> (u16, Value) {
    let Ok(chain) = shared.chain.lock() else {
        return (503, json!({"error": "busy"}));
    };
    let rules = chain.executor().rules();
    match parts {
        ["status"] | [""] => {
            let info = shared.info.lock();
            let (peers, pool, pool_bytes, running, evidence, net_errors) =
                info.map_or((0, 0, 0, false, 0, Vec::new()), |i| {
                    (
                        i.peers.len(),
                        i.pool_len,
                        i.pool_bytes,
                        i.engine_running,
                        i.evidence.len(),
                        i.net_errors.clone(),
                    )
                });
            (
                200,
                json!({
                    "chain_id": chain.genesis().chain_id,
                    "genesis_hash": chain.genesis_hash().to_hex(),
                    "height": chain.height(),
                    "epoch": chain.epoch(),
                    "tip": chain.state().last_hash().map(|h| h.to_hex()),
                    "state_root": chain.state().root().to_hex(),
                    "validators": chain.validators().iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>(),
                    "me": Address::from_public_key(&shared.me).encode(),
                    "peers": peers, "mempool": pool, "mempool_bytes": pool_bytes,
                    "consensus_running": running, "evidence": evidence, "net_errors": net_errors,
                    "issuer_threshold": rules.issuer_threshold(),
                }),
            )
        }
        ["validators"] => {
            let set = chain.validators();
            let pending = chain.pending_change().map(|p| {
                json!({
                    "action": if p.action == brdu_ledger::gov::Action::Add { "add" } else { "remove" },
                    "validator": Address::from_public_key(&p.key).encode(),
                    "effective_height": p.effective_height,
                })
            });
            (
                200,
                json!({
                    "epoch": chain.epoch(),
                    "validators": set.iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>(),
                    "pending": pending,
                    "me_in_set": set.contains(&shared.me),
                }),
            )
        }
        ["credential-types"] => {
            let list: Vec<Value> = brdu_credentials::list_types(chain.state())
                .iter()
                .map(type_json)
                .collect();
            (200, json!({"types": list}))
        }
        ["credential", id] => match brdu_credentials::get_credential(chain.state(), id) {
            Some(c) => (
                200,
                credential_json(
                    &c,
                    chain.state().time(),
                    brdu_credentials::valid(chain.state(), id, chain.state().time()),
                ),
            ),
            None => (404, json!({"error": "credential not found"})),
        },
        ["credentials", addr] => match Address::decode(addr) {
            Ok(a) => {
                let now = chain.state().time();
                let list: Vec<Value> = brdu_credentials::credentials_of(chain.state(), &a)
                    .iter()
                    .map(|c| {
                        credential_json(c, now, brdu_credentials::valid(chain.state(), &c.id, now))
                    })
                    .collect();
                (200, json!({"address": a.encode(), "credentials": list}))
            }
            Err(_) => (400, json!({"error": "invalid address"})),
        },
        ["contract-templates"] => {
            let list: Vec<Value> = brdu_contracts::TEMPLATE_IDS
                .iter()
                .filter_map(|id| brdu_contracts::template(id))
                .map(|t| {
                    json!({
                        "id": t.id, "terms": t.terms, "initial_state": t.initial_state,
                        "locks_at_creation": !t.initial_effects.is_empty(),
                        "transitions": t.transitions.iter().map(|x| json!({
                            "name": x.name, "from": x.from, "to": x.to,
                            "actor": match x.actor {
                                brdu_contracts::Actor::OnlyA => "a",
                                brdu_contracts::Actor::OnlyB => "b",
                                brdu_contracts::Actor::Any => "any",
                            },
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            (200, json!({"templates": list}))
        }
        ["contract", id] => match brdu_contracts::get_contract(chain.state(), id) {
            Some(c) => (200, contract_json(&c)),
            None => (404, json!({"error": "contract not found"})),
        },
        ["contracts", addr] => match Address::decode(addr) {
            Ok(a) => {
                let list: Vec<Value> = brdu_contracts::contracts_of(chain.state(), &a)
                    .iter()
                    .map(contract_json)
                    .collect();
                (200, json!({"address": a.encode(), "contracts": list}))
            }
            Err(_) => (400, json!({"error": "invalid address"})),
        },
        ["asset-classes"] => {
            let list: Vec<Value> = brdu_assets::list_classes(chain.state())
                .iter()
                .map(|c| json!({"id": c.id, "threshold": c.threshold, "veto_window": c.veto_window,
                    "organ": c.organ.iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>()}))
                .collect();
            (200, json!({"classes": list}))
        }
        ["asset", id] => match brdu_assets::get_asset(chain.state(), id) {
            Some(a) => (
                200,
                asset_json(&a, brdu_assets::get_deal(chain.state(), id).as_ref()),
            ),
            None => (404, json!({"error": "asset not found"})),
        },
        ["assets", addr] => match Address::decode(addr) {
            Ok(a) => {
                let list: Vec<Value> = brdu_assets::assets_of(chain.state(), &a)
                    .iter()
                    .map(|x| asset_json(x, brdu_assets::get_deal(chain.state(), &x.id).as_ref()))
                    .collect();
                (200, json!({"address": a.encode(), "assets": list}))
            }
            Err(_) => (400, json!({"error": "invalid address"})),
        },
        ["deals"] => {
            let list: Vec<Value> = brdu_assets::list_deals(chain.state())
                .iter()
                .map(deal_json)
                .collect();
            (
                200,
                json!({"deals": list, "escrow": brdu_assets::escrow_total(chain.state())}),
            )
        }
        ["supply"] => {
            let s = chain.state();
            let cov = match s.coverage() {
                Coverage::None => json!({"state": "none"}),
                Coverage::Covered(x) => json!({"state": "covered", "surplus": x}),
                Coverage::Under(x) => json!({"state": "under", "shortfall": x}),
            };
            (
                200,
                json!({
                    "minted": s.minted(), "burned": s.burned(), "supply": s.supply(),
                    "reserve": s.attestation().map(|a| a.reserve), "attestation_seq": s.attestation().map(|a| a.seq),
                    "coverage": cov, "unit": "1 BRDU = 1,000,000 base units",
                }),
            )
        }
        ["fees"] => (
            200,
            json!({"transfer_base": rules.transfer_base, "per_byte": rules.per_byte, "max_block_txs": rules.max_block_txs, "max_block_bytes": rules.max_block_bytes, "registration_fee": rules.registration_fee, "credential_fee": rules.credential_fee, "step_fee": rules.step_fee, "contract_fee": rules.contract_fee}),
        ),
        ["account", addr] => match Address::decode(addr) {
            Ok(a) => (
                200,
                json!({"address": a.encode(), "balance": chain.state().balance(&a), "nonce": chain.state().nonce(&a)}),
            ),
            Err(_) => (400, json!({"error": "invalid address"})),
        },
        ["block", h] => match h
            .parse::<u64>()
            .ok()
            .filter(|h| *h >= 1)
            .and_then(|h| chain.blocks().get(usize::try_from(h - 1).ok()?))
        {
            Some(d) => (200, block_json(d, true)),
            None => (404, json!({"error": "block not found"})),
        },
        ["blocks"] => {
            let from = q(query, "from").unwrap_or(1).max(1);
            let limit = q(query, "limit").unwrap_or(20).min(100);
            let list: Vec<Value> = chain
                .blocks_after(from - 1)
                .iter()
                .take(usize::try_from(limit).unwrap_or(20))
                .map(|d| block_json(d, false))
                .collect();
            (200, json!({"height": chain.height(), "blocks": list}))
        }
        ["blocks", "raw"] => {
            // encoded final blocks (block + certificate) in hex: whoever does not trust the node re-verifies them itself
            let from = q(query, "from").unwrap_or(1).max(1);
            let limit = usize::try_from(q(query, "limit").unwrap_or(16).clamp(1, 32)).unwrap_or(16);
            let list: Vec<String> = chain
                .blocks_after(from - 1)
                .iter()
                .take(limit)
                .filter_map(|d| d.encode().ok())
                .map(|raw| hex::encode(&raw))
                .collect();
            (200, json!({"height": chain.height(), "blocks": list}))
        }
        ["tx", id] => {
            let Some(raw) = hex::decode_array::<32>(id) else {
                return (400, json!({"error": "invalid id"}));
            };
            let id = Hash32(raw);
            let height = shared
                .info
                .lock()
                .ok()
                .and_then(|i| i.tx_index.get(&id).copied());
            match height.and_then(|h| chain.blocks().get(usize::try_from(h - 1).ok()?)) {
                Some(d) => match d.block.txs.iter().find(|t| t.id() == id) {
                    Some(t) => (
                        200,
                        json!({"id": id.to_hex(), "height": d.block.height, "tx": tx_json(t)}),
                    ),
                    None => (404, json!({"error": "transaction not found"})),
                },
                None => (404, json!({"error": "transaction not found"})),
            }
        }
        ["evidence"] => {
            let ev = shared
                .info
                .lock()
                .map(|i| i.evidence.clone())
                .unwrap_or_default();
            let list: Vec<Value> = ev
                .iter()
                .map(|(a, b)| {
                    json!({"voter": Address::from_public_key(&a.voter).encode(), "height": a.height, "round": a.round,
                           "a": a.block_hash.map(|h| h.to_hex()), "b": b.block_hash.map(|h| h.to_hex())})
                })
                .collect();
            (200, json!({"evidence": list}))
        }
        ["metrics"] => {
            let st = &shared.stats;
            let g = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
            (
                200,
                json!({
                    "txs_accepted": g(&st.txs_accepted), "txs_rejected": g(&st.txs_rejected), "txs_duplicate": g(&st.txs_duplicate),
                    "txs_relayed": g(&st.txs_relayed), "blocks": g(&st.blocks), "extra_rounds": g(&st.extra_rounds), "msgs_in": g(&st.msgs_in),
                }),
            )
        }
        _ => (404, json!({"error": "not found"})),
    }
}

fn tx_json(t: &Tx) -> Value {
    json!({
        "id": t.id().to_hex(), "kind": t.kind, "sender": t.sender_address().encode(),
        "nonce": t.nonce, "fee": t.fee, "size": t.size(),
    })
}

fn block_json(d: &brdu_chain::DecidedBlock, full: bool) -> Value {
    let b = &d.block;
    let mut v = json!({
        "height": b.height, "time": b.time, "hash": b.hash().to_hex(),
        "prev_hash": b.prev_hash.map(|h| h.to_hex()), "proposer": b.proposer.encode(),
        "state_root": b.state_root.to_hex(), "tx_count": b.txs.len(), "round": d.cert.round,
        "signed_by": d.cert.signers().iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>(),
    });
    if full && let Some(o) = v.as_object_mut() {
        o.insert(
            "txs".into(),
            Value::Array(b.txs.iter().map(tx_json).collect()),
        );
    }
    v
}

fn type_json(t: &brdu_credentials::TypeSpec) -> Value {
    json!({
        "id": t.id, "organ": t.organ.iter().map(|k| Address::from_public_key(k).encode()).collect::<Vec<_>>(),
        "threshold": t.threshold, "contest_window": t.contest, "validity": t.validity,
        "privacy": if t.privacy == brdu_credentials::Privacy::Private { "private" } else { "public" },
        "ref_type": t.ref_type, "revokes_reference": t.revokes,
    })
}

fn credential_json(c: &brdu_credentials::Credential, now: u64, valid: bool) -> Value {
    json!({
        "id": c.id, "type": c.type_id, "subject": c.subject.encode(), "data_hex": hex::encode(&c.data),
        "reference": c.reference, "status": c.status.name(), "issued_at": c.issued_at, "active_at": c.active_at,
        "expires_at": c.expires_at, "valid": valid, "now": now,
    })
}

fn deal_json(d: &brdu_assets::Deal) -> Value {
    json!({
        "asset": d.asset_id, "seller": d.seller.encode(), "buyer": d.buyer.encode(), "price": d.price,
        "status": d.status.name(), "deadline": d.deadline,
    })
}

fn asset_json(a: &brdu_assets::Asset, deal: Option<&brdu_assets::Deal>) -> Value {
    json!({
        "id": a.id, "class": a.class, "owner": a.owner.encode(), "meta_hex": hex::encode(&a.meta), "rev": a.rev,
        "liens": a.liens, "deal": deal.map(deal_json),
    })
}

fn contract_json(c: &brdu_contracts::Contract) -> Value {
    json!({
        "id": c.id, "template": c.template, "a": c.a.encode(), "b": c.b.encode(), "terms": c.terms,
        "state": c.state, "escrow": c.escrow, "vars": c.vars,
        "final": brdu_contracts::TERMINAL.contains(&c.state.as_str()),
    })
}
