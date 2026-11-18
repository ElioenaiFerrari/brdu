//! Carteira do BRDU (ADR-0005, 0021): frase BIP-39, derivação SLIP-10 Ed25519 e contas.
//!
//! - **A frase** tem 24 palavras (12 também é aceita ao importar). A **passphrase** é a "25ª palavra": a mesma frase
//!   com outra passphrase gera **outra carteira**, válida e vazia, sem nenhum erro. Por isso a importação pede
//!   a confirmação do endereço (ver [`Wallet::address`]).
//! - **A derivação** é SLIP-10 para Ed25519, com todos os índices endurecidos: `m/44'/1112687701'/conta'/0'/índice'`.
//!   `1112687701` é "BRDU" em ASCII (0x42524455) e **não está registrado no SLIP-44**.
//! - **Segredos** ficam em buffers `Zeroizing`, sem `Debug` nem `Clone`. A frase só sai daqui para ser mostrada ao
//!   dono, uma vez. **Não auditado.**
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::fmt;

use bip39::{Language, Mnemonic};
use brdu_crypto::{Address, PublicKey, SecretKey};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use thiserror::Error;
use zeroize::Zeroizing;

/// "BRDU" em ASCII, usado como tipo de moeda no caminho de derivação.
pub const COIN_TYPE: u32 = 0x4252_4455;
/// Maior passphrase aceita (só ASCII imprimível, para não depender de normalização Unicode).
pub const MAX_PASSPHRASE: usize = 64;
const HARDENED: u32 = 0x8000_0000;

/// Falhas da carteira. Nenhuma mensagem carrega segredo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WalletError {
    /// Não deu para obter entropia do sistema.
    #[error("sem entropia do sistema")]
    Entropy,
    /// Quantidade de palavras errada, palavra fora da lista ou checksum inválido.
    #[error("frase inválida")]
    InvalidPhrase,
    /// Passphrase com caractere fora do ASCII imprimível ou longa demais.
    #[error("passphrase inválida")]
    InvalidPassphrase,
    /// Caminho de derivação malformado ou com índice não endurecido.
    #[error("caminho de derivação inválido")]
    BadPath,
}

/// A frase de recuperação. `Debug` é redigido; nunca é `Display`ada nem logada.
pub struct Phrase {
    mnemonic: Zeroizing<String>,
}

impl fmt::Debug for Phrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Phrase(<redigida>)")
    }
}

impl Phrase {
    /// Gera uma frase nova de 24 palavras com 32 bytes de entropia do sistema.
    pub fn generate() -> Result<Self, WalletError> {
        let mut entropy = Zeroizing::new([0u8; 32]);
        getrandom::fill(entropy.as_mut_slice()).map_err(|_| WalletError::Entropy)?;
        Self::from_entropy(&entropy)
    }

    /// Frase de 24 palavras a partir de 32 bytes de entropia (para testes e vetores).
    pub fn from_entropy(entropy: &[u8; 32]) -> Result<Self, WalletError> {
        let m = Mnemonic::from_entropy_in(Language::English, entropy)
            .map_err(|_| WalletError::InvalidPhrase)?;
        Ok(Self {
            mnemonic: Zeroizing::new(m.to_string()),
        })
    }

    /// Lê palavras digitadas: 12 ou 24, todas da lista BIP-39 em inglês, com checksum válido. Perdoa maiúsculas e
    /// espaços sobrando.
    pub fn parse(text: &str) -> Result<Self, WalletError> {
        let words: Vec<String> = text
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        if words.len() != 12 && words.len() != 24 {
            return Err(WalletError::InvalidPhrase);
        }
        let joined = Zeroizing::new(words.join(" "));
        Mnemonic::parse_in_normalized(Language::English, joined.as_str())
            .map_err(|_| WalletError::InvalidPhrase)?;
        Ok(Self { mnemonic: joined })
    }

    /// As palavras, para mostrar ao dono uma vez (na tela de criação). Quem chama é quem não pode registrar.
    #[must_use]
    pub fn words(&self) -> Vec<&str> {
        self.mnemonic.split(' ').collect()
    }

    /// Quantas palavras.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words().len()
    }

    /// Sem palavras? (nunca acontece numa frase válida)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mnemonic.is_empty()
    }

    /// A semente BIP-39 de 64 bytes (PBKDF2-HMAC-SHA512, 2048 voltas, sal `"mnemonic" + passphrase`).
    fn seed(&self, passphrase: &str) -> Result<Zeroizing<[u8; 64]>, WalletError> {
        check_passphrase(passphrase)?;
        let m = Mnemonic::parse_in_normalized(Language::English, self.mnemonic.as_str())
            .map_err(|_| WalletError::InvalidPhrase)?;
        Ok(Zeroizing::new(m.to_seed_normalized(passphrase)))
    }

    /// A carteira desta frase com esta passphrase (vazia se não houver).
    pub fn wallet(&self, passphrase: &str) -> Result<Wallet, WalletError> {
        Ok(Wallet {
            seed: self.seed(passphrase)?,
        })
    }
}

