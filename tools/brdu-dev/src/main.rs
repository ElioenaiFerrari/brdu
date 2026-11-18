//! `cargo dev …`: the BRDU development CLI. **Dev only**: TEST keys and phrases in plain text.

mod asset;
mod clean;
mod compose;
mod contract;
mod cred;
mod docs_check;
mod http;
mod load;
mod net;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use brdu_crypto::{Address, Amount, SecretKey, hex};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Tx, kind};
use brdu_wallet::Phrase;
use net::Layout;
use serde_json::Value;

const HELP: &str = "cargo dev <command>

local network (processes)
  init [--dir net] [--nodes 4] [--spare 0] [--issuers 3] [--threshold 2] [--chain brdu-devnet] [--port-base 7100] [--min-window 60]   creates keys and genesis
  up [--dir net]                      starts the validators (logs in net/logs)
  down [--dir net]                    stops the validators
  explorer [stop] [--listen 127.0.0.1:8545] [--node 1]   opens the web explorer (re-verifies every block)
  contract <command>                  contracts between parties (templates, create, act, list, show); see `cargo dev contract`
  asset <command>                     assets (classes, class, register, offer, accept, veto, finalize, deals, liens); see `cargo dev asset`
  cred <command>                      credentials (types, issue, contest, veto, activate, revoke, list); see `cargo dev cred`
  wallet-web [stop] [--listen 127.0.0.1:8546]   opens the wallet in the browser (TEST keys; vault in <dir>/wallet)
  stop N                              stops node N (simulated failure; frees its key)
  start N                             restarts node N (reads the log and syncs)
  reset [--dir net]                   stops and deletes the network (only the given folder)
  clean-all [--yes] [--tmp] [--image] [--cargo]   removes EVERYTHING test-related (net, compose lab and its Docker volumes, wallets, data, blocks);
                                      without --yes it only lists what it would free. --tmp test leftovers, --image the Docker image, --cargo `cargo clean`
  status [--dir net]                  height, peers, mempool, root of each node

Docker lab (single image, CPU and memory limits; use --dir compose or BRDU_DIR=compose on the other commands)
  compose init [--dir compose] [--nodes 4] [--spare 0] [--cpus 0.5] [--mem 256] [--min-block-ms 200]
  compose up | down | ps | logs [N] | pause N | unpause N | kill N | start N | stop N | restart N | stats | clean

test wallet (phrase in net/wallets, plain text: DEV ONLY)
  wallet new NAME                     creates a wallet and shows the address
  wallet list                         lists the wallets
  balance NAME|ADDRESS                balance and nonce
  send --from NAME --to NAME|ADDRESS --amount 1,50 [--count N] [--node I]   (amounts use the Brazilian format: decimal comma, `.` only as thousands separator)
  fund NAME|ADDRESS AMOUNT            test faucet: mint signed by the test issuers

issuance and reserve (test issuers)
  mint --to NAME|ADDRESS --amount 100 --ref dep-1
  burn --from NAME --amount 10 --ref withdrawal-1
  reserve --amount 1000 [--ref statement-1]      reserve attestation
  supply                              minted, burned, supply, reserve, coverage

simulators (dev ONLY; work on the local network and in the lab, with --dir)
  equivocate --as N                   validator N (stopped first!) sends 2 conflicting votes: the network must expose the evidence
  rotate [--in N] [--out M]           validator swap by more than 2/3 (N = spare created with `init --spare`)
  blocks-sim [--count 5]              N spaced-out transfers to force N small blocks

load test (Docker lab; measures CPU and memory per cgroup)
  load [--dir compose] [--rates 50,100,200,400,800] [--secs 20] [--accounts 8] [--warmup 10] [--repeats 1]

quality
  docs-check [--root .]               checks links, images, crates, ADRs, flows, titles and secrets in the docs
  check                               fmt + clippy + tests + docs-check (what CI runs)

query
  blocks [--from N] [--limit N]       latest blocks
  tx ID                               a transaction
";

fn die(msg: &str) -> ! {
    eprintln!("brdu-dev: {msg}");
    std::process::exit(1)
}

pub(crate) struct Args(pub(crate) Vec<String>);

impl Args {
    pub(crate) fn one(&self, flag: &str) -> Option<String> {
        self.0
            .iter()
            .position(|a| a == flag)
            .and_then(|i| self.0.get(i + 1).cloned())
    }
    pub(crate) fn need(&self, flag: &str) -> String {
        self.one(flag)
            .unwrap_or_else(|| die(&format!("missing {flag}")))
    }
    pub(crate) fn dir(&self) -> PathBuf {
        PathBuf::from(
            self.one("--dir")
                .or_else(|| std::env::var("BRDU_DIR").ok())
                .unwrap_or_else(|| "net".into()),
        )
    }
    pub(crate) fn num<T: std::str::FromStr>(&self, flag: &str, default: T) -> T {
        self.one(flag).map_or(default, |v| {
            v.parse()
                .unwrap_or_else(|_| die(&format!("{flag}: invalid number")))
        })
    }
    /// Positionals (whatever is neither a flag nor a flag's value).
    pub(crate) fn positional(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut skip = false;
        for a in &self.0 {
            if skip {
                skip = false;
            } else if a.starts_with("--") {
                skip = true;
            } else {
                out.push(a.clone());
            }
        }
        out
    }
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = all.split_first() else {
        print!("{HELP}");
        return;
    };
    let a = Args(rest.to_vec());
    let r = match cmd.as_str() {
        "init" => cmd_init(&a),
        "up" => cmd_up(&a),
        "down" => cmd_down(&a.dir()),
        "stop" => cmd_stop(&a),
        "explorer" => cmd_explorer(&a),
        "cred" => cred::run(&a),
        "asset" => asset::run(&a),
        "contract" => contract::run(&a),
        "wallet-web" => cmd_wallet_web(&a),
        "start" => cmd_start(&a),
        "equivocate" => cmd_equivocate(&a),
        "rotate" => cmd_rotate(&a),
        "blocks-sim" => cmd_blocks_sim(&a),
        "reset" => cmd_reset(&a.dir()),
        "clean-all" => cmd_clean_all(&a),
        "status" => cmd_status(&a),
        "wallet" => cmd_wallet(&a),
        "balance" => cmd_balance(&a),
        "send" => cmd_send(&a),
        "fund" => cmd_fund(&a),
        "mint" => cmd_mint(&a),
        "burn" => cmd_burn(&a),
        "reserve" => cmd_reserve(&a),
        "supply" => cmd_supply(&a),
        "blocks" => cmd_blocks(&a),
        "tx" => cmd_tx(&a),
        "compose" => cmd_compose(&a),
        "docs-check" => cmd_docs_check(&a),
        "check" => cmd_check(),
        "load" => cmd_load(&a),
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("unknown command: {other}\n\n{HELP}")),
    };
    if let Err(e) = r {
        die(&e);
    }
}

