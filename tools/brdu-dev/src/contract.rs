//! `cargo dev contract …`: contracts between parties (declarative templates) on the test network. Every step is
//! signed and paid by the wallet of the party that acts.

use brdu_contracts::{act_payload, create_payload, kind};
use brdu_ledger::Tx;
use serde_json::Value;

use crate::net::Layout;
use crate::{Args, account, first_api, http, load_account, parse_amount, resolve, submit};

const HELP: &str = "cargo dev contract <command>
  templates                                      list the templates, their terms and transitions
  create --template T --id ID --as NAME --with NAME|ADDR --terms V1,V2,...   create a contract (terms in the template's order)
  act --id ID --as NAME --action ACTION          perform a transition (accept, confirm, refund, pay_rent, repay, ...)
  list NAME|ADDRESS                              contracts of an account
  show ID                                        one contract
Amounts in terms use the same format as everywhere (`10`, `1,5`); durations and counts are plain integers.
Money terms by template: compra-venda: price, duration · aluguel: rent, period, periods, deposit, grace ·
emprestimo: principal, repay, duration. Use `m:` before a money term, e.g. `--terms m:5,3600`.
";

/// `m:5` is 5 BRDU in base units; a plain number is taken as it is (seconds or counts).
fn term(s: &str) -> Result<u64, String> {
    match s.strip_prefix("m:") {
        Some(amount) => parse_amount(amount),
        None => s
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("`{s}` is not a number (use `m:` for money)")),
    }
}

fn fee(api: &str) -> u64 {
    http::get(api, "/fees")
        .ok()
        .and_then(|f| f["contract_fee"].as_u64())
        .unwrap_or(10_000)
}

fn print_contract(c: &Value) {
    println!(
        "{} [{}] template {} A {} B {} escrow {}",
        c["id"].as_str().unwrap_or(""),
        c["state"].as_str().unwrap_or(""),
        c["template"].as_str().unwrap_or(""),
        c["a"].as_str().unwrap_or(""),
        c["b"].as_str().unwrap_or(""),
        c["escrow"]
    );
    println!("    terms {}  vars {}", c["terms"], c["vars"]);
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
        "templates" => {
            for t in http::get(&api, "/contract-templates")?["templates"]
                .as_array()
                .unwrap_or(&Vec::new())
            {
                println!("{}  terms {}", t["id"].as_str().unwrap_or(""), t["terms"]);
                for x in t["transitions"].as_array().unwrap_or(&Vec::new()) {
                    println!(
                        "    {:<15} {} -> {}  (actor {})",
                        x["name"].as_str().unwrap_or(""),
                        x["from"].as_str().unwrap_or(""),
                        x["to"].as_str().unwrap_or(""),
                        x["actor"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(())
        }
        "create" => {
            let (template, id) = (a.need("--template"), a.need("--id"));
            let other = resolve(&dir, &a.need("--with"))?;
            let terms = a
                .need("--terms")
                .split(',')
                .map(term)
                .collect::<Result<Vec<_>, _>>()?;
            let (key, me) = load_account(&dir, &a.need("--as"))?;
            let (_, nonce) = account(&api, &me)?;
            let tx = Tx::new_signed(
                &key,
                &chain,
                kind::CREATE,
                nonce,
                fee(&api),
                create_payload(&template, &id, &other, &terms),
            )
            .map_err(|e| e.to_string())?;
            println!("contract `{id}` sent: {}", submit(&api, &tx)?);
            Ok(())
        }
        "act" => {
            let (id, action) = (a.need("--id"), a.need("--action"));
            let (key, me) = load_account(&dir, &a.need("--as"))?;
            let (_, nonce) = account(&api, &me)?;
            let tx = Tx::new_signed(
                &key,
                &chain,
                kind::ACT,
                nonce,
                fee(&api),
                act_payload(&id, &action),
            )
            .map_err(|e| e.to_string())?;
            println!("`{action}` on `{id}` sent: {}", submit(&api, &tx)?);
            Ok(())
        }
        "list" => {
            let who = resolve(&dir, pos.get(1).ok_or("contract list NAME|ADDRESS")?)?;
            for c in http::get(&api, &format!("/contracts/{who}"))?["contracts"]
                .as_array()
                .unwrap_or(&Vec::new())
            {
                print_contract(c);
            }
            Ok(())
        }
        "show" => {
            print_contract(&http::get(
                &api,
                &format!("/contract/{}", pos.get(1).ok_or("contract show ID")?),
            )?);
            Ok(())
        }
        _ => Err(format!("unknown contract command `{sub}`\n\n{HELP}")),
    }
}
