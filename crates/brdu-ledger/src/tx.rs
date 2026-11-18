//! Transação assinada (formato do ADR-0005, o mesmo do BRDC).
//!
//! Campos: `chain_id`, `kind`, `nonce`, `fee`, `sender` (chave pública), `payload` e `signature`. A assinatura cobre
//! **todos** os campos, inclusive o `chain_id` (impede reaproveitar a transação em outra rede) e o `nonce` (impede
//! replay): `assinatura = Ed25519(chave, SHA3-256 etiquetado("brdu/tx", corpo))`.

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, SecretKey, Signature, tagged, tags};

use crate::error::LedgerError;

/// Maior carga aceita (64 KiB).
pub const MAX_PAYLOAD: usize = 64 * 1024;
/// Maior `chain_id` aceito, em bytes.
pub const MAX_CHAIN_ID: usize = 64;

/// Tipos de transação (`kind`). Os de 5 a 21 são dos módulos de bens, credenciais e contratos.
pub mod kind {
    /// Transferência entre contas.
    pub const TRANSFER: u8 = 1;
    /// Mint: criar BRDU contra reais recebidos (M-de-N emissores).
    pub const MINT: u8 = 2;
    /// Burn: destruir BRDU contra reais devolvidos (M-de-N emissores, só do próprio dono).
    pub const BURN: u8 = 3;
    /// Atestado de reserva (M-de-N emissores).
    pub const ATTESTATION: u8 = 4;
    /// Primeiro tipo de módulo (bens).
    pub const FIRST_MODULE: u8 = 5;
    /// Troca no conjunto de validadores (rotação, ADR-0015).
    pub const VALIDATOR_CHANGE: u8 = 22;
    /// Último tipo conhecido.
    pub const LAST: u8 = 22;
}

/// Uma transação assinada.
#[derive(Clone, PartialEq, Eq)]
pub struct Tx {
    /// A rede.
    pub chain_id: String,
    /// O tipo ([`kind`]).
    pub kind: u8,
    /// O nonce do remetente (começa em 0 e sobe de 1 em 1).
    pub nonce: u64,
    /// A taxa em unidades mínimas.
    pub fee: u64,
    /// A chave pública do remetente.
    pub sender: PublicKey,
    /// A carga, interpretada conforme o tipo.
    pub payload: Vec<u8>,
    /// A assinatura.
    pub signature: Signature,
}

impl core::fmt::Debug for Tx {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Tx(kind={}, nonce={}, fee={}, id={})",
            self.kind,
            self.nonce,
            self.fee,
            self.id()
        )
    }
}

fn body(
    chain_id: &str,
    kind: u8,
    nonce: u64,
    fee: u64,
    sender: &PublicKey,
    payload: &[u8],
) -> Result<Vec<u8>, LedgerError> {
    let mut e = Encoder::new();
    e.string(chain_id)
        .u8(kind)
        .u64(nonce)
        .u64(fee)
        .fixed(sender.as_bytes())
        .bytes(payload);
    e.finish().map_err(LedgerError::Decode)
}

impl Tx {
    /// Monta e assina. A chave pública do remetente sai da própria chave privada.
    pub fn new_signed(
        key: &SecretKey,
        chain_id: &str,
        kind: u8,
        nonce: u64,
        fee: u64,
        payload: Vec<u8>,
    ) -> Result<Self, LedgerError> {
        let sender = key.public();
        let signing = body(chain_id, kind, nonce, fee, &sender, &payload)?;
        let signature = key.sign_tagged(tags::TX, &signing);
        Ok(Self {
            chain_id: chain_id.to_string(),
            kind,
            nonce,
            fee,
            sender,
            payload,
            signature,
        })
    }

    /// Os bytes que são assinados (tudo, menos a assinatura).
    pub fn signing_bytes(&self) -> Result<Vec<u8>, LedgerError> {
        body(
            &self.chain_id,
            self.kind,
            self.nonce,
            self.fee,
            &self.sender,
            &self.payload,
        )
    }

