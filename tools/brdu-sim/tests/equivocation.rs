#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! 4 validators in process; the 4th is stopped; the simulator equivocates with its key; the remaining 3 expose the evidence.

use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::SecretKey;
use brdu_node::{Config, Node, Timeouts};

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

#[test]
fn a_validators_equivocation_is_detected_by_the_three_honest_ones() {
    let dir = std::env::temp_dir().join(format!("brdu-sim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let vals: Vec<SecretKey> = (20..24).map(key).collect();
    let genesis = Genesis::new(
        "brdu-sim",
        vals.iter().map(SecretKey::public).collect(),
        vec![key(10).public()],
        1,
    );
    let mut nodes: Vec<Node> = Vec::new();
    for (i, v) in vals.iter().enumerate() {
        let mut c = Config::new(
            genesis.clone(),
            SecretKey::from_bytes(*v.expose_seed()),
            dir.join(format!("v{i}")),
        );
        c.peers = nodes.iter().map(|n| n.listen_addr().to_string()).collect();
        c.timeouts = Timeouts {
            propose: Duration::from_millis(800),
            prevote: Duration::from_millis(300),
            precommit: Duration::from_millis(300),
            delta: Duration::from_millis(100),
        };
        nodes.push(Node::start(c).unwrap());
    }
    let end = Instant::now() + Duration::from_secs(20);
    while nodes.iter().any(|n| n.peer_count() < 3) {
        assert!(Instant::now() < end, "mesh");
        std::thread::sleep(Duration::from_millis(50));
    }
    // validator 4 goes down: its key is free for the simulator
    nodes.pop().unwrap().stop();
    let targets: Vec<String> = nodes.iter().map(|n| n.listen_addr().to_string()).collect();
    let delivered = brdu_sim::equivocate(&genesis, &vals[3], &targets, 1).unwrap();
    assert_eq!(delivered, 3);
    let end = Instant::now() + Duration::from_secs(20);
    while nodes.iter().any(|n| n.evidence_count() == 0) {
        assert!(
            Instant::now() < end,
            "evidence at {:?}",
            nodes.iter().map(Node::evidence_count).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // the evidence names the equivocator and the two different blocks
    let ev = nodes[0].shared().info.lock().unwrap().evidence.clone();
    let (a, b) = &ev[0];
    assert_eq!(a.voter, vals[3].public());
    assert_ne!(a.block_hash, b.block_hash);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
