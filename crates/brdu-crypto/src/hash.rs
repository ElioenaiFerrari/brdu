//! Hash SHA3-256 com prefixo de domínio (ADR-0011).
//!
//! `tagged(tag, dados) = SHA3-256(tamanho_da_tag (4 bytes, big-endian) ‖ tag ‖ dados)`.
//! O prefixo garante que o hash de uma coisa (uma transação) nunca seja aceito como o de outra (um bloco).

use alloc::string::String;
use core::fmt;
use sha3::{Digest, Sha3_256};

use crate::hex;

/// Tamanho do hash em bytes.
pub const HASH_LEN: usize = 32;

/// Prefixos de domínio do BRDU. Cada uso de hash tem o seu.
pub mod tags {
    /// Endereço: hash da chave pública.
    pub const ADDR: &str = "brdu/addr";
    /// Assinatura de transação (o que o remetente assina).
    pub const TX: &str = "brdu/tx";
    /// Identificador de transação (hash da codificação completa, com assinatura).
    pub const TXID: &str = "brdu/txid";
    /// Documento que os emissores assinam num mint (`brdu/issuance/mint`) ou burn (`brdu/issuance/burn`).
    pub const ISSUANCE_MINT: &str = "brdu/issuance/mint";
    /// Documento que os emissores assinam num burn.
    pub const ISSUANCE_BURN: &str = "brdu/issuance/burn";
    /// Documento que os emissores assinam num atestado de reserva.
    pub const ATTESTATION: &str = "brdu/attestation";
    /// Folha de uma conta na raiz de estado.
    pub const STATE_ACCOUNT: &str = "brdu/state/account";
    /// Folha de uma referência já usada (mint/burn) na raiz de estado.
    pub const STATE_REF: &str = "brdu/state/ref";
    /// Folha do armazenamento de um módulo na raiz de estado.
    pub const STATE_EXT: &str = "brdu/state/ext";
    /// Escalares do estado (oferta, atestado, altura, hora).
    pub const STATE_META: &str = "brdu/state/meta";
    /// Hash do bloco.
    pub const BLOCK: &str = "brdu/block";
    /// Raiz de estado.
    pub const STATE: &str = "brdu/state";
    /// Folha de Merkle.
    pub const MERKLE_LEAF: &str = "brdu/merkle/leaf";
    /// Nó interno de Merkle.
    pub const MERKLE_NODE: &str = "brdu/merkle/node";
    /// Raiz de uma árvore de Merkle vazia.
    pub const MERKLE_EMPTY: &str = "brdu/merkle/empty";
}

/// 32 bytes de hash. `Debug` e `Display` mostram hexadecimal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash32(pub [u8; HASH_LEN]);

impl Hash32 {
    /// 32 bytes zero (o "hash anterior" do primeiro bloco).
    pub const ZERO: Self = Self([0u8; HASH_LEN]);

    /// Os bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    /// Hexadecimal minúsculo.
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Lê 64 caracteres hexadecimais.
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        hex::decode_array::<HASH_LEN>(text).map(Self)
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32({})", self.to_hex())
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// SHA3-256 puro.
#[must_use]
pub fn sha3(data: &[u8]) -> Hash32 {
    Hash32(Sha3_256::digest(data).into())
}

/// SHA3-256 com prefixo de domínio, sobre a concatenação de `parts` (sem copiar tudo para um buffer).
#[must_use]
pub fn tagged(tag: &str, parts: &[&[u8]]) -> Hash32 {
    let mut h = Sha3_256::new();
    // o tamanho da tag nunca passa de u32: tags são constantes curtas; `try_from` evita um `as` silencioso
    let len = u32::try_from(tag.len()).unwrap_or(u32::MAX);
    h.update(len.to_be_bytes());
    h.update(tag.as_bytes());
    for p in parts {
        h.update(p);
    }
    Hash32(h.finalize().into())
}