// ---------------------------------------------------------------------------- local network

fn cmd_init(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let nodes: usize = a.num("--nodes", 4);
    let issuers: usize = a.num("--issuers", 3);
    let threshold: u32 = a.num("--threshold", 2);
    if nodes == 0 || nodes > 32 {
        return Err("--nodes must be between 1 and 32".into());
    }
    let spare: usize = a.num("--spare", 0);
    let base: usize = a.num("--port-base", 7100);
    let chain = a.one("--chain").unwrap_or_else(|| "brdu-devnet".into());
    net::init(
        &dir,
        &net::Spec {
            chain: &chain,
            nodes,
            spare,
            issuers,
            threshold,
            min_window: a.num("--min-window", 60),
        },
        &|i| format!("127.0.0.1:{}", base + i),
        &|i| format!("127.0.0.1:{}", base + i),
        &|i| format!("127.0.0.1:{}", base + 100 + i),
    )?;
    println!(
        "test network created in {} ({nodes} validators + {spare} spare(s), {issuers} issuers, M = {threshold})",
        dir.display()
    );
    println!("next: cargo dev up");
    Ok(())
}

/// Always runs an incremental `cargo build -p PACKAGE` (cheap when nothing changed) so a stale binary never runs
/// after the code was edited, then returns the path of the binary next to this executable.
fn fresh_binary(package: &str, bin: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe.with_file_name(bin);
    let built = Command::new("cargo")
        .args(["build", "-q", "-p", package])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !built && !path.exists() {
        return Err(format!("could not build {package}"));
    }
    if !built {
        eprintln!("warning: `cargo build -p {package}` failed; using the existing binary");
    }
    Ok(path)
}

fn node_binary() -> Result<PathBuf, String> {
    fresh_binary("brdu-nodebin", "brdu-node")
}

/// Starts the process of node `idx` (1-based). `all_others`: dials every other node (rejoin); otherwise only the earlier ones.
fn spawn_node(
    dir: &Path,
    l: &Layout,
    bin: &Path,
    idx: usize,
    all_others: bool,
) -> Result<u32, String> {
    let nodes = l.info["nodes"].as_array().cloned().unwrap_or_default();
    let n = nodes
        .get(idx - 1)
        .ok_or_else(|| format!("node {idx} does not exist"))?;
    let mut cmd = Command::new(bin);
    cmd.arg("run")
        .args(["--genesis", &dir.join("genesis.hex").to_string_lossy()])
        .args([
            "--key",
            &dir.join(format!("keys/v{idx}.key")).to_string_lossy(),
        ])
        .args([
            "--data",
            &dir.join(format!("data/v{idx}")).to_string_lossy(),
        ])
        .args([
            "--listen",
            n["p2p"].as_str().unwrap_or_default(),
            "--api",
            n["api"].as_str().unwrap_or_default(),
        ])
        .args(["--min-block-ms", "200"]);
    for (j, other) in nodes.iter().enumerate() {
        if (all_others && j + 1 != idx) || (!all_others && j + 1 < idx) {
            cmd.args(["--peer", other["p2p"].as_str().unwrap_or_default()]);
        }
    }
    fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
    let log = fs::File::create(dir.join(format!("logs/v{idx}.log"))).map_err(|e| e.to_string())?;
    let child = cmd
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(child.id())
}

fn read_pids(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("pids"))
        .map(|t| t.lines().map(String::from).collect())
        .unwrap_or_default()
}

fn write_pids(dir: &Path, pids: &[String]) -> Result<(), String> {
    fs::write(dir.join("pids"), pids.join("\n") + "\n").map_err(|e| e.to_string())
}

fn cmd_up(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let bin = node_binary()?;
    let count = l.info["nodes"].as_array().map_or(0, Vec::len);
    let mut pids = Vec::new();
    for idx in 1..=count {
        pids.push(spawn_node(&dir, &l, &bin, idx, false)?.to_string());
    }
    write_pids(&dir, &pids)?;
    println!(
        "{count} nodes up ({} validators + spares). APIs: {}",
        l.info["genesis_nodes"],
        l.apis().join(", ")
    );
    println!("next: cargo dev status");
    Ok(())
}

