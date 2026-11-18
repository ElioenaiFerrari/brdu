#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Cluster of 4 validators **in process**, with real TCP over loopback, Noise, disk and the HTTP API.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::{Address, SecretKey, hex};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Tx, kind};
use brdu_node::{Config, Node, Timeouts};
use serde_json::Value;

const CHAIN: &str = "brdu-cluster";

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

struct Net {
    vals: Vec<SecretKey>,
    issuers: Vec<SecretKey>,
    alice: SecretKey,
    bob: SecretKey,
    relayer: SecretKey,
    dir: PathBuf,
}

impl Net {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("brdu-cluster-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self {
            vals: (20..25).map(key).collect(),
            issuers: (10..13).map(key).collect(),
            alice: key(1),
            bob: key(2),
            relayer: key(9),
            dir,
        }
    }

    fn genesis(&self) -> Genesis {
        Genesis::new(
            CHAIN,
            self.vals.iter().take(4).map(SecretKey::public).collect(),
            self.issuers.iter().map(SecretKey::public).collect(),
            2,
        )
    }

    fn config(&self, i: usize, peers: Vec<String>, api: bool) -> Config {
        let mut c = Config::new(
            self.genesis(),
            SecretKey::from_bytes(*self.vals[i].expose_seed()),
            self.dir.join(format!("v{i}")),
        );
        c.peers = peers;
        c.timeouts = Timeouts {
            propose: Duration::from_millis(800),
            prevote: Duration::from_millis(300),
            precommit: Duration::from_millis(300),
            delta: Duration::from_millis(100),
        };
        c.min_block_interval = Duration::from_millis(60);
        if api {
            c.api = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
        }
        c
    }

    /// Starts `n` nodes: node j dials the earlier ones, and the mesh becomes complete.
    fn start(&self, n: usize, api: bool) -> Vec<Node> {
        let mut nodes: Vec<Node> = Vec::new();
        for i in 0..n {
            let peers = nodes.iter().map(|x| x.listen_addr().to_string()).collect();
            nodes.push(Node::start(self.config(i, peers, api)).unwrap());
        }
        nodes
    }

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

    fn transfer(&self, nonce: u64, from: &SecretKey, to: &SecretKey, amount: u64) -> Tx {
        let payload = coin::transfer_payload(&Address::from_public_key(&to.public()), amount);
        Tx::new_signed(from, CHAIN, kind::TRANSFER, nonce, 5_000, payload).unwrap()
    }
}

