// BRDU wallet UI. All secrets stay in the wallet service (Rust): this file only ever receives public data and the
// recovery words once, at creation or when re-exporting with the password. It builds DOM with textContent only.
import { rpc } from "./api.js";
import { errorText, statusText } from "./i18n.js";
import { installEffects } from "./effects.js";
import { shortAddr } from "./format.js";
import { splitAmount, pressKey, display, typedToAmount } from "./money.js";

const $ = (s) => document.querySelector(s);
const main = $("#main");
const lockBtn = $("#lock");
const toastEl = $("#toast");

function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === false || v == null) continue;
    el.setAttribute(k, v === true ? "" : v);
  }
  for (const k of kids.flat()) {
    if (k == null || k === false) continue;
    el.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
  return el;
}

const dec = (text, tag = "span") => h(tag, { "data-decrypt": true }, text);

function amountEl(units, small = false) {
  const { int, dec: d } = splitAmount(units);
  return h("div", { class: small ? "amount sm" : "amount" }, int, h("span", { class: "dim" }, `,${d}`), h("span", { class: "unit" }, "BRDU"));
}

let toastTimer = null;
function toast(text, isError = false) {
  toastEl.hidden = false;
  toastEl.textContent = text;
  toastEl.className = isError ? "toast error" : "toast";
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toastEl.hidden = true), 4500);
}

// --- navigation -----------------------------------------------------------------------------------------------

const S = { screen: "loading", data: {}, timer: null };
const UNLOCKED = new Set(["home", "send", "receive", "history", "settings"]);

function go(screen, data = {}) {
  clearTimeout(S.timer);
  S.screen = screen;
  S.data = data;
  lockBtn.hidden = !UNLOCKED.has(screen);
  const view = screens[screen]();
  main.replaceChildren(view);
  main.focus({ preventScroll: true });
}

/** Runs an async action; shows translated errors; a "locked" error (auto-lock) sends the user to the unlock screen. */
async function attempt(fn) {
  try {
    return await fn();
  } catch (e) {
    if (e.code === "locked" && S.screen !== "locked") {
      toast(errorText("locked"), true);
      go("locked");
    } else {
      toast(errorText(e.code, e.message), true);
    }
    return undefined;
  }
}

const value = (form, name) => form.elements[name]?.value ?? "";
const clear = (form, ...names) => names.forEach((n) => form.elements[n] && (form.elements[n].value = ""));
const field = (id, label, input) => h("div", { class: "field" }, h("label", { for: id }, label), input);
const password = (id, name = id, auto = "current-password") => h("input", { id, name, type: "password", class: "input", autocomplete: auto, required: true });

// --- screens ----------------------------------------------------------------------------------------------------

