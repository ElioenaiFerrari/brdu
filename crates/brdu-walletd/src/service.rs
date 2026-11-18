use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use brdu_crypto::{Address, Amount, SecretKey, hex};
use brdu_ledger::{Tx, coin, kind};
use brdu_vault::{KdfParams, Vault, VaultError};
use brdu_wallet::Phrase;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::error::WalletError;
use crate::http::request;

/// How long a prepared send stays valid.
const PREPARED_TTL: Duration = Duration::from_secs(300);
/// Most prepared sends kept at once.
const MAX_PREPARED: usize = 16;

/// Wallet configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where the encrypted vault lives.
    pub vault_path: PathBuf,
    /// A node API address (`host:port`).
    pub node: String,
    /// An explorer address (history); optional.
    pub explorer: Option<String>,
    /// Argon2id cost for new vaults (tests use cheap values).
    pub kdf: KdfParams,
    /// Lock after this much inactivity.
    pub lock_after: Duration,
    /// Wrong passwords allowed before a temporary lockout starts.
    pub max_attempts: u32,
    /// First lockout duration; it doubles with each further failure (capped at 60 s).
    pub lockout: Duration,
}

impl Config {
    /// Defaults: production KDF cost, 5 minutes auto-lock.
    #[must_use]
    pub fn new(vault_path: PathBuf, node: &str, explorer: Option<&str>) -> Self {
        Self {
            vault_path,
            node: node.to_string(),
            explorer: explorer.map(String::from),
            kdf: KdfParams::default(),
            lock_after: Duration::from_secs(300),
            max_attempts: 5,
            lockout: Duration::from_secs(2),
        }
    }
}

struct Unlocked {
    secret: SecretKey,
    address: Address,
    last_used: Instant,
}

struct Prepared {
    to: Address,
    amount: u64,
    fee: u64,
    at: Instant,
}

/// The wallet.
pub struct Wallet {
    cfg: Config,
    session: Mutex<Option<Unlocked>>,
    prepared: Mutex<HashMap<String, Prepared>>,
    /// Consecutive wrong passwords and when the lockout ends.
    failures: Mutex<(u32, Option<Instant>)>,
}

fn random_id() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    hex::encode(&b)
}

fn vault_err(e: &VaultError) -> WalletError {
    match e {
        VaultError::WeakPassword => WalletError::new("weak_password", e.to_string()),
        VaultError::Decrypt => WalletError::new("wrong_password", e.to_string()),
        other => WalletError::new("internal", other.to_string()),
    }
}

fn bad(msg: impl Into<String>) -> WalletError {
    WalletError::new("bad_request", msg)
}

fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str, WalletError> {
    p.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("missing text parameter `{key}`")))
}

