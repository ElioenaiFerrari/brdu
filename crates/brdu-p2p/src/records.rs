//! Descoberta de pares: cada validador publica um registro de endereço assinado; os nós guardam o registro válido
//! mais novo de cada validador em vigor e aprendem endereços uns dos outros. Só chaves do conjunto de validadores
//! contam, então a descoberta não consegue pôr um estranho na rede.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Hash32, PublicKey, SecretKey, Signature, tagged};

/// Prefixo de domínio do registro.
pub const NET_RECORD: &str = "brdu/net/record";
/// Um registro não pode alegar viver mais que isto (chave perdida não prende um endereço para sempre).
pub const MAX_LIFETIME_SECS: u64 = 7 * 86_400;
const MAX_ADDR: usize = 255;
const MAX_RECORDS: usize = 256;

/// Onde achar um validador: endereço para pares e para a API pública, assinado pela chave de consenso. `seq` só
/// sobe; `expires_at` é em segundos unix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRecord {
    /// Quem.
    pub consensus_key: PublicKey,
    /// `host:porta` para os outros validadores.
    pub peer_addr: String,
    /// `host:porta` da API pública (pode ser vazio).
    pub public_addr: String,
    /// Sequência.
    pub seq: u64,
    /// Validade (unix).
    pub expires_at: u64,
    /// Assinatura.
    pub signature: Signature,
}

fn signing_bytes(chain_id: &str, r: &PeerRecord) -> Hash32 {
    tagged(
        NET_RECORD,
        &[
            chain_id.as_bytes(),
            r.consensus_key.as_bytes(),
            r.peer_addr.as_bytes(),
            r.public_addr.as_bytes(),
            &r.seq.to_be_bytes(),
            &r.expires_at.to_be_bytes(),
        ],
    )
}

/// `host:porta` com porta numérica e host simples (IP ou nome DNS).
#[must_use]
pub fn valid_addr(a: &str) -> bool {
    let Some((host, port)) = a.rsplit_once(':') else {
        return false;
    };
    a.len() <= MAX_ADDR
        && !host.is_empty()
        && port.parse::<u16>().is_ok_and(|p| p != 0)
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

impl PeerRecord {
    /// Assina um registro.
    #[must_use]
    pub fn sign(
        key: &SecretKey,
        chain_id: &str,
        peer_addr: &str,
        public_addr: &str,
        seq: u64,
        expires_at: u64,
    ) -> Self {
        let mut r = Self {
            consensus_key: key.public(),
            peer_addr: peer_addr.to_string(),
            public_addr: public_addr.to_string(),
            seq,
            expires_at,
            signature: Signature([0; 64]),
        };
        r.signature = key.sign(signing_bytes(chain_id, &r).as_bytes());
        r
    }

    /// A assinatura confere?
    #[must_use]
    pub fn verify(&self, chain_id: &str) -> bool {
        self.consensus_key
            .verify(signing_bytes(chain_id, self).as_bytes(), &self.signature)
            .is_ok()
    }

    /// Codificação determinística.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.fixed(self.consensus_key.as_bytes())
            .string(&self.peer_addr)
            .string(&self.public_addr)
            .u64(self.seq)
            .u64(self.expires_at)
            .fixed(self.signature.as_bytes());
        e.finish().unwrap_or_default()
    }

    /// Lê um registro.
    #[must_use]
    pub fn decode(raw: &[u8]) -> Option<Self> {
        let mut d = Decoder::new(raw);
        let r = Self {
            consensus_key: PublicKey(d.fixed::<32>().ok()?),
            peer_addr: d.string(MAX_ADDR).ok()?.to_string(),
            public_addr: d.string(MAX_ADDR).ok()?.to_string(),
            seq: d.u64().ok()?,
            expires_at: d.u64().ok()?,
            signature: Signature(d.fixed::<64>().ok()?),
        };
        d.finish().ok()?;
        Some(r)
    }
}

