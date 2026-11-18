//! `cargo dev cred …`: credentials on the test network. Type creation is signed by the test ROOT AUTHORITIES
//! (`keys/authority1..3`, 2 of 3); issues, vetoes and revocations by the test ORGAN (`keys/organ1..3`, 2 of 3).
//! Test keys in plain text: dev only.

use std::path::Path;

use brdu_credentials::{
    Privacy, TypeSpec, id_payload, issue_doc, issue_payload, kind, revoke_doc, revoke_payload,
    type_doc, type_payload, veto_doc, veto_payload,
};
use brdu_crypto::{Address, Hash32, PublicKey, SecretKey, hex, tagged};
use brdu_ledger::Tx;
use brdu_ledger::coin::Approval;
use serde_json::Value;

use crate::net::{self, Layout};
use crate::{Args, account, first_api, http, load_account, resolve, submit};

const HELP: &str = "cargo dev cred <command>
  types                                          list credential types
  type --id ID --contest SECS [--validity SECS] [--private] [--ref-type T --revokes]   create a type (root authorities)
  issue --type T --id ID --to NAME|ADDRESS [--data TEXT | --commit TEXT] [--ref ID]    issue (the test organ approves)
  contest --id ID --as NAME                      the subject contests a pending credential
  veto --id ID                                   the organ vetoes a pending credential
  activate --id ID                               activates a pending credential after its window
  revoke --id ID [--reason TEXT]                 the organ revokes a pending or active credential
  list NAME|ADDRESS                              credentials of a holder
  show ID                                        one credential
";

