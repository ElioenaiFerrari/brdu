#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::process::Command;

use brdu_menu::{CATALOG, command_line, defaults};

#[test]
fn todo_item_vira_um_comando_que_o_brdu_dev_conhece() {
    // a ajuda do brdu-dev é a fonte da verdade dos comandos
    let out = Command::new("cargo")
        .args(["run", "-q", "-p", "brdu-dev", "--", "help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(help.contains("cargo dev <comando>"), "{help}");
    for item in CATALOG {
        let line = command_line(item, &defaults(item), "net");
        let first = &line[0];
        assert!(
            help.lines()
                .any(|l| l.trim_start().starts_with(first.as_str())),
            "o comando `{first}` do item `{}` não está na ajuda",
            item.label
        );
        // subcomandos de dois níveis também aparecem na ajuda
        if item.args.len() > 1 {
            let sub = item.args[1];
            assert!(
                help.lines()
                    .filter(|l| l.contains(first.as_str()))
                    .any(|l| l.contains(sub)),
                "`{}` não está na ajuda",
                item.args.join(" ")
            );
        }
    }
}

#[test]
fn a_linha_de_comando_leva_os_parametros_e_a_pasta() {
    let send = CATALOG.iter().find(|i| i.args == ["send"]).unwrap();
    let line = command_line(
        send,
        &["alice".into(), "bob".into(), "1,5".into(), "".into()],
        "compose",
    );
    assert_eq!(
        line,
        [
            "send", "--from", "alice", "--to", "bob", "--amount", "1,5", "--dir", "compose"
        ]
    );
    let fund = CATALOG.iter().find(|i| i.args == ["fund"]).unwrap();
    assert_eq!(
        command_line(fund, &["alice".into(), "10".into()], "net"),
        ["fund", "alice", "10", "--dir", "net"]
    );
    // comandos que não usam --dir não o recebem
    let load = CATALOG.iter().find(|i| i.args == ["load"]).unwrap();
    assert!(!command_line(load, &defaults(load), "net").contains(&"--dir".to_string()));
}

#[test]
fn o_catalogo_nao_tem_itens_repetidos() {
    let mut labels: Vec<&str> = CATALOG.iter().map(|i| i.label).collect();
    labels.sort_unstable();
    let n = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), n);
}
