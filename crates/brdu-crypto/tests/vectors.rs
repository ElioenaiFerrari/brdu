//! Vectors: the official ones (RFC 8032) and those of BRDC, which serve as an oracle for the Rust implementation.
//! The BRDC vectors use the domain prefix `brdc/…`; here the prefix is passed in from outside.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brdu_crypto::{bech32m, hex, sha3, tagged};
use serde_json::Value;

fn load(name: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/brdc/");
    let text = std::fs::read_to_string(format!("{path}{name}")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn brdc_hash_sha3_and_tagged() {
    let v = load("hash.json");
    let tag = v["tag"].as_str().unwrap();
    let cases = v["cases"].as_array().unwrap();
    assert!(cases.len() >= 20, "too few cases to count as an oracle");
    for c in cases {
        let input = hex::decode(c["input"].as_str().unwrap()).unwrap();
        assert_eq!(input.len() as u64, c["len"].as_u64().unwrap());
        assert_eq!(
            sha3(&input).to_hex(),
            c["sha3_256"].as_str().unwrap(),
            "sha3 of {input:?}"
        );
        assert_eq!(
            tagged(tag, &[&input]).to_hex(),
            c["tagged"].as_str().unwrap(),
            "tagged of {input:?}"
        );
    }
}

#[test]
fn isolated_tagged_hash_from_crypto_json() {
    let v = &load("crypto.json")["tagged_hash"];
    let data = hex::decode(v["data_hex"].as_str().unwrap()).unwrap();
    assert_eq!(
        tagged(v["tag"].as_str().unwrap(), &[&data]).to_hex(),
        v["hash"].as_str().unwrap()
    );
}

#[test]
fn tagged_hash_parts_are_equivalent_to_a_single_buffer() {
    // tagged(tag, [a, b]) == tagged(tag, [a‖b]): no boundary ambiguity
    let whole = tagged("brdu/test", &[b"hello world"]);
    assert_eq!(tagged("brdu/test", &[b"hello ", b"world"]), whole);
    assert_eq!(tagged("brdu/test", &[b"", b"hello world", b""]), whole);
}

#[test]
fn different_tags_give_different_hashes() {
    assert_ne!(tagged("brdu/tx", &[b"x"]), tagged("brdu/block", &[b"x"]));
    assert_ne!(
        tagged("brdu/tx", &[b"x"]),
        tagged("brdc/tx", &[b"x"]),
        "BRDU and BRDC must not be confused"
    );
}

#[test]
fn brdc_addresses_match_the_tagged_hash_and_bech32m() {
    // the brdc address is bech32m("brdc", tagged("brdc/addr", public key)); the oracle checks the SAME formula
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
