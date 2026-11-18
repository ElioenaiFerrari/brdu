//! O catálogo do menu: cada item é um comando do `brdu-dev` com parâmetros. Separado da tela para ser testável.

/// Um parâmetro: se `flag` existe, vai como `--flag valor`; senão é posicional. Valor vazio = omitido.
pub struct Param {
    /// Nome mostrado no formulário.
    pub name: &'static str,
    /// A flag (`--amount`) ou `None` para posicional.
    pub flag: Option<&'static str>,
    /// Valor sugerido.
    pub default: &'static str,
}

/// Um item do menu.
pub struct Item {
    /// Grupo (título da seção).
    pub group: &'static str,
    /// O que aparece no menu.
    pub label: &'static str,
    /// Os argumentos fixos do `brdu-dev` (antes dos parâmetros).
    pub args: &'static [&'static str],
    /// Os parâmetros a perguntar.
    pub params: &'static [Param],
    /// Este comando aceita `--dir`?
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

/// Todos os itens, na ordem do menu.
pub const CATALOG: &[Item] = &[
    it(
        "Rede local (processos)",
        "Criar a rede (init)",
        &["init"],
        &[
            p("validadores", Some("--nodes"), "4"),
            p("reservas", Some("--spare"), "0"),
        ],
        true,
    ),
    it(
        "Rede local (processos)",
        "Subir os nós (up)",
        &["up"],
        &[],
        true,
    ),
    it(
        "Rede local (processos)",
        "Estado dos nós (status)",
        &["status"],
        &[],
        true,
    ),
    it(
        "Rede local (processos)",
        "Derrubar o nó N (stop)",
        &["stop"],
        &[p("nó", None, "4")],
        true,
    ),
    it(
        "Rede local (processos)",
        "Religar o nó N (start)",
        &["start"],
        &[p("nó", None, "4")],
        true,
    ),
    it(
        "Rede local (processos)",
        "Derrubar todos (down)",
        &["down"],
        &[],
        true,
    ),
    it(
        "Rede local (processos)",
        "Apagar a rede (reset)",
        &["reset"],
        &[],
        true,
    ),
    it(
        "Laboratório Docker",
        "Criar o laboratório (compose init)",
        &["compose", "init"],
        &[
            p("validadores", Some("--nodes"), "4"),
            p("reservas", Some("--spare"), "0"),
            p("CPUs por nó", Some("--cpus"), "0.5"),
            p("MiB por nó", Some("--mem"), "256"),
        ],
        false,
    ),
    it(
        "Laboratório Docker",
        "Subir (compose up)",
        &["compose", "up"],
        &[],
        false,
    ),
    it(
        "Laboratório Docker",
        "Contêineres (compose ps)",
        &["compose", "ps"],
        &[],
        false,
    ),
    it(
        "Laboratório Docker",
        "Uso de CPU e memória (compose stats)",
        &["compose", "stats"],
        &[],
        false,
    ),
    it(
        "Laboratório Docker",
        "Matar o nó N (compose kill)",
        &["compose", "kill"],
        &[p("nó", None, "4")],
        false,
    ),
    it(
        "Laboratório Docker",
        "Pausar o nó N (compose pause)",
        &["compose", "pause"],
        &[p("nó", None, "3")],
        false,
    ),
    it(
        "Laboratório Docker",
        "Retomar o nó N (compose unpause)",
        &["compose", "unpause"],
        &[p("nó", None, "3")],
        false,
    ),
    it(
        "Laboratório Docker",
        "Religar o nó N (compose start)",
        &["compose", "start"],
        &[p("nó", None, "4")],
        false,
    ),
    it(
        "Laboratório Docker",
        "Logs do nó N (compose logs)",
        &["compose", "logs"],
        &[p("nó", None, "1")],
        false,
    ),
    it(
        "Laboratório Docker",
        "Derrubar (compose down)",
        &["compose", "down"],
        &[],
        false,
    ),
    it(
        "Laboratório Docker",
        "Apagar o laboratório (compose clean)",
        &["compose", "clean"],
        &[],
        false,
    ),
    it(
        "Carteira de teste",
        "Nova carteira",
        &["wallet", "new"],
        &[p("nome", None, "alice")],
        true,
    ),
    it(
        "Carteira de teste",
        "Listar carteiras",
        &["wallet", "list"],
        &[],
        true,
    ),
    it(
        "Carteira de teste",
        "Saldo",
        &["balance"],
        &[p("nome ou endereço", None, "alice")],
        true,
    ),
    it(
        "Carteira de teste",
        "Faucet de teste (fund)",
        &["fund"],
        &[p("nome", None, "alice"), p("valor", None, "1000")],
        true,
    ),
    it(
        "Carteira de teste",
        "Enviar (send)",
        &["send"],
        &[
            p("de", Some("--from"), "alice"),
            p("para", Some("--to"), "bob"),
            p("valor", Some("--amount"), "1,5"),
            p("quantas", Some("--count"), "1"),
        ],
        true,
    ),
    it(
        "Emissão e reserva",
        "Mint (emissores de teste)",
        &["mint"],
        &[
            p("para", Some("--to"), "alice"),
            p("valor", Some("--amount"), "100"),
            p("referência", Some("--ref"), "dep-1"),
        ],
        true,
    ),
    it(
        "Emissão e reserva",
        "Burn",
        &["burn"],
        &[
            p("de", Some("--from"), "alice"),
            p("valor", Some("--amount"), "10"),
            p("referência", Some("--ref"), "saque-1"),
        ],
        true,
    ),
    it(
        "Emissão e reserva",
        "Atestado de reserva",
        &["reserve"],
        &[p("valor", Some("--amount"), "1000")],
        true,
    ),
    it(
        "Emissão e reserva",
        "Oferta e cobertura (supply)",
        &["supply"],
        &[],
        true,
    ),
    it(
        "Simuladores (só dev)",
        "Equivocação do validador N (pare-o antes)",
        &["equivocate"],
        &[p("validador", Some("--as"), "4")],
        true,
    ),
    it(
        "Simuladores (só dev)",
        "Rotação de validadores",
        &["rotate"],
        &[p("entra", Some("--in"), "5"), p("sai", Some("--out"), "3")],
        true,
    ),
    it(
        "Simuladores (só dev)",
        "Blocos sob demanda (blocks-sim)",
        &["blocks-sim"],
        &[p("quantos", Some("--count"), "5")],
        true,
    ),
    it(
        "Teste de carga (Docker)",
        "Escada de carga (load)",
        &["load"],
        &[
            p("taxas", Some("--rates"), "50,100,200,400,800"),
            p("segundos", Some("--secs"), "20"),
            p("repetições", Some("--repeats"), "1"),
        ],
        false,
    ),
    it(
        "Qualidade",
        "Conferir a documentação (docs-check)",
        &["docs-check"],
        &[],
        false,
    ),
    it(
        "Qualidade",
        "Portão completo (check)",
        &["check"],
        &[],
        false,
    ),
    it(
        "Consulta",
        "Últimos blocos (blocks)",
        &["blocks"],
        &[p("quantos", Some("--limit"), "10")],
        true,
    ),
    it(
        "Consulta",
        "Uma transação (tx)",
        &["tx"],
        &[p("id", None, "")],
        true,
    ),
];

/// Monta a linha de comando do `brdu-dev` (sem o nome do programa). `dir` entra como `--dir` nos itens que o usam.
#[must_use]
pub fn command_line(item: &Item, values: &[String], dir: &str) -> Vec<String> {
    let mut out: Vec<String> = item.args.iter().map(|s| (*s).to_string()).collect();
    // posicionais primeiro (a ordem do catálogo), depois as flags
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

/// Os valores sugeridos de um item.
#[must_use]
pub fn defaults(item: &Item) -> Vec<String> {
    item.params.iter().map(|p| p.default.to_string()).collect()
}
