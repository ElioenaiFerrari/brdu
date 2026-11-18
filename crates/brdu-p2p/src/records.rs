//! Peer discovery: each validator publishes a signed address record; nodes keep the newest valid record
//! of each validator in force and learn addresses from one another. Only keys from the validator set
//! count, so discovery cannot put a stranger on the network.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Hash32, PublicKey, SecretKey, Signature, tagged};

/// Domain prefix of the record.
pub const NET_RECORD: &str = "brdu/net/record";
/// A record cannot claim to live longer than this (a lost key does not pin an address forever).
pub const MAX_LIFETIME_SECS: u64 = 7 * 86_400;
const MAX_ADDR: usize = 255;
const MAX_RECORDS: usize = 256;

/// Where to find a validator: address for peers and for the public API, signed by the consensus key. `seq` only
/// increases; `expires_at` is in unix seconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerRecord {
    /// Who.
    pub consensus_key: PublicKey,
    /// `host:port` for the other validators.
    pub peer_addr: String,
    /// `host:port` of the public API (may be empty).
    pub public_addr: String,
    /// Sequence.
    pub seq: u64,
    /// Expiry (unix).
    pub expires_at: u64,
    /// Signature.
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

/// `host:port` with a numeric port and a simple host (IP or DNS name).
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
    /// Signs a record.
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

    /// Does the signature check out?
    #[must_use]
    pub fn verify(&self, chain_id: &str) -> bool {
        self.consensus_key
            .verify(signing_bytes(chain_id, self).as_bytes(), &self.signature)
            .is_ok()
    }

    /// Deterministic encoding.
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

    /// Reads a record.
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

/// What a `merge` did with a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeOutcome {
    /// New.
    Added,
    /// Replaced an older one.
    Updated,
    /// Not newer than the one already there.
    Stale,
    /// The key is not in the validator set.
    NotValidator,
    /// Expired.
    Expired,
    /// Invalid signature.
    BadSignature,
    /// Malformed addresses or expiry beyond the maximum.
    Invalid,
}

/// The newest valid record of each validator.
pub struct PeerBook {
    chain_id: String,
    records: BTreeMap<PublicKey, PeerRecord>,
}

impl PeerBook {
    /// Empty book.
    #[must_use]
    pub fn new(chain_id: &str) -> Self {
        Self {
            chain_id: chain_id.to_string(),
            records: BTreeMap::new(),
        }
    }

    /// Considers a record. `is_validator` says whether the key belongs to the set in force.
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

    /// The record of a key.
    #[must_use]
    pub fn get(&self, key: &PublicKey) -> Option<&PeerRecord> {
        self.records.get(key)
    }

    /// The records still valid at `now`.
    #[must_use]
    pub fn live(&self, now: u64) -> Vec<PeerRecord> {
        self.records
            .values()
            .filter(|r| r.expires_at > now)
            .cloned()
            .collect()
    }

    /// Forgets the keys that left the set and the expired records.
    pub fn prune(&mut self, is_validator: &dyn Fn(&PublicKey) -> bool, now: u64) {
        self.records
            .retain(|k, r| is_validator(k) && r.expires_at > now);
    }

    /// Saves the book (atomic: writes and renames).
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

    /// Loads a saved book; a record that no longer checks out is discarded. Missing file = empty book.
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
