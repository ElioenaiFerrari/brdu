//! BRDU desktop wallet (Tauri v2). The webview shows the UI; every operation goes through ONE command, `rpc`,
//! into the wallet service running in this process. Private keys, the password and the recovery phrase never
//! travel to the webview (it only receives public data, and the recovery words once, when the user asks for them).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use brdu_walletd::{Config, Wallet};
use serde_json::{Value, json};
use tauri::Manager;

/// The single bridge from the UI to the wallet service. Runs off the UI thread (Argon2id takes a moment).
#[tauri::command]
async fn rpc(wallet: tauri::State<'_, Arc<Wallet>>, method: String, params: Value) -> Result<Value, Value> {
    let wallet = Arc::clone(&wallet);
    tauri::async_runtime::spawn_blocking(move || wallet.call(&method, &params))
        .await
        .map_err(|e| json!({"code": "internal", "message": e.to_string()}))?
        .map_err(|e| json!({"code": e.code, "message": e.message}))
}

fn main() {
    let result = tauri::Builder::default()
        .setup(|app| {
            // vault in the app data folder; the network endpoints can be overridden for other networks
            let dir: PathBuf = app.path().app_data_dir()?;
            let node = std::env::var("BRDU_NODE").unwrap_or_else(|_| "127.0.0.1:7201".into());
            let explorer = std::env::var("BRDU_EXPLORER").unwrap_or_else(|_| "127.0.0.1:8545".into());
            let cfg = Config::new(dir.join("vault.json"), &node, Some(&explorer));
            app.manage(Arc::new(Wallet::new(cfg)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![rpc])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("brdu-wallet-app: {e}");
        std::process::exit(1);
    }
}
