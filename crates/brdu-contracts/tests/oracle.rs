#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Oracle: the BRDC vectors (`contract_txs`): the create payload is byte for byte the same.

use brdu_contracts::{act_payload, create_payload};
use brdu_crypto::{Address, Hash32, bech32m, hex};

#[test]
fn create_payloads_match_the_brdc_vectors() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/brdc/tx.json");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let list = v["contract_txs"].as_array().unwrap();
    assert!(!list.is_empty());
    for t in list.iter().filter(|t| t["kind"] == 20) {
        let i = &t["input"];
        let (_, bytes) = bech32m::decode(i["party_b"].as_str().unwrap()).unwrap();
        let b = Address(Hash32(bytes.as_slice().try_into().unwrap()));
        let terms: Vec<u64> = i["terms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().parse().unwrap())
            .collect();
        let payload = create_payload(
            i["template"].as_str().unwrap(),
            i["id"].as_str().unwrap(),
            &b,
            &terms,
        );
        assert_eq!(hex::encode(&payload), t["payload"].as_str().unwrap());
    }
    for t in list.iter().filter(|t| t["kind"] == 21) {
        let i = &t["input"];
        let payload = act_payload(i["id"].as_str().unwrap(), i["action"].as_str().unwrap());
        assert_eq!(hex::encode(&payload), t["payload"].as_str().unwrap());
    }
}
