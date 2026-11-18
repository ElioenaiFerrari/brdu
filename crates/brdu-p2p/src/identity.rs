use std::fs;
use std::io::Write;
use std::path::Path;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Hash32, PublicKey, SecretKey, Signature, tagged};
use zeroize::Zeroizing;

use crate::error::NetError;

/// Prefixo de domínio da prova de identidade.
pub const NET_BIND: &str = "brdu/net/bind";

/// Quem pode falar com este nó: as chaves de consenso dos validadores em vigor (o hospedeiro as mantém em dia).
pub trait PeerPolicy: Send + Sync {
    /// Esta chave de consenso é aceita?
    fn accepts(&self, consensus_key: &PublicKey) -> bool;
}

/// O que um nó é na rede: a chave de consenso (a instituição), a rede e a gênese a que pertence, e o par estático
/// do Noise (feito na primeira execução, nunca sai do nó).
pub struct NetIdentity {
    pub(crate) consensus: SecretKey,
    pub(crate) chain_id: String,
    pub(crate) genesis_hash: Hash32,
    pub(crate) noise_private: Zeroizing<Vec<u8>>,
    pub(crate) noise_public: Vec<u8>,
}

/// Enviada dentro do aperto de mão: a chave de consenso e a assinatura dela sobre **esta** chave estática do
/// Noise, a rede e a gênese. Ninguém empresta a prova a outra chave nem a repete em outra rede.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityProof {
    /// Chave de consenso.
    pub consensus_key: PublicKey,
    /// Rede.
    pub chain_id: String,
    /// Gênese.
    pub genesis_hash: Hash32,
    /// Assinatura.
    pub signature: Signature,
}

pub(crate) const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_SHA256";

/// O que a prova assina. `role` (1 = quem disca, 2 = quem aceita) impede refletir a prova contra o autor.
fn bound(chain_id: &str, genesis: &Hash32, noise_static: &[u8], role: u8) -> Hash32 {
    tagged(
        NET_BIND,
        &[
            chain_id.as_bytes(),
            genesis.as_bytes(),
            noise_static,
            &[role],
        ],
    )
}

impl IdentityProof {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.fixed(self.consensus_key.as_bytes())
            .string(&self.chain_id)
            .fixed(self.genesis_hash.as_bytes())
            .fixed(self.signature.as_bytes());
        e.finish().unwrap_or_default()
    }

    pub(crate) fn decode(raw: &[u8]) -> Option<Self> {
        let mut d = Decoder::new(raw);
        let consensus_key = PublicKey(d.fixed::<32>().ok()?);
        let chain_id = d.string(64).ok()?.to_string();
        let genesis_hash = Hash32(d.fixed::<32>().ok()?);
        let signature = Signature(d.fixed::<64>().ok()?);
        d.finish().ok()?;
        Some(Self {
            consensus_key,
            chain_id,
            genesis_hash,
            signature,
        })
    }
}

impl NetIdentity {
    /// Uma identidade nova, com par Noise novo (em memória).
    pub fn generate(
        consensus: SecretKey,
        chain_id: &str,
        genesis_hash: Hash32,
    ) -> Result<Self, NetError> {
        let params = NOISE_PARAMS
            .parse()
            .map_err(|_| NetError::Handshake("noise parameters".into()))?;
        let kp = snow::Builder::new(params)
            .generate_keypair()
            .map_err(|e| NetError::Handshake(e.to_string()))?;
        Ok(Self {
            consensus,
            chain_id: chain_id.to_string(),
            genesis_hash,
            noise_private: Zeroizing::new(kp.private),
            noise_public: kp.public,
        })
    }

    /// Carrega o par Noise de `path` (hex, `privada || pública`), ou o cria e grava (0600).
    pub fn load_or_create(
        consensus: SecretKey,
        chain_id: &str,
        genesis_hash: Hash32,
        path: &Path,
    ) -> Result<Self, NetError> {
        if let Ok(text) = fs::read_to_string(path) {
            let raw = brdu_crypto::hex::decode(text.trim()).ok_or_else(|| bad_file(path))?;
            if raw.len() == 64 {
                return Ok(Self {
                    consensus,
                    chain_id: chain_id.to_string(),
                    genesis_hash,
                    noise_private: Zeroizing::new(raw[..32].to_vec()),
                    noise_public: raw[32..].to_vec(),
                });
            }
            return Err(bad_file(path));
        }
        let id = Self::generate(consensus, chain_id, genesis_hash)?;
        let mut both = id.noise_private.to_vec();
        both.extend_from_slice(&id.noise_public);
        let mut f = fs::OpenOptions::new();
        f.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            f.mode(0o600);
        }
        let mut file = f.open(path)?;
        file.write_all(brdu_crypto::hex::encode(&both).as_bytes())?;
        Ok(id)
    }

    /// A chave de consenso que nomeia este nó.
    #[must_use]
    pub fn consensus_key(&self) -> PublicKey {
        self.consensus.public()
    }

    pub(crate) fn proof(&self, role: u8) -> IdentityProof {
        let msg = bound(&self.chain_id, &self.genesis_hash, &self.noise_public, role);
        IdentityProof {
            consensus_key: self.consensus_key(),
            chain_id: self.chain_id.clone(),
            genesis_hash: self.genesis_hash,
            signature: self.consensus.sign(msg.as_bytes()),
        }
    }

    /// Confere a prova do par contra a chave estática DELE e o papel; devolve a chave de consenso dele.
    pub(crate) fn check_proof(
        &self,
        raw: &[u8],
        remote_static: &[u8],
        role: u8,
        policy: &dyn PeerPolicy,
        expect: Option<&PublicKey>,
    ) -> Result<PublicKey, NetError> {
        let bad = |m: &str| NetError::Handshake(m.to_string());
        let p = IdentityProof::decode(raw).ok_or_else(|| bad("malformed identity proof"))?;
        if p.chain_id != self.chain_id {
            return Err(bad("another chain"));
        }
        if p.genesis_hash != self.genesis_hash {
            return Err(bad("another genesis"));
        }
        let msg = bound(&p.chain_id, &p.genesis_hash, remote_static, role);
        if p.consensus_key
            .verify(msg.as_bytes(), &p.signature)
            .is_err()
        {
            return Err(bad("the proof does not bind this connection's key"));
        }
        if expect.is_some_and(|want| *want != p.consensus_key) {
            return Err(bad("not the institution that was dialled"));
        }
        if !policy.accepts(&p.consensus_key) {
            return Err(bad("not a validator in force"));
        }
        Ok(p.consensus_key)
    }
}

fn bad_file(path: &Path) -> NetError {
    NetError::Handshake(format!("{}: not a Noise key file", path.display()))
}