/// O que um `merge` fez com um registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Novo.
    Added,
    /// Substituiu um mais antigo.
    Updated,
    /// Não é mais novo que o que já há.
    Stale,
    /// A chave não está no conjunto de validadores.
    NotValidator,
    /// Vencido.
    Expired,
    /// Assinatura inválida.
    BadSignature,
    /// Endereços malformados ou validade além do máximo.
    Invalid,
}

/// O registro válido mais novo de cada validador.
pub struct PeerBook {
    chain_id: String,
    records: BTreeMap<PublicKey, PeerRecord>,
}

impl PeerBook {
    /// Livro vazio.
    #[must_use]
    pub fn new(chain_id: &str) -> Self {
        Self {
            chain_id: chain_id.to_string(),
            records: BTreeMap::new(),
        }
    }

    /// Considera um registro. `is_validator` diz se a chave pertence ao conjunto em vigor.
    pub fn merge(
        &mut self,
        r: PeerRecord,
        is_validator: &dyn Fn(&PublicKey) -> bool,
        now: u64,
    ) -> MergeOutcome {
        if !is_validator(&r.consensus_key) {
            return MergeOutcome::NotValidator;
        }
        if !valid_addr(&r.peer_addr)
            || (!r.public_addr.is_empty() && !valid_addr(&r.public_addr))
            || r.expires_at > now.saturating_add(MAX_LIFETIME_SECS)
        {
            return MergeOutcome::Invalid;
        }
        if r.expires_at <= now {
            return MergeOutcome::Expired;
        }
        if !r.verify(&self.chain_id) {
            return MergeOutcome::BadSignature;
        }
        match self.records.get(&r.consensus_key) {
            Some(old) if old.seq >= r.seq => MergeOutcome::Stale,
            Some(_) => {
                self.records.insert(r.consensus_key, r);
                MergeOutcome::Updated
            }
            None => {
                if self.records.len() >= MAX_RECORDS {
                    return MergeOutcome::Invalid;
                }
                self.records.insert(r.consensus_key, r);
                MergeOutcome::Added
            }
        }
    }

    /// O registro de uma chave.
    #[must_use]
    pub fn get(&self, key: &PublicKey) -> Option<&PeerRecord> {
        self.records.get(key)
    }

    /// Os registros ainda válidos em `now`.
    #[must_use]
    pub fn live(&self, now: u64) -> Vec<PeerRecord> {
        self.records
            .values()
            .filter(|r| r.expires_at > now)
            .cloned()
            .collect()
    }

    /// Esquece as chaves que saíram do conjunto e os registros vencidos.
    pub fn prune(&mut self, is_validator: &dyn Fn(&PublicKey) -> bool, now: u64) {
        self.records
            .retain(|k, r| is_validator(k) && r.expires_at > now);
    }