/// `stop N`: stops node N (to simulate a failure, or to free its key for the simulator).
fn cmd_stop(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let n: usize = a
        .positional()
        .first()
        .and_then(|v| v.parse().ok())
        .ok_or("stop NODE_NUMBER")?;
    let mut pids = read_pids(&dir);
    let pid = pids.get_mut(n.saturating_sub(1)).ok_or("unknown node")?;
    if pid.chars().all(|c| c.is_ascii_digit()) && !pid.is_empty() && pid != "0" {
        let _ = Command::new("kill").arg(pid.as_str()).status();
        *pid = "-".into();
        write_pids(&dir, &pids)?;
        println!("node {n} stopped");
    } else {
        println!("node {n} was already stopped");
    }
    Ok(())
}

/// `start N`: starts node N again (same data folder: reads the log and syncs).
fn cmd_start(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let n: usize = a
        .positional()
        .first()
        .and_then(|v| v.parse().ok())
        .ok_or("start NODE_NUMBER")?;
    let mut pids = read_pids(&dir);
    if pids.get(n.saturating_sub(1)).is_none() {
        return Err("unknown node (run `cargo dev up`)".into());
    }
    let pid = spawn_node(&dir, &l, &node_binary()?, n, true)?;
    pids[n - 1] = pid.to_string();
    write_pids(&dir, &pids)?;
    println!("node {n} up (pid {pid})");
    Ok(())
}

fn cmd_down(dir: &Path) -> Result<(), String> {
    let p = dir.join("pids");
    let Ok(text) = fs::read_to_string(&p) else {
        // no validators recorded, but an explorer or the wallet daemon may still be running from this folder
        stop_explorer(dir);
        stop_wallet_web(dir);
        println!("no validators running in {}", dir.display());
        return Ok(());
    };
    let mut n = 0;
    for pid in text
        .lines()
        .filter(|l| l.chars().all(|c| c.is_ascii_digit()) && !l.is_empty())
    {
        if Command::new("kill")
            .arg(pid)
            .status()
            .is_ok_and(|s| s.success())
        {
            n += 1;
        }
    }
    let _ = fs::remove_file(&p);
    stop_explorer(dir);
    stop_wallet_web(dir);
    println!("{n} validators stopped");
    Ok(())
}

fn cmd_clean_all(a: &Args) -> Result<(), String> {
    let flag = |f: &str| a.0.iter().any(|x| x == f);
    let opts = clean::Options {
        yes: flag("--yes"),
        tmp: flag("--tmp"),
        image: flag("--image"),
        cargo: flag("--cargo"),
        docker: true,
    };
    clean::run(Path::new("."), &std::env::temp_dir(), &opts, &|dir| {
        let _ = cmd_down(dir);
    })
}

fn cmd_reset(dir: &Path) -> Result<(), String> {
    cmd_down(dir)?;
    // only deletes what THIS command creates inside the given folder
    for sub in ["keys", "wallets", "wallet", "data", "logs"] {
        let _ = fs::remove_dir_all(dir.join(sub));
    }
    for f in [
        "devnet.json",
        "genesis.hex",
        "pids",
        "explorer.pid",
        "wallet-web.pid",
    ] {
        let _ = fs::remove_file(dir.join(f));
    }
    let _ = fs::remove_dir(dir);
    println!("test network in {} deleted", dir.display());
    Ok(())
}

fn cmd_status(a: &Args) -> Result<(), String> {
    let l = Layout::load(&a.dir())?;
    println!(
        "network {} (genesis {})",
        l.chain_id(),
        l.info["genesis_hash"]
            .as_str()
            .unwrap_or("")
            .get(..12)
            .unwrap_or("")
    );
    for (i, api) in l.apis().iter().enumerate() {
        match http::get(api, "/status") {
            Ok(s) => println!(
                "  v{}  height {:<5} peers {}  mempool {:<4} consensus {}  root {}  evidence {}",
                i + 1,
                s["height"],
                s["peers"],
                s["mempool"],
                if s["consensus_running"] == true {
                    "running"
                } else {
                    "sleeping"
                },
                s["state_root"]
                    .as_str()
                    .unwrap_or("")
                    .get(..12)
                    .unwrap_or(""),
                s["evidence"]
            ),
            Err(e) => println!("  v{}  DOWN ({e})", i + 1),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------- wallets

fn wallet_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("wallets/{name}.mnemonic"))
}

pub(crate) fn load_account(dir: &Path, name: &str) -> Result<(SecretKey, Address), String> {
    let text = fs::read_to_string(wallet_path(dir, name))
        .map_err(|_| format!("wallet `{name}` does not exist (cargo dev wallet new {name})"))?;
    let wallet = Phrase::parse(text.trim())
        .and_then(|p| p.wallet(""))
        .map_err(|e| e.to_string())?;
    let acc = wallet.account(0, 0).map_err(|e| e.to_string())?;
    Ok((
        SecretKey::from_bytes(*acc.secret_key().expose_seed()),
        *acc.address(),
    ))
}

/// A wallet `NAME` or `brdu1…`.
pub(crate) fn resolve(dir: &Path, who: &str) -> Result<Address, String> {
    if who.starts_with("brdu1") {
        return Address::decode(who).map_err(|e| e.to_string());
    }
    load_account(dir, who).map(|(_, a)| a)
}

fn cmd_wallet(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let pos = a.positional();
    match pos.first().map(String::as_str) {
        Some("new") => {
            let name = pos.get(1).ok_or("wallet new NAME")?;
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err("name: letters, digits, - and _".into());
            }
            fs::create_dir_all(dir.join("wallets")).map_err(|e| e.to_string())?;
            let phrase = Phrase::generate().map_err(|e| e.to_string())?;
            net::write_secret(&wallet_path(&dir, name), &phrase.words().join(" "))?;
            let (_, addr) = load_account(&dir, name)?;
            println!(
                "wallet `{name}` created (TEST phrase in {})\naddress: {addr}",
                wallet_path(&dir, name).display()
            );
            Ok(())
        }
        Some("list") => {
            let mut names: Vec<String> = fs::read_dir(dir.join("wallets"))
                .map_err(|_| "no wallets".to_string())?
                .filter_map(|e| {
                    e.ok()?
                        .file_name()
                        .to_str()?
                        .strip_suffix(".mnemonic")
                        .map(String::from)
                })
                .collect();
            names.sort();
            for n in names {
                let (_, addr) = load_account(&dir, &n)?;
                println!("{n:<12} {addr}");
            }
            Ok(())
        }
        _ => Err("wallet new NAME | wallet list".into()),
    }
}

