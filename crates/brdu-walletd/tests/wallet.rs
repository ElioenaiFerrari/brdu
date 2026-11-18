#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The wallet service against a real 4-validator network (in-process, TCP + Noise) and the explorer.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::{Address, SecretKey};
use brdu_explorer::{Explorer, serve, spawn_sync};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Tx, kind};
use brdu_node::{Config as NodeConfig, Node, Timeouts};
use brdu_vault::KdfParams;
use brdu_walletd::{Config, Wallet, WalletError};
use serde_json::{Value, json};

const CHAIN: &str = "brdu-wallet-test";
const FAST: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out: {what}");
}

struct Lab {
    nodes: Vec<Node>,
    explorer_addr: SocketAddr,
    issuers: Vec<SecretKey>,
    relayer: SecretKey,
    dir: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Lab {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("brdu-walletd-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let vals: Vec<SecretKey> = (20..24).map(key).collect();
        let issuers: Vec<SecretKey> = (10..13).map(key).collect();
        let mut genesis = Genesis::new(
            CHAIN,
            vals.iter().map(SecretKey::public).collect(),
            issuers.iter().map(SecretKey::public).collect(),
            2,
        );
        genesis.authorities = (30..33).map(key).map(|k| k.public()).collect();
        genesis.authority_threshold = 2;
        genesis.min_window = 1;
        let mut nodes: Vec<Node> = Vec::new();
        for (i, v) in vals.iter().enumerate() {
            let mut c = NodeConfig::new(
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
        wait("mesh", || nodes.iter().all(|n| n.peer_count() == 3));
        let explorer: brdu_explorer::Shared = Arc::new(RwLock::new(
            Explorer::new(genesis, &nodes[0].api_addr().unwrap().to_string()).unwrap(),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let _ = spawn_sync(explorer.clone(), stop.clone());
        let (explorer_addr, _) = serve(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            explorer,
            stop.clone(),
        )
        .unwrap();
        Self {
            nodes,
            explorer_addr,
            issuers,
            relayer: key(9),
            dir,
            stop,
        }
    }

    fn api(&self) -> String {
        self.nodes[0].api_addr().unwrap().to_string()
    }

    fn wallet(&self, name: &str, with_explorer: bool) -> Wallet {
        let mut c = Config::new(
            self.dir.join(format!("{name}.vault.json")),
            &self.api(),
            with_explorer
                .then(|| self.explorer_addr.to_string())
                .as_deref(),
        );
        c.kdf = FAST;
        Wallet::new(c)
    }

    fn balance(&self, addr: &str) -> u64 {
        self.nodes[0]
            .shared()
            .chain
            .lock()
            .unwrap()
            .state()
            .balance(&Address::decode(addr).unwrap())
    }

    fn fund(&self, addr: &str, amount: u64, reference: &str) {
        let to = Address::decode(addr).unwrap();
        let doc = coin::issuance_doc(false, CHAIN, &to, amount, reference);
        let ap: Vec<Approval> = self
            .issuers
            .iter()
            .take(2)
            .map(|k| Approval::new(&doc, k))
            .collect();
        let nonce = self.nodes[0]
            .shared()
            .chain
            .lock()
            .unwrap()
            .state()
            .nonce(&Address::from_public_key(&self.relayer.public()));
        let tx = Tx::new_signed(
            &self.relayer,
            CHAIN,
            kind::MINT,
            nonce,
            0,
            coin::issuance_payload(&to, amount, reference, &ap),
        )
        .unwrap();
        self.nodes[0].submit(&tx).unwrap();
        wait("funding", || self.balance(addr) >= amount);
    }

    fn stop(self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        for n in self.nodes {
            n.stop();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn code<T: std::fmt::Debug>(r: Result<T, WalletError>) -> &'static str {
    r.unwrap_err().code
}

#[test]
fn lifecycle_create_lock_unlock_with_the_vault_on_disk() {
    let lab = Lab::start("lifecycle");
    let w = lab.wallet("a", false);
    assert_eq!(
        w.status(),
        json!({"has_vault": false, "unlocked": false, "address": null})
    );
    assert_eq!(code(w.unlock("whatever-password", "")), "no_vault");
    assert_eq!(code(w.create("short", "")), "weak_password");

    let created = w.create("correct horse battery", "").unwrap();
    let words = created["words"].as_array().unwrap();
    assert_eq!(words.len(), 24);
    let address = created["address"].as_str().unwrap().to_string();
    assert!(address.starts_with("brdu1"));

    // the vault is on disk, private (0600), and never contains the words
    let path = lab.dir.join("a.vault.json");
    let text = std::fs::read_to_string(&path).unwrap();
    // single words are common substrings of the JSON; what must never appear is the whole phrase
    assert!(
        !text.contains(
            &words
                .iter()
                .map(|w| w.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        )
    );
    assert!(
        text.contains(&address),
        "the public address is stored in the clear for the UI"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        code(w.create("another password", "")),
        "vault_exists",
        "never overwrites an existing wallet"
    );

    // locked: nothing that needs the key works, but the status shows the address
    w.lock();
    assert_eq!(
        w.status(),
        json!({"has_vault": true, "unlocked": false, "address": address})
    );
    assert_eq!(code(w.overview()), "locked");
    assert_eq!(code(w.prepare_send(&address, "1")), "locked");
    assert_eq!(code(w.history(0, 10)), "locked");
    assert_eq!(code(w.unlock("wrong password!", "")), "wrong_password");
    assert_eq!(
        w.unlock("correct horse battery", "").unwrap()["address"],
        address
    );
    assert_eq!(w.status()["unlocked"], true);
    lab.stop();
}

#[test]
fn the_passphrase_is_never_stored_and_a_wrong_one_is_detected() {
    let lab = Lab::start("passphrase");
    let w = lab.wallet("p", false);
    let created = w
        .create("correct horse battery", "my secret 25th word")
        .unwrap();
    let with_pass = created["address"].as_str().unwrap().to_string();
    w.lock();
    assert_eq!(
        code(w.unlock("correct horse battery", "")),
        "wrong_passphrase"
    );
    assert_eq!(
        code(w.unlock("correct horse battery", "another")),
        "wrong_passphrase"
    );
    assert_eq!(
        w.unlock("correct horse battery", "my secret 25th word")
            .unwrap()["address"],
        with_pass
    );
    assert!(
        !std::fs::read_to_string(lab.dir.join("p.vault.json"))
            .unwrap()
            .contains("my secret 25th word")
    );

    // the same words without a passphrase are a different wallet
    let words: Vec<String> = created["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    let phrase = words.join(" ");
    let plain = w.preview_import(&phrase, "").unwrap()["address"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(plain, with_pass);
    assert_eq!(
        w.preview_import(&phrase, "my secret 25th word").unwrap()["address"],
        with_pass
    );
    lab.stop();
}

#[test]
fn import_requires_the_confirmed_address_and_a_valid_phrase() {
    let lab = Lab::start("import");
    let original = lab.wallet("orig", false);
    let created = original.create("correct horse battery", "").unwrap();
    let phrase: String = created["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    let address = created["address"].as_str().unwrap().to_string();

    let w = lab.wallet("imp", false);
    assert_eq!(
        code(w.preview_import("not a real phrase at all", "")),
        "invalid_phrase"
    );
    // a phrase with a bad checksum (one word swapped)
    let mut bad: Vec<&str> = phrase.split(' ').collect();
    bad[23] = if bad[23] == "abandon" {
        "ability"
    } else {
        "abandon"
    };
    assert_eq!(code(w.preview_import(&bad.join(" "), "")), "invalid_phrase");

    let preview = w.preview_import(&phrase, "").unwrap();
    assert_eq!(preview["address"], address);
    assert_eq!(preview["word_count"], 24);
    assert_eq!(w.status()["has_vault"], false, "previewing stores nothing");
    // importing with an address the user did NOT confirm is refused
    assert_eq!(
        code(w.import(
            &phrase,
            "",
            "another password 1",
            &Address::from_public_key(&key(5).public()).encode()
        )),
        "address_mismatch"
    );
    assert_eq!(w.status()["has_vault"], false);
    assert_eq!(
        w.import(&phrase, "", "another password 1", &address)
            .unwrap()["address"],
        address
    );
    w.lock();
    assert_eq!(
        w.unlock("another password 1", "").unwrap()["address"],
        address
    );
    lab.stop();
}

#[test]
fn export_needs_the_password_again_and_changing_it_retires_the_old_one() {
    let lab = Lab::start("password");
    let w = lab.wallet("pw", false);
    let created = w.create("correct horse battery", "").unwrap();
    let phrase = created["words"].clone();
    assert_eq!(code(w.export_phrase("wrong password!")), "wrong_password");
    assert_eq!(
        w.export_phrase("correct horse battery").unwrap()["words"],
        phrase
    );
    assert_eq!(
        code(w.change_password("wrong password!", "new password 123")),
        "wrong_password"
    );
    assert_eq!(
        code(w.change_password("correct horse battery", "short")),
        "weak_password"
    );
    w.change_password("correct horse battery", "new password 123")
        .unwrap();
    w.lock();
    assert_eq!(
        code(w.unlock("correct horse battery", "")),
        "wrong_password"
    );
    assert!(w.unlock("new password 123", "").is_ok());
    // export is refused while locked even with the right password
    w.lock();
    assert_eq!(code(w.export_phrase("new password 123")), "locked");
    lab.stop();
}

#[test]
fn repeated_wrong_passwords_trigger_a_growing_lockout_that_a_right_password_cannot_skip() {
    let lab = Lab::start("lockout");
    let mut c = Config::new(lab.dir.join("l.vault.json"), &lab.api(), None);
    c.kdf = FAST;
    c.max_attempts = 3;
    c.lockout = Duration::from_millis(400);
    let w = Wallet::new(c);
    w.create("correct horse battery", "").unwrap();
    w.lock();
    for _ in 0..3 {
        assert_eq!(code(w.unlock("wrong password!", "")), "wrong_password");
    }
    // locked out: even the RIGHT password is refused until the lockout ends
    assert_eq!(
        code(w.unlock("correct horse battery", "")),
        "too_many_attempts"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(code(w.unlock("wrong password!", "")), "wrong_password");
    assert_eq!(
        code(w.unlock("correct horse battery", "")),
        "too_many_attempts",
        "the lockout doubled"
    );
    std::thread::sleep(Duration::from_millis(900));
    assert!(w.unlock("correct horse battery", "").is_ok());
    // success resets the counter: two more wrong ones do not lock out
    w.lock();
    for _ in 0..2 {
        assert_eq!(code(w.unlock("wrong password!", "")), "wrong_password");
    }
    assert!(w.unlock("correct horse battery", "").is_ok());
    // export and change-password are throttled too
    for _ in 0..3 {
        assert_eq!(code(w.export_phrase("wrong password!")), "wrong_password");
    }
    assert_eq!(
        code(w.export_phrase("correct horse battery")),
        "too_many_attempts"
    );
    assert_eq!(
        code(w.change_password("correct horse battery", "new password 123")),
        "too_many_attempts"
    );
    lab.stop();
}

#[test]
fn the_wallet_locks_itself_after_inactivity() {
    let lab = Lab::start("autolock");
    let mut c = Config::new(lab.dir.join("auto.vault.json"), &lab.api(), None);
    c.kdf = FAST;
    c.lock_after = Duration::from_millis(250);
    let w = Wallet::new(c);
    w.create("correct horse battery", "").unwrap();
    assert!(w.overview().is_ok());
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(code(w.overview()), "locked");
    assert_eq!(w.status()["unlocked"], false);
    // activity keeps it open
    w.unlock("correct horse battery", "").unwrap();
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(100));
        assert!(w.overview().is_ok());
    }
    lab.stop();
}

#[test]
fn balance_send_preview_confirm_and_history_against_a_real_network() {
    let lab = Lab::start("send");
    let w = lab.wallet("send", true);
    let address = w.create("correct horse battery", "").unwrap()["address"]
        .as_str()
        .unwrap()
        .to_string();
    let bob = Address::from_public_key(&key(2).public()).encode();

    assert_eq!(w.overview().unwrap()["balance"], 0);
    lab.fund(&address, 50_000_000, "dep-1");
    let o = w.overview().unwrap();
    assert_eq!(
        (o["balance"].as_u64(), o["chain_id"].as_str()),
        (Some(50_000_000), Some(CHAIN))
    );
    assert_eq!(o["supply"]["supply"], 50_000_000);

    // validation: bad address (typo), bad amounts, not enough funds
    let mut typo = bob.clone();
    typo.replace_range(20..21, if &bob[20..21] == "q" { "p" } else { "q" });
    assert_eq!(code(w.prepare_send(&typo, "1")), "invalid_address");
    assert_eq!(
        code(w.prepare_send("not an address", "1")),
        "invalid_address"
    );
    for bad in ["abc", "0", "0,0", "1.5", "-1", "1,0000001", ""] {
        assert_eq!(
            code(w.prepare_send(&bob, bad)),
            "invalid_amount",
            "amount {bad:?}"
        );
    }
    assert_eq!(
        code(w.prepare_send(&bob, "50,000000")),
        "insufficient_funds",
        "the fee must also fit"
    );
    assert_eq!(code(w.prepare_send(&bob, "51")), "insufficient_funds");

    // preview: amount + fee = total, and the balance that would remain
    let p = w.prepare_send(&bob, "12,5").unwrap();
    let (amount, fee, total) = (
        p["amount"].as_u64().unwrap(),
        p["fee"].as_u64().unwrap(),
        p["total"].as_u64().unwrap(),
    );
    assert_eq!(amount, 12_500_000);
    // minimum fee = 1000 base + 1 per byte of the ~170-byte encoded transfer (plus the chain id)
    assert!((1_150..=1_250).contains(&fee), "unexpected fee {fee}");
    assert_eq!(total, amount + fee);
    assert_eq!(p["balance_after"].as_u64(), Some(50_000_000 - total));
    assert_eq!(
        (p["to"].as_str(), p["to_self"].as_bool()),
        (Some(bob.as_str()), Some(false))
    );
    assert_eq!(lab.balance(&bob), 0, "preparing sends nothing");

    // confirm: signs and submits; the id is single-use
    let id = p["id"].as_str().unwrap().to_string();
    let sent = w.confirm_send(&id).unwrap();
    assert!(sent["tx_id"].as_str().unwrap().len() == 64);
    wait("bob receives", || lab.balance(&bob) == 12_500_000);
    assert_eq!(code(w.confirm_send(&id)), "unknown_prepared_send");
    assert_eq!(
        code(w.confirm_send("deadbeefdeadbeef")),
        "unknown_prepared_send"
    );

    // sending to yourself is allowed but flagged
    let me = w.prepare_send(&address, "1").unwrap();
    assert_eq!(me["to_self"], true);

    // a prepared send does not survive a lock
    w.lock();
    w.unlock("correct horse battery", "").unwrap();
    assert_eq!(
        code(w.confirm_send(me["id"].as_str().unwrap())),
        "unknown_prepared_send"
    );

    // history comes from the explorer and shows both the mint received and the transfer sent
    wait("explorer indexes", || {
        w.history(0, 10)
            .is_ok_and(|h| h["tx_count"].as_u64() == Some(2))
    });
    let h = w.history(0, 10).unwrap();
    let kinds: Vec<u64> = h["txs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["kind"].as_u64().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![1, 2],
        "newest first: the transfer, then the mint"
    );
    assert_eq!(code(lab.wallet("nox", false).history(0, 10)), "locked");
    lab.stop();
}

#[test]
fn history_without_an_explorer_and_an_unreachable_node_are_clear_errors() {
    let lab = Lab::start("errors");
    let w = lab.wallet("e", false);
    w.create("correct horse battery", "").unwrap();
    assert_eq!(code(w.history(0, 5)), "no_explorer");

    let mut c = Config::new(lab.dir.join("e.vault.json"), "127.0.0.1:1", None);
    c.kdf = FAST;
    let offline = Wallet::new(c);
    offline.unlock("correct horse battery", "").unwrap();
    assert_eq!(code(offline.overview()), "node_unreachable");
    assert_eq!(
        code(offline.prepare_send(&Address::from_public_key(&key(2).public()).encode(), "1")),
        "node_unreachable"
    );
    lab.stop();
}

#[test]
fn the_json_interface_routes_methods_validates_params_and_never_echoes_secrets() {
    let lab = Lab::start("rpc");
    let w = lab.wallet("rpc", false);
    assert_eq!(w.call("status", &Value::Null).unwrap()["has_vault"], false);
    assert_eq!(code(w.call("nope", &json!({}))), "unknown_method");
    assert_eq!(code(w.call("create", &json!({}))), "bad_request");
    assert_eq!(
        code(w.call("create", &json!({"password": 42}))),
        "bad_request"
    );

    let secret = "correct horse battery";
    let created = w
        .call(
            "create",
            &json!({"password": secret, "passphrase": "pp-secret"}),
        )
        .unwrap();
    // only the words (shown once) and the address come back; never the password or passphrase
    let text = created.to_string();
    assert!(!text.contains(secret) && !text.contains("pp-secret"));
    assert_eq!(w.call("lock", &json!({})).unwrap(), json!({"locked": true}));
    let unlocked = w
        .call(
            "unlock",
            &json!({"password": secret, "passphrase": "pp-secret"}),
        )
        .unwrap();
    assert!(!unlocked.to_string().contains(secret));
    // errors carry codes, not the secrets that caused them
    let err = w
        .call("unlock", &json!({"password": "wrong password!"}))
        .unwrap_err();
    assert_eq!(err.code, "wrong_password");
    assert!(!err.to_string().contains("wrong password!"));
    lab.stop();
}

#[test]
fn the_wallet_lists_the_credentials_issued_to_it() {
    use brdu_credentials::{Privacy, TypeSpec, issue_doc, issue_payload, type_doc, type_payload};
    let lab = Lab::start("creds");
    let w = lab.wallet("c", false);
    let address = w.create("correct horse battery", "").unwrap()["address"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        w.credentials().unwrap()["credentials"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    lab.fund(
        &Address::from_public_key(&lab.relayer.public()).encode(),
        50_000_000,
        "dep-relayer",
    );
    let authorities: Vec<SecretKey> = (30..33).map(key).collect();
    let organ: Vec<SecretKey> = (40..43).map(key).collect();
    let ap = |doc: &brdu_crypto::Hash32, keys: &[SecretKey]| -> Vec<Approval> {
        keys.iter().take(2).map(|k| Approval::new(doc, k)).collect()
    };
    let send = |k: u8, payload: Vec<u8>| {
        let (nonce, fee) = {
            let c = lab.nodes[0].shared().chain.lock().unwrap();
            let n = c
                .state()
                .nonce(&Address::from_public_key(&lab.relayer.public()));
            let probe = Tx::new_signed(&lab.relayer, CHAIN, k, n, 0, payload.clone()).unwrap();
            (n, c.executor().min_fee(&probe).unwrap())
        };
        lab.nodes[0]
            .submit(&Tx::new_signed(&lab.relayer, CHAIN, k, nonce, fee, payload).unwrap())
            .unwrap();
    };
    let spec = TypeSpec {
        id: "cnh".into(),
        organ: organ.iter().map(SecretKey::public).collect(),
        threshold: 2,
        contest: 1,
        validity: 0,
        privacy: Privacy::Public,
        ref_type: String::new(),
        revokes: false,
    };
    send(
        brdu_credentials::kind::TYPE,
        type_payload(&spec, &ap(&type_doc(CHAIN, &spec), &authorities)),
    );
    wait("type", || {
        lab.nodes[0]
            .shared()
            .chain
            .lock()
            .unwrap()
            .state()
            .ext_scan(b"cred/t/")
            .next()
            .is_some()
    });
    let me = Address::decode(&address).unwrap();
    let doc = issue_doc(CHAIN, "cnh", "cnh-1", &me, b"category=B", "");
    send(
        brdu_credentials::kind::ISSUE,
        issue_payload("cnh", "cnh-1", &me, b"category=B", "", &ap(&doc, &organ)),
    );
    wait("credential listed", || {
        w.credentials()
            .is_ok_and(|c| c["credentials"].as_array().is_some_and(|l| l.len() == 1))
    });
    let c = w.credentials().unwrap();
    assert_eq!(
        (
            c["credentials"][0]["id"].as_str(),
            c["credentials"][0]["status"].as_str()
        ),
        (Some("cnh-1"), Some("pending"))
    );
    assert_eq!(
        w.call("credentials", &Value::Null).unwrap()["credentials"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.lock();
    assert_eq!(code(w.credentials()), "locked");
    assert_eq!(code(w.assets()), "locked");
    assert_eq!(code(w.contracts()), "locked");
    lab.stop();
}

#[test]
fn forgetting_the_wallet_moves_the_vault_aside_and_lets_a_new_one_be_made_without_the_old_password()
{
    let lab = Lab::start("forget");
    let w = lab.wallet("f", false);
    let first = w.create("correct horse battery", "").unwrap();
    let words: Vec<String> = first["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    let address = first["address"].as_str().unwrap().to_string();
    w.lock();
    // creating or importing on top of an existing wallet is refused...
    assert_eq!(code(w.create("another password 1", "")), "vault_exists");
    // ...and removal needs the explicit confirmation word, but NOT the password
    assert_eq!(code(w.forget("yes")), "bad_request");
    assert_eq!(code(w.forget("")), "bad_request");
    assert_eq!(
        w.status()["has_vault"],
        true,
        "a refused removal changes nothing"
    );
    let r = w.forget("forget").unwrap();
    assert_eq!(r["removed"], true);
    let kept = r["kept_as"].as_str().unwrap().to_string();
    assert!(kept.starts_with("f.vault.json.removed-"), "{kept}");
    assert!(
        lab.dir.join(&kept).exists(),
        "the old vault is kept, not deleted"
    );
    assert_eq!(
        w.status(),
        json!({"has_vault": false, "unlocked": false, "address": null})
    );
    assert_eq!(code(w.forget("forget")), "no_vault");
    // now the same recovery phrase can be imported (this is the "forgot my password" path)
    let phrase = words.join(" ");
    assert_eq!(w.preview_import(&phrase, "").unwrap()["address"], address);
    assert_eq!(
        w.import(&phrase, "", "a brand new password", &address)
            .unwrap()["address"],
        address
    );
    w.lock();
    assert!(w.unlock("a brand new password", "").is_ok());
    assert_eq!(
        code(w.unlock("correct horse battery", "")),
        "wrong_password",
        "the old password no longer opens it"
    );
    // an unlocked session is dropped by the removal
    assert_eq!(
        w.call("forget", &json!({"confirm": "forget"})).unwrap()["removed"],
        true
    );
    assert_eq!(code(w.overview()), "locked");
    lab.stop();
}