    /// Grava o livro (atômico: escreve e renomeia).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut e = Encoder::new();
        e.count(self.records.len());
        for r in self.records.values() {
            e.bytes(&r.encode());
        }
        let bytes = e
            .finish()
            .map_err(|_| std::io::Error::other("book too large"))?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)
    }

    /// Carrega um livro salvo; registro que não confere mais é descartado. Arquivo ausente = livro vazio.
    #[must_use]
    pub fn load(chain_id: &str, path: &Path) -> Self {
        let mut me = Self::new(chain_id);
        let Ok(raw) = fs::read(path) else { return me };
        let mut d = Decoder::new(&raw);
        let Ok(n) = d.count(MAX_RECORDS) else {
            return me;
        };
        for _ in 0..n {
            let Some(r) = d.bytes(1024).ok().and_then(PeerRecord::decode) else {
                break;
            };
            if r.verify(chain_id) && valid_addr(&r.peer_addr) {
                me.records.insert(r.consensus_key, r);
            }
        }
        me
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn key(seed: u64) -> SecretKey {
        SecretKey::from_bytes([seed as u8 + 1; 32])
    }

    const NOW: u64 = 1_900_000_000;

    fn rec(seed: u64, addr: &str, seq: u64, ttl: u64) -> PeerRecord {
        PeerRecord::sign(&key(seed), "c", addr, "", seq, NOW + ttl)
    }

    fn only(seeds: &[u64]) -> impl Fn(&PublicKey) -> bool {
        let keys: Vec<PublicKey> = seeds.iter().map(|s| key(*s).public()).collect();
        move |k: &PublicKey| keys.iter().any(|x| x == k)
    }

    #[test]
    fn only_the_newest_valid_record_of_a_validator_is_kept() {
        let mut b = PeerBook::new("c");
        let v = only(&[1, 2]);
        assert_eq!(
            b.merge(rec(1, "a.example:7001", 1, 100), &v, NOW),
            MergeOutcome::Added
        );
        assert_eq!(
            b.merge(rec(1, "b.example:7001", 1, 100), &v, NOW),
            MergeOutcome::Stale
        );
        assert_eq!(
            b.merge(rec(1, "b.example:7001", 2, 100), &v, NOW),
            MergeOutcome::Updated
        );
        let k = key(1).public();
        assert_eq!(b.get(&k).unwrap().peer_addr, "b.example:7001");
        // an older record cannot take the address back
        assert_eq!(
            b.merge(rec(1, "a.example:7001", 1, 100), &v, NOW),
            MergeOutcome::Stale
        );
    }

    #[test]
    fn strangers_forgeries_expired_and_malformed_records_are_ignored() {
        let mut b = PeerBook::new("c");
        let v = only(&[1]);
        assert_eq!(
            b.merge(rec(9, "x:1", 1, 100), &v, NOW),
            MergeOutcome::NotValidator
        );
        // signed by someone else but naming validator 1
        let mut forged = rec(9, "evil:7001", 5, 100);
        forged.consensus_key = key(1).public();
        assert_eq!(b.merge(forged, &v, NOW), MergeOutcome::BadSignature);
        // another chain
        let other = PeerRecord::sign(&key(1), "other", "a:7001", "", 1, NOW + 100);
        assert_eq!(b.merge(other, &v, NOW), MergeOutcome::BadSignature);
        // expired, too long-lived, bad addresses
        assert_eq!(
            b.merge(rec(1, "a:7001", 1, 0), &v, NOW),
            MergeOutcome::Expired
        );
        assert_eq!(
            b.merge(rec(1, "a:7001", 1, 8 * 86_400), &v, NOW),
            MergeOutcome::Invalid
        );
        for bad in ["nohost", ":7001", "a:0", "a:99999", "a b:7001", "a:7001\n"] {
            assert_eq!(
                b.merge(rec(1, bad, 1, 100), &v, NOW),
                MergeOutcome::Invalid,
                "{bad}"
            );
        }
        assert!(b.live(NOW).is_empty());
        // editing any signed field breaks the signature
        let mut r = rec(1, "a:7001", 1, 100);
        r.seq = 9;
        assert_eq!(b.merge(r, &v, NOW), MergeOutcome::BadSignature);
    }

    #[test]
    fn the_book_forgets_who_left_the_set_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("brdu-book-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("peers.book");
        let mut b = PeerBook::new("c");
        let v = only(&[1, 2]);
        b.merge(rec(1, "a:7001", 1, 100), &v, NOW);
        b.merge(rec(2, "b:7002", 1, 100), &v, NOW);
        b.save(&path).unwrap();
        let again = PeerBook::load("c", &path);
        assert_eq!(again.live(NOW).len(), 2);
        // another chain's book is worth nothing; so is garbage
        assert!(PeerBook::load("other", &path).live(NOW).is_empty());
        std::fs::write(&path, b"garbage").unwrap();
        assert!(PeerBook::load("c", &path).live(NOW).is_empty());
        // key 2 leaves the set, and records expire
        let mut b = again;
        b.prune(&only(&[1]), NOW);
        assert_eq!(b.live(NOW).len(), 1);
        b.prune(&only(&[1]), NOW + 200);
        assert!(b.live(NOW + 200).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