pub(crate) fn first_api(l: &Layout, a: &Args) -> Result<String, String> {
    let apis = l.apis();
    let i: usize = a.num("--node", 1);
    apis.get(i.saturating_sub(1))
        .cloned()
        .ok_or_else(|| format!("node {i} does not exist"))
}

pub(crate) fn account(api: &str, addr: &Address) -> Result<(u64, u64), String> {
    let v = http::get(api, &format!("/account/{addr}"))?;
    Ok((
        v["balance"].as_u64().unwrap_or(0),
        v["nonce"].as_u64().unwrap_or(0),
    ))
}

fn show_amount(units: u64) -> String {
    format!("{} BRDU", Amount(units).to_full())
}

fn cmd_balance(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let who = a
        .positional()
        .first()
        .cloned()
        .ok_or("balance NAME|ADDRESS")?;
    let addr = resolve(&dir, &who)?;
    let (bal, nonce) = account(&first_api(&l, a)?, &addr)?;
    println!("{addr}\nbalance {}  (nonce {nonce})", show_amount(bal));
    Ok(())
}

pub(crate) fn parse_amount(s: &str) -> Result<u64, String> {
    Amount::parse(s)
        .map(Amount::units)
        .map_err(|e| format!("amount `{s}`: {e}"))
}

/// Minimum fee: base + per-byte × size (the fee has a fixed width, so the size does not change).
fn min_fee(api: &str, probe: &Tx) -> Result<u64, String> {
    let f = http::get(api, "/fees")?;
    Ok(f["transfer_base"].as_u64().unwrap_or(1000)
        + f["per_byte"].as_u64().unwrap_or(1) * probe.size() as u64)
}

pub(crate) fn submit(api: &str, tx: &Tx) -> Result<String, String> {
    let (code, v) = http::request(
        api,
        "POST",
        "/tx",
        &hex::encode(&tx.encode().map_err(|e| e.to_string())?),
    )?;
    if code == 200 {
        Ok(v["id"].as_str().unwrap_or("").to_string())
    } else {
        Err(v["error"].as_str().unwrap_or("rejected").to_string())
    }
}

fn cmd_send(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let (key, from) = load_account(&dir, &a.need("--from"))?;
    let to = resolve(&dir, &a.need("--to"))?;
    let amount = parse_amount(&a.need("--amount"))?;
    let count: u64 = a.num("--count", 1);
    let (_, mut nonce) = account(&api, &from)?;
    let chain = l.chain_id();
    let probe = Tx::new_signed(
        &key,
        &chain,
        kind::TRANSFER,
        nonce,
        0,
        coin::transfer_payload(&to, amount),
    )
    .map_err(|e| e.to_string())?;
    let fee = min_fee(&api, &probe)?;
    let mut last = String::new();
    for _ in 0..count {
        let tx = Tx::new_signed(
            &key,
            &chain,
            kind::TRANSFER,
            nonce,
            fee,
            coin::transfer_payload(&to, amount),
        )
        .map_err(|e| e.to_string())?;
        last = submit(&api, &tx)?;
        nonce += 1;
    }
    println!(
        "{count} transfer(s) of {} sent; fee {} each; last: {last}",
        show_amount(amount),
        show_amount(fee)
    );
    Ok(())
}

// ---------------------------------------------------------------------------- issuance

fn relayer_nonce(l: &Layout, api: &str) -> Result<(SecretKey, u64), String> {
    let k = l.relayer()?;
    let (_, n) = account(api, &Address::from_public_key(&k.public()))?;
    Ok((k, n))
}

pub(crate) fn approvals(l: &Layout, doc: &brdu_crypto::Hash32) -> Result<Vec<Approval>, String> {
    let m = l.info["threshold"].as_u64().unwrap_or(1) as usize;
    Ok(l.issuers()?
        .iter()
        .take(m)
        .map(|k| Approval::new(doc, k))
        .collect())
}

fn issue(a: &Args, burn: bool, who_flag: &str) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let who = a.need(who_flag);
    let amount = parse_amount(&a.need("--amount"))?;
    let reference = a.need("--ref");
    let chain = l.chain_id();
    let (target, signer): (Address, Option<SecretKey>) = if burn {
        let (k, addr) = load_account(&dir, &who)?;
        (addr, Some(k))
    } else {
        (resolve(&dir, &who)?, None)
    };
    let doc = coin::issuance_doc(burn, &chain, &target, amount, &reference);
    let payload = coin::issuance_payload(&target, amount, &reference, &approvals(&l, &doc)?);
    let (sender, nonce, tx_kind) = match signer {
        Some(k) => {
            let (_, n) = account(&api, &target)?;
            (k, n, kind::BURN)
        }
        None => {
            let (k, n) = relayer_nonce(&l, &api)?;
            (k, n, kind::MINT)
        }
    };
    let tx =
        Tx::new_signed(&sender, &chain, tx_kind, nonce, 0, payload).map_err(|e| e.to_string())?;
    let id = submit(&api, &tx)?;
    println!(
        "{} of {} to {target} sent: {id}",
        if burn { "burn" } else { "mint" },
        show_amount(amount)
    );
    Ok(())
}

