// BRDU explorer: single page with hash routes. It only reads its own server's API (`/api/...`) and builds everything
// with safe DOM (textContent): no node data ever becomes HTML.
import { installEffects } from "./effects.js";
import { splitAmount, shortHash, shortAddr, ago, when } from "./format.js";
import { kindName, action, message, status } from "./i18n.js";

const $ = (s) => document.querySelector(s);
const main = $("#main");
const hint = $("#hint");

/** Creates an element: h("div", {class:"x", href:"#"}, child1, "text"...). */
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

async function api(path) {
  const r = await fetch(`/api/${path}`, { headers: { accept: "application/json" } });
  const body = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(body.error ? message(body.error) : `erro ${r.status}`);
  return body;
}

// --- translation layer: the API returns English codes; people read pt-BR ------------------------------------------------------------------------------------------------------

function amount(units, { small = false, unit = true } = {}) {
  const { int, dec } = splitAmount(units);
  return h("div", { class: small ? "amount sm" : "amount" }, int, h("span", { class: "dim" }, `,${dec}`), unit ? h("span", { class: "unit" }, "BRDU") : null);
}

const chip = (text, tone = "") => h("span", { class: `chip ${tone}`.trim() }, text);
const dec = (text) => h("span", { "data-decrypt": true }, text);
const link = (href, text, cls = "") => h("a", { href, class: cls }, text);

function kv(pairs) {
  return h("dl", { class: "kv" }, pairs.filter(Boolean).flatMap(([k, v]) => [h("dt", {}, k), h("dd", {}, v)]));
}

function blockRow(b) {
  return h(
    "li",
    {},
    h(
      "a",
      { class: "row", href: `#/block/${b.height}` },
      h("span", { class: "num" }, `#${b.height}`),
      h("span", { class: "main" }, h("span", { class: "mono" }, dec(shortHash(b.hash, 12))), h("div", { class: "sub" }, `${b.tx_count} transação(ões) · proponente ${shortAddr(b.proposer)} · rodada ${b.round}`)),
      h("span", { class: "end" }, `✓ ${b.signed_by.length}/${b.validators}`, h("div", {}, when(b.time))),
    ),
  );
}

function txRow(t, me) {
  const s = t.summary ?? {};
  let what = kindName(t.kind_name);
  if (t.kind === 1 && s.to) what = me && s.to === me ? `recebeu de ${shortAddr(t.sender)}` : `enviou para ${shortAddr(s.to)}`;
  return h(
    "li",
    {},
    h(
      "a",
      { class: "row", href: `#/tx/${t.id}` },
      h("span", { class: "num" }, `#${t.height}`),
      h("span", { class: "main" }, h("span", { class: "mono" }, shortHash(t.id, 12)), h("div", { class: "sub" }, what)),
      h("span", { class: "end" }, s.amount != null ? amount(s.amount, { small: true, unit: false }) : kindName(t.kind_name), h("div", {}, `taxa ${splitAmount(t.fee).int},${splitAmount(t.fee).dec}`)),
    ),
  );
}

function coverageChip(sup) {
  const c = sup.coverage?.state;
  if (c === "covered") return chip(`coberta · sobra ${splitAmount(sup.coverage.surplus).int} BRDU`, "green");
  if (c === "under") return chip(`ABAIXO · faltam ${splitAmount(sup.coverage.shortfall).int} BRDU`, "yellow");
  return chip("sem atestado de reserva", "blue");
}

// --- pages ----------------------------------------------------------------------------------------------------