impl Wallet {
    /// A wallet service over `cfg`.
    #[must_use]
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            session: Mutex::new(None),
            prepared: Mutex::new(HashMap::new()),
            failures: Mutex::new((0, None)),
        }
    }

    fn read_vault(&self) -> Result<Vault, WalletError> {
        let text = std::fs::read_to_string(&self.cfg.vault_path)
            .map_err(|_| WalletError::new("no_vault", "no wallet has been created yet"))?;
        Vault::from_json(&text).map_err(|e| vault_err(&e))
    }

    fn write_vault(&self, v: &Vault, overwrite: bool) -> Result<(), WalletError> {
        if let Some(dir) = self.cfg.vault_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true);
        if overwrite {
            opts.create(true).truncate(true);
        } else {
            opts.create_new(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&self.cfg.vault_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                WalletError::new("vault_exists", "a wallet already exists here")
            } else {
                WalletError::new("internal", e.to_string())
            }
        })?;
        std::io::Write::write_all(&mut f, v.to_json().as_bytes())
            .map_err(|e| WalletError::new("internal", e.to_string()))
    }

    /// Applies the inactivity lock, then returns the unlocked account.
    fn account(&self) -> Result<(SecretKey, Address), WalletError> {
        let mut s = self
            .session
            .lock()
            .map_err(|_| WalletError::new("internal", "poisoned"))?;
        if s.as_ref()
            .is_some_and(|u| u.last_used.elapsed() > self.cfg.lock_after)
        {
            *s = None;
        }
        match s.as_mut() {
            Some(u) => {
                u.last_used = Instant::now();
                Ok((SecretKey::from_bytes(*u.secret.expose_seed()), u.address))
            }
            None => Err(WalletError::new("locked", "the wallet is locked")),
        }
    }

    fn set_session(&self, secret: SecretKey, address: Address) {
        if let Ok(mut s) = self.session.lock() {
            *s = Some(Unlocked {
                secret,
                address,
                last_used: Instant::now(),
            });
        }
    }

    fn derive(phrase: &Phrase, passphrase: &str) -> Result<(SecretKey, Address), WalletError> {
        let wallet = phrase
            .wallet(passphrase)
            .map_err(|e| WalletError::new("invalid_phrase", e.to_string()))?;
        let acc = wallet
            .account(0, 0)
            .map_err(|e| WalletError::new("internal", e.to_string()))?;
        Ok((
            SecretKey::from_bytes(*acc.secret_key().expose_seed()),
            *acc.address(),
        ))
    }

    /// Refuses password checks during a lockout (brute-force defense; the Argon2id cost already slows each try).
    fn check_not_locked_out(&self) -> Result<(), WalletError> {
        let f = self
            .failures
            .lock()
            .map_err(|_| WalletError::new("internal", "poisoned"))?;
        match f.1 {
            Some(until) if until > Instant::now() => Err(WalletError::new(
                "too_many_attempts",
                format!(
                    "too many wrong passwords; try again in {} s",
                    until.duration_since(Instant::now()).as_secs() + 1
                ),
            )),
            _ => Ok(()),
        }
    }

    /// Records the outcome of a password check.
    fn note_password<T>(&self, r: Result<T, WalletError>) -> Result<T, WalletError> {
        if let Ok(mut f) = self.failures.lock() {
            match &r {
                Err(e) if e.code == "wrong_password" => {
                    f.0 += 1;
                    if f.0 >= self.cfg.max_attempts {
                        let extra = f.0 - self.cfg.max_attempts;
                        let secs = (self.cfg.lockout.as_secs_f64()
                            * 2f64.powi(i32::try_from(extra.min(10)).unwrap_or(10)))
                        .min(60.0);
                        f.1 = Some(Instant::now() + Duration::from_secs_f64(secs));
                    }
                }
                Ok(_) => *f = (0, None),
                Err(_) => {}
            }
        }
        r
    }

    // ------------------------------------------------------------------ lifecycle

    /// Whether a vault exists, whether it is unlocked, and the address.
    #[must_use]
    pub fn status(&self) -> Value {
        let vault = self.read_vault().ok();
        let unlocked = self.account().ok().map(|(_, a)| a.encode());
        json!({"has_vault": vault.is_some(), "unlocked": unlocked.is_some(), "address": unlocked.or_else(|| vault.map(|v| v.address))})
    }

    /// Creates a new wallet: a fresh 24-word phrase, sealed in the vault. The words are returned **once**.
    pub fn create(&self, password: &str, passphrase: &str) -> Result<Value, WalletError> {
        let phrase = Phrase::generate().map_err(|e| WalletError::new("internal", e.to_string()))?;
        let (secret, address) = Self::derive(&phrase, passphrase)?;
        let words = phrase.words().join(" ");
        let vault = Vault::seal_with(&words, password, &address.encode(), self.cfg.kdf)
            .map_err(|e| vault_err(&e))?;
        self.write_vault(&vault, false)?;
        self.set_session(secret, address);
        Ok(json!({"words": phrase.words(), "address": address.encode()}))
    }

    /// Shows the address a phrase (and optional passphrase) would give, **without** storing anything: the user
    /// confirms it before importing.
    pub fn preview_import(&self, words: &str, passphrase: &str) -> Result<Value, WalletError> {
        let phrase = Phrase::parse(words.trim())
            .map_err(|e| WalletError::new("invalid_phrase", e.to_string()))?;
        let (_, address) = Self::derive(&phrase, passphrase)?;
        Ok(json!({"address": address.encode(), "word_count": phrase.len()}))
    }

    /// Imports a phrase. `expected_address` is what the user confirmed in `preview_import`; a mismatch is refused.
    pub fn import(
        &self,
        words: &str,
        passphrase: &str,
        password: &str,
        expected_address: &str,
    ) -> Result<Value, WalletError> {
        let phrase = Phrase::parse(words.trim())
            .map_err(|e| WalletError::new("invalid_phrase", e.to_string()))?;
        let (secret, address) = Self::derive(&phrase, passphrase)?;
        if address.encode() != expected_address {
            return Err(WalletError::new(
                "address_mismatch",
                "the derived address is not the one that was confirmed",
            ));
        }
        let normalized = phrase.words().join(" ");
        let vault = Vault::seal_with(&normalized, password, &address.encode(), self.cfg.kdf)
            .map_err(|e| vault_err(&e))?;
        self.write_vault(&vault, false)?;
        self.set_session(secret, address);
        Ok(json!({"address": address.encode()}))
    }

    /// Unlocks. The passphrase (if the wallet uses one) is asked every time because it is never stored.
    pub fn unlock(&self, password: &str, passphrase: &str) -> Result<Value, WalletError> {
        self.check_not_locked_out()?;
        let vault = self.read_vault()?;
        let words = self.note_password(vault.open(password).map_err(|e| vault_err(&e)))?;
        let phrase =
            Phrase::parse(&words).map_err(|e| WalletError::new("invalid_phrase", e.to_string()))?;
        let (secret, address) = Self::derive(&phrase, passphrase)?;
        if address.encode() != vault.address {
            return Err(WalletError::new(
                "wrong_passphrase",
                "the passphrase does not match this wallet",
            ));
        }
        self.set_session(secret, address);
        Ok(json!({"address": address.encode()}))
    }

    /// Removes the wallet from this device so a new one can be created or imported (forgotten password, or another
    /// wallet wanted). It needs NO password (the user may have forgotten it) but an explicit `confirm == "forget"`.
    /// The vault file is **moved aside** (`vault.json.removed-<unix time>`), never deleted: if it was a mistake, the
    /// file can be put back by hand. The recovery phrase is the only way to get the money back after this.
    pub fn forget(&self, confirm: &str) -> Result<Value, WalletError> {
        if confirm != "forget" {
            return Err(WalletError::new(
                "bad_request",
                "confirmation missing: pass confirm = \"forget\"",
            ));
        }
        self.lock();
        if !self.cfg.vault_path.exists() {
            return Err(WalletError::new("no_vault", "there is no wallet to remove"));
        }
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut aside = self.cfg.vault_path.clone().into_os_string();
        aside.push(format!(".removed-{when}"));
        std::fs::rename(&self.cfg.vault_path, &aside)
            .map_err(|e| WalletError::new("internal", e.to_string()))?;
        if let Ok(mut f) = self.failures.lock() {
            *f = (0, None);
        }
        Ok(
            json!({"removed": true, "kept_as": std::path::PathBuf::from(aside).file_name().map(|n| n.to_string_lossy().to_string())}),
        )
    }

    /// Locks and forgets any prepared send.
    pub fn lock(&self) {
        if let Ok(mut s) = self.session.lock() {
            *s = None;
        }
        if let Ok(mut p) = self.prepared.lock() {
            p.clear();
        }
    }

    /// Shows the recovery phrase again. Requires the password again, even when unlocked.
    pub fn export_phrase(&self, password: &str) -> Result<Value, WalletError> {
        self.account()?;
        self.check_not_locked_out()?;
        let words =
            self.note_password(self.read_vault()?.open(password).map_err(|e| vault_err(&e)))?;
        let w: Vec<&str> = words.split_whitespace().collect();
        Ok(json!({"words": w}))
    }

    /// Changes the vault password (needs the old one).
    pub fn change_password(&self, old: &str, new: &str) -> Result<Value, WalletError> {
        self.account()?;
        self.check_not_locked_out()?;
        let vault = self.read_vault()?;
        let words: Zeroizing<String> =
            self.note_password(vault.open(old).map_err(|e| vault_err(&e)))?;
        let sealed = Vault::seal_with(&words, new, &vault.address, self.cfg.kdf)
            .map_err(|e| vault_err(&e))?;
        self.write_vault(&sealed, true)?;
        Ok(json!({"changed": true}))
    }

    // ------------------------------------------------------------------ reading the chain

    fn node_get(&self, path: &str) -> Result<Value, WalletError> {
        let r = request(&self.cfg.node, "GET", path, "")
            .map_err(|e| WalletError::new("node_unreachable", e))?;
        if r.code == 200 {
            Ok(r.body)
        } else {
            Err(WalletError::new(
                "rejected",
                r.body["error"].as_str().unwrap_or("node error"),
            ))
        }
    }

    /// Balance, nonce and the supply/reserve summary.
    pub fn overview(&self) -> Result<Value, WalletError> {
        let (_, address) = self.account()?;
        let account = self.node_get(&format!("/account/{address}"))?;
        let supply = self.node_get("/supply")?;
        let status = self.node_get("/status")?;
        Ok(json!({
            "address": address.encode(), "balance": account["balance"], "nonce": account["nonce"],
            "supply": supply, "chain_id": status["chain_id"], "height": status["height"],
        }))
    }

    /// The credentials issued to this account (read from the node).
    pub fn credentials(&self) -> Result<Value, WalletError> {
        let (_, address) = self.account()?;
        let r = self.node_get(&format!("/credentials/{address}"))?;
        Ok(json!({"address": address.encode(), "credentials": r["credentials"]}))
    }

    /// Transaction history for this account, from the explorer.
    pub fn history(&self, skip: u64, limit: u64) -> Result<Value, WalletError> {
        let (_, address) = self.account()?;
        let ex = self.cfg.explorer.as_deref().ok_or_else(|| {
            WalletError::new("no_explorer", "no explorer is configured for history")
        })?;
        let r = request(
            ex,
            "GET",
            &format!(
                "/api/account/{address}?skip={skip}&limit={}",
                limit.min(100)
            ),
            "",
        )
        .map_err(|e| WalletError::new("node_unreachable", e))?;
        if r.code == 200 {
            Ok(r.body)
        } else {
            Err(WalletError::new(
                "rejected",
                r.body["error"].as_str().unwrap_or("explorer error"),
            ))
        }
    }

    // ------------------------------------------------------------------ sending

    fn fee_for(&self, probe: &Tx) -> Result<u64, WalletError> {
        let f = self.node_get("/fees")?;
        let base = f["transfer_base"].as_u64().unwrap_or(1000);
        let per_byte = f["per_byte"].as_u64().unwrap_or(1);
        Ok(base + per_byte * probe.size() as u64)
    }

    /// Validates a send and returns a preview (amount, fee, total, balance after). Nothing is signed yet.
    pub fn prepare_send(&self, to: &str, amount_text: &str) -> Result<Value, WalletError> {
        let (secret, from) = self.account()?;
        let to = Address::decode(to.trim()).map_err(|_| {
            WalletError::new(
                "invalid_address",
                "the address has a typo (checksum mismatch) or is not a BRDU address",
            )
        })?;
        let amount = Amount::parse(amount_text.trim())
            .map_err(|e| WalletError::new("invalid_amount", e.to_string()))?
            .units();
        if amount == 0 {
            return Err(WalletError::new(
                "invalid_amount",
                "the amount must be greater than zero",
            ));
        }
        let account = self.node_get(&format!("/account/{from}"))?;
        let (balance, nonce) = (
            account["balance"].as_u64().unwrap_or(0),
            account["nonce"].as_u64().unwrap_or(0),
        );
        let chain_id = self.node_get("/status")?["chain_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let probe = Tx::new_signed(
            &secret,
            &chain_id,
            kind::TRANSFER,
            nonce,
            0,
            coin::transfer_payload(&to, amount),
        )
        .map_err(|e| WalletError::new("internal", e.to_string()))?;
        let fee = self.fee_for(&probe)?;
        let total = amount
            .checked_add(fee)
            .ok_or_else(|| WalletError::new("invalid_amount", "amount too large"))?;
        if total > balance {
            return Err(WalletError::new(
                "insufficient_funds",
                format!("balance {balance} is less than amount + fee {total}"),
            ));
        }
        let id = random_id();
        let mut p = self
            .prepared
            .lock()
            .map_err(|_| WalletError::new("internal", "poisoned"))?;
        p.retain(|_, v| v.at.elapsed() < PREPARED_TTL);
        if p.len() >= MAX_PREPARED {
            p.clear();
        }
        p.insert(
            id.clone(),
            Prepared {
                to,
                amount,
                fee,
                at: Instant::now(),
            },
        );
        Ok(json!({
            "id": id, "to": to.encode(), "amount": amount, "fee": fee, "total": total,
            "balance_after": balance - total, "to_self": to == from, "expires_in_secs": PREPARED_TTL.as_secs(),
        }))
    }

    /// Signs and submits a prepared send. Single-use.
    pub fn confirm_send(&self, id: &str) -> Result<Value, WalletError> {
        let (secret, from) = self.account()?;
        let prepared = {
            let mut p = self
                .prepared
                .lock()
                .map_err(|_| WalletError::new("internal", "poisoned"))?;
            p.remove(id).filter(|v| v.at.elapsed() < PREPARED_TTL)
        }
        .ok_or_else(|| {
            WalletError::new(
                "unknown_prepared_send",
                "this send expired or was already used; prepare it again",
            )
        })?;
        let account = self.node_get(&format!("/account/{from}"))?;
        let nonce = account["nonce"].as_u64().unwrap_or(0);
        let chain_id = self.node_get("/status")?["chain_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let tx = Tx::new_signed(
            &secret,
            &chain_id,
            kind::TRANSFER,
            nonce,
            prepared.fee,
            coin::transfer_payload(&prepared.to, prepared.amount),
        )
        .map_err(|e| WalletError::new("internal", e.to_string()))?;
        let raw = tx
            .encode()
            .map_err(|e| WalletError::new("internal", e.to_string()))?;
        let r = request(&self.cfg.node, "POST", "/tx", &hex::encode(&raw))
            .map_err(|e| WalletError::new("node_unreachable", e))?;
        if r.code == 200 {
            Ok(
                json!({"tx_id": r.body["id"], "amount": prepared.amount, "fee": prepared.fee, "to": prepared.to.encode()}),
            )
        } else {
            Err(WalletError::new(
                "rejected",
                r.body["error"]
                    .as_str()
                    .unwrap_or("the node refused the transaction"),
            ))
        }
    }

    // ------------------------------------------------------------------ JSON interface

    /// The single entry point for UIs: `method` + JSON `params` → JSON result. Secrets in `params` (password,
    /// phrase, passphrase) are only read here and never echoed back.
    pub fn call(&self, method: &str, p: &Value) -> Result<Value, WalletError> {
        let opt = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
        match method {
            "status" => Ok(self.status()),
            "create" => self.create(str_param(p, "password")?, opt("passphrase")),
            "preview_import" => self.preview_import(str_param(p, "words")?, opt("passphrase")),
            "import" => self.import(
                str_param(p, "words")?,
                opt("passphrase"),
                str_param(p, "password")?,
                str_param(p, "expected_address")?,
            ),
            "unlock" => self.unlock(str_param(p, "password")?, opt("passphrase")),
            "lock" => {
                self.lock();
                Ok(json!({"locked": true}))
            }
            "forget" => self.forget(str_param(p, "confirm")?),
            "export_phrase" => self.export_phrase(str_param(p, "password")?),
            "change_password" => self.change_password(str_param(p, "old")?, str_param(p, "new")?),
            "overview" => self.overview(),
            "credentials" => self.credentials(),
            "history" => self.history(
                p.get("skip").and_then(Value::as_u64).unwrap_or(0),
                p.get("limit").and_then(Value::as_u64).unwrap_or(25),
            ),
            "prepare_send" => self.prepare_send(str_param(p, "to")?, str_param(p, "amount")?),
            "confirm_send" => self.confirm_send(str_param(p, "id")?),
            other => Err(WalletError::new(
                "unknown_method",
                format!("unknown method `{other}`"),
            )),
        }
    }
}