fn check_passphrase(p: &str) -> Result<(), WalletError> {
    if p.len() > MAX_PASSPHRASE || !p.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err(WalletError::InvalidPassphrase);
    }
    Ok(())
}

/// A carteira: de uma semente, deriva contas. `Debug` é redigido e não há `Clone`.
pub struct Wallet {
    seed: Zeroizing<[u8; 64]>,
}

impl fmt::Debug for Wallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Wallet(<redigida>)")
    }
}

impl Wallet {
    /// O caminho de derivação da conta `account` e índice `index`.
    #[must_use]
    pub fn path(account: u32, index: u32) -> String {
        format!("m/44'/{COIN_TYPE}'/{account}'/0'/{index}'")
    }

    /// A conta no caminho padrão.
    pub fn account(&self, account: u32, index: u32) -> Result<Account, WalletError> {
        self.derive(&Self::path(account, index))
    }

    /// A conta no caminho `path` (todos os índices endurecidos, ex.: `m/0'`). Serve também aos vetores SLIP-10.
    pub fn derive(&self, path: &str) -> Result<Account, WalletError> {
        let node = derive_path(self.seed.as_slice(), path)?;
        let secret = SecretKey::from_bytes(*node.key);
        let public = secret.public();
        Ok(Account {
            path: path.to_string(),
            address: Address::from_public_key(&public),
            public,
            secret,
        })
    }

    /// A raiz da carteira (chave e código de cadeia do nó mestre), para os vetores.
    pub fn master(&self) -> Result<Node, WalletError> {
        master_node(self.seed.as_slice())
    }
}

/// Uma conta derivada: caminho, chave pública, endereço e a chave privada.
pub struct Account {
    path: String,
    address: Address,
    public: PublicKey,
    secret: SecretKey,
}

impl Account {
    /// O caminho de derivação.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// O endereço `brdu1…`.
    #[must_use]
    pub const fn address(&self) -> &Address {
        &self.address
    }

    /// A chave pública.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// A chave privada (para assinar). Nunca é `Debug`.
    #[must_use]
    pub const fn secret_key(&self) -> &SecretKey {
        &self.secret
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Account({}, {})", self.path, self.address)
    }
}

/// Um nó SLIP-10: chave de 32 bytes e código de cadeia de 32 bytes.
pub struct Node {
    /// A chave privada do nó.
    pub key: Zeroizing<[u8; 32]>,
    /// O código de cadeia.
    pub chain_code: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Node(<redigido>)")
    }
}

type HmacSha512 = Hmac<Sha512>;

fn split(mac: &[u8]) -> Result<Node, WalletError> {
    let (k, c) = mac.split_at_checked(32).ok_or(WalletError::BadPath)?;
    Ok(Node {
        key: Zeroizing::new(k.try_into().map_err(|_| WalletError::BadPath)?),
        chain_code: Zeroizing::new(c.try_into().map_err(|_| WalletError::BadPath)?),
    })
}

/// Nó mestre do SLIP-10 para Ed25519: `HMAC-SHA512("ed25519 seed", semente)`.
fn master_node(seed: &[u8]) -> Result<Node, WalletError> {
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed").map_err(|_| WalletError::BadPath)?;
    mac.update(seed);
    split(&mac.finalize().into_bytes())
}

/// Filho endurecido: `HMAC-SHA512(código_de_cadeia, 0x00 ‖ chave ‖ índice_big_endian)`.
fn child(parent: &Node, index: u32) -> Result<Node, WalletError> {
    let mut mac = HmacSha512::new_from_slice(parent.chain_code.as_slice())
        .map_err(|_| WalletError::BadPath)?;
    mac.update(&[0x00]);
    mac.update(parent.key.as_slice());
    mac.update(&(index | HARDENED).to_be_bytes());
    split(&mac.finalize().into_bytes())
}

/// Deriva um nó SLIP-10 Ed25519 de uma semente **qualquer** (não só de 64 bytes) e de um caminho endurecido.
/// Existe para os vetores oficiais do SLIP-10 (semente de 16 bytes); a carteira usa [`Wallet::derive`].
pub fn slip10_derive(seed: &[u8], path: &str) -> Result<Node, WalletError> {
    derive_path(seed, path)
}

fn derive_path(seed: &[u8], path: &str) -> Result<Node, WalletError> {
    let mut parts = path.split('/');
    if parts.next() != Some("m") {
        return Err(WalletError::BadPath);
    }
    let mut node = master_node(seed)?;
    for part in parts {
        // só índices endurecidos (`44'`): Ed25519 não tem derivação pública
        let digits = part.strip_suffix('\'').ok_or(WalletError::BadPath)?;
        let index: u32 = digits.parse().map_err(|_| WalletError::BadPath)?;
        if index >= HARDENED {
            return Err(WalletError::BadPath);
        }
        node = child(&node, index)?;
    }
    Ok(node)
}
