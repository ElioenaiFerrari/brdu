// Smoke test: runs the REAL app.js against a tiny fake DOM and a scripted wallet service, through every flow.
// It catches what a syntax check cannot: undefined names, wrong imports, broken wiring between screens.
import assert from "node:assert/strict";
import test from "node:test";

class FakeNode {}
class El extends FakeNode {
  constructor(tag) {
    super();
    this.tag = tag;
    this.attrs = {};
    this.kids = [];
    this.parent = null;
    this.listeners = {};
    this.dataset = {};
    this.hidden = false;
    this.value = "";
    this.checked = false;
    this.disabled = false;
    this.className = "";
    this.classList = { add() {}, remove() {} };
  }
  setAttribute(k, v) {
    this.attrs[k] = v;
    if (k === "value") this.value = v;
    if (k === "id") this.id = v;
    if (k === "disabled") this.disabled = true;
    if (k === "hidden") this.hidden = true;
    if (k === "class") this.className = v;
    if (k.startsWith("data-")) this.dataset[k.slice(5).replace(/-(\w)/g, (_, c) => c.toUpperCase())] = v;
  }
  removeAttribute(k) { delete this.attrs[k]; }
  get name() { return this.attrs.name; }
  _adopt(k) { if (k instanceof El) k.parent = this; }
  // like the real DOM, plain strings become text nodes
  _wrap(k) { return k.map((x) => (typeof x === "string" ? { data: x } : x)); }
  append(...k) { k = this._wrap(k); k.forEach((x) => this._adopt(x)); this.kids.push(...k); }
  replaceChildren(...k) { k = this._wrap(k); k.forEach((x) => this._adopt(x)); this.kids = k; }
  addEventListener(t, f) { (this.listeners[t] ??= []).push(f); }
  focus() {}
  set textContent(v) { this._text = String(v); this.kids = []; }
  get textContent() { return (this._text ?? "") + this.kids.map((k) => (k instanceof El ? k.textContent : String(k.data ?? ""))).join(""); }
  all() { return this.kids.flatMap((k) => (k instanceof El ? [k, ...k.all()] : [])); }
  find(pred) { return this.all().find(pred); }
  byId(id) { return this.find((e) => e.attrs.id === id); }
  byTag(tag, pred = () => true) { return this.find((e) => e.tag === tag && pred(e)); }
  byText(text, tag = "button") { return this.find((e) => e.tag === tag && e.textContent.includes(text)); }
  get elements() { return Object.fromEntries(this.all().filter((e) => e.attrs.name).map((e) => [e.attrs.name, e])); }
  closest(sel) {
    for (let n = this; n; n = n.parent) {
      if (sel.startsWith("[data-")) {
        const key = sel.slice(6, -1).replace(/-(\w)/g, (_, c) => c.toUpperCase());
        if (n.dataset?.[key] !== undefined) return n;
      } else if (sel.split(",").includes(n.tag)) return n;
    }
    return null;
  }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  matches() { return false; }
}

const els = {};
const windowListeners = {};
const docListeners = {};
globalThis.Node = FakeNode;
globalThis.window = { addEventListener: (t, f) => (windowListeners[t] ??= []).push(f), matchMedia: () => ({ matches: true }) };
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = () => {};
globalThis.MutationObserver = class { observe() {} takeRecords() { return []; } };
Object.defineProperty(globalThis, "navigator", { value: { clipboard: { writeText: async () => {} } }, configurable: true });
globalThis.document = {
  body: new El("body"),
  querySelector: (s) => (els[s] ??= new El(s)),
  querySelectorAll: () => [],
  createElement: (t) => new El(t),
  createTextNode: (d) => ({ data: d }),
  addEventListener: (t, f) => (docListeners[t] ??= []).push(f),
};

// ---- the scripted wallet service behind /rpc
const ADDR = "brdu1" + "q".repeat(58);
const WORDS = Array.from({ length: 24 }, (_, i) => `word${i + 1}`);
let calls = [];
let script = {};
let failNext = null;
globalThis.fetch = async (url, opts) => {
  assert.equal(url, "/rpc");
  const { method, params } = JSON.parse(opts.body);
  calls.push({ method, params });
  if (failNext) {
    const e = failNext;
    failNext = null;
    return { ok: false, status: 400, json: async () => ({ error: e }) };
  }
  const handler = script[method];
  const result = typeof handler === "function" ? handler(params) : handler ?? {};
  return { ok: true, status: 200, json: async () => ({ result }) };
};
const reset = (s) => ((calls = []), (script = s));
const settle = () => new Promise((r) => setTimeout(r, 25));
const main = () => els["#main"];
const toast = () => els["#toast"].textContent;
const click = async (el) => {
  assert.ok(el, "element to click was not found");
  // bubble like a browser: the element, then every ancestor up to <main>
  for (let n = el; n; n = n.parent) for (const f of n.listeners.click ?? []) await f({ target: el, preventDefault() {} });
  await settle();
};
const submit = async (form) => {
  for (const f of form.listeners.submit ?? []) await f({ preventDefault() {} });
  await settle();
};
const bootWith = async (s) => {
  reset(s);
  for (const f of windowListeners.hashchange ?? []) f();
  // re-run boot through the lock button path (it calls boot again)
  for (const f of els["#lock"].listeners.click ?? []) await f({});
  await settle();
};