fn cmd_mint(a: &Args) -> Result<(), String> {
    issue(a, false, "--to")
}

fn cmd_burn(a: &Args) -> Result<(), String> {
    issue(a, true, "--from")
}

fn cmd_fund(a: &Args) -> Result<(), String> {
    let pos = a.positional();
    let (who, amount) = (
        pos.first().ok_or("fund NAME AMOUNT")?,
        pos.get(1).ok_or("fund NAME AMOUNT")?,
    );
    let mut stamp = [0u8; 4];
    getrandom::fill(&mut stamp).map_err(|e| e.to_string())?;
    let mut args = vec![
        "--to".into(),
        who.clone(),
        "--amount".into(),
        amount.clone(),
        "--ref".into(),
        format!("faucet-{}", hex::encode(&stamp)),
    ];
    args.extend(
        a.0.iter()
            .filter(|x| x.starts_with("--dir") || x.starts_with("--node"))
            .cloned(),
    );
    // reuses --dir/--node with their values
    let mut full = args;
    for flag in ["--dir", "--node"] {
        if let Some(v) = a.one(flag) {
            full.retain(|x| x != flag && x != &v);
            full.push(flag.into());
            full.push(v);
        }
    }
    issue(&Args(full), false, "--to")
}

fn cmd_reserve(a: &Args) -> Result<(), String> {
    let l = Layout::load(&a.dir())?;
    let api = first_api(&l, a)?;
    let reserve = parse_amount(&a.need("--amount"))?;
    let reference = a.one("--ref").unwrap_or_else(|| "statement".into());
    let sup = http::get(&api, "/supply")?;
    let seq = sup["attestation_seq"].as_u64().unwrap_or(0) + 1;
    let chain = l.chain_id();
    let doc = coin::attestation_doc(&chain, seq, reserve, &reference);
    let payload = coin::attestation_payload(seq, reserve, &reference, &approvals(&l, &doc)?);
    let (k, nonce) = relayer_nonce(&l, &api)?;
    let tx = Tx::new_signed(&k, &chain, kind::ATTESTATION, nonce, 0, payload)
        .map_err(|e| e.to_string())?;
    let id = submit(&api, &tx)?;
    println!(
        "attestation #{seq} of reserve {} sent: {id}",
        show_amount(reserve)
    );
    Ok(())
}

fn cmd_supply(a: &Args) -> Result<(), String> {
    let l = Layout::load(&a.dir())?;
    let s = http::get(&first_api(&l, a)?, "/supply")?;
    let u = |k: &str| s[k].as_u64().unwrap_or(0);
    println!(
        "minted   {}\nburned   {}\nsupply   {}",
        show_amount(u("minted")),
        show_amount(u("burned")),
        show_amount(u("supply"))
    );
    match s["reserve"].as_u64() {
        Some(r) => println!(
            "reserve  {} (attestation #{})",
            show_amount(r),
            s["attestation_seq"]
        ),
        None => println!("reserve  (no attestation yet)"),
    }
    match s["coverage"]["state"].as_str() {
        Some("covered") => println!(
            "coverage: covered, surplus {}",
            show_amount(s["coverage"]["surplus"].as_u64().unwrap_or(0))
        ),
        Some("under") => println!(
            "coverage: UNDER, shortfall {} (information only; the network freezes nothing)",
            show_amount(s["coverage"]["shortfall"].as_u64().unwrap_or(0))
        ),
        _ => println!("coverage: no attestation"),
    }
    Ok(())
}

fn cmd_blocks(a: &Args) -> Result<(), String> {
    let l = Layout::load(&a.dir())?;
    let api = first_api(&l, a)?;
    let status = http::get(&api, "/status")?;
    let height = status["height"].as_u64().unwrap_or(0);
    let limit: u64 = a.num("--limit", 10);
    let from: u64 = a.num(
        "--from",
        height.saturating_sub(limit.saturating_sub(1)).max(1),
    );
    let v = http::get(&api, &format!("/blocks?from={from}&limit={limit}"))?;
    for b in v["blocks"].as_array().unwrap_or(&Vec::new()) {
        let who = |x: &Value| x.as_str().unwrap_or("").get(..10).unwrap_or("").to_string();
        println!(
            "#{:<5} {} txs {:<4} round {} proposer {}… signed by {}/{}",
            b["height"],
            b["hash"].as_str().unwrap_or("").get(..10).unwrap_or(""),
            b["tx_count"],
            b["round"],
            who(&b["proposer"]),
            b["signed_by"].as_array().map_or(0, Vec::len),
            status["validators"].as_array().map_or(0, Vec::len)
        );
    }
    Ok(())
}

