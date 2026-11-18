//! The menu catalog: each item is a `brdu-dev` command with parameters. Kept separate from the screen so it is testable.

/// A parameter: if `flag` exists, it goes as `--flag value`; otherwise it is positional. An empty value is omitted.
pub struct Param {
    /// Name shown in the form.
    pub name: &'static str,
    /// The flag (`--amount`) or `None` for positional.
    pub flag: Option<&'static str>,
    /// Suggested value.
    pub default: &'static str,
}

/// A menu item.
pub struct Item {
    /// Group (section title).
    pub group: &'static str,
    /// What appears in the menu.
    pub label: &'static str,
    /// The fixed `brdu-dev` arguments (before the parameters).
    pub args: &'static [&'static str],
    /// The parameters to ask for.
    pub params: &'static [Param],
    /// Does this command accept `--dir`?
    pub uses_dir: bool,
}

const fn p(name: &'static str, flag: Option<&'static str>, default: &'static str) -> Param {
    Param {
        name,
        flag,
        default,
    }
}

const fn it(
    group: &'static str,
    label: &'static str,
    args: &'static [&'static str],
    params: &'static [Param],
    uses_dir: bool,
) -> Item {
    Item {
        group,
        label,
        args,
        params,
        uses_dir,
    }
}

/// All items, in menu order.
pub const CATALOG: &[Item] = &[
    it(
        "Local network (processes)",
        "Create the network (init)",
        &["init"],
        &[
            p("validators", Some("--nodes"), "4"),
            p("spares", Some("--spare"), "0"),
        ],
        true,
    ),
    it(
        "Local network (processes)",
        "Start the nodes (up)",
        &["up"],
        &[],
        true,
    ),
    it(
        "Local network (processes)",
        "Node status (status)",
        &["status"],
        &[],
        true,
    ),
    it(
        "Local network (processes)",
        "Stop node N (stop)",
        &["stop"],
        &[p("node", None, "4")],
        true,
    ),
    it(
        "Local network (processes)",
        "Restart node N (start)",
        &["start"],
        &[p("node", None, "4")],
        true,
    ),
    it(
        "Local network (processes)",
        "Open the web explorer (explorer)",
        &["explorer"],
        &[p("listen", Some("--listen"), "127.0.0.1:8545")],
        true,
    ),
    it(
        "Local network (processes)",
        "Open the wallet in the browser (wallet-web)",
        &["wallet-web"],
        &[p("listen", Some("--listen"), "127.0.0.1:8546")],
        true,
    ),
    it(
        "Local network (processes)",
        "Credentials: list types (cred types)",
        &["cred", "types"],
        &[],
        true,
    ),
    it(
        "Local network (processes)",
        "Credentials: list a holder's credentials (cred list)",
        &["cred", "list"],
        &[p("name or address", None, "alice")],
        true,
    ),
    it(
        "Local network (processes)",
        "Stop all (down)",
        &["down"],
        &[],
        true,
    ),
    it(
        "Local network (processes)",
        "Delete the network (reset)",
        &["reset"],
        &[],
        true,
    ),
    it(
        "Docker lab",
        "Create the lab (compose init)",
        &["compose", "init"],
        &[
            p("validators", Some("--nodes"), "4"),
            p("spares", Some("--spare"), "0"),
            p("CPUs per node", Some("--cpus"), "0.5"),
            p("MiB per node", Some("--mem"), "256"),
        ],
        false,
    ),
    it(
        "Docker lab",
        "Start (compose up)",
        &["compose", "up"],
        &[],
        false,
    ),
    it(
        "Docker lab",
        "Containers (compose ps)",
        &["compose", "ps"],
        &[],
        false,
    ),
    it(
        "Docker lab",
        "CPU and memory usage (compose stats)",
        &["compose", "stats"],
        &[],
        false,
    ),
    it(
        "Docker lab",
        "Kill node N (compose kill)",
        &["compose", "kill"],
        &[p("node", None, "4")],
        false,
    ),
    it(
        "Docker lab",
        "Pause node N (compose pause)",
        &["compose", "pause"],
        &[p("node", None, "3")],
        false,
    ),
    it(
        "Docker lab",
        "Resume node N (compose unpause)",
        &["compose", "unpause"],
        &[p("node", None, "3")],
        false,
    ),
    it(
        "Docker lab",
        "Restart node N (compose start)",
        &["compose", "start"],
        &[p("node", None, "4")],
        false,
    ),
    it(
        "Docker lab",
        "Logs of node N (compose logs)",
        &["compose", "logs"],
        &[p("node", None, "1")],
        false,
    ),
    it(
        "Docker lab",
        "Stop (compose down)",
        &["compose", "down"],
        &[],
        false,
    ),
    it(
        "Docker lab",
        "Delete the lab (compose clean)",
        &["compose", "clean"],
        &[],
        false,
    ),
    it(
        "Test wallet",
        "New wallet",
        &["wallet", "new"],
        &[p("name", None, "alice")],
        true,
    ),
    it(
        "Test wallet",
        "List wallets",
        &["wallet", "list"],
        &[],
        true,
    ),
    it(
        "Test wallet",
        "Balance",
        &["balance"],
        &[p("name or address", None, "alice")],
        true,
    ),
    it(
        "Test wallet",
        "Test faucet (fund)",
        &["fund"],
        &[p("name", None, "alice"), p("amount", None, "1000")],
        true,
    ),
    it(
        "Test wallet",
        "Send (send)",
        &["send"],
        &[
            p("from", Some("--from"), "alice"),
            p("to", Some("--to"), "bob"),
            p("amount", Some("--amount"), "1,5"),
            p("how many", Some("--count"), "1"),
        ],
        true,
    ),
    it(
        "Issuance and reserve",
        "Mint (test issuers)",
        &["mint"],
        &[
            p("to", Some("--to"), "alice"),
            p("amount", Some("--amount"), "100"),
            p("reference", Some("--ref"), "dep-1"),
        ],
        true,
    ),
    it(
        "Issuance and reserve",
        "Burn",
        &["burn"],
        &[
            p("from", Some("--from"), "alice"),
            p("amount", Some("--amount"), "10"),
            p("reference", Some("--ref"), "withdrawal-1"),
        ],
        true,
    ),
    it(
        "Issuance and reserve",
        "Reserve attestation",
        &["reserve"],
        &[p("amount", Some("--amount"), "1000")],
        true,
    ),
    it(
        "Issuance and reserve",
        "Supply and coverage (supply)",
        &["supply"],
        &[],
        true,
    ),
    it(
        "Simulators (dev only)",
        "Equivocation by validator N (stop it first)",
        &["equivocate"],
        &[p("validator", Some("--as"), "4")],
        true,
    ),
    it(
        "Simulators (dev only)",
        "Validator rotation",
        &["rotate"],
        &[
            p("joins", Some("--in"), "5"),
            p("leaves", Some("--out"), "3"),
        ],
        true,
    ),
    it(
        "Simulators (dev only)",
        "Blocks on demand (blocks-sim)",
        &["blocks-sim"],
        &[p("how many", Some("--count"), "5")],
        true,
    ),
    it(
        "Load test (Docker)",
        "Load ladder (load)",
        &["load"],
        &[
            p("rates", Some("--rates"), "50,100,200,400,800"),
            p("seconds", Some("--secs"), "20"),
            p("repetitions", Some("--repeats"), "1"),
        ],
        false,
    ),
    it(
        "Quality",
        "Check the documentation (docs-check)",
        &["docs-check"],
        &[],
        false,
    ),
    it("Quality", "Full gate (check)", &["check"], &[], false),
    it(
        "Query",
        "Latest blocks (blocks)",
        &["blocks"],
        &[p("how many", Some("--limit"), "10")],
        true,
    ),
    it(
        "Query",
        "A transaction (tx)",
        &["tx"],
        &[p("id", None, "")],
        true,
    ),
];

/// Builds the `brdu-dev` command line (without the program name). `dir` goes in as `--dir` for the items that use it.
#[must_use]
pub fn command_line(item: &Item, values: &[String], dir: &str) -> Vec<String> {
    let mut out: Vec<String> = item.args.iter().map(|s| (*s).to_string()).collect();
    // positionals first (catalog order), then the flags
    for (param, v) in item.params.iter().zip(values) {
        if param.flag.is_none() && !v.trim().is_empty() {
            out.push(v.trim().to_string());
        }
    }
    for (param, v) in item.params.iter().zip(values) {
        if let Some(f) = param.flag
            && !v.trim().is_empty()
        {
            out.push(f.to_string());
            out.push(v.trim().to_string());
        }
    }
    if item.uses_dir && !dir.is_empty() {
        out.push("--dir".into());
        out.push(dir.to_string());
    }
    out
}

/// The suggested values of an item.
#[must_use]
pub fn defaults(item: &Item) -> Vec<String> {
    item.params.iter().map(|p| p.default.to_string()).collect()
}