    /// Confere forma e assinatura (não confere a rede nem o nonce: isso é do executor).
    pub fn verify(&self) -> Result<(), LedgerError> {
        self.verify_with_tag(tags::TX)
    }

    /// Como [`Tx::verify`], com outro prefixo de domínio (os vetores do BRDC usam `brdc/tx`).
    pub fn verify_with_tag(&self, tag: &str) -> Result<(), LedgerError> {
        self.check_shape()?;
        self.sender
            .verify_tagged(tag, &self.signing_bytes()?, &self.signature)
            .map_err(|_| LedgerError::BadSignature)
    }

    /// Só a forma (limites e tipo conhecido), sem criptografia.
    pub fn check_shape(&self) -> Result<(), LedgerError> {
        if self.payload.len() > MAX_PAYLOAD || self.chain_id.len() > MAX_CHAIN_ID {
            return Err(LedgerError::PayloadTooLarge);
        }
        if !(kind::TRANSFER..=kind::LAST).contains(&self.kind) {
            return Err(LedgerError::UnknownKind(self.kind));
        }
        Ok(())
    }

    /// A mensagem (32 bytes) que a assinatura cobre, para quem confere em lote.
    pub fn signing_message(&self) -> Result<Hash32, LedgerError> {
        Ok(tagged(tags::TX, &[&self.signing_bytes()?]))
    }

    /// Codificação completa (com a assinatura).
    pub fn encode(&self) -> Result<Vec<u8>, LedgerError> {
        let mut e = Encoder::new();
        e.string(&self.chain_id)
            .u8(self.kind)
            .u64(self.nonce)
            .u64(self.fee)
            .fixed(self.sender.as_bytes())
            .bytes(&self.payload)
            .fixed(self.signature.as_bytes());
        e.finish().map_err(LedgerError::Decode)
    }

    /// Lê uma transação. Recusa lixo, sobras, tamanhos acima do limite e tipo desconhecido.
    pub fn decode(bytes: &[u8]) -> Result<Self, LedgerError> {
        let mut d = Decoder::new(bytes);
        let chain_id = d
            .string(MAX_CHAIN_ID)
            .map_err(|_| LedgerError::Malformed)?
            .to_string();
        let kind = d.u8().map_err(|_| LedgerError::Malformed)?;
        let nonce = d.u64().map_err(|_| LedgerError::Malformed)?;
        let fee = d.u64().map_err(|_| LedgerError::Malformed)?;
        let sender = PublicKey(d.fixed::<32>().map_err(|_| LedgerError::Malformed)?);
        let payload = d
            .bytes(MAX_PAYLOAD)
            .map_err(|_| LedgerError::Malformed)?
            .to_vec();
        let signature = Signature(d.fixed::<64>().map_err(|_| LedgerError::Malformed)?);
        d.finish().map_err(|_| LedgerError::Malformed)?;
        let tx = Self {
            chain_id,
            kind,
            nonce,
            fee,
            sender,
            payload,
            signature,
        };
        tx.check_shape()?;
        Ok(tx)
    }

    /// Identificador: hash etiquetado da codificação completa.
    #[must_use]
    pub fn id(&self) -> Hash32 {
        self.id_with_tag(tags::TXID)
    }

    /// Como [`Tx::id`], com outro prefixo de domínio (os vetores do BRDC usam `brdc/txid`).
    #[must_use]
    pub fn id_with_tag(&self, tag: &str) -> Hash32 {
        self.encode().map_or(Hash32::ZERO, |b| tagged(tag, &[&b]))
    }

    /// O endereço do remetente.
    #[must_use]
    pub fn sender_address(&self) -> Address {
        Address::from_public_key(&self.sender)
    }

    /// Tamanho da codificação completa, em bytes (a taxa mínima cresce com ele).
    #[must_use]
    pub fn size(&self) -> usize {
        // chain_id (4 + n) + kind (1) + nonce (8) + fee (8) + sender (32) + payload (4 + n) + assinatura (64)
        4 + self.chain_id.len() + 1 + 8 + 8 + 32 + 4 + self.payload.len() + 64
    }
}
