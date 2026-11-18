//! Local web daemon for the wallet. It serves the same UI as the desktop app and forwards `POST /rpc` to the
//! wallet service. IMPORTANT: here the keys live in THIS process (the browser is not a hardware wallet): use it with
//! TEST keys only. Protections: loopback by default, `Host` and `Origin` checks (DNS rebinding and cross-site
//! requests), JSON-only `/rpc`, body size limit, strict CSP, no CORS, no caching of RPC answers.
#![forbid(unsafe_code)]

use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use brdu_walletd::Wallet;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const MAX_BODY: u64 = 64 * 1024;

/// The UI files (the same ones the Tauri app uses), embedded at build time.
const ASSETS: &[(&str, &str, &str)] = &[
    (
        "/",
        "text/html; charset=utf-8",
        include_str!("../../../apps/wallet/ui/index.html"),
    ),
    (
        "/index.html",
        "text/html; charset=utf-8",
        include_str!("../../../apps/wallet/ui/index.html"),
    ),
    (
        "/wallet.css",
        "text/css; charset=utf-8",
        include_str!("../../../apps/wallet/ui/wallet.css"),
    ),
    (
        "/app.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/app.js"),
    ),
    (
        "/api.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/api.js"),
    ),
    (
        "/i18n.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/i18n.js"),
    ),
    (
        "/money.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/money.js"),
    ),
    (
        "/format.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/format.js"),
    ),
    (
        "/effects.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/effects.js"),
    ),
    (
        "/scramble.js",
        "text/javascript; charset=utf-8",
        include_str!("../../../apps/wallet/ui/scramble.js"),
    ),
    (
        "/favicon.svg",
        "image/svg+xml",
        include_str!("../../../apps/wallet/ui/favicon.svg"),
    ),
];

/// Daemon options.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Skip the `Host`/`Origin` allow-list (only for deployments behind a trusted proxy, e.g. Docker port mapping).
    pub allow_any_host: bool,
}

/// Starts the daemon (4 threads). Returns the bound address and the threads.
pub fn serve(
    addr: SocketAddr,
    wallet: Arc<Wallet>,
    opts: Options,
    stop: Arc<AtomicBool>,
) -> std::io::Result<(SocketAddr, Vec<JoinHandle<()>>)> {
    let server = Arc::new(Server::http(addr).map_err(|e| std::io::Error::other(e.to_string()))?);
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| std::io::Error::other("no ip"))?;
    let handles = (0..4)
        .map(|_| {
            let (server, wallet, stop) = (server.clone(), wallet.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match server.recv_timeout(Duration::from_millis(200)) {
                        Ok(Some(req)) => handle(req, &wallet, bound, opts),
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

fn reply(req: Request, code: u16, ctype: &str, body: String, cache: &str) {
    let mut r = Response::from_string(body).with_status_code(code);
    for (k, v) in [
        ("Content-Type", ctype),
        ("Content-Security-Policy", CSP),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        ("Cache-Control", cache),
        ("Cross-Origin-Resource-Policy", "same-origin"),
    ] {
        if let Some(h) = header(k, v) {
            r = r.with_header(h);
        }
    }
    let _ = req.respond(r);
}

fn json_reply(req: Request, code: u16, body: &Value) {
    reply(req, code, "application/json", body.to_string(), "no-store");
}

fn header_value(req: &Request, name: &str) -> Option<String> {
    let wanted = name.to_ascii_lowercase();
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().to_ascii_lowercase() == wanted)
        .map(|h| h.value.to_string())
}

/// `Host` must be the daemon's own loopback address (or `localhost`); `Origin`, if sent, must be that same host.
fn host_allowed(req: &Request, bound: SocketAddr) -> bool {
    let port = bound.port();
    let ok = |h: &str| {
        h == format!("127.0.0.1:{port}")
            || h == format!("localhost:{port}")
            || h == format!("[::1]:{port}")
    };
    let host_ok = header_value(req, "Host").is_some_and(|h| ok(&h));
    let origin_ok =
        header_value(req, "Origin").is_none_or(|o| o.strip_prefix("http://").is_some_and(ok));
    host_ok && origin_ok
}

fn handle(mut req: Request, wallet: &Wallet, bound: SocketAddr, opts: Options) {
    if !opts.allow_any_host && !host_allowed(&req, bound) {
        return json_reply(
            req,
            403,
            &json!({"error": {"code": "forbidden", "message": "unexpected Host or Origin"}}),
        );
    }
    let url = req.url().split('?').next().unwrap_or("").to_string();
    if url == "/rpc" {
        if req.method() != &Method::Post {
            return json_reply(
                req,
                405,
                &json!({"error": {"code": "bad_request", "message": "POST only"}}),
            );
        }
        let is_json = header_value(&req, "Content-Type")
            .is_some_and(|c| c.to_ascii_lowercase().starts_with("application/json"));
        if !is_json {
            return json_reply(
                req,
                415,
                &json!({"error": {"code": "bad_request", "message": "content-type must be application/json"}}),
            );
        }
        let mut body = Vec::new();
        let _ = req.as_reader().take(MAX_BODY + 1).read_to_end(&mut body);
        if body.len() as u64 > MAX_BODY {
            return json_reply(
                req,
                413,
                &json!({"error": {"code": "bad_request", "message": "body too large"}}),
            );
        }
        let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
            return json_reply(
                req,
                400,
                &json!({"error": {"code": "bad_request", "message": "invalid JSON"}}),
            );
        };
        let Some(method) = parsed.get("method").and_then(Value::as_str) else {
            return json_reply(
                req,
                400,
                &json!({"error": {"code": "bad_request", "message": "missing `method`"}}),
            );
        };
        return match wallet.call(method, parsed.get("params").unwrap_or(&Value::Null)) {
            Ok(result) => json_reply(req, 200, &json!({"result": result})),
            Err(e) => json_reply(
                req,
                400,
                &json!({"error": {"code": e.code, "message": e.message}}),
            ),
        };
    }
    if req.method() != &Method::Get {
        return json_reply(
            req,
            405,
            &json!({"error": {"code": "bad_request", "message": "method not allowed"}}),
        );
    }
    match ASSETS.iter().find(|(p, _, _)| *p == url) {
        Some((_, ctype, body)) => reply(req, 200, ctype, (*body).to_string(), "no-cache"),
        None => json_reply(
            req,
            404,
            &json!({"error": {"code": "bad_request", "message": "not found"}}),
        ),
    }
}
