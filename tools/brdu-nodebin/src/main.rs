//! `brdu-node`: o validador. Chaves de **TESTE** em arquivo de texto (hex): só para redes de teste.
//!
//! ```text
//! brdu-node keygen ARQUIVO                      cria uma chave de consenso (0600) e mostra a pública
//! brdu-node pubkey ARQUIVO                      mostra a chave pública (hex) e o endereço
//! brdu-node genesis --chain ID --validator HEX... --issuer HEX... --threshold M --out ARQ
//! brdu-node info --genesis ARQ                  mostra a gênese
//! brdu-node health [--api A]                    sai 0 se a API responde (healthcheck do Docker)
//! brdu-node run --genesis ARQ --key ARQ --data PASTA [--listen A] [--api A] [--peer A]... [opções]
//! ```

use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use brdu_chain::Genesis;
use brdu_crypto::{Address, PublicKey, SecretKey, hex};
use brdu_node::{Config, Node};

fn die(msg: &str) -> ! {
    eprintln!("brdu-node: {msg}");
    std::process::exit(2)
}

fn read_key(path: &Path) -> SecretKey {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("{}: {e}", path.display())));
    let seed = hex::decode_array::<32>(text.trim())
        .unwrap_or_else(|| die("a chave deve ter 32 bytes em hex"));
    SecretKey::from_bytes(seed)
}

fn read_genesis(path: &Path) -> Genesis {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("{}: {e}", path.display())));
    let raw = hex::decode(text.trim()).unwrap_or_else(|| die("a gênese deve estar em hex"));
    Genesis::decode(&raw).unwrap_or_else(|e| die(&format!("gênese inválida: {e}")))
}

fn pubkey(s: &str) -> PublicKey {
    PublicKey(
        hex::decode_array::<32>(s).unwrap_or_else(|| die(&format!("chave pública inválida: {s}"))),
    )
}

struct Args(Vec<String>);

impl Args {
    fn one(&self, flag: &str) -> Option<String> {
        self.0
            .iter()
            .position(|a| a == flag)
            .and_then(|i| self.0.get(i + 1).cloned())
    }
    fn many(&self, flag: &str) -> Vec<String> {
        self.0
            .windows(2)
            .filter(|w| w[0] == flag)
            .map(|w| w[1].clone())
            .collect()
    }
    fn has(&self, flag: &str) -> bool {
        self.0.iter().any(|a| a == flag)
    }
    fn need(&self, flag: &str) -> String {
        self.one(flag)
            .unwrap_or_else(|| die(&format!("falta {flag}")))
    }
    fn ms(&self, flag: &str) -> Option<Duration> {
        self.one(flag).map(|v| {
            Duration::from_millis(
                v.parse()
                    .unwrap_or_else(|_| die(&format!("{flag}: número de milissegundos"))),
            )
        })
    }
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = all.split_first() else {
        die("use: keygen | pubkey | genesis | info | run")
    };
    let a = Args(rest.to_vec());
    match cmd.as_str() {
        "keygen" => {
            let path = PathBuf::from(rest.first().unwrap_or_else(|| die("keygen ARQUIVO")));
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).unwrap_or_else(|e| die(&format!("sem aleatoriedade: {e}")));
            let mut opts = fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts
                .open(&path)
                .unwrap_or_else(|e| die(&format!("{}: {e}", path.display())));
            let _ = writeln!(f, "{}", hex::encode(&seed));
            show(&SecretKey::from_bytes(seed));
        }
        "pubkey" => show(&read_key(Path::new(
            rest.first().unwrap_or_else(|| die("pubkey ARQUIVO")),
        ))),
        "genesis" => {
            let validators: Vec<PublicKey> =
                a.many("--validator").iter().map(|s| pubkey(s)).collect();
            let issuers: Vec<PublicKey> = a.many("--issuer").iter().map(|s| pubkey(s)).collect();
            let threshold: u32 = a
                .need("--threshold")
                .parse()
                .unwrap_or_else(|_| die("--threshold: número"));
            let g = Genesis::new(&a.need("--chain"), validators, issuers, threshold);
            if let Err(e) = g.rules() {
                die(&format!("gênese inválida: {e}"));
            }
            let out = a.need("--out");
            fs::write(&out, format!("{}\n", hex::encode(&g.encode())))
                .unwrap_or_else(|e| die(&format!("{out}: {e}")));
            println!("gênese {} gravada em {out}", g.hash().to_hex());
        }
        "info" => {
            let g = read_genesis(Path::new(&a.need("--genesis")));
            println!("rede: {}\nhash: {}", g.chain_id, g.hash().to_hex());
            println!("validadores ({}):", g.validators.len());
            for v in &g.validators {
                println!("  {}", Address::from_public_key(v));
            }
            println!(
                "emissores: {} (M = {})",
                g.issuers.len(),
                g.issuer_threshold
            );
        }
        "health" => {
            // usado pelo healthcheck do Docker: sai 0 se a API responde 200
            use std::io::{Read, Write};
            let api = a.one("--api").unwrap_or_else(|| "127.0.0.1:8080".into());
            let ok = std::net::TcpStream::connect(&api)
                .ok()
                .is_some_and(|mut s| {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
                    let mut out = String::new();
                    write!(
                        s,
                        "GET /status HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
                    )
                    .is_ok()
                        && s.read_to_string(&mut out).is_ok()
                        && out.starts_with("HTTP/1.1 200")
                });
            std::process::exit(i32::from(!ok));
        }
        "run" => run(&a),
        _ => die("comando desconhecido: keygen | pubkey | genesis | info | run"),
    }
}