const screens = {
  loading: () => h("p", { class: "muted" }, "Carregando…"),

  offline: () => h("section", { class: "card" }, h("h1", {}, "Sem conexão com a carteira"), h("p", { class: "muted" }, "Não consegui falar com o serviço da carteira."), h("button", { class: "primary", type: "button", id: "back" }, "Tentar de novo")),

  welcome: () =>
    h("section", { class: "card-glow" }, h("h1", {}, dec("Sua carteira BRDU")), h("p", { class: "muted" }, "1 BRDU vale R$ 1. As chaves nascem e ficam neste aparelho; a frase de recuperação é a única forma de recuperá-las."), h("div", { class: "field" }), h("button", { class: "primary", type: "button", id: "go-create" }, "Criar carteira nova"), h("div", { class: "field" }), h("button", { class: "ghost", type: "button", id: "go-import" }, "Importar uma frase de recuperação")),

  create: () => {
    const form = h("form", { id: "create" }, field("pw", "Senha da carteira (mínimo 8 caracteres)", password("pw", "password", "new-password")), field("pw2", "Repita a senha", password("pw2", "password2", "new-password")), h("details", { class: "field" }, h("summary", { class: "muted" }, "Avançado: passphrase (25ª palavra)"), h("p", { class: "muted" }, "Opcional. Ela NÃO é guardada em lugar nenhum: sem ela você não reabre a carteira."), h("input", { name: "passphrase", class: "input", type: "password", autocomplete: "off", "aria-label": "Passphrase" })), h("button", { class: "primary", type: "submit" }, "Criar carteira"));
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (value(form, "password") !== value(form, "password2")) return toast("As senhas não são iguais.", true);
      const r = await attempt(() => rpc("create", { password: value(form, "password"), passphrase: value(form, "passphrase") }));
      clear(form, "password", "password2", "passphrase");
      if (r) go("words", { words: r.words, address: r.address });
    });
    return h("section", { class: "card" }, h("h1", {}, "Criar carteira"), form, h("button", { class: "ghost small", type: "button", id: "back" }, "Voltar"));
  },

  words: () => {
    const { words } = S.data;
    const ok = h("input", { type: "checkbox", id: "wrote" });
    const next = h("button", { class: "primary", type: "button", disabled: true }, "Continuar");
    ok.addEventListener("change", () => (next.disabled = !ok.checked));
    next.addEventListener("click", () => go("home"));
    return h("section", { class: "card" }, h("h1", {}, "Sua frase de recuperação"), h("p", { class: "warn" }, "Anote as 24 palavras, em ordem, em papel. Quem tiver a frase controla seu dinheiro; sem ela, uma carteira perdida não volta. Ela não será mostrada de novo sem a sua senha."), h("ol", { class: "words" }, words.map((w) => h("li", {}, w))), h("label", { class: "check", for: "wrote" }, ok, "Anotei as 24 palavras em um lugar seguro."), next);
  },

  import: () => {
    const form = h("form", { id: "import" }, field("words", "Frase de recuperação (12 ou 24 palavras)", h("textarea", { id: "words", name: "words", class: "input", autocomplete: "off", spellcheck: "false", required: true })), h("details", { class: "field" }, h("summary", { class: "muted" }, "Avançado: passphrase (25ª palavra)"), h("input", { name: "passphrase", class: "input", type: "password", autocomplete: "off", "aria-label": "Passphrase" })), h("button", { class: "primary", type: "submit" }, "Continuar"));
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      const words = value(form, "words");
      const passphrase = value(form, "passphrase");
      const r = await attempt(() => rpc("preview_import", { words, passphrase }));
      if (r) go("confirm_import", { words, passphrase, address: r.address, count: r.word_count });
    });
    return h("section", { class: "card" }, h("h1", {}, "Importar carteira"), form, h("button", { class: "ghost small", type: "button", id: "back" }, "Voltar"));
  },

  confirm_import: () => {
    const { words, passphrase, address, count } = S.data;
    const form = h("form", { id: "confirm-import", hidden: true }, field("pw", "Escolha uma senha para este aparelho (mínimo 8 caracteres)", password("pw", "password", "new-password")), field("pw2", "Repita a senha", password("pw2", "password2", "new-password")), h("button", { class: "primary", type: "submit" }, "Importar"));
    const yes = h("button", { class: "primary", type: "button", id: "yes" }, "Sim, é este");
    yes.addEventListener("click", () => {
      form.hidden = false;
      yes.hidden = true;
    });
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (value(form, "password") !== value(form, "password2")) return toast("As senhas não são iguais.", true);
      const r = await attempt(() => rpc("import", { words, passphrase, password: value(form, "password"), expected_address: address }));
      clear(form, "password", "password2");
      if (r) go("home");
    });
    return h("section", { class: "card" }, h("h1", {}, "Confira o endereço"), h("p", { class: "muted" }, `Esta frase (${count} palavras) abre a carteira com o endereço abaixo. É o seu?`), h("div", { class: "addr" }, dec(address)), h("div", { class: "field" }), yes, form, h("button", { class: "ghost small", type: "button", id: "back" }, "Não é este, voltar"));
  },

  locked: () => {
    const form = h("form", { id: "unlock" }, field("pw", "Senha", password("pw", "password")), h("details", { class: "field" }, h("summary", { class: "muted" }, "Tenho uma passphrase"), h("input", { name: "passphrase", class: "input", type: "password", autocomplete: "off", "aria-label": "Passphrase" })), h("button", { class: "primary", type: "submit" }, "Desbloquear"));
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      const r = await attempt(() => rpc("unlock", { password: value(form, "password"), passphrase: value(form, "passphrase") }));
      clear(form, "password", "passphrase");
      if (r) go("home");
    });
    const out = h("details", { class: "field", id: "forget-box" }, h("summary", { class: "muted" }, "Esqueci a senha ou quero outra carteira"), h("p", { class: "warn" }, "Isto remove a carteira DESTE aparelho (o arquivo é guardado de lado, não apagado). Só a frase de recuperação de 24 palavras devolve o acesso ao dinheiro: sem ela, o saldo desta carteira se perde."), h("p", { class: "muted" }, "Depois você poderá criar uma carteira nova ou importar a sua frase."), field("confirm-forget", "Para confirmar, digite APAGAR", (S.forgetInput = h("input", { id: "confirm-forget", name: "confirm", class: "input", autocomplete: "off" }))), h("button", { class: "ghost", type: "button", id: "forget" }, "Remover desta carteira"));
    return h("section", { class: "card" }, h("h1", {}, dec("Carteira bloqueada")), h("p", { class: "muted" }, S.data.address ? `Endereço ${shortAddr(S.data.address)}` : ""), form, out);
  },

  home: () => {
    const wrap = h("div", { class: "card-glow" }, h("p", { class: "muted" }, "Carregando saldo…"));
    const rest = h("div", { id: "home-rest" });
    const creds = h("div", { id: "home-creds" });
    const assetsBox = h("div", { id: "home-assets" });
    const contractsBox = h("div", { id: "home-contracts" });
    const load = async () => {
      const o = await attempt(() => rpc("overview"));
      if (S.screen !== "home") return;
      if (o) {
        const sup = o.supply?.coverage?.state;
        const copyBtn = h("button", { class: "chip", type: "button", id: "copy-address", "aria-label": "Copiar endereço" }, `${shortAddr(o.address)} · copiar`);
        copyBtn.addEventListener("click", () => copy(o.address));
        wrap.replaceChildren(h("div", { class: "label" }, "Saldo"), amountEl(o.balance), h("div", { class: "chips" }, copyBtn));
        rest.replaceChildren(h("div", { class: "card" }, h("h2", {}, "Rede"), h("div", { class: "chips" }, h("span", { class: "chip green" }, `altura ${o.height}`), h("span", { class: sup === "under" ? "chip yellow" : "chip blue" }, sup === "covered" ? "reserva cobre a oferta" : sup === "under" ? "reserva ABAIXO da oferta" : "sem atestado de reserva")), h("p", { class: "muted" }, `Oferta em circulação: ${splitAmount(o.supply.supply).int} BRDU`)));
      }
      const cr = await rpc("credentials").catch(() => null);
      if (S.screen === "home" && cr?.credentials?.length) {
        creds.replaceChildren(
          h("div", { class: "card" }, h("h2", {}, "Credenciais"), h("ul", { class: "list" }, cr.credentials.map((c) => h("li", { class: "row" }, h("div", {}, h("div", {}, c.id), h("div", { class: "sub" }, `tipo ${c.type}`)), h("div", { class: "end" }, h("span", { class: c.valid ? "chip green" : c.status === "pending" ? "chip yellow" : "chip" }, c.valid ? "válida" : statusText(c.status))))))),
        );
      }
      const as = await rpc("assets").catch(() => null);
      if (S.screen === "home" && as?.assets?.length) {
        assetsBox.replaceChildren(
          h("div", { class: "card" }, h("h2", {}, "Bens"), h("ul", { class: "list" }, as.assets.map((x) => h("li", { class: "row" }, h("div", {}, h("div", {}, x.id), h("div", { class: "sub" }, `classe ${x.class}${Object.keys(x.liens || {}).length ? " · com gravame" : ""}`)), h("div", { class: "end" }, x.deal ? h("span", { class: "chip yellow" }, x.deal.status === "accepted" ? "em negociação" : "oferecido") : null))))),
        );
      }
      const ct = await rpc("contracts").catch(() => null);
      if (S.screen === "home" && ct?.contracts?.length) {
        contractsBox.replaceChildren(
          h("div", { class: "card" }, h("h2", {}, "Contratos"), h("ul", { class: "list" }, ct.contracts.map((c) => h("li", { class: "row" }, h("div", {}, h("div", {}, c.id), h("div", { class: "sub" }, `modelo ${c.template}`)), h("div", { class: "end" }, h("span", { class: "chip" }, c.state)))))),
        );
      }
      S.timer = setTimeout(load, 4000);
    };
    const actions = h("div", { class: "actions" }, ...[["send", "Enviar"], ["receive", "Receber"], ["history", "Histórico"], ["settings", "Ajustes"]].map(([to, label]) => h("button", { class: "ghost", type: "button", "data-go": to }, label)));
    actions.addEventListener("click", (e) => {
      const to = e.target.closest?.("[data-go]")?.dataset.go;
      if (to) go(to === "send" ? "send" : to, to === "send" ? { step: "edit", to: "", typed: "" } : {});
    });
    load();
    return h("div", { class: "stack" }, wrap, actions, rest, creds, assetsBox, contractsBox);
  },

  receive: () => {
    const box = h("div", { class: "addr" }, "…");
    attempt(() => rpc("status")).then((s) => s?.address && box.replaceChildren(dec(s.address)));
    return h("section", { class: "card" }, h("h1", {}, "Receber"), h("p", { class: "muted" }, "Passe este endereço para quem vai enviar BRDU. Ele é público e seguro de compartilhar."), box, h("div", { class: "field" }), h("button", { class: "primary", type: "button", id: "copy" }, "Copiar endereço"), h("button", { class: "ghost small", type: "button", id: "back" }, "Voltar"));
  },

  history: () => {
    const list = h("ul", { class: "list" }, h("li", { class: "muted" }, "Carregando…"));
    attempt(() => rpc("history", { skip: 0, limit: 25 })).then((r) => {
      if (!r) return list.replaceChildren(h("li", { class: "muted" }, "Histórico indisponível."));
      const items = r.txs.map((t) => {
        const incoming = t.kind === 2 || (t.kind === 1 && t.summary?.to === r.address);
        const what = t.kind === 1 ? (incoming ? `Recebido de ${shortAddr(t.sender)}` : `Enviado para ${shortAddr(t.summary?.to)}`) : t.kind === 2 ? "Depósito (emissão)" : t.kind === 3 ? "Resgate" : "Operação";
        return h("li", { class: "row" }, h("div", {}, h("div", {}, what), h("div", { class: "sub" }, `bloco #${t.height} · ${t.id.slice(0, 10)}…`)), h("div", { class: incoming ? "end in" : "end" }, t.summary?.amount != null ? `${incoming ? "+" : "−"}${splitAmount(t.summary.amount).int},${splitAmount(t.summary.amount).dec.slice(0, 2)}` : ""));
      });
      list.replaceChildren(...(items.length ? items : [h("li", { class: "muted" }, "Nenhuma movimentação ainda.")]));
    });
    return h("section", { class: "card" }, h("h1", {}, "Histórico"), list, h("div", { class: "field" }), h("button", { class: "ghost small", type: "button", id: "back" }, "Voltar"));
  },

  send: () => {
    const d = S.data;
    if (d.step === "review") return reviewSend(d);
    if (d.step === "done") return doneSend(d);
    return editSend(d);
  },

  settings: () => {
    const out = h("div", { id: "settings-out" });
    const showForm = h("form", { id: "show-phrase" }, field("pw", "Senha", password("pw", "password")), h("button", { class: "ghost", type: "submit" }, "Mostrar a frase de recuperação"));
    showForm.addEventListener("submit", async (e) => {
      e.preventDefault();
      const r = await attempt(() => rpc("export_phrase", { password: value(showForm, "password") }));
      clear(showForm, "password");
      if (r) out.replaceChildren(h("p", { class: "warn" }, "Não deixe ninguém ver esta tela."), h("ol", { class: "words" }, r.words.map((w) => h("li", {}, w))));
    });
    const pwForm = h("form", { id: "change-pw" }, field("old", "Senha atual", password("old", "old")), field("new", "Nova senha (mínimo 8 caracteres)", password("new", "new", "new-password")), h("button", { class: "ghost", type: "submit" }, "Trocar a senha"));
    pwForm.addEventListener("submit", async (e) => {
      e.preventDefault();
      const r = await attempt(() => rpc("change_password", { old: value(pwForm, "old"), new: value(pwForm, "new") }));
      clear(pwForm, "old", "new");
      if (r) toast("Senha trocada.");
    });
    return h("section", { class: "card" }, h("h1", {}, "Ajustes"), h("h2", {}, "Frase de recuperação"), showForm, out, h("div", { class: "field" }), h("h2", {}, "Senha"), pwForm, h("div", { class: "field" }), h("button", { class: "ghost small", type: "button", id: "back" }, "Voltar"));
  },
};

