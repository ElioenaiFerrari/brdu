#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Oracle: the BRDC test vectors (`vectors/brdc/tx.json`, entries `assets` and `asset_ops`). Documents and payloads
//! are identical to the BRDC's; only the domain prefix differs (`brdc/asset` there, `brdu/asset` here).

use brdu_assets::*;
use brdu_crypto::{Address, Hash32, bech32m, hex};
use brdu_ledger::coin::Approval;
use serde_json::Value;

const PREFIX: &str = "brdc/asset";

fn vectors() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/brdc/tx.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn who(s: &str) -> Address {
    let (_, bytes) = bech32m::decode(s).unwrap();
    Address(Hash32(bytes.as_slice().try_into().unwrap()))
}

fn approvals(list: &Value) -> Vec<Approval> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|a| Approval::from_bytes(&hex::decode(a.as_str().unwrap()).unwrap()).unwrap())
        .collect()
}

#[test]
fn offer_and_asset_payloads_match_the_brdc_vectors() {
    let v = vectors();
    let list = v["assets"].as_array().unwrap();
    assert!(!list.is_empty());
    for a in list {
        let id = a["asset"].as_str().unwrap();
        let price: u64 = a["price"].as_str().unwrap().parse().unwrap();
        assert_eq!(
            hex::encode(&offer_payload(
                id,
                &who(a["buyer"].as_str().unwrap()),
                price
            )),
            a["offer_payload"].as_str().unwrap()
        );
        assert_eq!(
            hex::encode(&asset_payload(id)),
            a["asset_payload"].as_str().unwrap()
        );
    }
}

#[test]
fn organ_documents_and_payloads_match_the_brdc_vectors_byte_for_byte() {
    let v = vectors();
    let chain = v["chain_id"].as_str().unwrap();
    let ops = v["asset_ops"].as_array().unwrap();
    assert_eq!(ops.len(), 4, "register, veto, lien add and lien remove");
    for o in ops {
        let asset = o["asset"].as_str().unwrap();
        let ap = approvals(&o["approvals"]);
        let (doc, payload) = match o["op"].as_str().unwrap() {
            "register" => {
                let (class, owner, meta) = (
                    o["class"].as_str().unwrap(),
                    who(o["owner"].as_str().unwrap()),
                    o["meta"].as_str().unwrap().as_bytes(),
                );
                (
                    register_doc_with_prefix(PREFIX, chain, class, asset, &owner, meta),
                    register_payload(class, asset, &owner, meta, &ap),
                )
            }
            "veto" => {
                let price: u64 = o["price"].as_str().unwrap().parse().unwrap();
                let deadline: u64 = o["deadline"].as_str().unwrap().parse().unwrap();
                (
                    veto_doc_with_prefix(
                        PREFIX,
                        chain,
                        asset,
                        o["rev"].as_u64().unwrap(),
                        &who(o["buyer"].as_str().unwrap()),
                        price,
                        deadline,
                    ),
                    veto_payload(asset, &ap),
                )
            }
            op @ ("lien_add" | "lien_remove") => {
                let (lien, reference) = (o["lien"].as_str().unwrap(), o["ref"].as_str().unwrap());
                let doc = lien_doc_with_prefix(
                    PREFIX,
                    op,
                    chain,
                    asset,
                    o["rev"].as_u64().unwrap(),
                    lien,
                    reference,
                );
                let payload = if op == "lien_add" {
                    lien_add_payload(asset, lien, reference, &ap)
                } else {
                    lien_remove_payload(asset, lien, &ap)
                };
                (doc, payload)
            }
            other => panic!("unexpected op {other}"),
        };
        assert_eq!(
            doc.to_hex(),
            o["doc"].as_str().unwrap(),
            "document of {}",
            o["op"]
        );
        assert_eq!(
            hex::encode(&payload),
            o["payload"].as_str().unwrap(),
            "payload of {}",
            o["op"]
        );
    }
}

#[test]
fn with_the_brdu_prefix_the_documents_differ() {
    let v = vectors();
    let chain = v["chain_id"].as_str().unwrap();
    let o = &v["asset_ops"][2];
    let brdc = lien_doc_with_prefix(
        PREFIX,
        "lien_add",
        chain,
        "casa-1",
        2,
        "hip-1",
        "contrato 42",
    );
    assert_eq!(brdc.to_hex(), o["doc"].as_str().unwrap());
    assert_ne!(
        brdc,
        lien_add_doc(chain, "casa-1", 2, "hip-1", "contrato 42")
    );
}
