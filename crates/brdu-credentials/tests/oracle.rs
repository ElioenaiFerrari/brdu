#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Oracle: the BRDC test vectors (`vectors/brdc/tx.json`, entries `issues` and `cred_ops`). The documents the organs
//! sign and the payload bytes are identical to the BRDC's; only the domain prefix differs (`brdc/cred` there,
//! `brdu/cred` here), and with the BRDC prefix our functions must reproduce every vector byte for byte.

use brdu_credentials::{
    issue_doc_with_prefix, issue_payload, revoke_doc_with_prefix, revoke_payload,
    veto_doc_with_prefix, veto_payload,
};
use brdu_crypto::{Address, Hash32, PublicKey, bech32m, hex};
use brdu_ledger::coin::Approval;
use serde_json::Value;

fn vectors() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/brdc/tx.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn subject(s: &str) -> Address {
    let (hrp, bytes) = bech32m::decode(s).unwrap();
    assert_eq!(hrp, "brdc");
    Address(Hash32(bytes.as_slice().try_into().unwrap()))
}

fn approvals(list: &Value) -> Vec<Approval> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|a| Approval::from_bytes(&hex::decode(a.as_str().unwrap()).unwrap()).unwrap())
        .collect()
}

const PREFIX: &str = "brdc/cred";

#[test]
fn issue_documents_and_payloads_match_the_brdc_vectors_byte_for_byte() {
    let v = vectors();
    let chain = v["chain_id"].as_str().unwrap();
    let issues = v["issues"].as_array().unwrap();
    assert!(
        issues.len() >= 3,
        "marriage, divorce and a public credential"
    );
    for i in issues {
        let id = i["id"].as_str().unwrap();
        let data = hex::decode(i["data_hex"].as_str().unwrap()).unwrap();
        let sub = subject(i["subject"].as_str().unwrap());
        let (ty, reference) = (i["type"].as_str().unwrap(), i["ref"].as_str().unwrap());
        let doc = issue_doc_with_prefix(PREFIX, chain, ty, id, &sub, &data, reference);
        assert_eq!(
            doc.to_hex(),
            i["doc"].as_str().unwrap(),
            "issue document of {id}"
        );
        // the BRDC approvals are valid signatures over exactly that document
        let ap = approvals(&i["approvals"]);
        let keys: Vec<PublicKey> = i["member_public_keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| PublicKey(hex::decode_array::<32>(k.as_str().unwrap()).unwrap()))
            .collect();
        assert_eq!(
            brdu_ledger::approvals::signers(&doc, &ap, &keys).len(),
            keys.len(),
            "all organ approvals of {id} verify"
        );
        // and the payload we build is the very same bytes
        let payload = issue_payload(ty, id, &sub, &data, reference, &ap);
        assert_eq!(
            hex::encode(&payload),
            i["payload"].as_str().unwrap(),
            "issue payload of {id}"
        );
    }
}

#[test]
fn veto_and_revoke_documents_and_payloads_match_the_brdc_vectors() {
    let v = vectors();
    let chain = v["chain_id"].as_str().unwrap();
    for op in v["cred_ops"].as_array().unwrap() {
        let id = op["id"].as_str().unwrap();
        let ap = approvals(&op["approvals"]);
        match op["op"].as_str().unwrap() {
            "veto" => {
                let issued_at: u64 = op["issued_at"].as_str().unwrap().parse().unwrap();
                assert_eq!(
                    veto_doc_with_prefix(PREFIX, chain, id, issued_at).to_hex(),
                    op["doc"].as_str().unwrap()
                );
                assert_eq!(
                    hex::encode(&veto_payload(id, &ap)),
                    op["payload"].as_str().unwrap()
                );
            }
            "revoke" => {
                let reason = op["reason"].as_str().unwrap();
                assert_eq!(
                    revoke_doc_with_prefix(PREFIX, chain, id, reason).to_hex(),
                    op["doc"].as_str().unwrap()
                );
                assert_eq!(
                    hex::encode(&revoke_payload(id, reason, &ap)),
                    op["payload"].as_str().unwrap()
                );
            }
            other => panic!("unknown op {other}"),
        }
    }
}

#[test]
fn with_the_brdu_prefix_the_same_inputs_give_different_documents() {
    // domain separation: an approval collected on the BRDC can never authorize anything on a BRDU network
    let v = vectors();
    let chain = v["chain_id"].as_str().unwrap();
    let i = &v["issues"][0];
    let sub = subject(i["subject"].as_str().unwrap());
    let data = hex::decode(i["data_hex"].as_str().unwrap()).unwrap();
    let brdc = issue_doc_with_prefix(
        "brdc/cred",
        chain,
        i["type"].as_str().unwrap(),
        i["id"].as_str().unwrap(),
        &sub,
        &data,
        i["ref"].as_str().unwrap(),
    );
    let brdu = brdu_credentials::issue_doc(
        chain,
        i["type"].as_str().unwrap(),
        i["id"].as_str().unwrap(),
        &sub,
        &data,
        i["ref"].as_str().unwrap(),
    );
    assert_ne!(brdc, brdu);
}
