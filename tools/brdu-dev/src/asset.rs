//! `cargo dev asset …`: assets on the test network. Classes are created by the test ROOT AUTHORITIES
//! (`keys/authority1..3`, 2 of 3); registrations, vetoes and liens are approved by the test ORGAN
//! (`keys/organ1..3`, 2 of 3). Offers and accepts are signed by the wallets of the seller and the buyer.
//! Test keys in plain text: dev only.

use std::path::Path;

use brdu_assets::{
    ClassSpec, asset_payload, class_doc, class_payload, kind, lien_add_doc, lien_add_payload,
    lien_remove_doc, lien_remove_payload, offer_payload, register_doc, register_payload, veto_doc,
    veto_payload,
};
use brdu_crypto::{Address, Hash32, SecretKey};
use brdu_ledger::Tx;
use brdu_ledger::coin::Approval;
use serde_json::Value;

use crate::net::{self, Layout};
use crate::{Args, account, first_api, http, load_account, resolve, submit};

const HELP: &str = "cargo dev asset <command>
  classes                                        list asset classes
  class --id ID --window SECS                    create a class (root authorities; the test organ is its organ)
  register --class C --id ID --owner NAME|ADDR [--meta TEXT]   register an asset (the test organ approves)
  offer --id ID --as NAME --to NAME|ADDR --price AMOUNT        the owner offers the asset
  accept --id ID --as NAME                       the buyer accepts: the price goes to escrow, the veto window starts
  cancel --id ID --as NAME                       the owner cancels an offer that was not accepted
  veto --id ID                                   the organ vetoes an accepted trade before its deadline
  finalize --id ID                               anyone finalizes after the window (ownership and money move)
  lien --id ID --lien L [--ref TEXT]             the organ adds a lien
  unlien --id ID --lien L                        the organ removes a lien
  list NAME|ADDRESS                              assets of an owner
  show ID                                        one asset (and its trade)
  deals                                          trades in progress and the escrow total
";

fn keys(dir: &Path, prefix: &str) -> Result<Vec<SecretKey>, String> {
    let first = dir.join(format!("keys/{prefix}1.key"));
    if !first.exists() {
        return Err(format!(
            "this network has no test `{prefix}` keys ({} is missing): it was created before assets existed, and its genesis does not list those authorities, so they cannot be added afterwards.\nCreate a new network: `cargo dev down && cargo dev reset && cargo dev init && cargo dev up` (reset deletes only what init created)",
            first.display()
        ));
    }
    (1..=3)
        .map(|i| net::read_key(&dir.join(format!("keys/{prefix}{i}.key"))))
        .collect()
}

fn approve(doc: &Hash32, signers: &[SecretKey]) -> Vec<Approval> {
    signers
        .iter()
        .take(2)
        .map(|k| Approval::new(doc, k))
        .collect()
}

/// Minimum fee of a kind, read from the node.
fn fee_of(api: &str, tx_kind: u8) -> u64 {
    let fees = http::get(api, "/fees").unwrap_or(Value::Null);
    match tx_kind {
        kind::CLASS | kind::REGISTER => fees["registration_fee"].as_u64().unwrap_or(50_000),
        kind::OFFER | kind::ACCEPT | kind::CANCEL | kind::FINALIZE => {
            fees["step_fee"].as_u64().unwrap_or(20_000)
        }
        _ => 0,
    }
}

/// Paid by the relayer (funded from the test issuers the first time): organ actions and finalization.
fn send_relayed(l: &Layout, api: &str, tx_kind: u8, payload: Vec<u8>) -> Result<String, String> {
    let relayer = l.relayer()?;
    let addr = Address::from_public_key(&relayer.public());
    let fee = fee_of(api, tx_kind);
    if account(api, &addr)?.0 < fee + 1_000_000 {
        println!("(funding the relayer with test BRDU first)");
        crate::fund_address(l, api, &addr, 100_000_000)?;
    }
    let (_, nonce) = account(api, &addr)?;
    let tx = Tx::new_signed(&relayer, &l.chain_id(), tx_kind, nonce, fee, payload)
        .map_err(|e| e.to_string())?;
    submit(api, &tx)
}

