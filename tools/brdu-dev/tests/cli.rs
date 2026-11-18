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
    // the repeated mint above is invalid: wait until every pool dropped it before the relayer sends again
    wait("pools empty", || {
        nodes
            .iter()
            .all(|n| n.shared().info.lock().unwrap().pool_len == 0)
    });
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

#[test]
fn the_cred_commands_run_a_credential_through_its_whole_life() {
    let dir = tmp("cred");
    let base = 17_500;
    ok(
        &dir,
        &[
            "init",
            "--port-base",
            &base.to_string(),
            "--min-window",
            "1",
        ],
    );
    let nodes = start_nodes(&dir, base);
    ok(&dir, &["wallet", "new", "alice"]);

    // only the root authorities can create a type; the CLI signs with the test authorities
    ok(&dir, &["cred", "type", "--id", "cnh", "--contest", "1"]);
    wait("type on the chain", || {
        dev(&dir, &["cred", "types"]).1.contains("cnh")
    });
    let types = ok(&dir, &["cred", "types"]);
    assert!(
        types.contains("organ 2 of 3") && types.contains("public"),
        "{types}"
    );

    ok(
        &dir,
        &[
            "cred",
            "issue",
            "--type",
            "cnh",
            "--id",
            "cnh-1",
            "--to",
            "alice",
            "--data",
            "category=B",
        ],
    );
    wait("pending", || {
        dev(&dir, &["cred", "show", "cnh-1"])
            .1
            .contains("[pending]")
    });
    // a type that does not exist, and a reused id, are refused by the network (the CLI only reports the send)
    assert!(
        dev(
            &dir,
            &[
                "cred", "issue", "--type", "ghost", "--id", "x", "--to", "alice"
            ]
        )
        .0,
        "the node admits it; the block builder drops it"
    );

    std::thread::sleep(Duration::from_millis(2_200));
    ok(&dir, &["cred", "activate", "--id", "cnh-1"]);
    wait("active", || {
        dev(&dir, &["cred", "show", "cnh-1"])
            .1
            .contains("valid: true")
    });
    let listed = ok(&dir, &["cred", "list", "alice"]);
    assert!(
        listed.contains("cnh-1") && listed.contains("[active]"),
        "{listed}"
    );

    ok(
        &dir,
        &["cred", "revoke", "--id", "cnh-1", "--reason", "fraud"],
    );
    wait("revoked", || {
        dev(&dir, &["cred", "show", "cnh-1"])
            .1
            .contains("[revoked]")
    });
    assert!(ok(&dir, &["cred", "list", "alice"]).contains("valid: false"));
    // help and unknown subcommands
    assert!(ok(&dir, &["cred"]).contains("cargo dev cred <command>"));
    assert!(!dev(&dir, &["cred", "nonsense"]).0);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_asset_commands_run_a_trade_with_escrow_veto_and_liens() {
    let dir = tmp("asset");
    let base = 17_800;
    ok(
        &dir,
        &[
            "init",
            "--port-base",
            &base.to_string(),
            "--min-window",
            "1",
        ],
    );
    let nodes = start_nodes(&dir, base);
    ok(&dir, &["wallet", "new", "alice"]);
    ok(&dir, &["wallet", "new", "bob"]);
    ok(&dir, &["fund", "alice", "10"]);
    wait("alice funded", || {
        ok(&dir, &["balance", "alice"]).contains("10,000000")
    });
    ok(&dir, &["fund", "bob", "100"]);
    wait("funded", || {
        ok(&dir, &["balance", "bob"]).contains("100,000000")
    });

    ok(&dir, &["asset", "class", "--id", "imovel", "--window", "2"]);
    wait("class", || {
        dev(&dir, &["asset", "classes"]).1.contains("imovel")
    });
    assert!(ok(&dir, &["asset", "classes"]).contains("organ 2 of 3"));
    ok(
        &dir,
        &[
            "asset", "register", "--class", "imovel", "--id", "casa-1", "--owner", "alice",
            "--meta", "deed",
        ],
    );
    wait("registered", || {
        dev(&dir, &["asset", "list", "alice"]).1.contains("casa-1")
    });

    // trade 1: bob accepts, the organ vetoes, the money goes back and alice stays the owner
    ok(
        &dir,
        &[
            "asset", "offer", "--id", "casa-1", "--as", "alice", "--to", "bob", "--price", "40",
        ],
    );
    wait("offered", || {
        dev(&dir, &["asset", "show", "casa-1"])
            .1
            .contains("trade offered")
    });
    ok(&dir, &["asset", "accept", "--id", "casa-1", "--as", "bob"]);
    wait("escrow", || {
        dev(&dir, &["asset", "deals"])
            .1
            .contains("escrow total: 40000000")
    });
    ok(&dir, &["asset", "veto", "--id", "casa-1"]);
    wait("vetoed", || {
        dev(&dir, &["asset", "deals"]).1.contains("escrow total: 0")
    });
    assert!(dev(&dir, &["asset", "list", "alice"]).1.contains("casa-1"));
    assert!(
        ok(&dir, &["balance", "bob"]).contains("99,"),
        "bob got the price back (minus fees)"
    );

    // trade 2: after the window anyone finalizes; ownership and money move
    ok(
        &dir,
        &[
            "asset", "offer", "--id", "casa-1", "--as", "alice", "--to", "bob", "--price", "40",
        ],
    );
    wait("offered again", || {
        dev(&dir, &["asset", "show", "casa-1"])
            .1
            .contains("trade offered")
    });
    ok(&dir, &["asset", "accept", "--id", "casa-1", "--as", "bob"]);
    wait("accepted", || {
        dev(&dir, &["asset", "deals"]).1.contains("accepted")
    });
    std::thread::sleep(Duration::from_millis(2_500));
    ok(&dir, &["asset", "finalize", "--id", "casa-1"]);
    wait("new owner", || {
        dev(&dir, &["asset", "list", "bob"]).1.contains("casa-1")
    });
    assert!(!dev(&dir, &["asset", "list", "alice"]).1.contains("casa-1"));
    assert!(ok(&dir, &["asset", "deals"]).contains("escrow total: 0"));

    // liens
    ok(
        &dir,
        &[
            "asset",
            "lien",
            "--id",
            "casa-1",
            "--lien",
            "hip-1",
            "--ref",
            "contract 42",
        ],
    );
    wait("lien", || {
        dev(&dir, &["asset", "show", "casa-1"])
            .1
            .contains("liens 1")
    });
    ok(
        &dir,
        &["asset", "unlien", "--id", "casa-1", "--lien", "hip-1"],
    );
    wait("lien removed", || {
        dev(&dir, &["asset", "show", "casa-1"])
            .1
            .contains("liens 0")
    });
    assert!(ok(&dir, &["asset"]).contains("cargo dev asset <command>"));
    assert!(!dev(&dir, &["asset", "nonsense"]).0);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_contract_commands_run_a_sale_and_a_loan_with_escrow() {
    let dir = tmp("contract");
    let base = 18_000;
    ok(&dir, &["init", "--port-base", &base.to_string()]);
    let nodes = start_nodes(&dir, base);
    ok(&dir, &["wallet", "new", "alice"]);
    ok(&dir, &["wallet", "new", "bob"]);
    ok(&dir, &["fund", "alice", "50"]);
    wait("alice funded", || {
        ok(&dir, &["balance", "alice"]).contains("50,000000")
    });
    ok(&dir, &["fund", "bob", "50"]);
    wait("bob funded", || {
        ok(&dir, &["balance", "bob"]).contains("50,000000")
    });

    let templates = ok(&dir, &["contract", "templates"]);
    assert!(
        templates.contains("compra-venda")
            && templates.contains("refund_expired")
            && templates.contains("emprestimo"),
        "{templates}"
    );

    // sale: bob accepts (10 BRDU to escrow), then confirms (alice is paid)
    ok(
        &dir,
        &[
            "contract",
            "create",
            "--template",
            "compra-venda",
            "--id",
            "cv-1",
            "--as",
            "alice",
            "--with",
            "bob",
            "--terms",
            "m:10,3600",
        ],
    );
    wait("created", || {
        dev(&dir, &["contract", "show", "cv-1"])
            .1
            .contains("[open]")
    });
    ok(
        &dir,
        &[
            "contract", "act", "--id", "cv-1", "--as", "bob", "--action", "accept",
        ],
    );
    wait("funded", || {
        dev(&dir, &["contract", "show", "cv-1"])
            .1
            .contains("escrow 10000000")
    });
    // the wrong party cannot confirm: the network drops it and the state does not change
    ok(
        &dir,
        &[
            "contract", "act", "--id", "cv-1", "--as", "alice", "--action", "confirm",
        ],
    );
    ok(
        &dir,
        &[
            "contract", "act", "--id", "cv-1", "--as", "bob", "--action", "confirm",
        ],
    );
    wait("done", || {
        dev(&dir, &["contract", "show", "cv-1"])
            .1
            .contains("[done]")
    });
    assert!(
        dev(&dir, &["contract", "show", "cv-1"])
            .1
            .contains("escrow 0")
    );
    assert!(ok(&dir, &["contract", "list", "alice"]).contains("cv-1"));
    assert!(
        ok(&dir, &["balance", "alice"]).contains("59,"),
        "alice was paid the price (minus fees)"
    );

    // loan: the principal is locked at creation, paid out on accept, repaid with the agreed amount
    ok(
        &dir,
        &[
            "contract",
            "create",
            "--template",
            "emprestimo",
            "--id",
            "em-1",
            "--as",
            "alice",
            "--with",
            "bob",
            "--terms",
            "m:5,m:6,600",
        ],
    );
    wait("locked", || {
        dev(&dir, &["contract", "show", "em-1"])
            .1
            .contains("escrow 5000000")
    });
    ok(
        &dir,
        &[
            "contract", "act", "--id", "em-1", "--as", "bob", "--action", "accept",
        ],
    );
    wait("active", || {
        dev(&dir, &["contract", "show", "em-1"])
            .1
            .contains("[active]")
    });
    ok(
        &dir,
        &[
            "contract", "act", "--id", "em-1", "--as", "bob", "--action", "repay",
        ],
    );
    wait("repaid", || {
        dev(&dir, &["contract", "show", "em-1"])
            .1
            .contains("[repaid]")
    });

    assert!(ok(&dir, &["contract"]).contains("cargo dev contract <command>"));
    assert!(!dev(&dir, &["contract", "nonsense"]).0);
    assert!(
        !dev(
            &dir,
            &[
                "contract",
                "create",
                "--template",
                "x",
                "--id",
                "i",
                "--as",
                "alice",
                "--with",
                "bob",
                "--terms",
                "abc"
            ]
        )
        .0
    );
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn credential_and_asset_commands_explain_what_to_do_on_a_network_without_test_organ_keys() {
    let dir = tmp("oldnet");
    ok(&dir, &["init", "--port-base", "18200"]);
    // simulates a network created before the organ keys existed
    for i in 1..=3 {
        let _ = std::fs::remove_file(dir.join(format!("keys/organ{i}.key")));
        let _ = std::fs::remove_file(dir.join(format!("keys/authority{i}.key")));
    }
    let nodes = start_nodes(&dir, 18200);
    let (success, out) = dev(&dir, &["cred", "type", "--id", "cnh", "--contest", "60"]);
    assert!(
        !success
            && out.contains("created before credentials existed")
            && out.contains("cargo dev reset"),
        "{out}"
    );
    let (success, out) = dev(
        &dir,
        &["asset", "class", "--id", "imovel", "--window", "60"],
    );
    assert!(
        !success && out.contains("created before assets existed"),
        "{out}"
    );
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
