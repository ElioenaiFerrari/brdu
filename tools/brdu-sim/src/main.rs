//! `brdu-sim equivocate --genesis ARQ --key ARQ --height H --target HOST:PORTA...` (só dev).

use std::fs;

use brdu_chain::Genesis;
use brdu_crypto::{SecretKey, hex};

fn die(m: &str) -> ! {
    eprintln!("brdu-sim: {m}");
    std::process::exit(2)
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.first().map(String::as_str) != Some("equivocate") {
        die("uso: brdu-sim equivocate --genesis ARQ --key ARQ --height H --target HOST:PORTA...");
    }
    let one = |f: &str| all.windows(2).find(|w| w[0] == f).map(|w| w[1].clone());
    let need = |f: &str| one(f).unwrap_or_else(|| die(&format!("falta {f}")));
    let g_text = fs::read_to_string(need("--genesis")).unwrap_or_else(|e| die(&e.to_string()));
    let genesis =
        Genesis::decode(&hex::decode(g_text.trim()).unwrap_or_else(|| die("gênese em hex")))
            .unwrap_or_else(|e| die(&e.to_string()));
    let k_text = fs::read_to_string(need("--key")).unwrap_or_else(|e| die(&e.to_string()));
    let key = SecretKey::from_bytes(
        hex::decode_array::<32>(k_text.trim()).unwrap_or_else(|| die("chave em hex")),
    );
    let height: u64 = need("--height")
        .parse()
        .unwrap_or_else(|_| die("--height: número"));
    let targets: Vec<String> = all
        .windows(2)
        .filter(|w| w[0] == "--target")
        .map(|w| w[1].clone())
        .collect();
    match brdu_sim::equivocate(&genesis, &key, &targets, height) {
        Ok(n) => println!(
            "votos conflitantes entregues a {n} de {} alvo(s)",
            targets.len()
        ),
        Err(e) => die(&e),
    }
}
