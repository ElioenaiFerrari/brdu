//! Vetores: os oficiais (RFC 8032) e os do BRDC, que servem de oráculo para a implementação em Rust.
//! Os vetores do BRDC usam o prefixo de domínio `brdc/…`; aqui o prefixo é passado de fora.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brdu_crypto::{bech32m, hex, sha3, tagged};
use serde_json::Value;

fn load(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/brdc/");
    let text = std::fs::read_to_string(format!("{path}{name}")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn hash_do_brdc_sha3_e_etiquetado() {
    let v = load("hash.json");
    let tag = v["tag"].as_str().unwrap();
    let cases = v["cases"].as_array().unwrap();
    assert!(cases.len() >= 20, "poucos casos para valer como oráculo");
    for c in cases {
        let input = hex::decode(c["input"].as_str().unwrap()).unwrap();
        assert_eq!(input.len() as u64, c["len"].as_u64().unwrap());
        assert_eq!(
            sha3(&input).to_hex(),
            c["sha3_256"].as_str().unwrap(),
            "sha3 de {input:?}"
        );
        assert_eq!(
            tagged(tag, &[&input]).to_hex(),
            c["tagged"].as_str().unwrap(),
            "etiquetado de {input:?}"
        );
    }
}

#[test]
fn hash_etiquetado_isolado_do_crypto_json() {
    let v = &load("crypto.json")["tagged_hash"];
    let data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
    assert_eq!(
        tagged(v["tag"].as_str().unwrap(), &[&data]).to_hex(),
        v["hash"].as_str().unwrap()
    );
}

#[test]
fn partes_do_hash_etiquetado_equivalem_a_um_buffer_so() {
    // tagged(tag, [a, b]) == tagged(tag, [a‖b]): sem ambiguidade de fronteira
    let whole = tagged("brdu/teste", &[b"ola mundo"]);
    assert_eq!(tagged("brdu/teste", &[b"ola ", b"mundo"]), whole);
    assert_eq!(tagged("brdu/teste", &[b"", b"ola mundo", b""]), whole);
}

#[test]
fn tags_diferentes_dao_hashes_diferentes() {
    assert_ne!(tagged("brdu/tx", &[b"x"]), tagged("brdu/block", &[b"x"]));
    assert_ne!(
        tagged("brdu/tx", &[b"x"]),
        tagged("brdc/tx", &[b"x"]),
        "BRDU e BRDC não podem se confundir"
    );
}

#[test]
fn enderecos_do_brdc_batem_com_o_hash_etiquetado_e_o_bech32m() {
    // o endereço do brdc é bech32m("brdc", tagged("brdc/addr", chave pública)); o oráculo confere a MESMA fórmula
    let accounts = load("crypto.json")["accounts"].clone();
    for a in accounts.as_array().unwrap() {
        let pk = hex::decode(a["public_key"].as_str().unwrap()).unwrap();
        let hash = tagged("brdc/addr", &[&pk]);
        let addr = bech32m::encode("brdc", hash.as_bytes()).unwrap();
        assert_eq!(addr, a["address"].as_str().unwrap());
        let (hrp, back) = bech32m::decode(&addr).unwrap();
        assert_eq!(
            (hrp.as_str(), back.as_slice()),
            ("brdc", hash.as_bytes().as_slice())
        );
    }
}
