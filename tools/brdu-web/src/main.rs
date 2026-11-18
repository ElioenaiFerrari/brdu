//! `brdu-web --vault FILE --node HOST:API [--explorer HOST:PORT] [--listen 127.0.0.1:8546] [--allow-remote]`
//!
//! TEST KEYS ONLY: the keys live inside this process and the browser is not a hardware wallet.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use brdu_walletd::{Config, Wallet};
use brdu_web::{Options, serve};

fn die(m: &str) -> ! {
    eprintln!("brdu-web: {m}");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let one = |f: &str| args.windows(2).find(|w| w[0] == f).map(|w| w[1].clone());
    let need = |f: &str| {
        one(f).unwrap_or_else(|| die(&format!("missing {f}\nusage: brdu-web --vault FILE --node HOST:API [--explorer HOST:PORT] [--listen IP:PORT] [--allow-remote]")))
    };
    let remote = args.iter().any(|a| a == "--allow-remote");
    let listen: SocketAddr = one("--listen")
        .unwrap_or_else(|| "127.0.0.1:8546".into())
        .parse()
        .unwrap_or_else(|_| die("--listen: IP:PORT"));
    if !listen.ip().is_loopback() && !remote {
        die(
            "refusing to listen on a non-loopback address without --allow-remote (the keys live in this process; test keys only)",
        );
    }
    let mut cfg = Config::new(
        PathBuf::from(need("--vault")),
        &need("--node"),
        one("--explorer").as_deref(),
    );
    if let Some(m) = one("--lock-after-secs").and_then(|v| v.parse().ok()) {
        cfg.lock_after = std::time::Duration::from_secs(m);
    }
    let wallet = Arc::new(Wallet::new(cfg));
    let stop = Arc::new(AtomicBool::new(false));
    let (bound, handles) = serve(
        listen,
        wallet,
        Options {
            allow_any_host: remote,
        },
        stop,
    )
    .unwrap_or_else(|e| die(&format!("could not open {listen}: {e}")));
    println!("wallet at http://{bound}   TEST KEYS ONLY: the keys live in this process");
    for h in handles {
        let _ = h.join();
    }
}