fn show(k: &SecretKey) {
    println!(
        "pública: {}\nendereço: {}",
        k.public().to_hex(),
        Address::from_public_key(&k.public())
    );
}

fn run(a: &Args) {
    let genesis = read_genesis(Path::new(&a.need("--genesis")));
    let key = read_key(Path::new(&a.need("--key")));
    let mut cfg = Config::new(genesis, key, PathBuf::from(a.need("--data")));
    if let Some(l) = a.one("--listen") {
        cfg.listen = l
            .parse::<SocketAddr>()
            .unwrap_or_else(|_| die("--listen: IP:porta"));
    }
    cfg.api = a.one("--api").map(|v| {
        v.parse::<SocketAddr>()
            .unwrap_or_else(|_| die("--api: IP:porta"))
    });
    cfg.peers = a.many("--peer");
    cfg.empty_blocks = a.has("--empty-blocks");
    if let Some(d) = a.ms("--min-block-ms") {
        cfg.min_block_interval = d;
    }
    if let Some(d) = a.ms("--propose-ms") {
        cfg.timeouts.propose = d;
    }
    if let Some(d) = a.ms("--prevote-ms") {
        cfg.timeouts.prevote = d;
    }
    if let Some(d) = a.ms("--precommit-ms") {
        cfg.timeouts.precommit = d;
    }
    if let Some(n) = a.one("--max-pool-txs") {
        cfg.max_pool_txs = n.parse().unwrap_or_else(|_| die("--max-pool-txs: número"));
    }
    if let Some(n) = a.one("--max-pool-mib") {
        cfg.max_pool_bytes = n
            .parse::<usize>()
            .unwrap_or_else(|_| die("--max-pool-mib: número"))
            * 1024
            * 1024;
    }
    let log_every = a
        .one("--log-every-secs")
        .and_then(|v| v.parse::<u64>().ok());
    let node = Node::start(cfg).unwrap_or_else(|e| die(&format!("não subiu: {e}")));
    println!(
        "brdu-node no ar: pares em {}, API {}",
        node.listen_addr(),
        node.api_addr()
            .map_or("desligada".into(), |x| x.to_string())
    );
    loop {
        std::thread::sleep(Duration::from_secs(log_every.unwrap_or(3600)));
        if log_every.is_some() {
            println!(
                "altura {} pares {} raiz {}",
                node.height(),
                node.peer_count(),
                node.state_root()
            );
        }
    }
}