// --- send flow ------------------------------------------------------------------------------------------------

function editSend(d) {
  const shown = h("div", { class: "amount", id: "typed", "aria-live": "polite" });
  const paint = () => {
    shown.replaceChildren(display(d.typed), h("span", { class: "unit" }, "BRDU"));
    review.disabled = !(typedToAmount(d.typed) > 0n) || !to.value.trim();
  };
  const to = h("input", { id: "to", class: "input", placeholder: "Endereço brdu1…", autocomplete: "off", spellcheck: "false", value: d.to ?? "" });
  const review = h("button", { class: "primary", type: "button", id: "review", disabled: true }, "Revisar");
  const press = (k) => {
    d.typed = pressKey(d.typed, k);
    paint();
  };
  const pad = h("div", { class: "keypad", role: "group", "aria-label": "Teclado numérico" }, ...["1", "2", "3", "4", "5", "6", "7", "8", "9", ",", "0", "back"].map((k) => h("button", { class: "key", type: "button", "data-key": k, "aria-label": k === "back" ? "apagar" : k === "," ? "vírgula" : k }, k === "back" ? "⌫" : k)));
  pad.addEventListener("click", (e) => {
    const k = e.target.closest?.("[data-key]")?.dataset.key;
    if (k) press(k);
  });
  to.addEventListener("input", () => {
    d.to = to.value;
    paint();
  });
  review.addEventListener("click", async () => {
    const amount = display(d.typed).replace(/,$/, "");
    const r = await attempt(() => rpc("prepare_send", { to: to.value.trim(), amount }));
    if (r) go("send", { step: "review", prepared: r, to: to.value.trim(), typed: d.typed });
  });
  S.keyHandler = (e) => {
    if (e.target.closest?.("input,textarea")) return;
    if (/^\d$/.test(e.key) || e.key === "," || e.key === ".") press(e.key);
    else if (e.key === "Backspace") press("back");
  };
  paint();
  return h("section", { class: "card" }, h("h1", {}, "Enviar"), field("to", "Para", to), h("div", { class: "label" }, "Valor"), shown, pad, h("div", { class: "field" }), review, h("button", { class: "ghost small", type: "button", id: "back" }, "Voltar"));
}

