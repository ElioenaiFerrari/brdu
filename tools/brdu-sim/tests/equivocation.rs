#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! 4 validadores em processo; o 4º é parado; o simulador equivoca com a chave dele; os 3 restantes expõem a evidência.

use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::SecretKey;
use brdu_node::{Config, Node, Timeouts};

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

#[test]
fn equivocacao_de_um_validador_e_detectada_pelos_tres_honestos() {
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
        assert!(Instant::now() < end, "malha");
        std::thread::sleep(Duration::from_millis(50));
    }
    // o validador 4 sai do ar: sua chave fica livre para o simulador
    nodes.pop().unwrap().stop();
    let targets: Vec<String> = nodes.iter().map(|n| n.listen_addr().to_string()).collect();
    let delivered = brdu_sim::equivocate(&genesis, &vals[3], &targets, 1).unwrap();
    assert_eq!(delivered, 3);
    let end = Instant::now() + Duration::from_secs(20);
    while nodes.iter().any(|n| n.evidence_count() == 0) {
        assert!(
            Instant::now() < end,
            "evidência em {:?}",
            nodes.iter().map(Node::evidence_count).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // a evidência nomeia o equivocador e os dois blocos diferentes
    let ev = nodes[0].shared().info.lock().unwrap().evidence.clone();
    let (a, b) = &ev[0];
    assert_eq!(a.voter, vals[3].public());
    assert_ne!(a.block_hash, b.block_hash);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