fn keys(dir: &Path, prefix: &str) -> Result<Vec<SecretKey>, String> {
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

/// Sends a transaction paid by the relayer (funding it from the test issuers the first time).
fn send(l: &Layout, api: &str, tx_kind: u8, payload: Vec<u8>) -> Result<String, String> {
    let relayer = l.relayer()?;
    let relayer_addr = Address::from_public_key(&relayer.public());
    let fees = http::get(api, "/fees")?;
    let fee = match tx_kind {
        kind::TYPE => fees["registration_fee"].as_u64().unwrap_or(50_000),
        kind::ISSUE | kind::REVOKE => fees["credential_fee"].as_u64().unwrap_or(10_000),
        _ => 0,
    };
    let (balance, _) = account(api, &relayer_addr)?;
    if balance < fee + 1_000_000 {
        println!("(funding the relayer with test BRDU first)");
        crate::fund_address(l, api, &relayer_addr, 100_000_000)?;
    }
    let (_, nonce) = account(api, &relayer_addr)?;
    let tx = Tx::new_signed(&relayer, &l.chain_id(), tx_kind, nonce, fee, payload)
        .map_err(|e| e.to_string())?;
    submit(api, &tx)
}

fn print_credential(c: &Value) {
    println!(
        "{} [{}] type {} subject {} valid: {}",
        c["id"].as_str().unwrap_or(""),
        c["status"].as_str().unwrap_or(""),
        c["type"].as_str().unwrap_or(""),
        c["subject"].as_str().unwrap_or(""),
        c["valid"]
    );
    println!(
        "    issued {}  window ends {}  expires {}  now {}",
        c["issued_at"],
        c["active_at"],
        if c["expires_at"] == 0 {
            "never".into()
        } else {
            c["expires_at"].to_string()
        },
        c["now"]
    );
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
    match sub {
        "types" => {
            for t in http::get(&api, "/credential-types")?["types"]
                .as_array()
                .unwrap_or(&Vec::new())
            {
                println!(
                    "{}  organ {} of {}  window {} s  validity {}  {}{}",
                    t["id"].as_str().unwrap_or(""),
                    t["threshold"],
                    t["organ"].as_array().map_or(0, Vec::len),
                    t["contest_window"],
                    if t["validity"] == 0 {
                        "never expires".into()
                    } else {
                        format!("{} s", t["validity"])
                    },
                    t["privacy"].as_str().unwrap_or(""),
                    if t["ref_type"] == "" {
                        String::new()
                    } else {
                        format!(
                            "  references {}{}",
                            t["ref_type"].as_str().unwrap_or(""),
                            if t["revokes_reference"] == true {
                                " (revokes it)"
                            } else {
                                ""
                            }
                        )
                    }
                );
            }
            Ok(())
        }
        "type" => {
            let organ: Vec<PublicKey> =
                keys(&dir, "organ")?.iter().map(SecretKey::public).collect();
            let spec = TypeSpec {
                id: a.need("--id"),
                organ,
                threshold: 2,
                contest: a.num("--contest", 60),
                validity: a.num("--validity", 0),
                privacy: if a.0.iter().any(|x| x == "--private") {
                    Privacy::Private
                } else {
                    Privacy::Public
                },
                ref_type: a.one("--ref-type").unwrap_or_default(),
                revokes: a.0.iter().any(|x| x == "--revokes"),
            };
            let payload = type_payload(
                &spec,
                &approve(&type_doc(&chain, &spec), &keys(&dir, "authority")?),
            );
            println!(
                "type `{}` sent: {}",
                spec.id,
                send(&l, &api, kind::TYPE, payload)?
            );
            Ok(())
        }
        "issue" => {
            let (type_id, id) = (a.need("--type"), a.need("--id"));
            let subject = resolve(&dir, &a.need("--to"))?;
            let reference = a.one("--ref").unwrap_or_default();
            let data: Vec<u8> = if let Some(text) = a.one("--commit") {
                // private credential: only a commitment goes on the chain; the holder keeps the salt to reveal the data
                let mut salt = [0u8; 16];
                getrandom::fill(&mut salt).map_err(|e| e.to_string())?;
                let c = tagged("brdu/cred/commit", &[&salt, text.as_bytes()]);
                println!(
                    "commitment of `{text}` with salt {} (keep the salt: it is how the holder reveals the data)",
                    hex::encode(&salt)
                );
                c.as_bytes().to_vec()
            } else {
                a.one("--data").unwrap_or_default().into_bytes()
            };
            let doc = issue_doc(&chain, &type_id, &id, &subject, &data, &reference);
            let payload = issue_payload(
                &type_id,
                &id,
                &subject,
                &data,
                &reference,
                &approve(&doc, &keys(&dir, "organ")?),
            );
            println!(
                "credential `{id}` issued (pending until its window ends): {}",
                send(&l, &api, kind::ISSUE, payload)?
            );
            Ok(())
        }
        "contest" => {
            let id = a.need("--id");
            let (key, subject) = load_account(&dir, &a.need("--as"))?;
            // the subject pays the (zero) fee and signs with their own key
            let (_, nonce) = account(&api, &subject)?;
            let tx = Tx::new_signed(&key, &chain, kind::CONTEST, nonce, 0, id_payload(&id))
                .map_err(|e| e.to_string())?;
            println!("contest of `{id}` sent: {}", submit(&api, &tx)?);
            Ok(())
        }
        "veto" => {
            let id = a.need("--id");
            let c = http::get(&api, &format!("/credential/{id}"))?;
            let issued_at = c["issued_at"].as_u64().ok_or("unknown credential")?;
            let payload = veto_payload(
                &id,
                &approve(&veto_doc(&chain, &id, issued_at), &keys(&dir, "organ")?),
            );
            println!(
                "veto of `{id}` sent: {}",
                send(&l, &api, kind::VETO, payload)?
            );
            Ok(())
        }
        "activate" => {
            let id = a.need("--id");
            println!(
                "activation of `{id}` sent: {}",
                send(&l, &api, kind::ACTIVATE, id_payload(&id))?
            );
            Ok(())
        }
        "revoke" => {
            let id = a.need("--id");
            let reason = a.one("--reason").unwrap_or_else(|| "revoked".into());
            let payload = revoke_payload(
                &id,
                &reason,
                &approve(&revoke_doc(&chain, &id, &reason), &keys(&dir, "organ")?),
            );
            println!(
                "revocation of `{id}` sent: {}",
                send(&l, &api, kind::REVOKE, payload)?
            );
            Ok(())
        }
        "list" => {
            let who = pos.get(1).ok_or("cred list NAME|ADDRESS")?;
            let addr = resolve(&dir, who)?;
            let v = http::get(&api, &format!("/credentials/{addr}"))?;
            for c in v["credentials"].as_array().unwrap_or(&Vec::new()) {
                print_credential(c);
            }
            Ok(())
        }
        "show" => {
            let id = pos.get(1).ok_or("cred show ID")?;
            print_credential(&http::get(&api, &format!("/credential/{id}"))?);
            Ok(())
        }
        _ => Err(format!("unknown cred command `{sub}`\n\n{HELP}")),
    }
}