function reviewSend(d) {
  const p = d.prepared;
  const confirm = h("button", { class: "primary", type: "button", id: "confirm" }, "Confirmar e enviar");
  confirm.addEventListener("click", async () => {
    confirm.disabled = true;
    const r = await attempt(() => rpc("confirm_send", { id: p.id }));
    if (r) go("send", { step: "done", result: r });
    else confirm.disabled = false;
  });
  const row = (k, v) => [h("dt", {}, k), h("dd", {}, v)];
  return h("section", { class: "card" }, h("h1", {}, "Revise o envio"), p.to_self ? h("p", { class: "warn" }, "Você está enviando para o seu próprio endereço.") : null, h("dl", { class: "kv" }, ...row("Para", h("span", { class: "mono" }, p.to)), ...row("Valor", amountEl(p.amount, true)), ...row("Taxa", amountEl(p.fee, true)), ...row("Total", amountEl(p.total, true)), ...row("Saldo depois", amountEl(p.balance_after, true))), h("p", { class: "muted" }, "Transações na rede BRDU não podem ser desfeitas nem congeladas."), h("div", { class: "field" }), confirm, h("button", { class: "ghost small", type: "button", id: "edit" }, "Voltar e editar"));
}

function doneSend(d) {
  const r = d.result;
  return h("section", { class: "card-glow" }, h("h1", {}, dec("Enviado")), amountEl(r.amount), h("p", { class: "muted" }, `para ${shortAddr(r.to)}`), h("p", { class: "mono" }, dec(r.tx_id)), h("p", { class: "muted" }, "A transação entra no próximo bloco, em instantes."), h("div", { class: "field" }), h("button", { class: "primary", type: "button", id: "home" }, "Voltar ao início"));
}

