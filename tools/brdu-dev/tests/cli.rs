#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The CLI against real nodes: `brdu-dev init` generates the network, 4 nodes start in-process with the generated keys and the
//! commands (wallet, fund, send, reserve, burn, supply, blocks…) run against the APIs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::{SecretKey, hex};
use brdu_node::{Config, Node, Timeouts};

fn dev(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_brdu-dev"))
        .args(args)
        .args(["--dir", &dir.to_string_lossy()])
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let (good, text) = dev(dir, args);
    assert!(good, "`{}` failed: {text}", args.join(" "));
    text
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out: {what}");
}

fn start_nodes(dir: &Path, base: u16) -> Vec<Node> {
    let g = std::fs::read_to_string(dir.join("genesis.hex")).unwrap();
    let genesis = Genesis::decode(&hex::decode(g.trim()).unwrap()).unwrap();
    let mut nodes: Vec<Node> = Vec::new();
    for i in 1..=4u16 {
        let k = std::fs::read_to_string(dir.join(format!("keys/v{i}.key"))).unwrap();
        let key = SecretKey::from_bytes(hex::decode_array::<32>(k.trim()).unwrap());
        let mut c = Config::new(genesis.clone(), key, dir.join(format!("data/v{i}")));
        c.listen = format!("127.0.0.1:{}", base + i).parse().unwrap();
        c.api = Some(format!("127.0.0.1:{}", base + 100 + i).parse().unwrap());
        c.peers = nodes.iter().map(|n| n.listen_addr().to_string()).collect();
        c.min_block_interval = Duration::from_millis(60);
        c.timeouts = Timeouts {
            propose: Duration::from_millis(800),
            prevote: Duration::from_millis(300),
            precommit: Duration::from_millis(300),
            delta: Duration::from_millis(100),
        };
        nodes.push(Node::start(c).unwrap());
    }
    wait("mesh", || nodes.iter().all(|n| n.peer_count() == 3));
    nodes
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("brdu-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn the_cli_runs_the_full_cycle_against_real_nodes() {
    let dir = tmp("cycle");
    let base = 17_100;
    let text = ok(&dir, &["init", "--port-base", &base.to_string()]);
    assert!(text.contains("4 validators"), "{text}");
    // init refuses to recreate over an existing network
    assert!(!dev(&dir, &["init"]).0);
    let nodes = start_nodes(&dir, base);

    assert!(ok(&dir, &["wallet", "new", "alice"]).contains("brdu1"));
    ok(&dir, &["wallet", "new", "bob"]);
    assert!(ok(&dir, &["wallet", "list"]).contains("alice"));
    assert!(
        !dev(&dir, &["wallet", "new", "alice"]).0,
        "does not overwrite the existing phrase"
    );

    ok(&dir, &["fund", "alice", "1000"]);
    wait("alice balance", || {
        ok(&dir, &["balance", "alice"]).contains("1.000,000000 BRDU")
    });
    ok(
        &dir,
        &[
            "send", "--from", "alice", "--to", "bob", "--amount", "12,5", "--count", "3",
        ],
    );
    wait("bob received", || {
        ok(&dir, &["balance", "bob"]).contains("37,500000 BRDU")
    });

    ok(&dir, &["reserve", "--amount", "1000"]);
    wait("attestation", || {
        ok(&dir, &["supply"]).contains("attestation #1")
    });
    ok(
        &dir,
        &[
            "burn",
            "--from",
            "alice",
            "--amount",
            "10",
            "--ref",
            "withdrawal-1",
        ],
    );
    wait("burn", || {
        ok(&dir, &["supply"]).contains("burned   10,000000 BRDU")
    });
    let supply = ok(&dir, &["supply"]);
    assert!(
        supply.contains("coverage: covered, surplus 10,000000 BRDU"),
        "{supply}"
    );

    // clear errors: invalid amount, missing wallet, mint with a repeated reference
    assert!(
        dev(
            &dir,
            &["send", "--from", "alice", "--to", "bob", "--amount", "abc"]
        )
        .1
        .contains("amount")
    );
    assert!(
        dev(&dir, &["balance", "ghost"])
            .1
            .contains("does not exist")
    );
    ok(
        &dir,
        &["mint", "--to", "bob", "--amount", "1", "--ref", "dep-x"],
    );
    wait("bob mint", || {
        ok(&dir, &["balance", "bob"]).contains("38,500000 BRDU")
    });
    let again = dev(
        &dir,
        &["mint", "--to", "bob", "--amount", "1", "--ref", "dep-x"],
    );
    assert!(
        !again.0 || ok(&dir, &["balance", "bob"]).contains("38,500000 BRDU"),
        "a repeated reference must not mint twice"
    );

    let blocks = ok(&dir, &["blocks", "--limit", "3"]);
    assert!(
        blocks.contains("signed by 3/4") || blocks.contains("signed by 4/4"),
        "{blocks}"
    );
    let status = ok(&dir, &["status"]);
    assert_eq!(status.matches("peers 3").count(), 4, "{status}");
    ok(&dir, &["blocks-sim", "--count", "2"]);

    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn commands_without_a_network_ask_for_init_and_reset_only_deletes_what_it_created() {
    let dir = tmp("noinit");
    let (good, text) = dev(&dir, &["status"]);
    assert!(!good && text.contains("cargo dev init"), "{text}");
    ok(&dir, &["init", "--nodes", "3", "--port-base", "17300"]);
    std::fs::write(dir.join("my-file.txt"), "stays").unwrap();
    ok(&dir, &["reset"]);
    assert!(
        dir.join("my-file.txt").exists(),
        "reset does not delete what it did not create"
    );
    assert!(!dir.join("devnet.json").exists() && !dir.join("keys").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
