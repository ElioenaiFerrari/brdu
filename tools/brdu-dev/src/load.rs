//! Teste de carga **aberto** (open-loop) com medição por cgroup: envia à taxa pedida **mesmo que a rede atrase**
//! (a latência conta a partir da hora agendada, sem "omissão coordenada"), mede a confirmação (envio → bloco),
//! CPU e memória reais de cada contêiner e eventos de morte/OOM do Docker.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use brdu_crypto::{Address, Hash32, SecretKey, hex, tagged};
use brdu_ledger::{Tx, coin, kind};
use serde_json::Value;

use crate::http::{self, Conn};
use crate::net::Layout;

fn sender_key(i: usize) -> SecretKey {
    SecretKey::from_bytes(*tagged("brdu/load/sender", &[&i.to_be_bytes()]).as_bytes())
}

/// Contadores do cgroup de um contêiner.
#[derive(Clone, Copy, Default, Debug)]
struct Cg {
    usage_usec: u64,
    nr_periods: u64,
    nr_throttled: u64,
    throttled_usec: u64,
    mem_peak: u64,
    mem_limit: u64,
    oom: u64,
    oom_kill: u64,
}

fn cg_read(container: &str) -> Option<Cg> {
    let out = Command::new("docker")
        .args(["exec", container, "sh", "-c", "cat /sys/fs/cgroup/cpu.stat; echo ---; cat /sys/fs/cgroup/memory.peak /sys/fs/cgroup/memory.max; echo ---; cat /sys/fs/cgroup/memory.events"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut cg = Cg::default();
    let mut section = 0;
    let mut nums = Vec::new();
    for line in text.lines() {
        if line == "---" {
            section += 1;
            continue;
        }
        match section {
            0 | 2 => {
                let mut it = line.split_whitespace();
                let (k, v) = (it.next()?, it.next()?.parse::<u64>().ok()?);
                match k {
                    "usage_usec" => cg.usage_usec = v,
                    "nr_periods" => cg.nr_periods = v,
                    "nr_throttled" => cg.nr_throttled = v,
                    "throttled_usec" => cg.throttled_usec = v,
                    "oom" => cg.oom = v,
                    "oom_kill" => cg.oom_kill = v,
                    _ => {}
                }
            }
            _ => nums.push(line.trim().parse::<u64>().unwrap_or(u64::MAX)),
        }
    }
    cg.mem_peak = nums.first().copied().unwrap_or(0);
    cg.mem_limit = nums.get(1).copied().unwrap_or(0);
    Some(cg)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Envios agendados de uma conta: (hora relativa, corpo hex, id).
type Plan = Vec<(Duration, String, Hash32)>;

/// O resultado de um degrau.
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub accepted_per_s: f64,
    pub rejected: u64,
    pub confirmed: usize,
    pub lost: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub drain_s: f64,
    pub txs_per_block: f64,
    pub cpu_max: f64,
    pub throttle_pct: f64,
    pub mem_peak_mib: f64,
    pub mem_limit_mib: f64,
    pub oom: u64,
    pub saturated: bool,
}

pub struct Params {
    pub dir: std::path::PathBuf,
    pub rates: Vec<f64>,
    pub secs: u64,
    pub accounts: usize,
    pub warmup_secs: u64,
    pub repeats: usize,
    pub containers: Vec<String>,
    /// As APIs que recebem a carga: só nós que votam (um observador não propõe nem repassa).
    pub apis: Vec<String>,
}

fn fee(api: &str, probe: &Tx) -> Result<u64, String> {
    let f = http::get(api, "/fees")?;
    Ok(f["transfer_base"].as_u64().unwrap_or(1000)
        + f["per_byte"].as_u64().unwrap_or(1) * probe.size() as u64)
}

fn wait_height_stable(apis: &[String]) {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        let ok = apis
            .iter()
            .all(|a| http::get(a, "/status").is_ok_and(|s| s["mempool"] == 0));
        if ok {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Financia as contas de envio fora da medição.
fn prepare(l: &Layout, accounts: usize, apis: &[String]) -> Result<(), String> {
    println!("preparando {accounts} contas de remetente (financiadas fora da medição)…");
    let relayer = l.relayer()?;
    let chain = l.chain_id();
    let (_, mut nonce) = {
        let v = http::get(
            &apis[0],
            &format!("/account/{}", Address::from_public_key(&relayer.public())),
        )?;
        (0, v["nonce"].as_u64().unwrap_or(0))
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for i in 0..accounts {
        let to = Address::from_public_key(&sender_key(i).public());
        let reference = format!("load-{stamp}-{i}");
        let doc = coin::issuance_doc(false, &chain, &to, 1_000_000_000_000, &reference);
        let payload = coin::issuance_payload(
            &to,
            1_000_000_000_000,
            &reference,
            &crate::approvals(l, &doc)?,
        );
        let tx = Tx::new_signed(&relayer, &chain, kind::MINT, nonce, 0, payload)
            .map_err(|e| e.to_string())?;
        crate::submit(&apis[i % apis.len()], &tx)?;
        nonce += 1;
    }
    wait_height_stable(apis);
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn run_step(l: &Layout, p: &Params, rate: f64, secs: u64, apis: &[String]) -> Result<Step, String> {
    let chain = l.chain_id();
    let sink = Address::from_public_key(&SecretKey::from_bytes([7; 32]).public());
    let total = (rate * secs as f64) as usize;
    let per_account = total.div_ceil(p.accounts).max(1);
    let interval = Duration::from_secs_f64(p.accounts as f64 / rate);

    // nonces e taxa
    let mut plans: Vec<(usize, Plan)> = Vec::new();
    let mut fee_v = 0;
    for i in 0..p.accounts {
        let key = sender_key(i);
        let addr = Address::from_public_key(&key.public());
        let api = &apis[i % apis.len()];
        let v = http::get(api, &format!("/account/{addr}"))?;
        let nonce0 = v["nonce"].as_u64().unwrap_or(0);
        if fee_v == 0 {
            let probe = Tx::new_signed(
                &key,
                &chain,
                kind::TRANSFER,
                nonce0,
                0,
                coin::transfer_payload(&sink, 1),
            )
            .map_err(|e| e.to_string())?;
            fee_v = fee(api, &probe)?;
        }
        let mut list = Vec::with_capacity(per_account);
        for k in 0..per_account {
            let tx = Tx::new_signed(
                &key,
                &chain,
                kind::TRANSFER,
                nonce0 + k as u64,
                fee_v,
                coin::transfer_payload(&sink, 1),
            )
            .map_err(|e| e.to_string())?;
            let offset =
                interval.mul_f64(k as f64) + interval.mul_f64(i as f64 / p.accounts as f64);
            list.push((
                offset,
                hex::encode(&tx.encode().map_err(|e| e.to_string())?),
                tx.id(),
            ));
        }
        plans.push((i, list));
    }
    let total = per_account * p.accounts;

    let before: Vec<Option<Cg>> = p.containers.iter().map(|c| cg_read(c)).collect();
    let status0 = http::get(&apis[0], "/status")?;
    let height0 = status0["height"].as_u64().unwrap_or(0);

    // poller de blocos: quando cada altura aparece pela primeira vez, anota a hora das suas transações
    let stop = Arc::new(AtomicBool::new(false));
    let seen: Arc<Mutex<HashMap<Hash32, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let blocks_seen = Arc::new(AtomicU64::new(0));
    let poller = {
        let (stop, seen, blocks_seen, api) = (
            stop.clone(),
            seen.clone(),
            blocks_seen.clone(),
            apis[0].clone(),
        );
        std::thread::spawn(move || {
            let mut conn = Conn::new(&api);
            let mut next = height0 + 1;
            while !stop.load(Ordering::SeqCst) {
                let Ok((200, body)) = conn.request("GET", &format!("/block/{next}"), "") else {
                    std::thread::sleep(Duration::from_millis(15));
                    continue;
                };
                let now = Instant::now();
                if let Ok(v) = serde_json::from_str::<Value>(&body) {
                    let mut m = seen
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    for t in v["txs"].as_array().unwrap_or(&Vec::new()) {
                        if let Some(id) = t["id"].as_str().and_then(hex::decode_array::<32>) {
                            m.insert(Hash32(id), now);
                        }
                    }
                    blocks_seen.fetch_add(1, Ordering::SeqCst);
                }
                next += 1;
            }
        })
    };

    // envio aberto: uma thread por conta, na hora agendada
    let start = Instant::now() + Duration::from_millis(300);
    let sent_at: Arc<Mutex<HashMap<Hash32, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let accepted = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));
    let mut senders = Vec::new();
    for (i, list) in plans {
        let (api, sent_at, accepted, rejected) = (
            apis[i % apis.len()].clone(),
            sent_at.clone(),
            accepted.clone(),
            rejected.clone(),
        );
        senders.push(std::thread::spawn(move || {
            let mut conn = Conn::new(&api);
            for (offset, hexed, id) in list {
                let due = start + offset;
                let now = Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                sent_at
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(id, due);
                match conn.request("POST", "/tx", &hexed) {
                    Ok((200, _)) => accepted.fetch_add(1, Ordering::Relaxed),
                    _ => rejected.fetch_add(1, Ordering::Relaxed),
                };
            }
        }));
    }
    for s in senders {
        let _ = s.join();
    }
    let send_end = Instant::now();
    let send_secs = send_end.duration_since(start).as_secs_f64().max(0.001);

    // espera esvaziar (até 60 s)
    let drain_start = Instant::now();
    let deadline = drain_start + Duration::from_secs(60);
    let want = accepted.load(Ordering::SeqCst) as usize;
    while Instant::now() < deadline {
        if seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
            >= want
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let drain_s = drain_start.elapsed().as_secs_f64();
    std::thread::sleep(Duration::from_millis(200));
    stop.store(true, Ordering::SeqCst);
    let _ = poller.join();

    let after: Vec<Option<Cg>> = p.containers.iter().map(|c| cg_read(c)).collect();
    let seen = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let sent = sent_at
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut lat: Vec<f64> = sent
        .iter()
        .filter_map(|(id, t0)| {
            seen.get(id)
                .map(|t1| t1.saturating_duration_since(*t0).as_secs_f64() * 1000.0)
        })
        .collect();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let accepted_n = accepted.load(Ordering::SeqCst);
    let blocks = blocks_seen.load(Ordering::SeqCst).max(1);

    let wall = (send_secs + drain_s).max(0.001);
    let (mut cpu_max, mut thr_max, mut mem_peak, mut mem_limit, mut oom) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0u64);
    for (b, a) in before.iter().zip(&after) {
        if let (Some(b), Some(a)) = (b, a) {
            cpu_max = cpu_max.max((a.usage_usec - b.usage_usec) as f64 / 1e6 / wall);
            let periods = (a.nr_periods - b.nr_periods).max(1) as f64;
            thr_max = thr_max.max((a.nr_throttled - b.nr_throttled) as f64 / periods * 100.0);
            mem_peak = mem_peak.max(a.mem_peak as f64 / 1_048_576.0);
            if a.mem_limit < u64::MAX / 2 {
                mem_limit = mem_limit.max(a.mem_limit as f64 / 1_048_576.0);
            }
            oom += (a.oom + a.oom_kill).saturating_sub(b.oom + b.oom_kill);
        }
    }
    let accepted_per_s = accepted_n as f64 / send_secs;
    let step = Step {
        accepted_per_s,
        rejected: rejected.load(Ordering::SeqCst),
        confirmed: lat.len(),
        lost: want.saturating_sub(lat.len()),
        p50: percentile(&lat, 0.50),
        p95: percentile(&lat, 0.95),
        p99: percentile(&lat, 0.99),
        drain_s,
        txs_per_block: lat.len() as f64 / blocks as f64,
        cpu_max,
        throttle_pct: thr_max,
        mem_peak_mib: mem_peak,
        mem_limit_mib: mem_limit,
        oom,
        saturated: accepted_per_s < rate * 0.95 || drain_s > 10.0 || lat.len() < want,
    };
    let _ = total;
    Ok(step)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    percentile(v, 0.5)
}

/// Executa a escada.
pub fn run(p: &Params) -> Result<(), String> {
    let l = Layout::load(&p.dir)?;
    let apis = p.apis.clone();
    let started = std::time::SystemTime::now();
    prepare(&l, p.accounts, &apis)?;
    if p.warmup_secs > 0 {
        let r = p.rates.first().copied().unwrap_or(50.0);
        println!("aquecimento: {r} tx/s por {} s (descartado)", p.warmup_secs);
        run_step(&l, p, r, p.warmup_secs, &apis)?;
        wait_height_stable(&apis);
    }
    let mut summary: Vec<(f64, Vec<Step>)> = Vec::new();
    for &rate in &p.rates {
        let mut runs = Vec::new();
        for rep in 0..p.repeats {
            println!(
                "\n=== degrau {rate} tx/s por {} s (repetição {}/{}) ===",
                p.secs,
                rep + 1,
                p.repeats
            );
            let s = run_step(&l, p, rate, p.secs, &apis)?;
            println!(
                "aceitas {:.1}/s (recusadas {}); confirmação p50 {:.0} ms · p95 {:.0} ms · p99 {:.0} ms ({} confirmadas, {} perdidas); esvaziou em {:.1} s; {:.0} tx/bloco",
                s.accepted_per_s,
                s.rejected,
                s.p50,
                s.p95,
                s.p99,
                s.confirmed,
                s.lost,
                s.drain_s,
                s.txs_per_block
            );
            println!(
                "CPU máx {:.3} núcleos · throttle {:.1}% · memória pico {:.0}/{:.0} MiB · OOM {}",
                s.cpu_max, s.throttle_pct, s.mem_peak_mib, s.mem_limit_mib, s.oom
            );
            runs.push(s);
            wait_height_stable(&apis);
        }
        summary.push((rate, runs));
    }
    println!(
        "\n=========== RESUMO (mediana de {} repetição(ões); [mín–máx] do p50) ===========",
        p.repeats
    );
    println!(
        "{:>9} {:>9} {:>11} {:>11} {:>9} {:>9} {:>8} {:>9} {:>10}",
        "oferecida",
        "aceita/s",
        "confirm p50",
        "confirm p95",
        "esvaziou",
        "tx/bloco",
        "CPU máx",
        "throttle",
        "mem pico"
    );
    for (rate, runs) in &summary {
        let col = |f: fn(&Step) -> f64| median(&mut runs.iter().map(f).collect::<Vec<_>>());
        let p50s: Vec<f64> = runs.iter().map(|s| s.p50).collect();
        let (lo, hi) = (
            p50s.iter().copied().fold(f64::MAX, f64::min),
            p50s.iter().copied().fold(0.0, f64::max),
        );
        println!(
            "{:>7.0}/s {:>9.1} {:>8.0} ms {:>8.0} ms {:>7.1} s {:>9.1} {:>8.3} {:>8.1}% {:>7.0} MiB  [{:.0}–{:.0}]{}",
            rate,
            col(|s| s.accepted_per_s),
            col(|s| s.p50),
            col(|s| s.p95),
            col(|s| s.drain_s),
            col(|s| s.txs_per_block),
            col(|s| s.cpu_max),
            col(|s| s.throttle_pct),
            col(|s| s.mem_peak_mib),
            lo,
            hi,
            if runs.iter().any(|s| s.saturated) {
                "   <- SATURADO"
            } else {
                ""
            }
        );
    }
    // eventos de morte/OOM do Docker durante o teste
    let since = started
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        .to_string();
    let ev = Command::new("docker")
        .args([
            "events",
            "--since",
            &since,
            "--until",
            &(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
                + 1)
            .to_string(),
            "--filter",
            "event=die",
            "--filter",
            "event=oom",
            "--filter",
            "label=com.docker.compose.project=brdu-lab",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&ev.stdout);
    println!(
        "\neventos do Docker (die/oom) durante o teste: {}",
        if text.trim().is_empty() {
            "nenhum".to_string()
        } else {
            text.trim().to_string()
        }
    );
    Ok(())
}