async function pageHome() {
  const o = await api("overview");
  const sup = o.supply;
  return h(
    "div",
    { class: "grid" },
    h("h1", {}, dec("Rede BRDU")),
    o.node.rejected_blocks > 0 ? h("p", { class: "alert", role: "alert" }, `O nó entregou ${o.node.rejected_blocks} bloco(s) que a verificação RECUSOU: ${o.node.last_error}`) : null,
    o.pending_change ? h("p", { class: "warn" }, `Troca de validador pendente: ${action(o.pending_change.action)} de ${shortAddr(o.pending_change.validator)}, efetiva na altura ${o.pending_change.effective_height}.`) : null,
    h(
      "section",
      { class: "card-glow", "aria-label": "Oferta" },
      h("div", { class: "label" }, "Oferta em circulação (1 BRDU = R$ 1)"),
      amount(sup.supply),
      h("div", { class: "chips" }, coverageChip(sup), chip(`reserva ${sup.reserve != null ? splitAmount(sup.reserve).int : "—"} BRDU`), chip(`emitido ${splitAmount(sup.minted).int}`), chip(`queimado ${splitAmount(sup.burned).int}`)),
    ),
    h("div", { class: "chips" }, chip(`altura ${o.height}`, "green"), chip(`época ${o.epoch}`, "blue"), chip(`${o.validators.length} validadores`), chip(`${o.tx_total} transações`), chip(`${o.accounts} contas`)),
    h(
      "div",
      { class: "grid cols" },
      h("section", { class: "card" }, h("h2", {}, "Últimos blocos"), o.recent_blocks.length ? h("ul", { class: "list" }, o.recent_blocks.map(blockRow)) : h("p", { class: "empty" }, "Nenhum bloco ainda. A rede dorme até haver uma transação."), h("a", { class: "more", href: "#/blocks" }, "Ver todos os blocos")),
      h("section", { class: "card" }, h("h2", {}, "Validadores em vigor"), h("ul", { class: "list" }, o.validators.map((a) => h("li", {}, h("a", { class: "row", href: `#/account/${a}` }, h("span", { class: "num" }, "✓"), h("span", { class: "main mono" }, shortAddr(a, 16)))))), h("a", { class: "more", href: "#/validators" }, "Detalhes")),
    ),
    h("p", { class: "label" }, "Raiz de estado ", h("span", { class: "mono" }, shortHash(o.state_root, 16)), " · gênese ", h("span", { class: "mono" }, shortHash(o.genesis_hash, 16))),
  );
}

async function pageBlocks(state) {
  const wrap = h("div", { class: "grid" }, h("h1", {}, dec("Blocos")));
  const list = h("ul", { class: "list" });
  const more = h("button", { class: "more", type: "button" }, "Carregar mais");
  let before = null;
  const load = async () => {
    more.disabled = true;
    const r = await api(`blocks?limit=20${before ? `&before=${before}` : ""}`);
    list.append(...r.blocks.map(blockRow));
    before = r.blocks.length ? r.blocks[r.blocks.length - 1].height : 1;
    more.disabled = false;
    more.hidden = before <= 1;
    if (!r.blocks.length && !list.children.length) list.append(h("li", { class: "empty" }, "Nenhum bloco ainda."));
  };
  await load();
  more.addEventListener("click", () => load().catch((e) => showHint(e.message)));
  wrap.append(list, more);
  state.skipRefresh = true;
  return wrap;
}

async function pageBlock(n) {
  const b = await api(`block/${n}`);
  const certs = b.signed_by.map((a) => h("li", {}, h("a", { class: "row", href: `#/account/${a}` }, h("span", { class: "num" }, "✓"), h("span", { class: "main mono" }, shortAddr(a, 16)))));
  return h(
    "div",
    { class: "grid" },
    h("h1", {}, `Bloco #${b.height}`),
    h("div", { class: "chips" }, chip("verificado por este explorador", "green"), chip(`rodada ${b.round}`, "blue"), chip(`${b.tx_count} transação(ões)`)),
    h("section", { class: "card" }, kv([["Hash", h("span", { class: "mono" }, dec(b.hash))], ["Bloco anterior", b.prev_hash ? link(`#/block/${b.height - 1}`, shortHash(b.prev_hash, 16), "mono") : "— (primeiro bloco)"], ["Hora", when(b.time)], ["Proponente", link(`#/account/${b.proposer}`, shortAddr(b.proposer, 16), "mono")], ["Raiz de estado", h("span", { class: "mono" }, shortHash(b.state_root, 24))]])),
    h("section", { class: "card" }, h("h2", {}, `Certificado de commit: ${b.signed_by.length} de ${b.validators} validadores (mais de 2/3)`), h("ul", { class: "list" }, certs)),
    h("section", { class: "card" }, h("h2", {}, "Transações"), b.txs.length ? h("ul", { class: "list" }, b.txs.map((t) => txRow(t))) : h("p", { class: "empty" }, "Bloco sem transações.")),
    h("div", { class: "chips" }, b.height > 1 ? link(`#/block/${b.height - 1}`, "← bloco anterior", "chip") : null, link(`#/block/${b.height + 1}`, "próximo bloco →", "chip")),
  );
}

