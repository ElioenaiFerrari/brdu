//! Endereço BRDU (ADR-0010): `brdu1…` em bech32m.
//!
//! `endereço = bech32m("brdu", hash_etiquetado("brdu/addr", chave_pública))`. O endereço guarda o **hash** da chave,
//! então não revela a chave pública até a primeira assinatura.

use alloc::string::String;
use core::fmt;

use crate::bech32m;
use crate::error::AddressError;
use crate::hash::{Hash32, tagged, tags};
use crate::sign::PublicKey;

/// Prefixo legível dos endereços do BRDU.
pub const HRP: &str = "brdu";

/// O hash que identifica uma conta no livro-razão.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address(pub Hash32);

impl Address {
    /// Endereço de uma chave pública.
    #[must_use]
    pub fn from_public_key(key: &PublicKey) -> Self {
        Self(tagged(tags::ADDR, &[key.as_bytes()]))
    }

    /// O hash de 32 bytes que o livro-razão guarda.
    #[must_use]
    pub const fn hash(&self) -> &Hash32 {
        &self.0
    }

    /// O texto `brdu1…`.
    #[must_use]
    pub fn encode(&self) -> String {
        // o prefixo é constante e válido, e o conteúdo tem 32 bytes: não há como falhar
        bech32m::encode(HRP, self.0.as_bytes()).unwrap_or_default()
    }

    /// Lê e valida um endereço `brdu1…`.
    pub fn decode(text: &str) -> Result<Self, AddressError> {
        let (hrp, data) = bech32m::decode(text)?;
        if hrp != HRP {
            return Err(AddressError::WrongPrefix);
        }
        let bytes: [u8; 32] = data.try_into().map_err(|_| AddressError::BadSize)?;
        Ok(Self(Hash32(bytes)))
    }

    /// A chave pública corresponde a este endereço?
    #[must_use]
    pub fn matches(&self, key: &PublicKey) -> bool {
        *self == Self::from_public_key(key)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.encode())
    }
}
