//! Sync: fetches final blocks from a node and **re-verifies** them before storing.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use brdu_chain::DecidedBlock;
use brdu_crypto::hex;

use crate::http::get_json;
use crate::index::Explorer;

/// One round: asks the node for new blocks and applies those that pass. Returns how many got in.
pub fn sync_once(shared: &RwLock<Explorer>) -> Result<usize, String> {
    let (node, from, max_txs, max_bytes) = {
        let e = shared.read().map_err(|_| "poisoned")?;
        let r = e.chain.executor().rules();
        (
            e.node.clone(),
            e.height() + 1,
            r.max_block_txs,
            r.max_block_bytes,
        )
    };
    let status = get_json(&node, "/status");
    if let Ok(mut e) = shared.write() {
        match &status {
            Ok(s) => e.status.node_height = s["height"].as_u64().unwrap_or(0),
            Err(err) => e.status.last_error = Some(err.clone()),
        }
    }
    status?;
    if let Ok(ev) = get_json(&node, "/evidence")
        && let Ok(mut e) = shared.write()
    {
        e.evidence = ev["evidence"].clone();
    }
    let raw = get_json(&node, &format!("/blocks/raw?from={from}&limit=32"))?;
    let list = raw["blocks"].as_array().cloned().unwrap_or_default();
    let mut applied = 0;
    for item in list {
        let bytes = item
            .as_str()
            .and_then(hex::decode)
            .ok_or("invalid hex block")?;
        let verdict = DecidedBlock::decode(&bytes, max_txs, max_bytes)
            .map_err(|e| e.to_string())
            .and_then(|d| {
                let mut e = shared.write().map_err(|_| "poisoned".to_string())?;
                e.ingest(d)
            });
        let mut e = shared.write().map_err(|_| "poisoned")?;
        match verdict {
            Ok(()) => {
                applied += 1;
                e.status.last_ok = Some(Instant::now());
                e.status.last_error = None;
            }
            Err(msg) => {
                // the node delivered something verification rejects: stop here and report it
                e.status.rejected_blocks += 1;
                e.status.last_error = Some(format!("the node delivered an invalid block: {msg}"));
                return Err(msg);
            }
        }
    }
    if let Ok(mut e) = shared.write()
        && applied == 0
        && e.status.last_error.is_none()
    {
        e.status.last_ok = Some(Instant::now());
    }
    Ok(applied)
}

/// Syncs in a loop (every 400 ms, or without pause while there are blocks).
pub fn spawn_sync(
    shared: Arc<RwLock<Explorer>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(std::sync::atomic::Ordering::SeqCst) {
            let n = sync_once(&shared);
            if !matches!(n, Ok(k) if k > 0) {
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    })
}
