#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The explorer against a real network (4 in-process validators): it syncs, **re-verifies**, indexes and serves;
//! and against lying nodes (tampered blocks, different genesis) it rejects and reports.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::{Address, SecretKey, hex};
use brdu_explorer::{Explorer, Shared, serve, spawn_sync, sync_once};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Tx, kind};
use brdu_node::{Config, Node, Timeouts};
use serde_json::Value;

const CHAIN: &str = "brdu-explorer-test";

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

struct Net {
    nodes: Vec<Node>,
    genesis: Genesis,
    issuers: Vec<SecretKey>,
    relayer: SecretKey,
    alice: SecretKey,
    bob: SecretKey,
    dir: std::path::PathBuf,
}

fn wait(what: &str, secs: u64, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn start(tag: &str) -> Net {
    let dir = std::env::temp_dir().join(format!("brdu-exp-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let vals: Vec<SecretKey> = (20..24).map(key).collect();
    let issuers: Vec<SecretKey> = (10..13).map(key).collect();
    let genesis = Genesis::new(
        CHAIN,
        vals.iter().map(SecretKey::public).collect(),
        issuers.iter().map(SecretKey::public).collect(),
        2,
    );
    let mut nodes: Vec<Node> = Vec::new();
    for (i, v) in vals.iter().enumerate() {
        let mut c = Config::new(
            genesis.clone(),
            SecretKey::from_bytes(*v.expose_seed()),
            dir.join(format!("v{i}")),
        );
        c.peers = nodes.iter().map(|n| n.listen_addr().to_string()).collect();
        c.api = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
        c.min_block_interval = Duration::from_millis(60);
        c.timeouts = Timeouts {
            propose: Duration::from_millis(800),
            prevote: Duration::from_millis(300),
            precommit: Duration::from_millis(300),
            delta: Duration::from_millis(100),
        };
        nodes.push(Node::start(c).unwrap());
    }
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    Net {
        nodes,
        genesis,
        issuers,
        relayer: key(9),
        alice: key(1),
        bob: key(2),
        dir,
    }
}

impl Net {
    fn mint(&self, nonce: u64, to: &SecretKey, amount: u64, reference: &str) -> Tx {
        let addr = Address::from_public_key(&to.public());
        let doc = coin::issuance_doc(false, CHAIN, &addr, amount, reference);
        let ap: Vec<Approval> = self
            .issuers
            .iter()
            .take(2)
            .map(|k| Approval::new(&doc, k))
            .collect();
        Tx::new_signed(
            &self.relayer,
            CHAIN,
            kind::MINT,
            nonce,
            0,
            coin::issuance_payload(&addr, amount, reference, &ap),
        )
        .unwrap()
    }

    fn transfer(&self, nonce: u64, amount: u64) -> Tx {
        let payload = coin::transfer_payload(&Address::from_public_key(&self.bob.public()), amount);
        Tx::new_signed(&self.alice, CHAIN, kind::TRANSFER, nonce, 5_000, payload).unwrap()
    }

    fn api(&self) -> String {
        self.nodes[0].api_addr().unwrap().to_string()
    }

    fn stop(self) {
        for n in self.nodes {
            n.stop();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn get(addr: SocketAddr, path: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, head.to_string(), body.to_string())
}

fn json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let (c, _, b) = get(addr, path);
    (c, serde_json::from_str(&b).unwrap_or(Value::Null))
}

fn explorer(net: &Net, node: &str) -> (Shared, Arc<AtomicBool>, SocketAddr) {
    let shared: Shared = Arc::new(RwLock::new(
        Explorer::new(net.genesis.clone(), node).unwrap(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let _ = spawn_sync(shared.clone(), stop.clone());
    let (addr, _) = serve(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        shared.clone(),
        stop.clone(),
    )
    .unwrap();
    (shared, stop, addr)
}

#[test]
fn explorer_syncs_reverifies_indexes_and_serves() {
    let net = start("main");
    net.nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || net.nodes.iter().all(|n| n.height() >= 1));
    for i in 0..3u64 {
        net.nodes[(i % 4) as usize]
            .submit(&net.transfer(i, 1_000_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(120));
    }
    wait("transfers", 30, || {
        net.nodes[0]
            .shared()
            .chain
            .lock()
            .unwrap()
            .state()
            .balance(&Address::from_public_key(&net.bob.public()))
            == 3_000_000
    });
    let (shared, stop, addr) = explorer(&net, &net.api());
    let target = net.nodes[0].height();
    wait("explorer catches up", 20, || {
        shared.read().unwrap().height() >= target
    });

    // overview: everything comes from the verified replica
    let (code, o) = json(addr, "/api/overview");
    assert_eq!(code, 200);
    assert_eq!(o["chain_id"], CHAIN);
    assert_eq!(o["validators"].as_array().unwrap().len(), 4);
    assert_eq!(o["supply"]["supply"], 50_000_000);
    assert_eq!(o["node"]["rejected_blocks"], 0);
    assert_eq!(
        o["state_root"].as_str().unwrap(),
        net.nodes[0].state_root().to_hex()
    );

    // block with certificate and transactions
    let (_, b) = json(addr, "/api/block/1");
    assert_eq!(
        (b["verified"] == true, b["tx_count"].as_u64()),
        (true, Some(1))
    );
    assert!(b["signed_by"].as_array().unwrap().len() >= 3);
    assert_eq!(b["txs"][0]["kind"], 2);
    assert_eq!(b["txs"][0]["kind_name"], "mint");
    assert_eq!(b["txs"][0]["summary"]["reference"], "dep-1");
    let (code, missing) = json(addr, "/api/block/9999");
    assert_eq!(
        (code, missing["error"].as_str()),
        (404, Some("block not found"))
    );

    // transaction by id, and both ends of the transfer in the account index
    let t = net.transfer(0, 1_000_000);
    let (code, tx) = json(addr, &format!("/api/tx/{}", t.id().to_hex()));
    assert_eq!(code, 200, "{tx}");
    assert_eq!(tx["summary"]["amount"], 1_000_000);
    let bob = Address::from_public_key(&net.bob.public()).encode();
    let alice = Address::from_public_key(&net.alice.public()).encode();
    let (_, b_acc) = json(addr, &format!("/api/account/{bob}"));
    assert_eq!(
        (b_acc["balance"].as_u64(), b_acc["tx_count"].as_u64()),
        (Some(3_000_000), Some(3)),
        "the destination also sees the transfer"
    );
    let (_, a_acc) = json(addr, &format!("/api/account/{alice}?limit=2"));
    assert_eq!(
        a_acc["tx_count"].as_u64(),
        Some(4),
        "mint received + 3 sent"
    );
    assert_eq!(a_acc["txs"].as_array().unwrap().len(), 2);
    assert_eq!(json(addr, "/api/account/garbage").0, 400);

    // paginated list from newest to oldest
    let (_, l) = json(addr, "/api/blocks?limit=2");
    assert_eq!(
        l["blocks"][0]["height"].as_u64(),
        Some(shared.read().unwrap().height())
    );
    let (_, older) = json(
        addr,
        &format!("/api/blocks?limit=50&before={}", l["blocks"][1]["height"]),
    );
    assert!(
        older["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["height"].as_u64() < l["blocks"][1]["height"].as_u64())
    );

    // search: height, transaction id, address, block hash; and a typo in the address
    assert_eq!(json(addr, "/api/search?q=1").1["kind"], "block");
    assert_eq!(
        json(addr, &format!("/api/search?q={}", t.id().to_hex())).1["kind"],
        "tx"
    );
    assert_eq!(
        json(addr, &format!("/api/search?q={bob}")).1["kind"],
        "account"
    );
    assert_eq!(
        json(
            addr,
            &format!("/api/search?q={}", b["hash"].as_str().unwrap())
        )
        .1["kind"],
        "block"
    );
    let mut typo = bob.clone();
    typo.replace_range(20..21, if &bob[20..21] == "q" { "p" } else { "q" });
    let (_, r) = json(addr, &format!("/api/search?q={typo}"));
    assert_eq!(r["found"], false);
    assert!(r["hint"].as_str().unwrap().contains("checksum"));
    assert_eq!(json(addr, "/api/search?q=99999").1["found"], false);

    // validators
    let (_, v) = json(addr, "/api/validators");
    assert_eq!(v["epoch"], 0);
    assert_eq!(v["validators"].as_array().unwrap().len(), 4);

    // static page with strict CSP, and read-only
    let (code, head, html) = get(addr, "/");
    assert_eq!(code, 200);
    assert!(
        head.to_ascii_lowercase().contains(
            "content-security-policy: default-src 'none'; script-src 'self'; style-src 'self'"
        ),
        "{head}"
    );
    assert!(
        html.contains("<title>Explorador BRDU</title>")
            && !html.contains("<script>")
            && !html.contains("style="),
        "nothing inline"
    );
    for asset in [
        "/app.css",
        "/app.js",
        "/scramble.js",
        "/format.js",
        "/effects.js",
        "/favicon.svg",
    ] {
        assert_eq!(get(addr, asset).0, 200, "{asset}");
    }
    let mut s = TcpStream::connect(addr).unwrap();
    write!(
        s,
        "POST /api/overview HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 405"), "{out}");
    assert_eq!(get(addr, "/segredo").0, 404);

    stop.store(true, Ordering::SeqCst);
    net.stop();
}

/// A "node" that only returns what it is told: pretends to be a node and serves `status` and (tampered) blocks.
fn fake_node(blocks: Vec<String>) -> (SocketAddr, Arc<AtomicBool>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    l.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    std::thread::spawn(move || {
        while !s2.load(Ordering::SeqCst) {
            let Ok((mut c, _)) = l.accept() else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            c.set_nonblocking(false).unwrap();
            let mut buf = [0u8; 2048];
            let n = c.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = if req.starts_with("GET /status") {
                format!("{{\"height\":{}}}", blocks.len())
            } else if req.starts_with("GET /blocks/raw") {
                format!(
                    "{{\"height\":{},\"blocks\":{}}}",
                    blocks.len(),
                    serde_json::to_string(&blocks).unwrap()
                )
            } else {
                "{\"evidence\":[]}".to_string()
            };
            let _ = write!(
                c,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (addr, stop)
}

#[test]
fn lying_node_is_rejected_and_reported() {
    let net = start("liar");
    net.nodes[0]
        .submit(&net.mint(0, &net.alice, 7_000_000, "dep-1"))
        .unwrap();
    net.nodes[1].submit(&net.transfer(0, 1_000)).ok();
    wait("blocks", 20, || net.nodes.iter().all(|n| n.height() >= 1));
    let honest: Vec<String> = net.nodes[0]
        .shared()
        .chain
        .lock()
        .unwrap()
        .blocks()
        .iter()
        .map(|d| hex::encode(&d.encode().unwrap()))
        .collect();
    assert!(!honest.is_empty());

    // 1) the node delivers block 1 with one byte of the state root changed: the certificate was for the true block
    let mut forged = hex::decode(&honest[0]).unwrap();
    let at = forged.len() / 3;
    forged[at] ^= 1;
    let (addr, stop) = fake_node(vec![hex::encode(&forged)]);
    let shared: Shared = Arc::new(RwLock::new(
        Explorer::new(net.genesis.clone(), &addr.to_string()).unwrap(),
    ));
    assert!(sync_once(&shared).is_err());
    {
        let e = shared.read().unwrap();
        assert_eq!(e.height(), 0, "nothing got in");
        assert_eq!(e.status.rejected_blocks, 1);
        assert!(e.status.last_error.as_deref().unwrap().contains("invalid"));
    }
    stop.store(true, Ordering::SeqCst);

    // 2) the node delivers true blocks, but the explorer was pinned to ANOTHER genesis: it rejects the first
    let (addr, stop) = fake_node(honest.clone());
    let mut other = net.genesis.clone();
    other.chain_id = "other-network".into();
    let shared: Shared = Arc::new(RwLock::new(
        Explorer::new(other, &addr.to_string()).unwrap(),
    ));
    assert!(sync_once(&shared).is_err());
    assert_eq!(shared.read().unwrap().height(), 0);
    stop.store(true, Ordering::SeqCst);

    // 3) the node delivers blocks out of order (skips 1): rejects
    if honest.len() >= 2 {
        let (addr, stop) = fake_node(vec![honest[1].clone()]);
        let shared: Shared = Arc::new(RwLock::new(
            Explorer::new(net.genesis.clone(), &addr.to_string()).unwrap(),
        ));
        assert!(sync_once(&shared).is_err());
        assert_eq!(shared.read().unwrap().height(), 0);
        stop.store(true, Ordering::SeqCst);
    }

    // 4) the honest node: everything gets in
    let (addr, stop) = fake_node(honest.clone());
    let shared: Shared = Arc::new(RwLock::new(
        Explorer::new(net.genesis.clone(), &addr.to_string()).unwrap(),
    ));
    assert_eq!(sync_once(&shared).unwrap(), honest.len());
    stop.store(true, Ordering::SeqCst);
    net.stop();
}

#[test]
fn pure_ui_passes_node_tests() {
    // the environment may lack `node`; without it this test proves nothing and says so
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found: UI tests skipped");
        return;
    }
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/ui/ui.test.mjs");
    let out = Command::new("node")
        .args(["--test", file])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

use std::process::Command;
