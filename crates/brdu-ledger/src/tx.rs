//! Signed transaction (ADR-0005 format, the same as BRDC).
//!
//! Fields: `chain_id`, `kind`, `nonce`, `fee`, `sender` (public key), `payload` and `signature`. The signature covers
//! **all** fields, including `chain_id` (prevents replaying the transaction on another network) and `nonce` (prevents
//! replay): `signature = Ed25519(key, tagged SHA3-256("brdu/tx", body))`.

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{Address, Hash32, PublicKey, SecretKey, Signature, tagged, tags};

use crate::error::LedgerError;

/// Largest accepted payload (64 KiB).
pub const MAX_PAYLOAD: usize = 64 * 1024;
/// Largest accepted `chain_id`, in bytes.
pub const MAX_CHAIN_ID: usize = 64;

/// Transaction kinds (`kind`). Those from 5 to 21 belong to the assets, credentials and contracts modules.
pub mod kind {
    /// Transfer between accounts.
    pub const TRANSFER: u8 = 1;
    /// Mint: create BRDU against reais received (M-of-N issuers).
    pub const MINT: u8 = 2;
    /// Burn: destroy BRDU against reais returned (M-of-N issuers, only from the owner's own balance).
    pub const BURN: u8 = 3;
    /// Reserve attestation (M-of-N issuers).
    pub const ATTESTATION: u8 = 4;
    /// First module kind (assets).
    pub const FIRST_MODULE: u8 = 5;
    /// Change in the validator set (rotation, ADR-0015).
    pub const VALIDATOR_CHANGE: u8 = 22;
    /// Last known kind.
    pub const LAST: u8 = 22;
}

/// A signed transaction.
#[derive(Clone, PartialEq, Eq)]
pub struct Tx {
    /// The network.
    pub chain_id: String,
    /// The kind ([`kind`]).
    pub kind: u8,
    /// The sender's nonce (starts at 0 and goes up by 1).
    pub nonce: u64,
    /// The fee in minimum units.
    pub fee: u64,
    /// The sender's public key.
    pub sender: PublicKey,
    /// The payload, interpreted according to the kind.
    pub payload: Vec<u8>,
    /// The signature.
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
    /// Builds and signs. The sender's public key comes from the private key itself.
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

    /// The bytes that are signed (everything except the signature).
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

    /// Checks shape and signature (does not check the network or the nonce: that is the executor's job).
    pub fn verify(&self) -> Result<(), LedgerError> {
        self.verify_with_tag(tags::TX)
    }

    /// Like [`Tx::verify`], with another domain prefix (the BRDC vectors use `brdc/tx`).
    pub fn verify_with_tag(&self, tag: &str) -> Result<(), LedgerError> {
        self.check_shape()?;
        self.sender
            .verify_tagged(tag, &self.signing_bytes()?, &self.signature)
            .map_err(|_| LedgerError::BadSignature)
    }

    /// Shape only (limits and known kind), without cryptography.
    pub fn check_shape(&self) -> Result<(), LedgerError> {
        if self.payload.len() > MAX_PAYLOAD || self.chain_id.len() > MAX_CHAIN_ID {
            return Err(LedgerError::PayloadTooLarge);
        }
        if !(kind::TRANSFER..=kind::LAST).contains(&self.kind) {
            return Err(LedgerError::UnknownKind(self.kind));
        }
        Ok(())
    }

    /// The message (32 bytes) that the signature covers, for whoever verifies in batch.
    pub fn signing_message(&self) -> Result<Hash32, LedgerError> {
        Ok(tagged(tags::TX, &[&self.signing_bytes()?]))
    }

    /// Full encoding (with the signature).
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

    /// Reads a transaction. Rejects garbage, leftovers, sizes above the limit and unknown kind.
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

    /// Identifier: tagged hash of the full encoding.
    #[must_use]
    pub fn id(&self) -> Hash32 {
        self.id_with_tag(tags::TXID)
    }

    /// Like [`Tx::id`], with another domain prefix (the BRDC vectors use `brdc/txid`).
    #[must_use]
    pub fn id_with_tag(&self, tag: &str) -> Hash32 {
        self.encode().map_or(Hash32::ZERO, |b| tagged(tag, &[&b]))
    }

    /// The sender's address.
    #[must_use]
    pub fn sender_address(&self) -> Address {
        Address::from_public_key(&self.sender)
    }

    /// Size of the full encoding, in bytes (the minimum fee grows with it).
    #[must_use]
    pub fn size(&self) -> usize {
        // chain_id (4 + n) + kind (1) + nonce (8) + fee (8) + sender (32) + payload (4 + n) + signature (64)
        4 + self.chain_id.len() + 1 + 8 + 8 + 32 + 4 + self.payload.len() + 64
    }
}