/// Signed and paid by a wallet (the seller or the buyer).
fn send_as(
    dir: &Path,
    api: &str,
    chain: &str,
    who: &str,
    tx_kind: u8,
    payload: Vec<u8>,
) -> Result<String, String> {
    let (key, addr) = load_account(dir, who)?;
    let (_, nonce) = account(api, &addr)?;
    let tx = Tx::new_signed(&key, chain, tx_kind, nonce, fee_of(api, tx_kind), payload)
        .map_err(|e| e.to_string())?;
    submit(api, &tx)
}

fn print_asset(a: &Value) {
    println!(
        "{} [class {}] owner {} rev {} liens {}",
        a["id"].as_str().unwrap_or(""),
        a["class"].as_str().unwrap_or(""),
        a["owner"].as_str().unwrap_or(""),
        a["rev"],
        a["liens"].as_object().map_or(0, serde_json::Map::len)
    );
    if let Some(d) = a["deal"].as_object() {
        println!(
            "    trade {}: buyer {} price {} deadline {}",
            d["status"].as_str().unwrap_or(""),
            d["buyer"].as_str().unwrap_or(""),
            d["price"],
            d["deadline"]
        );
    }
}

pub fn run(a: &Args) -> Result<(), String> {
    let dir = a.dir();
    let pos = a.positional();
    let Some(sub) = pos.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    let l = Layout::load(&dir)?;
    let api = first_api(&l, a)?;
    let chain = l.chain_id();
    let organ_keys = || keys(&dir, "organ");
    let fetch_asset = |id: &str| http::get(&api, &format!("/asset/{id}"));
    match sub {
        "classes" => {
            for c in http::get(&api, "/asset-classes")?["classes"]
                .as_array()
                .unwrap_or(&Vec::new())
            {
                println!(
                    "{}  organ {} of {}  veto window {} s",
                    c["id"].as_str().unwrap_or(""),
                    c["threshold"],
                    c["organ"].as_array().map_or(0, Vec::len),
                    c["veto_window"]
                );
            }
            Ok(())
        }
        "class" => {
            let organ = organ_keys()?
                .iter()
                .map(SecretKey::public)
                .collect::<Vec<_>>();
            let spec = ClassSpec {
                id: a.need("--id"),
                organ,
                threshold: 2,
                veto_window: a.num("--window", 60),
            };
            let doc = class_doc(
                &chain,
                &spec.id,
                &spec.organ,
                spec.threshold,
                spec.veto_window,
            );
            let payload = class_payload(&spec, &approve(&doc, &keys(&dir, "authority")?));
            println!(
                "class `{}` sent: {}",
                spec.id,
                send_relayed(&l, &api, kind::CLASS, payload)?
            );
            Ok(())
        }
        "register" => {
            let (class, id) = (a.need("--class"), a.need("--id"));
            let owner = resolve(&dir, &a.need("--owner"))?;
            let meta = a.one("--meta").unwrap_or_default().into_bytes();
            let doc = register_doc(&chain, &class, &id, &owner, &meta);
            let payload =
                register_payload(&class, &id, &owner, &meta, &approve(&doc, &organ_keys()?));
            println!(
                "asset `{id}` registered: {}",
                send_relayed(&l, &api, kind::REGISTER, payload)?
            );
            Ok(())
        }
        "offer" => {
            let id = a.need("--id");
            let buyer = resolve(&dir, &a.need("--to"))?;
            let price = crate::parse_amount(&a.need("--price"))?;
            let r = send_as(
                &dir,
                &api,
                &chain,
                &a.need("--as"),
                kind::OFFER,
                offer_payload(&id, &buyer, price),
            )?;
            println!("offer on `{id}` sent: {r}");
            Ok(())
        }
        "accept" | "cancel" => {
            let id = a.need("--id");
            let k = if sub == "accept" {
                kind::ACCEPT
            } else {
                kind::CANCEL
            };
            println!(
                "{sub} of `{id}` sent: {}",
                send_as(&dir, &api, &chain, &a.need("--as"), k, asset_payload(&id))?
            );
            Ok(())
        }
        "veto" => {
            let id = a.need("--id");
            let asset = fetch_asset(&id)?;
            let deal = &asset["deal"];
            let buyer = Address::decode(
                deal["buyer"]
                    .as_str()
                    .ok_or("this asset has no accepted trade")?,
            )
            .map_err(|e| e.to_string())?;
            let doc = veto_doc(
                &chain,
                &id,
                asset["rev"].as_u64().unwrap_or(0),
                &buyer,
                deal["price"].as_u64().unwrap_or(0),
                deal["deadline"].as_u64().unwrap_or(0),
            );
            let payload = veto_payload(&id, &approve(&doc, &organ_keys()?));
            println!(
                "veto of `{id}` sent: {}",
                send_relayed(&l, &api, kind::VETO, payload)?
            );
            Ok(())
        }
        "finalize" => {
            let id = a.need("--id");
            println!(
                "finalization of `{id}` sent: {}",
                send_relayed(&l, &api, kind::FINALIZE, asset_payload(&id))?
            );
            Ok(())
        }
        "lien" => {
            let (id, lien) = (a.need("--id"), a.need("--lien"));
            let reference = a.one("--ref").unwrap_or_else(|| "lien".into());
            let rev = fetch_asset(&id)?["rev"].as_u64().ok_or("unknown asset")?;
            let doc = lien_add_doc(&chain, &id, rev, &lien, &reference);
            let payload = lien_add_payload(&id, &lien, &reference, &approve(&doc, &organ_keys()?));
            println!(
                "lien `{lien}` on `{id}` sent: {}",
                send_relayed(&l, &api, kind::LIEN_ADD, payload)?
            );
            Ok(())
        }
        "unlien" => {
            let (id, lien) = (a.need("--id"), a.need("--lien"));
            let asset = fetch_asset(&id)?;
            let rev = asset["rev"].as_u64().ok_or("unknown asset")?;
            let reference = asset["liens"][&lien]
                .as_str()
                .ok_or("no such lien")?
                .to_string();
            let doc = lien_remove_doc(&chain, &id, rev, &lien, &reference);
            let payload = lien_remove_payload(&id, &lien, &approve(&doc, &organ_keys()?));
            println!(
                "removal of lien `{lien}` on `{id}` sent: {}",
                send_relayed(&l, &api, kind::LIEN_REMOVE, payload)?
            );
            Ok(())
        }
        "list" => {
            let owner = resolve(&dir, pos.get(1).ok_or("asset list NAME|ADDRESS")?)?;
            for x in http::get(&api, &format!("/assets/{owner}"))?["assets"]
                .as_array()
                .unwrap_or(&Vec::new())
            {
                print_asset(x);
            }
            Ok(())
        }
        "show" => {
            print_asset(&fetch_asset(pos.get(1).ok_or("asset show ID")?)?);
            Ok(())
        }
        "deals" => {
            let v = http::get(&api, "/deals")?;
            for d in v["deals"].as_array().unwrap_or(&Vec::new()) {
                println!(
                    "{} [{}] buyer {} price {} deadline {}",
                    d["asset"].as_str().unwrap_or(""),
                    d["status"].as_str().unwrap_or(""),
                    d["buyer"].as_str().unwrap_or(""),
                    d["price"],
                    d["deadline"]
                );
            }
            println!("escrow total: {}", v["escrow"]);
            Ok(())
        }
        _ => Err(format!("unknown asset command `{sub}`\n\n{HELP}")),
    }
}