const LOCKED = { status: { has_vault: true, unlocked: false, address: ADDR }, lock: { locked: true } };

test("fresh install: welcome, create wallet, show the 24 words once, continue to the home", async () => {
  reset({ status: { has_vault: false, unlocked: false, address: null } });
  await import("./app.js");
  await settle();
  assert.ok(main().textContent.includes("Sua carteira BRDU"));

  await click(main().byId("go-create"));
  const form = main().byId("create");
  const [pw, pw2] = [form.elements.password, form.elements.password2];
  pw.value = "correct horse battery";
  pw2.value = "different password";
  await submit(form);
  assert.ok(toast().includes("As senhas não são iguais"), toast());
  assert.equal(calls.filter((c) => c.method === "create").length, 0, "mismatching passwords never reach the service");

  script.create = { words: WORDS, address: ADDR };
  pw2.value = "correct horse battery";
  form.elements.passphrase.value = "";
  await submit(form);
  const created = calls.find((c) => c.method === "create");
  assert.deepEqual(created.params, { password: "correct horse battery", passphrase: "" });
  assert.equal(pw.value, "", "the password field is wiped after use");
  assert.equal(main().all().filter((e) => e.tag === "li").length, 24);
  assert.ok(main().textContent.includes("word24"));

  const next = main().byText("Continuar");
  assert.equal(next.disabled, true, "cannot continue before ticking that the words were written down");
  const box = main().byId("wrote");
  box.checked = true;
  for (const f of box.listeners.change ?? []) f();
  assert.equal(next.disabled, false);
  script.overview = { address: ADDR, balance: 1_234_560_000, nonce: 0, chain_id: "t", height: 3, supply: { supply: 5_000_000, coverage: { state: "covered" } } };
  await click(next);
  const home = main().textContent;
  assert.ok(home.includes("Saldo") && home.includes("1.234") && home.includes("560000"), home);
  assert.ok(home.includes("reserva cobre a oferta"), home);
});

test("sending: keypad, review with fee and total, confirm, and a clear error mapping", async () => {
  await bootWith({ ...LOCKED, status: { has_vault: true, unlocked: true, address: ADDR }, overview: { address: ADDR, balance: 50_000_000, nonce: 0, chain_id: "t", height: 3, supply: { supply: 1, coverage: { state: "none" } } } });
  assert.ok(main().textContent.includes("Saldo"));
  await click(main().byText("Enviar"));
  assert.ok(main().textContent.includes("Para"));
  const to = main().byId("to");
  to.value = ADDR;
  for (const f of to.listeners.input ?? []) f();
  const review = main().byId("review");
  assert.equal(review.disabled, true, "no amount yet");
  for (const k of ["1", "2", ","]) await click(main().find((e) => e.dataset.key === k));
  await click(main().find((e) => e.dataset.key === "5"));
  assert.equal(main().byId("typed").textContent, "12,5BRDU");
  assert.equal(review.disabled, false);
  await click(main().find((e) => e.dataset.key === "back"));
  assert.equal(main().byId("typed").textContent, "12,BRDU");
  await click(main().find((e) => e.dataset.key === "5"));

  // review → the service answers with a preview
  script.prepare_send = { id: "abc123", to: ADDR, amount: 12_500_000, fee: 1172, total: 12_501_172, balance_after: 37_498_828, to_self: false };
  await click(review);
  const sent = calls.find((c) => c.method === "prepare_send");
  assert.deepEqual(sent.params, { to: ADDR, amount: "12,5" });
  const rv = main().textContent;
  assert.ok(rv.includes("Revise o envio") && rv.includes("Taxa") && rv.includes("Total") && rv.includes("Saldo depois"), rv);
  assert.ok(rv.includes("12") && rv.includes("501172"), rv);
  assert.equal(calls.filter((c) => c.method === "confirm_send").length, 0, "nothing is signed before the explicit confirmation");

  // going back keeps what was typed
  await click(main().byId("edit"));
  assert.equal(main().byId("typed").textContent, "12,5BRDU");
  await click(main().byId("review"));

  script.confirm_send = { tx_id: "ab".repeat(32), amount: 12_500_000, fee: 1172, to: ADDR };
  await click(main().byId("confirm"));
  assert.deepEqual(calls.find((c) => c.method === "confirm_send").params, { id: "abc123" });
  assert.ok(main().textContent.includes("Enviado"));
  await click(main().byId("home"));
  assert.ok(main().textContent.includes("Saldo"));
});

