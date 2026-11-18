#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;

use brdu_crypto::{PublicKey, bech32m, hex};
use brdu_ledger::tx::{MAX_CHAIN_ID, MAX_PAYLOAD};
use brdu_ledger::{LedgerError, Tx, kind};
use common::*;
use proptest::prelude::*;
use serde_json::Value;

fn sample() -> Tx {
    tx(&key(1), kind::TRANSFER, 7, 1_234, vec![1, 2, 3, 4])
}

#[test]
fn codifica_e_decodifica_igual_e_o_id_e_estavel() {
    let t = sample();
    let bytes = t.encode().unwrap();
    assert_eq!(bytes.len(), t.size());
    let back = Tx::decode(&bytes).unwrap();
    assert_eq!(back, t);
    assert_eq!(back.id(), t.id());
    assert!(back.verify().is_ok());
}

#[test]
fn lixo_sobra_truncado_e_limites_sao_recusados() {
    let good = sample().encode().unwrap();
    assert_eq!(Tx::decode(&[]), Err(LedgerError::Malformed));
    assert_eq!(
        Tx::decode(&good[..good.len() - 1]),
        Err(LedgerError::Malformed)
    );
    assert_eq!(
        Tx::decode(&[good.clone(), vec![0]].concat()),
        Err(LedgerError::Malformed)
    );
    // tipo 0 e tipo 23 não existem
    for bad_kind in [0u8, 23, 255] {
        let mut b = good.clone();
        b[4 + "brdu-test".len()] = bad_kind;
        assert_eq!(Tx::decode(&b), Err(LedgerError::UnknownKind(bad_kind)));
    }
    // carga gigante declarada: recusa sem alocar
    let mut huge = good.clone();
    let off = 4 + "brdu-test".len() + 1 + 8 + 8 + 32;
    huge[off..off + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(Tx::decode(&huge), Err(LedgerError::Malformed));
    // chain_id e carga acima do limite não montam
    let big = brdu_ledger::Tx::new_signed(
        &key(1),
        &"x".repeat(MAX_CHAIN_ID + 1),
        kind::TRANSFER,
        0,
        0,
        vec![],
    )
    .unwrap();
    assert_eq!(big.check_shape(), Err(LedgerError::PayloadTooLarge));
    let big = brdu_ledger::Tx::new_signed(
        &key(1),
        CHAIN,
        kind::TRANSFER,
        0,
        0,
        vec![0; MAX_PAYLOAD + 1],
    )
    .unwrap();
    assert_eq!(big.verify(), Err(LedgerError::PayloadTooLarge));
}

#[test]
fn a_assinatura_cobre_todos_os_campos() {
    let t = sample();
    let mut variants: Vec<Tx> = Vec::new();
    let mut v = t.clone();
    v.chain_id = "outra".into();
    variants.push(v);
    let mut v = t.clone();
    v.kind = kind::MINT;
    variants.push(v);
    let mut v = t.clone();
    v.nonce += 1;
    variants.push(v);
    let mut v = t.clone();
    v.fee += 1;
    variants.push(v);
    let mut v = t.clone();
    v.payload.push(9);
    variants.push(v);
    let mut v = t.clone();
    v.sender = key(2).public();
    variants.push(v);
    for v in variants {
        assert_eq!(v.verify(), Err(LedgerError::BadSignature), "{v:?}");
    }
}

#[test]
fn o_id_muda_com_qualquer_byte_e_nao_se_confunde_com_a_mensagem_assinada() {
    let t = sample();
    let mut other = t.clone();
    other.nonce += 1;
    assert_ne!(t.id(), other.id());
    assert_ne!(t.id(), t.signing_message().unwrap());
}

proptest! {
    #[test]
    fn qualquer_transacao_assinada_volta_igual_e_confere(
        seed in any::<[u8; 32]>(), nonce in any::<u64>(), fee in any::<u64>(),
        kind in 1u8..=22, payload in proptest::collection::vec(any::<u8>(), 0..300)
    ) {
        let t = Tx::new_signed(&brdu_crypto::SecretKey::from_bytes(seed), CHAIN, kind, nonce, fee, payload).unwrap();
        let back = Tx::decode(&t.encode().unwrap()).unwrap();
        prop_assert_eq!(&back, &t);
        prop_assert!(back.verify().is_ok());
    }

    #[test]
    fn trocar_um_bit_de_qualquer_byte_da_transacao_nunca_e_aceito(flip in 0usize..400) {
        let t = sample();
        let mut bytes = t.encode().unwrap();
        let i = flip % bytes.len();
        bytes[i] ^= 1 << (flip % 8);
        // ou nem decodifica, ou decodifica e a assinatura falha: nunca vira uma transação válida DIFERENTE
        if let Ok(d) = Tx::decode(&bytes) {
            prop_assert!(d == t || d.verify().is_err());
        }
    }

    #[test]
    fn lixo_nunca_causa_panico(data in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = Tx::decode(&data);
    }
}

fn load(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/brdc/");
    serde_json::from_str(&std::fs::read_to_string(format!("{path}{name}")).unwrap()).unwrap()
}

/// **Oráculo do BRDC:** as 4 transferências do `tx.json` (inclusive a de outra rede, a de passphrase e a de valor
/// máximo) decodificam, conferem a assinatura, dão o mesmo id e a mesma taxa mínima. O BRDC usa os prefixos
/// `brdc/tx` e `brdc/txid`; o BRDU usa `brdu/…` (o formato dos bytes é o mesmo).
#[test]
fn transacoes_do_brdc_decodificam_conferem_e_dao_o_mesmo_id_e_a_mesma_taxa_minima() {
    let v = load("tx.json");
    let env = Env::new();
    let txs = v["txs"].as_array().unwrap();
    assert_eq!(txs.len(), 4);
    for t in txs {
        let encoded = hex::decode(t["encoded"].as_str().unwrap()).unwrap();
        let decoded = Tx::decode(&encoded).unwrap();
        // os bytes são idênticos ao reencodificar
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(decoded.chain_id, t["chain_id"].as_str().unwrap());
        assert_eq!(decoded.nonce, t["nonce"].as_u64().unwrap());
        assert_eq!(decoded.fee, t["fee"].as_u64().unwrap());
        assert_eq!(
            decoded.kind,
            u8::try_from(t["kind"].as_u64().unwrap()).unwrap()
        );
        assert_eq!(
            decoded.sender,
            PublicKey(hex::decode_array::<32>(t["sender_public_key"].as_str().unwrap()).unwrap())
        );
        assert_eq!(
            hex::encode(decoded.signature.as_bytes()),
            t["signature"].as_str().unwrap()
        );
        assert_eq!(
            hex::encode(&decoded.signing_bytes().unwrap()),
            t["signing_bytes"].as_str().unwrap()
        );
        // assinatura sobre o hash etiquetado "brdc/tx" e id etiquetado "brdc/txid"
        assert!(
            decoded.verify_with_tag("brdc/tx").is_ok(),
            "assinatura do BRDC"
        );
        assert_eq!(
            decoded.id_with_tag("brdc/txid").to_hex(),
            t["txid"].as_str().unwrap()
        );
        // com os prefixos do BRDU a MESMA assinatura NÃO vale (domínios separados)
        assert_eq!(decoded.verify(), Err(LedgerError::BadSignature));
        // carga: destino e valor
        let (hrp, to) = bech32m::decode(t["to_address"].as_str().unwrap()).unwrap();
        assert_eq!(hrp, "brdc");
        assert_eq!(&decoded.payload[..32], to.as_slice());
        let amount = u64::from_be_bytes(decoded.payload[32..40].try_into().unwrap());
        assert_eq!(amount.to_string(), t["amount"].as_str().unwrap());
        // taxa mínima: base 1.000 + 1 por byte (1.173 para a rede "brdc-vectors", 1.171 para "outra-rede")
        assert_eq!(
            env.exec.min_fee(&decoded).unwrap().to_string(),
            t["min_fee"].as_str().unwrap()
        );
    }
}
