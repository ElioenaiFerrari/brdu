//! `brdu-node`: the validator. **TEST** keys in a plain-text (hex) file: for test networks only.
//!
//! ```text
//! brdu-node keygen FILE                         creates a consensus key (0600) and prints the public key
//! brdu-node pubkey FILE                         prints the public key (hex) and the address
//! brdu-node genesis --chain ID --validator HEX... --issuer HEX... --threshold M --out FILE
//! brdu-node info --genesis FILE                 prints the genesis
//! brdu-node health [--api A]                    exits 0 if the API responds (Docker healthcheck)
//! brdu-node run --genesis FILE --key FILE --data DIR [--listen A] [--api A] [--peer A]... [options]
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
        .unwrap_or_else(|| die("the key must be 32 bytes in hex"));
    SecretKey::from_bytes(seed)
}

fn read_genesis(path: &Path) -> Genesis {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("{}: {e}", path.display())));
    let raw = hex::decode(text.trim()).unwrap_or_else(|| die("the genesis must be hex"));
    Genesis::decode(&raw).unwrap_or_else(|e| die(&format!("invalid genesis: {e}")))
}

fn pubkey(s: &str) -> PublicKey {
    PublicKey(
        hex::decode_array::<32>(s).unwrap_or_else(|| die(&format!("invalid public key: {s}"))),
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
            .unwrap_or_else(|| die(&format!("missing {flag}")))
    }
    fn ms(&self, flag: &str) -> Option<Duration> {
        self.one(flag).map(|v| {
            Duration::from_millis(
                v.parse()
                    .unwrap_or_else(|_| die(&format!("{flag}: expected a number of milliseconds"))),
            )
        })
    }
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = all.split_first() else {
        die("usage: keygen | pubkey | genesis | info | health | run")
    };
    let a = Args(rest.to_vec());
    match cmd.as_str() {
        "keygen" => {
            let path = PathBuf::from(rest.first().unwrap_or_else(|| die("keygen FILE")));
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed)
                .unwrap_or_else(|e| die(&format!("no randomness available: {e}")));
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
            rest.first().unwrap_or_else(|| die("pubkey FILE")),
        ))),
        "genesis" => {
            let validators: Vec<PublicKey> =
                a.many("--validator").iter().map(|s| pubkey(s)).collect();
            let issuers: Vec<PublicKey> = a.many("--issuer").iter().map(|s| pubkey(s)).collect();
            let threshold: u32 = a
                .need("--threshold")
                .parse()
                .unwrap_or_else(|_| die("--threshold: expected a number"));
            let g = Genesis::new(&a.need("--chain"), validators, issuers, threshold);
            if let Err(e) = g.rules() {
                die(&format!("invalid genesis: {e}"));
            }
            let out = a.need("--out");
            fs::write(&out, format!("{}\n", hex::encode(&g.encode())))
                .unwrap_or_else(|e| die(&format!("{out}: {e}")));
            println!("genesis {} written to {out}", g.hash().to_hex());
        }
        "info" => {
            let g = read_genesis(Path::new(&a.need("--genesis")));
            println!("network: {}\nhash: {}", g.chain_id, g.hash().to_hex());
            println!("validators ({}):", g.validators.len());
            for v in &g.validators {
                println!("  {}", Address::from_public_key(v));
            }
            println!("issuers: {} (M = {})", g.issuers.len(), g.issuer_threshold);
        }
        "health" => {
            // used by the Docker healthcheck: exits 0 if the API answers 200
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
        _ => die("unknown command: keygen | pubkey | genesis | info | health | run"),
    }
}

fn show(k: &SecretKey) {
    println!(
        "public: {}\naddress: {}",
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
            .unwrap_or_else(|_| die("--listen: expected IP:port"));
    }
    cfg.api = a.one("--api").map(|v| {
        v.parse::<SocketAddr>()
            .unwrap_or_else(|_| die("--api: expected IP:port"))
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
        cfg.max_pool_txs = n
            .parse()
            .unwrap_or_else(|_| die("--max-pool-txs: expected a number"));
    }
    if let Some(n) = a.one("--max-pool-mib") {
        cfg.max_pool_bytes = n
            .parse::<usize>()
            .unwrap_or_else(|_| die("--max-pool-mib: expected a number"))
            * 1024
            * 1024;
    }
    let log_every = a
        .one("--log-every-secs")
        .and_then(|v| v.parse::<u64>().ok());
    let node = Node::start(cfg).unwrap_or_else(|e| die(&format!("failed to start: {e}")));
    println!(
        "brdu-node up: peers on {}, API {}",
        node.listen_addr(),
        node.api_addr().map_or("disabled".into(), |x| x.to_string())
    );
    loop {
        std::thread::sleep(Duration::from_secs(log_every.unwrap_or(3600)));
        if log_every.is_some() {
            println!(
                "height {} peers {} root {}",
                node.height(),
                node.peer_count(),
                node.state_root()
            );
        }
    }
}
