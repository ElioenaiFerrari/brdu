#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Cluster de 4 validadores **em processo**, com TCP de verdade em loopback, Noise, disco e API HTTP.

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

    /// Sobe `n` nós: o nó j disca aos anteriores, e a malha fica completa.
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
    panic!("tempo esgotado: {what}");
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
fn quatro_validadores_decidem_blocos_e_chegam_ao_mesmo_estado() {
    let net = Net::new("basic");
    let nodes = net.start(4, false);
    wait("malha completa", 20, || {
        nodes.iter().all(|n| n.peer_count() == 3)
    });

    nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1 em todos", 20, || all_at(&nodes, 1));
    assert_eq!(balance(&nodes[2], &net.alice), 50_000_000);

    // transferências enviadas a nós diferentes: a fofoca leva todas ao proponente da vez
    for i in 0..6u64 {
        nodes[(i % 4) as usize]
            .submit(&net.transfer(i, &net.alice, &net.bob, 1_000_000))
            .unwrap();
    }
    wait("bob recebeu 6 milhões em todos", 30, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 6_000_000)
    });
    wait("mesmo estado", 10, || same_state(&nodes));
    assert!(nodes.iter().all(|n| n.evidence_count() == 0));
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn a_mesma_transacao_enviada_a_dois_nos_entra_uma_vez() {
    let net = Net::new("dedupe");
    let nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    let t = net.mint(0, &net.alice, 7_000_000, "dep-1");
    let id1 = nodes[0].submit(&t).unwrap();
    let id2 = nodes[1].submit(&t);
    assert!(id2.is_ok() || id2.is_err()); // pode já ter chegado por fofoca
    wait("bloco", 20, || all_at(&nodes, 1));
    wait("mesmo estado", 10, || same_state(&nodes));
    assert_eq!(
        balance(&nodes[3], &net.alice),
        7_000_000,
        "emitido uma vez só"
    );
    // reenviar depois de incluída: nonce vencido, recusada
    assert!(nodes[2].submit(&t).is_err());
    assert_eq!(id1, t.id());
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn um_validador_fora_nao_para_a_rede_e_ao_voltar_alcanca_pelo_disco_e_pela_sincronizacao() {
    let net = Net::new("restart");
    let mut nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1", 20, || all_at(&nodes, 1));

    // derruba o nó 3
    let down = nodes.pop().unwrap();
    down.stop();
    for i in 0..5u64 {
        nodes[(i % 3) as usize]
            .submit(&net.transfer(i, &net.alice, &net.bob, 1_000_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }
    wait("3 de 4 seguem produzindo", 40, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 5_000_000)
    });
    let target = nodes[0].height();

    // volta com a MESMA pasta de dados: lê o log, depois sincroniza o que perdeu
    let peers: Vec<String> = nodes.iter().map(|x| x.listen_addr().to_string()).collect();
    let back = Node::start(net.config(3, peers, false)).unwrap();
    assert!(
        back.height() >= 1,
        "recuperou do disco antes de falar com ninguém"
    );
    nodes.push(back);
    wait("o voltou alcança", 40, || nodes[3].height() >= target);
    wait("mesmo estado", 20, || same_state(&nodes));
    assert_eq!(balance(&nodes[3], &net.bob), 5_000_000);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn a_rede_toda_reinicia_e_continua_do_mesmo_ponto() {
    let net = Net::new("allrestart");
    let nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[1]
        .submit(&net.mint(0, &net.alice, 9_000_000, "dep-1"))
        .unwrap();
    wait("bloco", 20, || all_at(&nodes, 1));
    wait("mesmo estado", 10, || same_state(&nodes));
    let (h, root) = (nodes[0].height(), nodes[0].state_root());
    for n in nodes {
        n.stop();
    }
    let again = net.start(4, false);
    assert!(
        again
            .iter()
            .all(|n| n.height() == h && n.state_root() == root),
        "o disco refaz o mesmo estado"
    );
    wait("malha", 20, || again.iter().all(|n| n.peer_count() == 3));
    again[2]
        .submit(&net.transfer(0, &net.alice, &net.bob, 1_000_000))
        .unwrap();
    wait("continua", 30, || {
        again.iter().all(|n| balance(n, &net.bob) == 1_000_000)
    });
    for n in again {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn api_http_status_conta_envio_de_transacao_e_blocos() {
    let net = Net::new("api");
    let nodes = net.start(4, true);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    let api = nodes[0].api_addr().unwrap();

    let (code, st) = http(api, "GET", "/status", "");
    assert_eq!(code, 200);
    assert_eq!(st["chain_id"], CHAIN);
    assert_eq!(st["height"], 0);

    let t = net.mint(0, &net.alice, 3_000_000, "dep-1");
    let (code, r) = http(api, "POST", "/tx", &hex::encode(&t.encode().unwrap()));
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["id"], t.id().to_hex());
    // lixo e assinatura adulterada
    assert_eq!(http(api, "POST", "/tx", "zz").0, 400);
    let mut bad = net.mint(1, &net.alice, 1, "x");
    bad.signature.0[0] ^= 1;
    assert_eq!(
        http(api, "POST", "/tx", &hex::encode(&bad.encode().unwrap())).0,
        400
    );

    wait("bloco", 20, || all_at(&nodes, 1));
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
        "certificado com mais de 2/3"
    );
    let (_, found) = http(api, "GET", &format!("/tx/{}", t.id().to_hex()), "");
    assert_eq!(found["height"], 1);
    assert_eq!(http(api, "GET", "/block/99", "").0, 404);
    assert_eq!(http(api, "GET", "/account/lixo", "").0, 400);
    assert_eq!(http(api, "GET", "/nada", "").0, 404);
    assert_eq!(http(api, "DELETE", "/tx", "").0, 405);
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn proponente_da_vez_fora_do_ar_a_rodada_muda_e_a_rede_decide() {
    let net = Net::new("proposer-down");
    let mut nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 50_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1", 20, || all_at(&nodes, 1));
    // o proponente do bloco 2 (rodada 0) é validators[2 % 4] = o nó 2: derruba
    let down = nodes.remove(2);
    down.stop();
    nodes[0]
        .submit(&net.transfer(0, &net.alice, &net.bob, 1_000_000))
        .unwrap();
    wait("a rede decide mesmo assim", 40, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 1_000_000)
    });
    wait("mesmo estado", 10, || same_state(&nodes));
    let blocks = nodes[0].shared().chain.lock().unwrap().blocks().to_vec();
    assert!(
        blocks.iter().any(|d| d.cert.round >= 1),
        "algum bloco precisou de outra rodada"
    );
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn rajada_de_trezentas_transacoes_de_varias_contas_entra_sem_perder_nem_duplicar() {
    let net = Net::new("burst");
    let nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    // 20 contas com saldo, cada uma envia 15 transferências em sequência a nós diferentes
    let senders: Vec<SecretKey> = (100..120).map(key).collect();
    for (i, s) in senders.iter().enumerate() {
        nodes[i % 4]
            .submit(&net.mint(i as u64, s, 100_000_000, &format!("dep-{i}")))
            .unwrap();
    }
    wait("saldos", 40, || {
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
    wait("bob recebeu tudo em todos os nós", 60, || {
        nodes.iter().all(|n| balance(n, &net.bob) == 300_000)
    });
    wait("mesmo estado", 20, || same_state(&nodes));
    let pool: usize = nodes
        .iter()
        .map(|n| n.shared().info.lock().unwrap().pool_len)
        .sum();
    assert_eq!(pool, 0, "nada ficou pendente");
    // a fofoca não ecoa: cada nó recebeu cada transação poucas vezes
    for n in &nodes {
        let dup = n
            .shared()
            .stats
            .txs_duplicate
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(dup <= 2 * 320 * 3, "duplicatas demais: {dup}");
    }
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

// ---------------------------------------------------------------- rotação de validadores

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
fn quinto_no_entra_por_rotacao_sincroniza_como_observador_e_depois_vota() {
    let net = Net::new("rotate-in");
    let mut nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    // o 5º sobe com a chave v5, que NÃO está na gênese: é recusado até a troca ser incluída
    let peers: Vec<String> = nodes.iter().map(|x| x.listen_addr().to_string()).collect();
    let fifth = Node::start(net.config(4, peers, false)).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        fifth.peer_count(),
        0,
        "ainda não é validador nem tem troca pendente"
    );

    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Add,
        &net.vals[4],
        6,
        3,
    );
    nodes[1].submit(&tx).unwrap();
    wait("troca pendente em todos", 30, || {
        nodes
            .iter()
            .all(|n| n.shared().chain.lock().unwrap().pending_change().is_some())
    });
    // com a troca pendente o 5º é admitido e sincroniza como observador
    wait("o 5º conecta e alcança", 30, || {
        fifth.peer_count() >= 3 && fifth.height() >= nodes[0].height()
    });
    // geramos blocos até a época virar
    let mut n = 0u64;
    while epoch(&nodes[0]) == 0 {
        nodes[(n % 4) as usize]
            .submit(&net.transfer(n, &net.alice, &net.bob, 1_000))
            .unwrap();
        n += 1;
        std::thread::sleep(Duration::from_millis(200));
        assert!(n < 80, "a época não virou");
    }
    nodes.push(fifth);
    wait("os 5 na mesma altura", 30, || same_state(&nodes));
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
    wait("mais blocos", 40, || nodes[0].height() >= from + 4);
    wait("o 5º assinou algum bloco", 40, || {
        signed_by_anyone_new(&nodes[0], &net.vals[4], from)
    });
    wait("mesmo estado", 20, || same_state(&nodes));
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn validador_removido_vira_observador_e_a_rede_segue_sem_ele() {
    let net = Net::new("rotate-out");
    let nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1", 20, || all_at(&nodes, 1));
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
        assert!(n < 80, "a época não virou");
    }
    wait("os 3 restantes desconectam do removido", 20, || {
        nodes[..3].iter().all(|x| x.peer_count() == 2)
    });
    let from = nodes[0].height();
    for i in 0..8u64 {
        nodes[(i % 3) as usize]
            .submit(&net.transfer(n + i, &net.alice, &net.bob, 1_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }
    wait("a rede de 3 segue", 40, || {
        nodes[..3].iter().all(|x| x.height() >= from + 3)
    });
    assert!(
        !signed_by_anyone_new(&nodes[0], &net.vals[3], from),
        "o removido não assina mais"
    );
    // o removido não consegue mais entrar: continua sem pares
    wait("o removido ficou isolado", 20, || {
        nodes[3].peer_count() == 0
    });
    for n in nodes {
        n.stop();
    }
    let _ = std::fs::remove_dir_all(&net.dir);
}

#[test]
fn troca_sem_mais_de_2_3_nunca_fica_pendente() {
    let net = Net::new("rotate-weak");
    let nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Remove,
        &net.vals[3],
        5,
        2,
    ); // 2 de 4
    nodes[0].submit(&tx).unwrap();
    for i in 0..4u64 {
        nodes[0]
            .submit(&net.transfer(i, &net.alice, &net.bob, 1_000))
            .unwrap();
        std::thread::sleep(Duration::from_millis(250));
    }
    wait("os blocos seguem", 30, || all_at(&nodes, 3));
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
fn reinicio_com_troca_pendente_refaz_o_mesmo_conjunto_pendente_e_efetiva_depois() {
    let net = Net::new("rotate-restart");
    let nodes = net.start(4, false);
    wait("malha", 20, || nodes.iter().all(|n| n.peer_count() == 3));
    nodes[0]
        .submit(&net.mint(0, &net.alice, 10_000_000, "dep-1"))
        .unwrap();
    wait("bloco 1", 20, || all_at(&nodes, 1));
    let tx = change_tx(
        &net,
        &nodes[0],
        brdu_ledger::gov::Action::Add,
        &net.vals[4],
        30,
        3,
    );
    nodes[0].submit(&tx).unwrap();
    wait("pendente", 30, || {
        nodes
            .iter()
            .all(|n| n.shared().chain.lock().unwrap().pending_change().is_some())
    });
    wait("mesmo estado", 10, || same_state(&nodes));
    let root = nodes[0].state_root();
    for n in nodes {
        n.stop();
    }
    let again = net.start(4, false);
    assert!(
        again.iter().all(|n| n.state_root() == root),
        "o disco refaz o mesmo estado"
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