fn cmd_tx(a: &Args) -> Result<(), String> {
    let l = Layout::load(&a.dir())?;
    let id = a.positional().first().cloned().ok_or("tx ID")?;
    let v = http::get(&first_api(&l, a)?, &format!("/tx/{id}"))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn cmd_compose(a: &Args) -> Result<(), String> {
    let pos = a.positional();
    let sub = pos.first().ok_or("compose init | up | down | …")?;
    let dir = PathBuf::from(a.one("--dir").unwrap_or_else(|| "compose".into()));
    if sub == "init" {
        return compose::init(
            &dir,
            a.num("--nodes", 4),
            a.num("--spare", 0),
            &a.one("--cpus").unwrap_or_else(|| "0.5".into()),
            a.num("--mem", 256),
            a.num("--min-block-ms", 200),
        );
    }
    // one command is enough: `compose up` creates the lab with the defaults when there is none yet
    if sub == "up" && !dir.join("docker-compose.yml").exists() {
        println!(
            "no lab in {}: creating one with the defaults (4 validators, 0.5 CPU and 256 MiB each)",
            dir.display()
        );
        compose::init(&dir, 4, 0, "0.5", 256, 200)?;
    }
    compose::run(&dir, sub, &pos[1..])
}

fn cmd_load(a: &Args) -> Result<(), String> {
    let dir = PathBuf::from(a.one("--dir").unwrap_or_else(|| "compose".into()));
    let l = Layout::load(&dir)?;
    let rates: Vec<f64> = a
        .one("--rates")
        .unwrap_or_else(|| "50,100,200,400,800".into())
        .split(',')
        .map(|r| {
            r.trim()
                .parse()
                .unwrap_or_else(|_| die("--rates: comma-separated numbers"))
        })
        .collect();
    // only loads nodes that are in the active validator set (observers and removed nodes are left out)
    let active: Vec<usize> = l
        .apis()
        .iter()
        .enumerate()
        .filter(|(_, api)| http::get(api, "/validators").is_ok_and(|v| v["me_in_set"] == true))
        .map(|(i, _)| i)
        .collect();
    if active.is_empty() {
        return Err("no validator node answered (is the network up?)".into());
    }
    load::run(&load::Params {
        dir,
        rates,
        secs: a.num("--secs", 20),
        accounts: a.num("--accounts", 8),
        warmup_secs: a.num("--warmup", 10),
        repeats: a.num("--repeats", 1),
        containers: active
            .iter()
            .map(|i| format!("brdu-lab-v{}-1", i + 1))
            .collect(),
        apis: active.iter().map(|i| l.apis()[*i].clone()).collect(),
    })
}

// ---------------------------------------------------------------------------- simulators (dev only)

fn ensure_wallet(dir: &Path, name: &str) -> Result<Address, String> {
    if !wallet_path(dir, name).exists() {
        fs::create_dir_all(dir.join("wallets")).map_err(|e| e.to_string())?;
        let phrase = Phrase::generate().map_err(|e| e.to_string())?;
        net::write_secret(&wallet_path(dir, name), &phrase.words().join(" "))?;
    }
    load_account(dir, name).map(|(_, a)| a)
}

/// The simulator's wallets, funded for the transfers.
fn sim_wallets(dir: &Path, l: &Layout, api: &str) -> Result<(), String> {
    let sim = ensure_wallet(dir, "sim")?;
    ensure_wallet(dir, "sim-sink")?;
    if account(api, &sim)?.0 < 2_000_000 {
        let mut stamp = [0u8; 4];
        getrandom::fill(&mut stamp).map_err(|e| e.to_string())?;
        let dir_s = dir.to_string_lossy().to_string();
        issue(
            &Args(vec![
                "--to".into(),
                "sim".into(),
                "--amount".into(),
                "1000".into(),
                "--ref".into(),
                format!("sim-{}", hex::encode(&stamp)),
                "--dir".into(),
                dir_s,
            ]),
            false,
            "--to",
        )?;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while account(api, &sim)?.0 < 2_000_000 {
            if std::time::Instant::now() > end {
                return Err("the simulator balance did not arrive (is the network up?)".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    let _ = l;
    Ok(())
}

/// A minimal simulator transfer.
fn sim_send(dir: &Path, l: &Layout, api: &str) -> Result<(), String> {
    let (key, from) = load_account(dir, "sim")?;
    let to = resolve(dir, "sim-sink")?;
    let (_, nonce) = account(api, &from)?;
    let chain = l.chain_id();
    let probe = Tx::new_signed(
        &key,
        &chain,
        kind::TRANSFER,
        nonce,
        0,
        coin::transfer_payload(&to, 1),
    )
    .map_err(|e| e.to_string())?;
    let fee = min_fee(api, &probe)?;
    let tx = Tx::new_signed(
        &key,
        &chain,
        kind::TRANSFER,
        nonce,
        fee,
        coin::transfer_payload(&to, 1),
    )
    .map_err(|e| e.to_string())?;
    submit(api, &tx).map(|_| ())
}

fn height_of(api: &str) -> Result<u64, String> {
    Ok(http::get(api, "/status")?["height"].as_u64().unwrap_or(0))
}

fn cmd_blocks_sim(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let count: u64 = a.num("--count", 5);
    sim_wallets(&dir, &l, &api)?;
    for i in 1..=count {
        let before = height_of(&api)?;
        sim_send(&dir, &l, &api)?;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while height_of(&api)? <= before {
            if std::time::Instant::now() > end {
                return Err(format!("block {i} was not produced within 20 s"));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        println!("block {} produced ({i}/{count})", before + 1);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(())
}

fn sim_binary() -> Result<PathBuf, String> {
    fresh_binary("brdu-sim", "brdu-sim")
}

fn cmd_equivocate(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let genesis_nodes = l.info["genesis_nodes"].as_u64().unwrap_or(0) as usize;
    let who: usize = a.num("--as", genesis_nodes);
    let nodes = l.info["nodes"].as_array().cloned().unwrap_or_default();
    let apis = l.apis();
    if who == 0 || who > genesis_nodes {
        return Err(format!("--as: a genesis validator (1 to {genesis_nodes})"));
    }
    if http::get(&apis[who - 1], "/status").is_ok() {
        return Err(format!(
            "node {who} is still up: stop it first (cargo dev stop {who}  or  cargo dev compose kill {who})"
        ));
    }
    let (live_api, height) = apis
        .iter()
        .enumerate()
        .filter(|(i, _)| i + 1 != who)
        .find_map(|(_, api)| height_of(api).ok().map(|h| (api.clone(), h)))
        .ok_or("no other node answers")?;
    let _ = live_api;
    let mut cmd = Command::new(sim_binary()?);
    cmd.args([
        "equivocate",
        "--genesis",
        &dir.join("genesis.hex").to_string_lossy(),
        "--key",
        &dir.join(format!("keys/v{who}.key")).to_string_lossy(),
    ])
    .args(["--height", &(height + 1).to_string()]);
    for (i, n) in nodes.iter().take(genesis_nodes).enumerate() {
        if i + 1 != who {
            cmd.args(["--target", n["p2p_host"].as_str().unwrap_or_default()]);
        }
    }
    if !cmd.status().map_err(|e| e.to_string())?.success() {
        return Err("the simulator failed".into());
    }
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let mut detected = 0;
    for (i, api) in apis.iter().take(genesis_nodes).enumerate() {
        if i + 1 == who {
            continue;
        }
        let n =
            http::get(api, "/evidence").map_or(0, |v| v["evidence"].as_array().map_or(0, Vec::len));
        println!("  v{}  evidence: {n}", i + 1);
        detected += usize::from(n > 0);
    }
    println!(
        "equivocation detected by {detected} of {} honest validators",
        genesis_nodes - 1
    );
    Ok(())
}

/// Sends minimal transfers until `done()` (blocks are what move the rotation along: without transactions the network sleeps).
fn drive_blocks(
    dir: &Path,
    l: &Layout,
    api: &str,
    secs: u64,
    done: &dyn Fn() -> bool,
) -> Result<(), String> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while !done() {
        if std::time::Instant::now() > end {
            return Err("the swap did not take effect in time".into());
        }
        let _ = sim_send(dir, l, api);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(())
}

fn rotate_step(
    dir: &Path,
    l: &Layout,
    api: &str,
    action: brdu_ledger::gov::Action,
    idx: usize,
) -> Result<(), String> {
    use brdu_ledger::gov;
    let who = brdu_wallet_key(dir, idx)?;
    sim_wallets(dir, l, api)?; // before the swap: the relayer's nonce must not be left pending
    let mut v = http::get(api, "/validators")?;
    if v["pending"].is_object() {
        println!("a swap is already pending; waiting for it to take effect…");
        let e0 = v["epoch"].as_u64().unwrap_or(0);
        drive_blocks(dir, l, api, 120, &|| {
            http::get(api, "/validators").is_ok_and(|x| x["epoch"].as_u64().unwrap_or(0) > e0)
        })?;
        v = http::get(api, "/validators")?;
    }
    let epoch = v["epoch"].as_u64().unwrap_or(0);
    let set: Vec<String> = v["validators"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let already = set.contains(&Address::from_public_key(&who.public()).encode());
    if (action == gov::Action::Add) == already {
        println!(
            "node {idx}: {} (nothing to do)",
            if already {
                "already a validator"
            } else {
                "already not a validator"
            }
        );
        return Ok(());
    }
    // the test keys of the active validators
    let total = l.info["nodes"].as_array().map_or(0, Vec::len);
    let mut approvers: Vec<SecretKey> = Vec::new();
    for i in 1..=total {
        if let Ok(k) = net::read_key(&dir.join(format!("keys/v{i}.key")))
            && set.contains(&Address::from_public_key(&k.public()).encode())
        {
            approvers.push(k);
        }
    }
    let height = height_of(api)?;
    let effective = height + 8;
    let chain = l.chain_id();
    let doc = gov::change_doc(&chain, action, &who.public(), effective, epoch);
    let ap: Vec<Approval> = approvers.iter().map(|k| Approval::new(&doc, k)).collect();
    let (relayer, nonce) = relayer_nonce(l, api)?;
    let tx = Tx::new_signed(
        &relayer,
        &chain,
        kind::VALIDATOR_CHANGE,
        nonce,
        0,
        gov::change_payload(action, &who.public(), effective, &ap),
    )
    .map_err(|e| e.to_string())?;
    let id = submit(api, &tx)?;
    println!(
        "{} swap of node {idx} sent ({} approvals of {}), effective at height {effective}: {id}",
        if action == gov::Action::Add {
            "JOIN"
        } else {
            "LEAVE"
        },
        ap.len(),
        set.len()
    );
    drive_blocks(dir, l, api, 120, &|| {
        http::get(api, "/validators").is_ok_and(|v| v["epoch"].as_u64().unwrap_or(0) > epoch)
    })?;
    let v = http::get(api, "/validators")?;
    println!(
        "epoch {}: {} active validators",
        v["epoch"],
        v["validators"].as_array().map_or(0, Vec::len)
    );
    Ok(())
}

/// The consensus key of node `idx` (the public one is what goes into the swap).
fn brdu_wallet_key(dir: &Path, idx: usize) -> Result<SecretKey, String> {
    net::read_key(&dir.join(format!("keys/v{idx}.key")))
}

fn cmd_rotate(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let (inn, out): (Option<usize>, Option<usize>) = (
        a.one("--in").and_then(|v| v.parse().ok()),
        a.one("--out").and_then(|v| v.parse().ok()),
    );
    if inn.is_none() && out.is_none() {
        return Err("rotate [--in N] [--out M]".into());
    }
    if let Some(n) = inn {
        rotate_step(&dir, &l, &api, brdu_ledger::gov::Action::Add, n)?;
    }
    if let Some(m) = out {
        rotate_step(&dir, &l, &api, brdu_ledger::gov::Action::Remove, m)?;
    }
    Ok(())
}

fn cmd_docs_check(a: &Args) -> Result<(), String> {
    let root = PathBuf::from(a.one("--root").unwrap_or_else(|| ".".into()));
    let problems = docs_check::check(&root);
    for p in &problems {
        println!("{}: {}", p.file, p.what);
    }
    if problems.is_empty() {
        println!("docs-check: all good");
        Ok(())
    } else {
        Err(format!("docs-check: {} problem(s)", problems.len()))
    }
}

/// The quality gate: the same one CI should run.
fn cmd_check() -> Result<(), String> {
    let steps: [(&str, &[&str]); 3] = [
        ("cargo fmt", &["fmt", "--all", "--", "--check"]),
        (
            "cargo clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("cargo test", &["test", "--workspace", "-q"]),
    ];
    for (name, args) in steps {
        println!("==> {name}");
        if !Command::new("cargo")
            .args(args)
            .status()
            .map_err(|e| e.to_string())?
            .success()
        {
            return Err(format!("{name} failed"));
        }
    }
    println!("==> docs-check");
    cmd_docs_check(&Args(Vec::new()))
}

// ---------------------------------------------------------------------------- explorer

fn stop_explorer(dir: &Path) {
    if let Ok(pid) = fs::read_to_string(dir.join("explorer.pid")) {
        let pid = pid.trim();
        if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) && pid != "0" {
            let _ = Command::new("kill").arg(pid).status();
        }
        let _ = fs::remove_file(dir.join("explorer.pid"));
    }
}

fn explorer_binary() -> Result<PathBuf, String> {
    fresh_binary("brdu-explorer", "brdu-explorer")
}

/// `explorer`: starts the web explorer (in the background) pointed at a node; `explorer stop` stops it.
fn cmd_explorer(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    if a.positional().first().map(String::as_str) == Some("stop") {
        stop_explorer(&dir);
        println!("explorer stopped");
        return Ok(());
    }
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let listen = a.one("--listen").unwrap_or_else(|| "127.0.0.1:8545".into());
    stop_explorer(&dir);
    fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
    let log = fs::File::create(dir.join("logs/explorer.log")).map_err(|e| e.to_string())?;
    let child = Command::new(explorer_binary()?)
        .args([
            "--genesis",
            &dir.join("genesis.hex").to_string_lossy(),
            "--node",
            &api,
            "--listen",
            &listen,
        ])
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| e.to_string())?;
    fs::write(dir.join("explorer.pid"), child.id().to_string()).map_err(|e| e.to_string())?;
    println!("explorer at http://{listen}  (reading node {api}; every block is re-verified here)");
    println!("to stop: cargo dev explorer stop   (or cargo dev down)");
    Ok(())
}

// ---------------------------------------------------------------------------- wallet in the browser

fn stop_wallet_web(dir: &Path) {
    if let Ok(pid) = fs::read_to_string(dir.join("wallet-web.pid")) {
        let pid = pid.trim();
        if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) && pid != "0" {
            let _ = Command::new("kill").arg(pid).status();
        }
        let _ = fs::remove_file(dir.join("wallet-web.pid"));
    }
}

fn web_binary() -> Result<PathBuf, String> {
    fresh_binary("brdu-web", "brdu-web")
}

/// `wallet-web`: starts the local wallet daemon (background) pointed at a node and, if running, the explorer;
/// `wallet-web stop` stops it. TEST keys only: they live in the daemon process.
fn cmd_wallet_web(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    if a.positional().first().map(String::as_str) == Some("stop") {
        stop_wallet_web(&dir);
        println!("wallet daemon stopped");
        return Ok(());
    }
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let listen = a.one("--listen").unwrap_or_else(|| "127.0.0.1:8546".into());
    stop_wallet_web(&dir);
    fs::create_dir_all(dir.join("wallet")).map_err(|e| e.to_string())?;
    fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
    let log = fs::File::create(dir.join("logs/wallet-web.log")).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(web_binary()?);
    cmd.args([
        "--vault",
        &dir.join("wallet/vault.json").to_string_lossy(),
        "--node",
        &api,
        "--listen",
        &listen,
    ]);
    let explorer = a
        .one("--explorer")
        .unwrap_or_else(|| "127.0.0.1:8545".into());
    if http::get(&explorer, "/api/overview").is_ok() {
        cmd.args(["--explorer", &explorer]);
    } else {
        println!(
            "(no explorer at {explorer}: the history screen will be unavailable; start it with `cargo dev explorer`)"
        );
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| e.to_string())?;
    fs::write(dir.join("wallet-web.pid"), child.id().to_string()).map_err(|e| e.to_string())?;
    println!(
        "wallet at http://{listen}  (node {api}; TEST keys only: they live in the daemon process)"
    );
    println!("to stop: cargo dev wallet-web stop   (or cargo dev down)");
    Ok(())
}

/// Mints `units` of test BRDU to `target` (the test issuers approve; the relayer sends) and waits for the balance.
pub(crate) fn fund_address(
    l: &Layout,
    api: &str,
    target: &Address,
    units: u64,
) -> Result<(), String> {
    let mut stamp = [0u8; 4];
    getrandom::fill(&mut stamp).map_err(|e| e.to_string())?;
    let reference = format!("faucet-{}", hex::encode(&stamp));
    let chain = l.chain_id();
    let doc = coin::issuance_doc(false, &chain, target, units, &reference);
    let payload = coin::issuance_payload(target, units, &reference, &approvals(l, &doc)?);
    let (relayer, nonce) = relayer_nonce(l, api)?;
    let tx = Tx::new_signed(&relayer, &chain, kind::MINT, nonce, 0, payload)
        .map_err(|e| e.to_string())?;
    submit(api, &tx)?;
    let before = account(api, target)?.0;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while account(api, target)?.0 < before + units {
        if std::time::Instant::now() > end {
            return Err("the funding did not arrive in 30 s (is the network running?)".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(())
}
