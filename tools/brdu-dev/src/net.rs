//! The on-disk layout of a test network (the same one the Docker lab uses): keys, genesis, wallets.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use brdu_chain::Genesis;
use brdu_crypto::{PublicKey, SecretKey, hex};
use serde_json::{Value, json};

/// Generates a new key at `path` (hex, 0600).
pub fn new_key(path: &Path) -> Result<SecretKey, String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    write_secret(path, &hex::encode(&seed))?;
    Ok(SecretKey::from_bytes(seed))
}

/// Writes a TEST secret with 0600 permission (does not overwrite).
pub fn write_secret(path: &Path, text: &str) -> Result<(), String> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{text}").map_err(|e| e.to_string())
}

/// Reads a hex key.
pub fn read_key(path: &Path) -> Result<SecretKey, String> {
    let t = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    hex::decode_array::<32>(t.trim())
        .map(SecretKey::from_bytes)
        .ok_or_else(|| format!("{}: invalid key", path.display()))
}

/// A test network.
pub struct Layout {
    /// Root folder.
    pub dir: PathBuf,
    /// Description (`devnet.json`).
    pub info: Value,
}

impl Layout {
    /// Loads `DIR/devnet.json`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let p = dir.join("devnet.json");
        let t = fs::read_to_string(&p)
            .map_err(|_| format!("{} does not exist: run `cargo dev init` first", p.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            info: serde_json::from_str(&t).map_err(|e| e.to_string())?,
        })
    }

    /// Network name.
    #[must_use]
    pub fn chain_id(&self) -> String {
        self.info["chain_id"].as_str().unwrap_or("").to_string()
    }

    /// API address of each node.
    #[must_use]
    pub fn apis(&self) -> Vec<String> {
        self.info["nodes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|n| n["api"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Issuer keys.
    pub fn issuers(&self) -> Result<Vec<SecretKey>, String> {
        let n = self.info["issuers"].as_u64().unwrap_or(0);
        (1..=n)
            .map(|i| read_key(&self.dir.join(format!("keys/issuer{i}.key"))))
            .collect()
    }

    /// The key that pays the nonce of mint/burn/attestation (zero fee; the sender does not authorize).
    pub fn relayer(&self) -> Result<SecretKey, String> {
        read_key(&self.dir.join("keys/relayer.key"))
    }
}

/// How a network is assembled.
pub struct Spec<'a> {
    /// Network name.
    pub chain: &'a str,
    /// Validators at genesis.
    pub nodes: usize,
    /// Spare keys (observers that can join by rotation): `v{nodes+1}`…
    pub spare: usize,
    /// Test issuers.
    pub issuers: usize,
    /// Issuers' threshold M.
    pub threshold: u32,
    /// Smallest credential contest window, in seconds.
    pub min_window: u64,
}

/// Creates the network: validators, spares, issuers (threshold `threshold`), relayer key and genesis.
/// `p2p_of(i)` and `api_of(i)` say how the nodes see each other; `p2p_host_of(i)` is the P2P address as seen **from
/// the host computer** (for the failure simulators).
pub fn init(
    dir: &Path,
    spec: &Spec,
    p2p_of: &dyn Fn(usize) -> String,
    p2p_host_of: &dyn Fn(usize) -> String,
    api_of: &dyn Fn(usize) -> String,
) -> Result<(), String> {
    if dir.join("devnet.json").exists() {
        return Err(format!(
            "{} already exists (run `cargo dev reset` to start over)",
            dir.display()
        ));
    }
    fs::create_dir_all(dir.join("keys")).map_err(|e| e.to_string())?;
    fs::create_dir_all(dir.join("wallets")).map_err(|e| e.to_string())?;
    let mut vals: Vec<PublicKey> = Vec::new();
    for i in 1..=spec.nodes + spec.spare {
        let k = new_key(&dir.join(format!("keys/v{i}.key")))?.public();
        if i <= spec.nodes {
            vals.push(k);
        }
    }
    let mut iss: Vec<PublicKey> = Vec::new();
    for i in 1..=spec.issuers {
        iss.push(new_key(&dir.join(format!("keys/issuer{i}.key")))?.public());
    }
    new_key(&dir.join("keys/relayer.key"))?;
    // root authorities (create credential types; 2 of 3) and a test organ (3 keys, 2 of 3) for the credential types
    let mut auth: Vec<PublicKey> = Vec::new();
    for i in 1..=3 {
        auth.push(new_key(&dir.join(format!("keys/authority{i}.key")))?.public());
        new_key(&dir.join(format!("keys/organ{i}.key")))?;
    }
    let mut g = Genesis::new(spec.chain, vals, iss, spec.threshold);
    g.min_validators = 3.min(u32::try_from(spec.nodes).unwrap_or(3)).max(1);
    g.authorities = auth;
    g.authority_threshold = 2;
    g.min_window = spec.min_window;
    g.rules().map_err(|e| e.to_string())?;
    fs::write(
        dir.join("genesis.hex"),
        format!("{}\n", hex::encode(&g.encode())),
    )
    .map_err(|e| e.to_string())?;
    let list: Vec<Value> = (1..=spec.nodes + spec.spare)
        .map(|i| json!({"name": format!("v{i}"), "p2p": p2p_of(i), "p2p_host": p2p_host_of(i), "api": api_of(i), "spare": i > spec.nodes}))
        .collect();
    let info = json!({
        "chain_id": spec.chain, "genesis_hash": g.hash().to_hex(), "issuers": spec.issuers, "threshold": spec.threshold,
        "genesis_nodes": spec.nodes, "nodes": list
    });
    fs::write(
        dir.join("devnet.json"),
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
