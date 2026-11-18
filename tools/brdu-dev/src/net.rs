//! O layout de uma rede de teste em disco (a mesma que o laboratório Docker usa): chaves, gênese, carteiras.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use brdu_chain::Genesis;
use brdu_crypto::{PublicKey, SecretKey, hex};
use serde_json::{Value, json};

/// Gera uma chave nova em `path` (hex, 0600).
pub fn new_key(path: &Path) -> Result<SecretKey, String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    write_secret(path, &hex::encode(&seed))?;
    Ok(SecretKey::from_bytes(seed))
}

/// Grava um segredo de TESTE com permissão 0600 (não sobrescreve).
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

/// Lê uma chave em hex.
pub fn read_key(path: &Path) -> Result<SecretKey, String> {
    let t = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    hex::decode_array::<32>(t.trim())
        .map(SecretKey::from_bytes)
        .ok_or_else(|| format!("{}: chave inválida", path.display()))
}

/// Uma rede de teste.
pub struct Layout {
    /// Pasta raiz.
    pub dir: PathBuf,
    /// Descrição (`devnet.json`).
    pub info: Value,
}

impl Layout {
    /// Carrega `DIR/devnet.json`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let p = dir.join("devnet.json");
        let t = fs::read_to_string(&p)
            .map_err(|_| format!("{} não existe: rode `cargo dev init` antes", p.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            info: serde_json::from_str(&t).map_err(|e| e.to_string())?,
        })
    }

    /// Nome da rede.
    #[must_use]
    pub fn chain_id(&self) -> String {
        self.info["chain_id"].as_str().unwrap_or("").to_string()
    }

    /// Endereços da API de cada nó.
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

    /// Chaves dos emissores.
    pub fn issuers(&self) -> Result<Vec<SecretKey>, String> {
        let n = self.info["issuers"].as_u64().unwrap_or(0);
        (1..=n)
            .map(|i| read_key(&self.dir.join(format!("keys/issuer{i}.key"))))
            .collect()
    }

    /// A chave que paga o nonce de mint/burn/atestado (taxa zero; quem envia não autoriza).
    pub fn relayer(&self) -> Result<SecretKey, String> {
        read_key(&self.dir.join("keys/relayer.key"))
    }
}

/// Como uma rede é montada.
pub struct Spec<'a> {
    /// Nome da rede.
    pub chain: &'a str,
    /// Validadores na gênese.
    pub nodes: usize,
    /// Chaves reservas (observadores que podem entrar por rotação): `v{nodes+1}`…
    pub spare: usize,
    /// Emissores de teste.
    pub issuers: usize,
    /// Limiar M dos emissores.
    pub threshold: u32,
}

/// Cria a rede: validadores, reservas, emissores (limiar `threshold`), chave do retransmissor e a gênese.
/// `p2p_of(i)` e `api_of(i)` dizem como os nós se enxergam entre si; `p2p_host_of(i)` é o endereço P2P visto **do
/// computador** (para os simuladores de falha).
pub fn init(
    dir: &Path,
    spec: &Spec,
    p2p_of: &dyn Fn(usize) -> String,
    p2p_host_of: &dyn Fn(usize) -> String,
    api_of: &dyn Fn(usize) -> String,
) -> Result<(), String> {
    if dir.join("devnet.json").exists() {
        return Err(format!(
            "{} já existe (rode `cargo dev reset` para recomeçar)",
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
    let mut g = Genesis::new(spec.chain, vals, iss, spec.threshold);
    g.min_validators = 3.min(u32::try_from(spec.nodes).unwrap_or(3)).max(1);
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