test("service errors are shown in Portuguese, and an auto-lock sends the user to the unlock screen", async () => {
  await bootWith({ status: { has_vault: true, unlocked: true, address: ADDR }, overview: { address: ADDR, balance: 1, nonce: 0, chain_id: "t", height: 1, supply: { supply: 1, coverage: { state: "none" } } } });
  await click(main().byText("Enviar"));
  const to = main().byId("to");
  to.value = "brdu1typo";
  for (const f of to.listeners.input ?? []) f();
  await click(main().find((e) => e.dataset.key === "1"));
  failNext = { code: "invalid_address", message: "checksum" };
  await click(main().byId("review"));
  assert.ok(toast().includes("erro de digitação"), toast());
  failNext = { code: "insufficient_funds", message: "x" };
  await click(main().byId("review"));
  assert.ok(toast().includes("Saldo insuficiente"), toast());
  // the wallet locked itself in the meantime
  failNext = { code: "locked", message: "locked" };
  await click(main().byId("review"));
  assert.ok(main().textContent.includes("Carteira bloqueada"), main().textContent);
  assert.ok(toast().includes("bloqueada"));
});

test("unlocking wipes the password field and reports a wrong password", async () => {
  await bootWith(LOCKED);
  assert.ok(main().textContent.includes("Carteira bloqueada"));
  const form = main().byId("unlock");
  form.elements.password.value = "wrong password!";
  failNext = { code: "wrong_password", message: "x" };
  await submit(form);
  assert.ok(toast().includes("Senha incorreta"), toast());
  assert.equal(form.elements.password.value, "", "the field is wiped even when the password was wrong");
  script.unlock = { address: ADDR };
  script.status = { has_vault: true, unlocked: true, address: ADDR };
  script.overview = { address: ADDR, balance: 7, nonce: 0, chain_id: "t", height: 1, supply: { supply: 1, coverage: { state: "under" } } };
  form.elements.password.value = "correct horse battery";
  await submit(form);
  assert.ok(main().textContent.includes("reserva ABAIXO da oferta"), main().textContent);
});

test("importing: the derived address must be confirmed before a password is even asked", async () => {
  await bootWith({ status: { has_vault: false, unlocked: false, address: null } });
  await click(main().byId("go-import"));
  const form = main().byId("import");
  script.preview_import = { address: ADDR, word_count: 24 };
  form.elements.words.value = WORDS.join(" ");
  await submit(form);
  assert.ok(main().textContent.includes("Confira o endereço") && main().textContent.includes("24 palavras"));
  const pwForm = main().byId("confirm-import");
  assert.equal(pwForm.hidden, true, "the password form stays hidden until the address is confirmed");
  await click(main().byId("yes"));
  assert.equal(pwForm.hidden, false);
  pwForm.elements.password.value = "correct horse battery";
  pwForm.elements.password2.value = "correct horse battery";
  script.import = { address: ADDR };
  script.status = { has_vault: true, unlocked: true, address: ADDR };
  script.overview = { address: ADDR, balance: 0, nonce: 0, chain_id: "t", height: 1, supply: { supply: 0, coverage: { state: "none" } } };
  await submit(pwForm);
  const imp = calls.find((c) => c.method === "import");
  assert.equal(imp.params.expected_address, ADDR, "the confirmed address travels with the import");
  assert.equal(pwForm.elements.password.value, "");
});

test("receive, history and settings screens render", async () => {
  await bootWith({
    status: { has_vault: true, unlocked: true, address: ADDR },
    overview: { address: ADDR, balance: 0, nonce: 0, chain_id: "t", height: 1, supply: { supply: 0, coverage: { state: "none" } } },
    history: { address: ADDR, txs: [{ kind: 1, height: 3, id: "ab".repeat(32), sender: "brdu1" + "p".repeat(58), summary: { to: ADDR, amount: 2_500_000 } }, { kind: 2, height: 2, id: "cd".repeat(32), sender: ADDR, summary: { account: ADDR, amount: 9_000_000 } }] },
    export_phrase: { words: WORDS },
    change_password: { changed: true },
  });
  await click(main().byText("Receber"));
  assert.ok(main().textContent.includes("Receber") && main().textContent.includes("brdu1"));
  await click(main().byId("back"));
  await click(main().byText("Histórico"));
  const hist = main().textContent;
  assert.ok(hist.includes("Recebido de") && hist.includes("Depósito (emissão)") && hist.includes("+2,50"), hist);
  await click(main().byId("back"));
  await click(main().byText("Ajustes"));
  const form = main().byId("show-phrase");
  form.elements.password.value = "correct horse battery";
  await submit(form);
  assert.ok(main().textContent.includes("word1") && main().textContent.includes("Não deixe ninguém ver"));
  assert.equal(form.elements.password.value, "");
  const pw = main().byId("change-pw");
  pw.elements.old.value = "correct horse battery";
  pw.elements.new.value = "another password 1";
  await submit(pw);
  assert.ok(toast().includes("Senha trocada"));
});

test("when the wallet service is unreachable the app says so and can retry", async () => {
  reset({});
  globalThis.fetch = async () => {
    throw new Error("network down");
  };
  for (const f of els["#lock"].listeners.click ?? []) await f({});
  await settle();
  assert.ok(main().textContent.includes("Sem conexão com a carteira"), main().textContent);
});
