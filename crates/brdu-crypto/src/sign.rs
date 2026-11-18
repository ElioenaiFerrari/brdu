//! Ed25519 (RFC 8032) com verificação estrita.
//!
//! - A chave privada nunca implementa `Debug`, `Display`, `Clone` nem `Serialize`, e é zerada ao sair da memória.
//! - A verificação é **estrita** (`verify_strict`): recusa assinaturas não canônicas e chaves de ordem pequena,
//!   para a mesma mensagem não ter duas assinaturas válidas.

use core::fmt;
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey, VerifyingKey};
use zeroize::Zeroizing;

use crate::error::CryptoError;
use crate::hash::tagged;
use crate::hex;

/// Chave privada Ed25519 (32 bytes de semente). Segredo: `Debug` é redigido.
pub struct SecretKey(Zeroizing<[u8; 32]>);

impl SecretKey {
    /// Monta a chave a partir de 32 bytes de semente (por exemplo, de uma derivação SLIP-10).
    #[must_use]
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        Self(Zeroizing::new(seed))
    }

    /// A chave pública correspondente.
    #[must_use]
    pub fn public(&self) -> PublicKey {
        PublicKey(SigningKey::from_bytes(&self.0).verifying_key().to_bytes())
    }

    /// Assina `message` (determinístico).
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(SigningKey::from_bytes(&self.0).sign(message).to_bytes())
    }

    /// Assina o hash etiquetado de `data` com o prefixo de domínio `tag` (é como o BRDU assina tudo).
    #[must_use]
    pub fn sign_tagged(&self, tag: &str, data: &[u8]) -> Signature {
        self.sign(tagged(tag, &[data]).as_bytes())
    }

    /// Expõe os 32 bytes da semente. **Só** para quem precisa gravar o segredo cifrado (o cofre da carteira).
    #[must_use]
    pub fn expose_seed(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(<redigida>)")
    }
}

/// Chave pública Ed25519 (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    /// Os bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hexadecimal minúsculo.
    #[must_use]
    pub fn to_hex(&self) -> alloc::string::String {
        hex::encode(&self.0)
    }

    /// Confere `signature` sobre `message` (estrito).
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), CryptoError> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| CryptoError::BadKey)?;
        let sig = DalekSignature::from_bytes(&signature.0);
        key.verify_strict(message, &sig)
            .map_err(|_| CryptoError::BadSignature)
    }

    /// Confere uma assinatura feita com [`SecretKey::sign_tagged`].
    pub fn verify_tagged(
        &self,
        tag: &str,
        data: &[u8],
        signature: &Signature,
    ) -> Result<(), CryptoError> {
        self.verify(tagged(tag, &[data]).as_bytes(), signature)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.to_hex())
    }
}

/// Assinatura Ed25519 (64 bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    /// Os bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({})", hex::encode(&self.0))
    }
}

/// Confere várias assinaturas de uma vez. Com a feature `batch` usa a verificação em lote do dalek (bem mais
/// rápida para um bloco cheio); sem ela, confere uma a uma. Em qualquer caso, **todas** têm de ser válidas.
///
/// Atenção: a verificação em lote do dalek **não é estrita** (aceita o que a estrita recusaria). Por isso, quando
/// o lote passa, o BRDU ainda confere cada item em modo estrito se `strict` for `true` (o padrão do nó).
pub fn verify_all(
    items: &[(&PublicKey, &[u8], &Signature)],
    strict: bool,
) -> Result<(), CryptoError> {
    #[cfg(feature = "batch")]
    {
        if !strict {
            return verify_batch(items);
        }
    }
    #[cfg(not(feature = "batch"))]
    let _ = strict;
    items
        .iter()
        .try_for_each(|(pk, msg, sig)| pk.verify(msg, sig))
}

#[cfg(feature = "batch")]
fn verify_batch(items: &[(&PublicKey, &[u8], &Signature)]) -> Result<(), CryptoError> {
    let mut keys = alloc::vec::Vec::with_capacity(items.len());
    let mut sigs = alloc::vec::Vec::with_capacity(items.len());
    let mut msgs = alloc::vec::Vec::with_capacity(items.len());
    for (pk, msg, sig) in items {
        keys.push(VerifyingKey::from_bytes(&pk.0).map_err(|_| CryptoError::BadKey)?);
        sigs.push(DalekSignature::from_bytes(&sig.0));
        msgs.push(*msg);
    }
    ed25519_dalek::verify_batch(&msgs, &sigs, &keys).map_err(|_| CryptoError::BadSignature)
}
