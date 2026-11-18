//! `brdu-explorer --genesis ARQ --node HOST:API [--listen 127.0.0.1:8545]`
//!
//! The genesis comes from a file that **you** pin (the node does not choose): without it the explorer would have no
//! way to reject a made-up network.

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use brdu_chain::Genesis;
use brdu_crypto::hex;
use brdu_explorer::{Explorer, serve, spawn_sync};

fn die(m: &str) -> ! {
    eprintln!("brdu-explorer: {m}");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let one = |f: &str| args.windows(2).find(|w| w[0] == f).map(|w| w[1].clone());
    let need = |f: &str| {
        one(f).unwrap_or_else(|| {
            die(&format!(
                "missing {f}\nusage: brdu-explorer --genesis FILE --node HOST:API [--listen IP:PORT]"
            ))
        })
    };
    let text = std::fs::read_to_string(need("--genesis")).unwrap_or_else(|e| die(&e.to_string()));
    let genesis = Genesis::decode(
        &hex::decode(text.trim()).unwrap_or_else(|| die("the genesis must be hex")),
    )
    .unwrap_or_else(|e| die(&format!("invalid genesis: {e}")));
    let node = need("--node");
    let listen: SocketAddr = one("--listen")
        .unwrap_or_else(|| "127.0.0.1:8545".into())
        .parse()
        .unwrap_or_else(|_| die("--listen: IP:PORT"));
    let explorer = Explorer::new(genesis, &node).unwrap_or_else(|e| die(&e));
    let shared = Arc::new(RwLock::new(explorer));
    let stop = Arc::new(AtomicBool::new(false));
    let _sync = spawn_sync(shared.clone(), stop.clone());
    let (bound, handles) = serve(listen, shared, stop)
        .unwrap_or_else(|e| die(&format!("could not open {listen}: {e}")));
    println!("explorer at http://{bound} (node {node}); read-only, re-verifies every block");
    for h in handles {
        let _ = h.join();
    }
}