// --- global wiring ----------------------------------------------------------------------------------------------

function copy(text) {
  navigator.clipboard?.writeText(text).then(() => toast("Copiado."), () => toast("Não consegui copiar.", true));
}

main.addEventListener("click", async (e) => {
  const id = e.target.closest?.("button")?.id;
  if (id === "go-create") go("create");
  else if (id === "go-import") go("import");
  else if (id === "back") boot();
  else if (id === "forget") {
    const typed = S.forgetInput?.value ?? "";
    if (typed.trim() !== "APAGAR") return toast("Digite APAGAR para confirmar.", true);
    const r = await attempt(() => rpc("forget", { confirm: "forget" }));
    if (r) {
      toast("Carteira removida deste aparelho. Crie uma nova ou importe a sua frase.");
      boot();
    }
  }
  else if (id === "home") go("home");
  else if (id === "edit") go("send", { step: "edit", to: S.data.to, typed: S.data.typed });
  else if (id === "copy" && S.screen === "receive") {
    const s = await attempt(() => rpc("status"));
    if (s?.address) copy(s.address);
  }
});

document.addEventListener("keydown", (e) => {
  if (S.screen === "send" && S.data.step === "edit") S.keyHandler?.(e);
});

lockBtn.addEventListener("click", async () => {
  await attempt(() => rpc("lock"));
  boot();
});

async function boot() {
  const s = await attempt(() => rpc("status"));
  if (!s) return go("offline");
  if (!s.has_vault) go("welcome");
  else if (!s.unlocked) go("locked", { address: s.address });
  else go("home");
}

installEffects();
boot();
