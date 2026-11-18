#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::process::Command;

use brdu_menu::{CATALOG, command_line, defaults};

#[test]
fn every_item_becomes_a_command_brdu_dev_knows() {
    // brdu-dev's help is the source of truth for the commands
    let out = Command::new("cargo")
        .args(["run", "-q", "-p", "brdu-dev", "--", "help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(help.contains("cargo dev <command>"), "{help}");
    for item in CATALOG {
        let line = command_line(item, &defaults(item), "net");
        let first = &line[0];
        assert!(
            help.lines()
                .any(|l| l.trim_start().starts_with(first.as_str())),
            "command `{first}` of item `{}` is not in the help",
            item.label
        );
        // two-level subcommands also appear in the help
        if item.args.len() > 1 {
            let sub = item.args[1];
            assert!(
                help.lines()
                    .filter(|l| l.contains(first.as_str()))
                    .any(|l| l.contains(sub)),
                "`{}` is not in the help",
                item.args.join(" ")
            );
        }
    }
}

#[test]
fn the_command_line_carries_the_parameters_and_the_folder() {
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
    // commands that do not use --dir do not receive it
    let load = CATALOG.iter().find(|i| i.args == ["load"]).unwrap();
    assert!(!command_line(load, &defaults(load), "net").contains(&"--dir".to_string()));
}

#[test]
fn the_catalog_has_no_duplicate_items() {
    let mut labels: Vec<&str> = CATALOG.iter().map(|i| i.label).collect();
    labels.sort_unstable();
    let n = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), n);
}
