//! The server: static page + **read-only** JSON API. No secrets, no writes. Strict CSP (nothing inline,
//! nothing from third parties).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use brdu_crypto::{Address, Hash32, hex};
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::index::Explorer;
use crate::view;

/// The explorer shared between the sync and the server.
pub type Shared = Arc<RwLock<Explorer>>;

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const INDEX: &str = include_str!("../ui/index.html");
const CSS: &str = include_str!("../ui/app.css");
const JS: &str = include_str!("../ui/app.js");
const SCRAMBLE: &str = include_str!("../ui/scramble.js");
const FORMAT: &str = include_str!("../ui/format.js");
const EFFECTS: &str = include_str!("../ui/effects.js");
const FAVICON: &str = include_str!("../ui/favicon.svg");

/// Starts the server (4 threads). Returns the actual address and the threads.
pub fn serve(
    addr: SocketAddr,
    shared: Shared,
    stop: Arc<AtomicBool>,
) -> std::io::Result<(SocketAddr, Vec<JoinHandle<()>>)> {
    let server = Arc::new(Server::http(addr).map_err(|e| std::io::Error::other(e.to_string()))?);
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| std::io::Error::other("no ip"))?;
    let handles = (0..4)
        .map(|_| {
            let (server, shared, stop) = (server.clone(), shared.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match server.recv_timeout(Duration::from_millis(200)) {
                        Ok(Some(req)) => handle(req, &shared),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            })
        })
        .collect();
    Ok((bound, handles))
}

fn header(k: &str, v: &str) -> Option<Header> {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).ok()
}

fn send(req: Request, code: u16, ctype: &str, body: String, cache: &str) {
    let mut r = Response::from_string(body).with_status_code(code);
    for (k, v) in [
        ("Content-Type", ctype),
        ("Content-Security-Policy", CSP),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        ("Cache-Control", cache),
    ] {
        if let Some(h) = header(k, v) {
            r = r.with_header(h);
        }
    }
    let _ = req.respond(r);
}

fn handle(req: Request, shared: &Shared) {
    if req.method() != &Method::Get {
        return send(
            req,
            405,
            "application/json",
            json!({"error": "read-only"}).to_string(),
            "no-store",
        );
    }
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let (js, css, html) = (
        "text/javascript; charset=utf-8",
        "text/css; charset=utf-8",
        "text/html; charset=utf-8",
    );
    match path {
        "/" | "/index.html" => send(req, 200, html, INDEX.into(), "no-cache"),
        "/app.css" => send(req, 200, css, CSS.into(), "no-cache"),
        "/app.js" => send(req, 200, js, JS.into(), "no-cache"),
        "/scramble.js" => send(req, 200, js, SCRAMBLE.into(), "no-cache"),
        "/format.js" => send(req, 200, js, FORMAT.into(), "no-cache"),
        "/effects.js" => send(req, 200, js, EFFECTS.into(), "no-cache"),
        "/favicon.svg" => send(req, 200, "image/svg+xml", FAVICON.into(), "no-cache"),
        p if p.starts_with("/api/") => {
            let (code, body) = shared.read().map_or((503, json!({"error": "busy"})), |e| {
                api(&e, p.trim_start_matches("/api/"), query)
            });
            send(req, code, "application/json", body.to_string(), "no-store");
        }
        _ => send(
            req,
            404,
            "application/json",
            json!({"error": "not found"}).to_string(),
            "no-store",
        ),
    }
}

fn q(query: &str, key: &str) -> Option<usize> {
    query.split('&').find_map(|kv| {
        kv.strip_prefix(&format!("{key}="))
            .and_then(|v| v.parse().ok())
    })
}

fn qs<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
}

fn block_at(e: &Explorer, h: u64) -> Option<&brdu_chain::DecidedBlock> {
    e.chain
        .blocks()
        .get(usize::try_from(h.checked_sub(1)?).ok()?)
}

/// The API: `GET /api/...`.
#[must_use]
pub fn api(e: &Explorer, path: &str, query: &str) -> (u16, Value) {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let n_vals = e.chain.validators().len();
    match parts.as_slice() {
        ["overview"] => (200, view::overview(e)),
        ["blocks"] => {
            let limit = q(query, "limit").unwrap_or(20).min(100);
            let height = e.height();
            // from `before` (exclusive) backwards: pagination from newest to oldest
            let before = q(query, "before")
                .map_or(height + 1, |b| b as u64)
                .min(height + 1);
            let list: Vec<Value> = (1..before)
                .rev()
                .take(limit)
                .filter_map(|h| block_at(e, h))
                .map(|d| view::block(d, n_vals, false))
                .collect();
            (200, json!({"height": height, "blocks": list}))
        }
        ["block", h] => match h.parse::<u64>().ok().and_then(|h| block_at(e, h)) {
            Some(d) => (200, view::block(d, n_vals, true)),
            None => (404, json!({"error": "block not found"})),
        },
        ["tx", id] => match hex::decode_array::<32>(id)
            .map(Hash32)
            .and_then(|id| e.find_tx(&id))
        {
            Some((h, i)) => match block_at(e, h).and_then(|d| d.block.txs.get(i)) {
                Some(t) => (200, view::tx(t, h, i)),
                None => (404, json!({"error": "transaction not found"})),
            },
            None => (404, json!({"error": "transaction not found"})),
        },
        ["account", a] => match Address::decode(a) {
            Ok(a) => (
                200,
                view::account(
                    e,
                    &a,
                    q(query, "skip").unwrap_or(0),
                    q(query, "limit").unwrap_or(25).min(100),
                ),
            ),
            Err(_) => (400, json!({"error": "invalid address"})),
        },
        ["validators"] => {
            let o = view::overview(e);
            (
                200,
                json!({"epoch": o["epoch"], "validators": o["validators"], "pending_change": o["pending_change"], "evidence": e.evidence, "evidence_note": "reported by the node; not verifiable here"}),
            )
        }
        ["search"] => (200, search(e, qs(query, "q").unwrap_or(""))),
        _ => (404, json!({"error": "not found"})),
    }
}

/// Search: height, block hash, transaction id or address. Returns where to go.
fn search(e: &Explorer, raw: &str) -> Value {
    let q = raw.trim();
    if q.is_empty() {
        return json!({"found": false});
    }
    if let Ok(h) = q.parse::<u64>() {
        return if block_at(e, h).is_some() {
            json!({"found": true, "kind": "block", "id": h})
        } else {
            json!({"found": false, "hint": "height does not exist yet"})
        };
    }
    if q.starts_with("brdu1") {
        return match Address::decode(q) {
            Ok(a) => json!({"found": true, "kind": "account", "id": a.encode()}),
            Err(_) => {
                json!({"found": false, "hint": "address has a typo (checksum mismatch)"})
            }
        };
    }
    if let Some(h) = hex::decode_array::<32>(q).map(Hash32) {
        if e.find_tx(&h).is_some() {
            return json!({"found": true, "kind": "tx", "id": h.to_hex()});
        }
        if let Some(height) = e.find_block(&h) {
            return json!({"found": true, "kind": "block", "id": height});
        }
    }
    json!({"found": false})
}