function describe(t) {
  const s = t.summary ?? {};
  if (t.kind === 1) return [["Para", link(`#/account/${s.to}`, s.to, "mono")], ["Valor", amount(s.amount, { small: true })]];
  if (t.kind === 2 || t.kind === 3) return [["Conta", link(`#/account/${s.account}`, s.account, "mono")], ["Valor", amount(s.amount, { small: true })], ["Referência", s.reference]];
  if (t.kind === 4) return [["Atestado nº", s.seq], ["Reserva atestada", amount(s.reserve, { small: true })], ["Referência", s.reference]];
  if (t.kind === 14) return [["Tipo criado", s.type]];
  if (t.kind === 15) return [["Tipo", s.type], ["Credencial", s.id], ["Titular", link(`#/account/${s.subject}`, s.subject, "mono")]];
  if (t.kind >= 16 && t.kind <= 18) return [["Credencial", s.id]];
  if (t.kind === 19) return [["Credencial", s.id], ["Motivo", s.reason]];
  if (t.kind === 22) return [["Ação", action(s.action)], ["Validador", link(`#/account/${s.validator}`, s.validator, "mono")], ["Efetiva na altura", s.effective_height]];
  return [];
}

async function pageTx(id) {
  const t = await api(`tx/${id}`);
  return h("div", { class: "grid" }, h("h1", {}, "Transação"), h("div", { class: "chips" }, chip(kindName(t.kind_name), "blue"), chip(`no bloco #${t.height}`, "green")), h("section", { class: "card" }, kv([["Id", h("span", { class: "mono" }, dec(t.id))], ["Bloco", link(`#/block/${t.height}`, `#${t.height} (posição ${t.index})`)], ["Remetente", link(`#/account/${t.sender}`, t.sender, "mono")], ["Nonce", t.nonce], ["Taxa", amount(t.fee, { small: true })], ["Tamanho", `${t.size} bytes`], ...describe(t)])));
}

async function pageAccount(addr, state) {
  const a = await api(`account/${addr}`);
  const list = h("ul", { class: "list" }, a.txs.map((t) => txRow(t, addr)));
  const more = h("button", { class: "more", type: "button" }, "Carregar mais");
  let shown = a.txs.length;
  more.hidden = shown >= a.tx_count;
  more.addEventListener("click", async () => {
    const r = await api(`account/${addr}?skip=${shown}&limit=25`);
    list.append(...r.txs.map((t) => txRow(t, addr)));
    shown += r.txs.length;
    more.hidden = shown >= a.tx_count || r.txs.length === 0;
  });
  state.skipRefresh = true;
  return h("div", { class: "grid" }, h("h1", {}, "Conta"), h("section", { class: "card-glow" }, h("div", { class: "label" }, "Saldo"), amount(a.balance), h("div", { class: "chips" }, chip(`nonce ${a.nonce}`), chip(`${a.tx_count} movimentação(ões)`), a.is_validator ? chip("validador em vigor", "green") : null), h("p", { class: "mono" }, a.address)), a.credentials?.length ? h("section", { class: "card" }, h("h2", {}, "Credenciais"), h("ul", { class: "list" }, a.credentials.map((c) => h("li", { class: "row" }, h("div", {}, h("div", {}, `${c.id} · ${c.type}`), h("div", { class: "sub" }, c.privacy === "private" ? "dados privados (só o compromisso está na rede)" : `dados: ${c.data_hex ? c.data_hex.slice(0, 24) : ""}`)), h("div", { class: "end" }, c.valid ? chip("válida", "green") : chip(status(c.status), c.status === "pending" ? "yellow" : "")))))) : null, h("section", { class: "card" }, h("h2", {}, "Movimentações"), a.txs.length ? list : h("p", { class: "empty" }, "Nenhuma movimentação."), more));
}

