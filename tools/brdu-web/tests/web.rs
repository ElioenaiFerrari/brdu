#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The wallet web daemon: static UI, the /rpc bridge, and the protections against hostile pages.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use brdu_vault::KdfParams;
use brdu_walletd::{Config, Wallet};
use brdu_web::{Options, serve};
use serde_json::{Value, json};

struct Daemon {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    dir: PathBuf,
}

fn start(tag: &str, opts: Options) -> Daemon {
    let dir = std::env::temp_dir().join(format!("brdu-web-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cfg = Config::new(dir.join("vault.json"), "127.0.0.1:1", None);
    cfg.kdf = KdfParams {
        m_kib: 8,
        t: 1,
        p: 1,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (addr, _) = serve(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Arc::new(Wallet::new(cfg)),
        opts,
        stop.clone(),
    )
    .unwrap();
    Daemon { addr, stop, dir }
}

impl Daemon {
    fn done(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Raw request with full control of the headers (a browser would not let a page forge `Host`).
fn raw(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &str,
) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        head.to_string(),
        body.to_string(),
    )
}

fn host(d: &Daemon) -> (&'static str, String) {
    ("Host", format!("127.0.0.1:{}", d.addr.port()))
}

fn rpc(d: &Daemon, method: &str, params: &Value) -> (u16, Value) {
    let body = json!({"method": method, "params": params}).to_string();
    let (code, _, b) = raw(
        d.addr,
        "POST",
        "/rpc",
        &[host(d), ("Content-Type", "application/json".into())],
        &body,
    );
    (code, serde_json::from_str(&b).unwrap_or(Value::Null))
}

#[test]
fn it_serves_the_ui_with_a_strict_csp_and_every_imported_module() {
    let d = start(
        "ui",
        Options {
            allow_any_host: false,
        },
    );
    let (code, head, html) = raw(d.addr, "GET", "/", &[host(&d)], "");
    assert_eq!(code, 200);
    let low = head.to_ascii_lowercase();
    assert!(
        low.contains(
            "content-security-policy: default-src 'none'; script-src 'self'; style-src 'self'"
        ),
        "{head}"
    );
    assert!(
        low.contains("x-content-type-options: nosniff")
            && low.contains("referrer-policy: no-referrer")
    );
    assert!(
        !low.contains("access-control-allow-origin"),
        "no CORS: only same-origin pages may use it"
    );
    assert!(
        html.contains("<title>Carteira BRDU</title>")
            && !html.contains("<script>")
            && !html.contains("style="),
        "nothing inline"
    );

    // every module the UI imports must be served (a missing one leaves a blank page)
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/wallet/ui");
    let mut checked = 0;
    for entry in std::fs::read_dir(&ui).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".js") || name.contains(".test.") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        assert_eq!(
            raw(d.addr, "GET", &format!("/{name}"), &[host(&d)], "").0,
            200,
            "/{name}"
        );
        for imported in text
            .lines()
            .filter_map(|l| l.split("from \"./").nth(1))
            .filter_map(|r| r.split('"').next())
        {
            assert_eq!(
                raw(d.addr, "GET", &format!("/{imported}"), &[host(&d)], "").0,
                200,
                "{name} imports ./{imported}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 6, "the import scan found only {checked} imports");
    for asset in ["/wallet.css", "/favicon.svg"] {
        assert_eq!(raw(d.addr, "GET", asset, &[host(&d)], "").0, 200, "{asset}");
    }
    // only the embedded files exist: no path traversal, no directory listing
    for bad in [
        "/../../Cargo.toml",
        "/..%2f..%2fCargo.toml",
        "/secret",
        "/vault.json",
        "//etc/passwd",
        "/app.js/../../../Cargo.toml",
    ] {
        assert_eq!(raw(d.addr, "GET", bad, &[host(&d)], "").0, 404, "{bad}");
    }
    d.done();
}

#[test]
fn the_rpc_bridge_runs_the_wallet_service_end_to_end() {
    let d = start(
        "rpc",
        Options {
            allow_any_host: false,
        },
    );
    assert_eq!(
        rpc(&d, "status", &Value::Null).1["result"]["has_vault"],
        false
    );
    let (code, created) = rpc(&d, "create", &json!({"password": "correct horse battery"}));
    assert_eq!(code, 200);
    assert_eq!(created["result"]["words"].as_array().unwrap().len(), 24);
    assert!(
        !created.to_string().contains("correct horse battery"),
        "the password is never echoed"
    );
    assert_eq!(rpc(&d, "lock", &json!({})).1["result"]["locked"], true);
    let (code, err) = rpc(&d, "unlock", &json!({"password": "wrong password!"}));
    assert_eq!(
        (code, err["error"]["code"].as_str()),
        (400, Some("wrong_password"))
    );
    assert!(!err.to_string().contains("wrong password!"));
    assert_eq!(
        rpc(&d, "unlock", &json!({"password": "correct horse battery"})).0,
        200
    );
    // the node is unreachable in this test: the error is clear, not a crash
    assert_eq!(
        rpc(&d, "overview", &json!({})).1["error"]["code"],
        "node_unreachable"
    );
    assert_eq!(
        rpc(&d, "nonsense", &json!({})).1["error"]["code"],
        "unknown_method"
    );
    d.done();
}

#[test]
fn hostile_requests_are_refused_before_reaching_the_wallet() {
    let d = start(
        "sec",
        Options {
            allow_any_host: false,
        },
    );
    let json_h = ("Content-Type", "application/json".to_string());
    let body = json!({"method": "status", "params": {}}).to_string();

    // DNS rebinding: a page served from evil.example (resolving to 127.0.0.1) sends its own Host
    let (code, _, b) = raw(
        d.addr,
        "POST",
        "/rpc",
        &[("Host", "evil.example".into()), json_h.clone()],
        &body,
    );
    assert_eq!(code, 403, "{b}");
    assert_eq!(
        raw(
            d.addr,
            "GET",
            "/",
            &[("Host", format!("evil.example:{}", d.addr.port()))],
            ""
        )
        .0,
        403
    );
    // cross-site request: the browser adds the attacker's Origin
    let (code, _, _) = raw(
        d.addr,
        "POST",
        "/rpc",
        &[
            host(&d),
            ("Origin", "https://evil.example".into()),
            json_h.clone(),
        ],
        &body,
    );
    assert_eq!(code, 403);
    // same-origin (what the real UI sends) is fine
    let own = format!("http://127.0.0.1:{}", d.addr.port());
    assert_eq!(
        raw(
            d.addr,
            "POST",
            "/rpc",
            &[host(&d), ("Origin", own), json_h.clone()],
            &body
        )
        .0,
        200
    );
    // a form post cannot use text/plain or urlencoded: only application/json
    for ct in [
        "text/plain",
        "application/x-www-form-urlencoded",
        "multipart/form-data; boundary=x",
    ] {
        assert_eq!(
            raw(
                d.addr,
                "POST",
                "/rpc",
                &[host(&d), ("Content-Type", ct.into())],
                &body
            )
            .0,
            415,
            "{ct}"
        );
    }
    // method, JSON and size checks
    assert_eq!(raw(d.addr, "GET", "/rpc", &[host(&d)], "").0, 405);
    assert_eq!(
        raw(d.addr, "PUT", "/rpc", &[host(&d), json_h.clone()], &body).0,
        405
    );
    assert_eq!(raw(d.addr, "DELETE", "/app.js", &[host(&d)], "").0, 405);
    assert_eq!(
        raw(
            d.addr,
            "POST",
            "/rpc",
            &[host(&d), json_h.clone()],
            "not json"
        )
        .0,
        400
    );
    assert_eq!(
        raw(
            d.addr,
            "POST",
            "/rpc",
            &[host(&d), json_h.clone()],
            "{\"params\": {}}"
        )
        .0,
        400
    );
    let big = format!(
        "{{\"method\":\"status\",\"params\":{{\"x\":\"{}\"}}}}",
        "a".repeat(70_000)
    );
    assert_eq!(
        raw(d.addr, "POST", "/rpc", &[host(&d), json_h.clone()], &big).0,
        413
    );
    // RPC answers are never cached
    let (_, head, _) = raw(d.addr, "POST", "/rpc", &[host(&d), json_h], &body);
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store"),
        "{head}"
    );
    d.done();
}

#[test]
fn the_host_allow_list_can_be_lifted_only_on_purpose() {
    let d = start(
        "remote",
        Options {
            allow_any_host: true,
        },
    );
    let body = json!({"method": "status", "params": {}}).to_string();
    assert_eq!(
        raw(
            d.addr,
            "POST",
            "/rpc",
            &[
                ("Host", "wallet.lab:8546".into()),
                ("Content-Type", "application/json".into())
            ],
            &body
        )
        .0,
        200
    );
    d.done();
}

#[test]
fn the_wallet_ui_passes_its_node_tests() {
    // without `node` this proves nothing and says so; with it, the pure-logic and the full-flow smoke tests must pass
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("node not found: wallet UI tests skipped");
        return;
    }
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/wallet/ui");
    // the smoke test runs the real app.js, which polls forever: --test-force-exit
    let out = std::process::Command::new("node")
        .args(["--test", "--test-force-exit"])
        .arg(ui.join("ui.test.mjs"))
        .arg(ui.join("smoke.test.mjs"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
