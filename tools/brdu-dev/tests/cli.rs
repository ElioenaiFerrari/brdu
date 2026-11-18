#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! O CLI contra nós de verdade: `brdu-dev init` gera a rede, 4 nós sobem em processo com as chaves geradas e os
//! comandos (carteira, fund, send, reserve, burn, supply, blocks…) rodam contra as APIs.

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
    assert!(good, "`{}` falhou: {text}", args.join(" "));
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
    panic!("tempo esgotado: {what}");
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
    wait("malha", || nodes.iter().all(|n| n.peer_count() == 3));
    nodes
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("brdu-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn o_cli_faz_o_ciclo_completo_contra_nos_de_verdade() {
    let dir = tmp("cycle");
    let base = 17_100;
    let text = ok(&dir, &["init", "--port-base", &base.to_string()]);
    assert!(text.contains("4 validadores"), "{text}");
    // init recusa recriar por cima
    assert!(!dev(&dir, &["init"]).0);
    let nodes = start_nodes(&dir, base);

    assert!(ok(&dir, &["wallet", "new", "alice"]).contains("brdu1"));
    ok(&dir, &["wallet", "new", "bob"]);
    assert!(ok(&dir, &["wallet", "list"]).contains("alice"));
    assert!(
        !dev(&dir, &["wallet", "new", "alice"]).0,
        "não sobrescreve a frase existente"
    );

    ok(&dir, &["fund", "alice", "1000"]);
    wait("saldo de alice", || {
        ok(&dir, &["balance", "alice"]).contains("1.000,000000 BRDU")
    });
    ok(
        &dir,
        &[
            "send", "--from", "alice", "--to", "bob", "--amount", "12,5", "--count", "3",
        ],
    );
    wait("bob recebeu", || {
        ok(&dir, &["balance", "bob"]).contains("37,500000 BRDU")
    });

    ok(&dir, &["reserve", "--amount", "1000"]);
    wait("atestado", || ok(&dir, &["supply"]).contains("atestado #1"));
    ok(
        &dir,
        &[
            "burn", "--from", "alice", "--amount", "10", "--ref", "saque-1",
        ],
    );
    wait("burn", || {
        ok(&dir, &["supply"]).contains("queimado 10,000000 BRDU")
    });
    let supply = ok(&dir, &["supply"]);
    assert!(
        supply.contains("cobertura: coberta, sobra 10,000000 BRDU"),
        "{supply}"
    );

    // erros claros: valor inválido, carteira inexistente, mint com referência repetida
    assert!(
        dev(
            &dir,
            &["send", "--from", "alice", "--to", "bob", "--amount", "abc"]
        )
        .1
        .contains("valor")
    );
    assert!(dev(&dir, &["balance", "fantasma"]).1.contains("não existe"));
    ok(
        &dir,
        &["mint", "--to", "bob", "--amount", "1", "--ref", "dep-x"],
    );
    wait("mint de bob", || {
        ok(&dir, &["balance", "bob"]).contains("38,500000 BRDU")
    });
    let again = dev(
        &dir,
        &["mint", "--to", "bob", "--amount", "1", "--ref", "dep-x"],
    );
    assert!(
        !again.0 || ok(&dir, &["balance", "bob"]).contains("38,500000 BRDU"),
        "a referência repetida não pode mintar duas vezes"
    );

    let blocks = ok(&dir, &["blocks", "--limit", "3"]);
    assert!(
        blocks.contains("assinado por 3/4") || blocks.contains("assinado por 4/4"),
        "{blocks}"
    );
    let status = ok(&dir, &["status"]);
    assert_eq!(status.matches("pares 3").count(), 4, "{status}");
    ok(&dir, &["blocks-sim", "--count", "2"]);

    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn comandos_sem_rede_pedem_init_e_o_reset_so_apaga_o_que_criou() {
    let dir = tmp("noinit");
    let (good, text) = dev(&dir, &["status"]);
    assert!(!good && text.contains("cargo dev init"), "{text}");
    ok(&dir, &["init", "--nodes", "3", "--port-base", "17300"]);
    std::fs::write(dir.join("meu-arquivo.txt"), "fica").unwrap();
    ok(&dir, &["reset"]);
    assert!(
        dir.join("meu-arquivo.txt").exists(),
        "o reset não apaga o que não criou"
    );
    assert!(!dir.join("devnet.json").exists() && !dir.join("keys").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