async function pageValidators() {
  const v = await api("validators");
  const ev = Array.isArray(v.evidence) ? v.evidence : [];
  return h(
    "div",
    { class: "grid" },
    h("h1", {}, "Validadores"),
    h("div", { class: "chips" }, chip(`época ${v.epoch}`, "blue"), chip(`${v.validators.length} em vigor`, "green")),
    v.pending_change ? h("p", { class: "warn" }, `Troca pendente: ${action(v.pending_change.action)} de ${v.pending_change.validator}, efetiva na altura ${v.pending_change.effective_height}.`) : null,
    h("section", { class: "card" }, h("h2", {}, "Conjunto em vigor (na ordem do rodízio)"), h("ul", { class: "list" }, v.validators.map((a, i) => h("li", {}, h("a", { class: "row", href: `#/account/${a}` }, h("span", { class: "num" }, i + 1), h("span", { class: "main mono" }, a)))))),
    h("section", { class: "card" }, h("h2", {}, "Evidências de equivocação"), ev.length ? h("ul", { class: "list" }, ev.map((e) => h("li", { class: "row" }, h("span", { class: "num" }, "!"), h("span", { class: "main" }, h("span", { class: "mono" }, shortAddr(e.voter, 16)), h("div", { class: "sub" }, `altura ${e.height}, rodada ${e.round}: dois votos diferentes`))))) : h("p", { class: "empty" }, "Nenhuma evidência."), h("p", { class: "label" }, message(v.evidence_note))),
  );
}

// --- routes ------------------------------------------------------------------------------------------------------

function route() {
  const [, kind, arg] = location.hash.split("/");
  const page = kind || "home";
  document.querySelectorAll("[data-nav]").forEach((a) => {
    const on = a.dataset.nav === page || (page === "block" && a.dataset.nav === "blocks");
    if (on) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  });
  return { page, arg };
}

let timer = null;
let current = "";

async function render(first) {
  const { page, arg } = route();
  const key = location.hash;
  const state = { skipRefresh: false };
  try {
    let view;
    if (page === "home") view = await pageHome();
    else if (page === "blocks") view = await pageBlocks(state);
    else if (page === "block") view = await pageBlock(arg);
    else if (page === "tx") view = await pageTx(arg);
    else if (page === "account") view = await pageAccount(arg, state);
    else if (page === "validators") view = await pageValidators();
    else view = h("p", { class: "empty" }, "Página não encontrada.");
    if (key !== location.hash) return; // the user already navigated
    main.replaceChildren(view);
    if (first) main.focus({ preventScroll: true });
    hideHint();
    clearTimeout(timer);
    // list pages with "load more" do not refresh by themselves (they would lose their position)
    if (!state.skipRefresh) timer = setTimeout(() => render(false), 2500);
  } catch (e) {
    main.replaceChildren(h("p", { class: "alert", role: "alert" }, e.message));
  }
  current = key;
  footer().catch(() => {});
}

async function footer() {
  const o = await api("overview");
  const n = o.node;
  const bad = n.rejected_blocks > 0 || n.last_error;
  $("#foot").replaceChildren(h("span", { class: bad ? "dot bad" : "dot" }), `Nó ${n.address} · altura do nó ${n.height} · verificado até ${o.height}${n.behind > 0 ? ` (${n.behind} atrás)` : ""} · sincronizado ${ago(n.synced_secs_ago)}`, n.last_error ? ` · ${n.last_error}` : "", " · só leitura; cada bloco é reverificado aqui (certificado, proponente, execução)");
}

function showHint(t) {
  hint.hidden = false;
  hint.textContent = t;
}
function hideHint() {
  hint.hidden = true;
  hint.textContent = "";
}

$("#search").addEventListener("submit", async (e) => {
  e.preventDefault();
  const q = $("#q").value.trim();
  if (!q) return;
  try {
    const r = await api(`search?q=${encodeURIComponent(q)}`);
    if (!r.found) return showHint(r.hint ? message(r.hint) : "Nada encontrado para essa busca.");
    hideHint();
    $("#q").value = "";
    location.hash = `#/${r.kind}/${r.id}`;
  } catch (err) {
    showHint(err.message);
  }
});

window.addEventListener("hashchange", () => render(true));
installEffects();
render(true);