fn wait(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

fn all_at(nodes: &[Node], h: u64) -> bool {
    nodes.iter().all(|n| n.height() >= h)
}

fn same_state(nodes: &[Node]) -> bool {
    let h = nodes[0].height();
    nodes.iter().all(|n| {
        n.height() == h && n.state_root() == nodes[0].state_root() && n.tip() == nodes[0].tip()
    })
}

fn http(addr: SocketAddr, method: &str, path: &str, body: &str) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let code = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = out.split("\r\n\r\n").nth(1).unwrap_or("");
    (code, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn balance(n: &Node, who: &SecretKey) -> u64 {
    n.shared()
        .chain
        .lock()
        .unwrap()
        .state()
        .balance(&Address::from_public_key(&who.public()))
}

#[test]
fn four_validators_decide_blocks_and_reach_the_same_state() {
    let net = Net::new("basic");
    let nodes = net.start(4, false);
    wait("full mesh", 20, || {
        nodes.iter().all(|n| n.peer_count() == 3)
    });

    nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("block 1 on all nodes", 20, || all_at(&nodes, 1));
    assert_eq!(balance(&nodes[2], &net.alice), 50_000_000);

    // transfers sent to different nodes: gossip carries all of them to the current proposer
    for i in 0..6u64 {
        nodes[(i % 4) as usize]
            .submit(&net.transfer(i, &net.alice, &net.bob, 1_000_000))
            .unwrap();
    }
    wait("bob received 6 million on all nodes", 30, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 6_000_000)
    });
    wait("same state", 10, || same_state(&nodes));
    assert!(nodes.iter().all(|n| n.evidence_count() == 0));
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn the_same_transaction_sent_to_two_nodes_is_included_once() {
    let net = Net::new("dedupe");
    let nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    let t = net.mint(0, &net.alice, 7_000_000, "dep-1");
    let id1 = nodes[0].submit(&t).unwrap();
    let id2 = nodes[1].submit(&t);
    assert!(id2.is_ok() || id2.is_err()); // it may have already arrived by gossip
    wait("block", 20, || all_at(&nodes, 1));
    wait("same state", 10, || same_state(&nodes));
    assert_eq!(
        balance(&nodes[3], &net.alice),
        7_000_000,
        "minted only once"
    );
    // resubmitting after inclusion: stale nonce, rejected
    assert!(nodes[2].submit(&t).is_err());
    assert_eq!(id1, t.id());
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn a_validator_going_down_does_not_stop_the_network_and_on_return_catches_up_from_disk_and_sync() {
    let net = Net::new("restart");
    let mut nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || all_at(&nodes, 1));

    // take node 3 down
    let down = nodes.pop().unwrap();
    down.stop();
    for i in 0..5u64 {
        nodes[(i % 3) as usize]
            .submit(&net.transfer(i, &net.alice, &net.bob, 1_000_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }
    wait("3 of 4 keep producing", 40, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 5_000_000)
    });
    let target = nodes[0].height();

    // comes back with the SAME data dir: reads the log, then syncs what it missed
    let peers: Vec<String> = nodes.iter().map(|x| x.listen_addr().to_string()).collect();
    let back = Node::start(net.config(3, peers, false)).unwrap();
    assert!(
        back.height() >= 1,
        "recovered from disk before talking to anyone"
    );
    nodes.push(back);
    wait("the returned node catches up", 40, || {
        nodes[3].height() >= target
    });
    wait("same state", 20, || same_state(&nodes));
    assert_eq!(balance(&nodes[3], &net.bob), 5_000_000);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn the_whole_network_restarts_and_continues_from_the_same_point() {
    let net = Net::new("allrestart");
    let nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[1]
        .submit(&net.mint(0, &net.alice, 9_000_000, "dep-1"))
        .unwrap();
    wait("block", 20, || all_at(&nodes, 1));
    wait("same state", 10, || same_state(&nodes));
    let (h, root) = (nodes[0].height(), nodes[0].state_root());
    for n in nodes {
        n.stop();
    }
    let again = net.start(4, false);
    assert!(
        again
            .iter()
            .all(|n| n.height() == h && n.state_root() == root),
        "the disk rebuilds the same state"
    );
    wait("mesh", 20, || again.iter().all(|n| n.peer_count() == 3));
    again[2]
        .submit(&net.transfer(0, &net.alice, &net.bob, 1_000_000))
        .unwrap();
    wait("continues", 30, || {
        again.iter().all(|n| balance(n, &net.bob) == 1_000_000)
    });
    for n in again {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn http_api_status_account_transaction_submission_and_blocks() {
    let net = Net::new("api");
    let nodes = net.start(4, true);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    let api = nodes[0].api_addr().unwrap();

    let (code, st) = http(api, "GET", "/status", "");
    assert_eq!(code, 200);
    assert_eq!(st["chain_id"], CHAIN);
    assert_eq!(st["height"], 0);

    let t = net.mint(0, &net.alice, 3_000_000, "dep-1");
    let (code, r) = http(api, "POST", "/tx", &hex::encode(&t.encode().unwrap()));
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["id"], t.id().to_hex());
    // garbage and a tampered signature
    assert_eq!(http(api, "POST", "/tx", "zz").0, 400);
    let mut bad = net.mint(1, &net.alice, 1, "x");
    bad.signature.0[0] ^= 1;
    assert_eq!(
        http(api, "POST", "/tx", &hex::encode(&bad.encode().unwrap())).0,
        400
    );

    wait("block", 20, || all_at(&nodes, 1));
    let alice = Address::from_public_key(&net.alice.public()).encode();
    let (_, acc) = http(api, "GET", &format!("/account/{alice}"), "");
    assert_eq!(acc["balance"], 3_000_000);
    let (_, sup) = http(api, "GET", "/supply", "");
    assert_eq!(
        (sup["minted"].as_u64(), sup["supply"].as_u64()),
        (Some(3_000_000), Some(3_000_000))
    );
    let (_, b) = http(api, "GET", "/block/1", "");
    assert_eq!(b["tx_count"], 1);
    assert!(
        b["signed_by"].as_array().unwrap().len() >= 3,
        "certificate with more than 2/3"
    );
    let (_, found) = http(api, "GET", &format!("/tx/{}", t.id().to_hex()), "");
    assert_eq!(found["height"], 1);
    assert_eq!(http(api, "GET", "/block/99", "").0, 404);
    assert_eq!(http(api, "GET", "/account/garbage", "").0, 400);
    assert_eq!(http(api, "GET", "/nothing", "").0, 404);
    assert_eq!(http(api, "DELETE", "/tx", "").0, 405);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn current_proposer_down_the_round_changes_and_the_network_decides() {
    let net = Net::new("proposer-down");
    let mut nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || all_at(&nodes, 1));
    // the proposer of block 2 (round 0) is validators[2 % 4] = node 2: take it down
    let down = nodes.remove(2);
    down.stop();
    nodes[0]
        .submit(&net.transfer(0, &net.alice, &net.bob, 1_000_000))
        .unwrap();
    wait("the network decides anyway", 40, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 1_000_000)
    });
    wait("same state", 10, || same_state(&nodes));
    let blocks = nodes[0].shared().chain.lock().unwrap().blocks().to_vec();
    assert!(
        blocks.iter().any(|d| d.cert.round >= 1),
        "some block needed another round"
    );
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn burst_of_three_hundred_transactions_from_many_accounts_is_included_without_loss_or_duplication()
{
    let net = Net::new("burst");
    let nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    // 20 funded accounts, each sending 15 transfers in sequence to different nodes
    let senders: Vec<SecretKey> = (100..120).map(key).collect();
    for (i, s) in senders.iter().enumerate() {
        nodes[i % 4]
            .submit(&net.mint(i as u64, s, 100_000_000, &format!("dep-{i}")))
            .unwrap();
    }
    wait("balances", 40, || {
        nodes
            .iter()
            .all(|n| balance(n, &senders[19]) == 100_000_000)
    });
    let mut sent = 0u64;
    for round in 0..15u64 {
        for (i, s) in senders.iter().enumerate() {
            nodes[(i + round as usize) % 4]
                .submit(&net.transfer(round, s, &net.bob, 1_000))
                .unwrap();
            sent += 1;
        }
    }
    assert_eq!(sent, 300);
    wait("bob received everything on all nodes", 60, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 300_000)
    });
    wait("same state", 20, || same_state(&nodes));
    let pool: usize = nodes
        .iter()
        .map(|n| n.shared().info.lock().unwrap().pool_len)
        .sum();
    assert_eq!(pool, 0, "nothing was left pending");
    // gossip does not echo: each node received each transaction only a few times
    for n in &nodes {
        let dup = n
            .shared()
            .stats
            .txs_duplicate
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(dup <= 2 * 320 * 3, "too many duplicates: {dup}");
    }
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

// ---------------------------------------------------------------- validator rotation

fn change_tx(
    net: &Net,
    node: &Node,
    action: brdu_ledger::gov::Action,
    who: &SecretKey,
    lead: u64,
    approvers: usize,
) -> Tx {
    let (epoch, height, nonce) = {
        let c = node.shared().chain.lock().unwrap();
        (
            c.epoch(),
            c.height(),
            c.state()
                .nonce(&Address::from_public_key(&net.relayer.public())),
        )
    };
    let effective = height + lead;
    let doc = brdu_ledger::gov::change_doc(CHAIN, action, &who.public(), effective, epoch);
    let ap: Vec<Approval> = net
        .vals
        .iter()
        .take(approvers)
        .map(|k| Approval::new(&doc, k))
        .collect();
    Tx::new_signed(
        &net.relayer,
        CHAIN,
        kind::VALIDATOR_CHANGE,
        nonce,
        0,
        brdu_ledger::gov::change_payload(action, &who.public(), effective, &ap),
    )
    .unwrap()
}

fn epoch(n: &Node) -> u64 {
    n.shared().chain.lock().unwrap().epoch()
}

fn signed_by_anyone_new(n: &Node, who: &SecretKey, after_height: u64) -> bool {
    let me = who.public();
    n.shared()
        .chain
        .lock()
        .unwrap()
        .blocks()
        .iter()
        .filter(|d| d.block.height > after_height)
        .any(|d| d.cert.signers().contains(&me))
}

#[test]
fn fifth_node_joins_by_rotation_syncs_as_observer_and_then_votes() {
    let net = Net::new("rotate-in");
    let mut nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    // the 5th node starts with key v5, which is NOT in the genesis: it is refused until the change is included
    let peers: Vec<String> = nodes.iter().map(|x| x.listen_addr().to_string()).collect();
    let fifth = Node::start(net.config(4, peers, false)).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        fifth.peer_count(),
        0,
        "not a validator yet and has no pending change"
    );

    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Add,
        &net.vals[4],
        6,
        3,
    );
    nodes[1].submit(&tx).unwrap();
    wait("pending change on all nodes", 30, || {
        nodes
            .iter()
            .all(|n| n.shared().chain.lock().unwrap().pending_change().is_some())
    });
    // with the change pending the 5th node is admitted and syncs as an observer
    wait("the 5th node connects and catches up", 30, || {
        fifth.peer_count() >= 3 && fifth.height() >= nodes[0].height()
    });
    // generate blocks until the epoch turns
    let mut n = 0u64;
    while epoch(&nodes[0]) == 0 {
        nodes[(n % 4) as usize]
            .submit(&net.transfer(n, &net.alice, &net.bob, 1_000))
            .unwrap();
        n += 1;
        std::thread::sleep(Duration::from_millis(200));
        assert!(n < 80, "the epoch did not turn");
    }
    nodes.push(fifth);
    wait("all 5 at the same height", 30, || same_state(&nodes));
    assert_eq!(
        nodes[0].shared().chain.lock().unwrap().validators().len(),
        5
    );
    let from = nodes[0].height();
    for i in 0..12u64 {
        nodes[(i % 5) as usize]
            .submit(&net.transfer(n + i, &net.alice, &net.bob, 1_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }
    wait("more blocks", 40, || nodes[0].height() >= from + 4);
    wait("the 5th node signed some block", 40, || {
        signed_by_anyone_new(&nodes[0], &net.vals[4], from)
    });
    wait("same state", 20, || same_state(&nodes));
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn removed_validator_becomes_observer_and_the_network_continues_without_it() {
    let net = Net::new("rotate-out");
    let nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Remove,
        &net.vals[3],
        5,
        3,
    );
    nodes[0].submit(&tx).unwrap();
    let mut n = 0u64;
    while epoch(&nodes[0]) == 0 {
        nodes[(n % 3) as usize]
            .submit(&net.transfer(n, &net.alice, &net.bob, 1_000))
            .unwrap();
        n += 1;
        std::thread::sleep(Duration::from_millis(200));
        assert!(n < 80, "the epoch did not turn");
    }
    wait(
        "the remaining 3 disconnect from the removed one",
        20,
        || nodes[..3].iter().all(|x| x.peer_count() == 2),
    );
    let from = nodes[0].height();
    for i in 0..8u64 {
        nodes[(i % 3) as usize]
            .submit(&net.transfer(n + i, &net.alice, &net.bob, 1_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }
    wait("the 3-node network continues", 40, || {
        nodes[..3].iter().all(|x| x.height() >= from + 3)
    });
    assert!(
        !signed_by_anyone_new(&nodes[0], &net.vals[3], from),
        "the removed node no longer signs"
    );
    // the removed node can no longer join: it stays without peers
    wait("the removed node became isolated", 20, || {
        nodes[3].peer_count() == 0
    });
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn change_without_more_than_2_3_never_becomes_pending() {
    let net = Net::new("rotate-weak");
    let nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Remove,
        &net.vals[3],
        5,
        2,
    ); // 2 of 4
    nodes[0].submit(&tx).unwrap();
    for i in 0..4u64 {
        nodes[0]
            .submit(&net.transfer(i, &net.alice, &net.bob, 1_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(250));
    }
    wait("blocks keep coming", 30, || all_at(&nodes, 3));
    assert!(
        nodes
            .iter()
            .all(|n| n.shared().chain.lock().unwrap().pending_change().is_none())
    );
    assert!(nodes.iter().all(|n| epoch(n) == 0));
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn restart_with_a_pending_change_rebuilds_the_same_pending_set_and_takes_effect_later() {
    let net = Net::new("rotate-restart");
    let nodes = net.start(4, false);
    wait("mesh", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("block 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Add,
        &net.vals[4],
        30,
        3,
    );
    nodes[0].submit(&tx).unwrap();
    wait("pending", 30, || {
        nodes
            .iter()
            .all(|n| n.shared().chain.lock().unwrap().pending_change().is_some())
    });
    wait("same state", 10, || same_state(&nodes));
    let root = nodes[0].state_root();
    for n in nodes {
        n.stop();
    }
    let again = net.start(4, false);
    assert!(
        again.iter().all(|n| n.state_root() == root),
        "the disk rebuilds the same state"
    );
    assert!(again.iter().all(|n| {
        let c = n.shared().chain.lock().unwrap();
        c.pending_change().is_some_and(|p| p.effective_height == 31) && c.epoch() == 0
    }));
    for n in again {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}
