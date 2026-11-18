// Smoke test: runs the real app.js against a tiny fake DOM and canned API replies, on EVERY page.
// It exists to catch what a syntax check cannot: undefined names, wrong imports, typos in rendering code.
import assert from "node:assert/strict";
import test from "node:test";

class FakeNode {}
class FakeEl extends FakeNode {
  constructor(tag) {
    super();
    this.tag = tag;
    this.attrs = {};
    this.kids = [];
    this.listeners = {};
    this.hidden = false;
    this._text = "";
    this.dataset = {};
    this.classList = { add() {}, remove() {} };
  }
  setAttribute(k, v) { this.attrs[k] = v; }
  removeAttribute(k) { delete this.attrs[k]; }
  append(...k) { this.kids.push(...k); }
  replaceChildren(...k) { this.kids = k; }
  addEventListener(t, f) { (this.listeners[t] ??= []).push(f); }
  focus() {}
  querySelectorAll() { return []; }
  matches() { return false; }
  set textContent(v) { this._text = String(v); this.kids = []; }
  get textContent() { return this._text + this.kids.map((k) => (k instanceof FakeEl ? k.textContent : String(k.data ?? ""))).join(""); }
  hasClass(c) { return (this.attrs.class ?? "").split(" ").includes(c) || this.kids.some((k) => k instanceof FakeEl && k.hasClass(c)); }
}

const ADDR = "brdu1" + "q".repeat(58);
const HASH = "ab".repeat(32);
const tx = (kind, kind_name, summary) => ({ id: HASH, height: 2, index: 0, kind, kind_name, sender: ADDR, nonce: 1, fee: 1172, size: 200, summary });
const block = (height, full = false) => ({ height, time: 1700000000, hash: HASH, prev_hash: height > 1 ? HASH : null, state_root: HASH, proposer: ADDR, round: 0, tx_count: 1, signed_by: [ADDR, ADDR, ADDR], validators: 4, verified: true, ...(full ? { txs: [tx(1, "transfer", { to: ADDR, amount: 1500000 })] } : {}) });
const overview = {
  chain_id: "t", genesis_hash: HASH, height: 2, epoch: 1, state_root: HASH, tx_total: 3, accounts: 2,
  supply: { minted: 5000000, burned: 0, supply: 5000000, reserve: 6000000, attestation_seq: 1, coverage: { state: "covered", surplus: 1000000 } },
  validators: [ADDR, ADDR], pending_change: { action: "add", validator: ADDR, effective_height: 9 },
  recent_blocks: [block(2), block(1)],
  node: { address: "127.0.0.1:1", height: 2, behind: 0, last_error: null, rejected_blocks: 0, synced_secs_ago: 1 },
};
const replies = [
  [/^overview/, overview],
  [/^blocks/, { height: 2, blocks: [block(2), block(1)] }],
  [/^block\//, block(2, true)],
  [/^tx\/.*-k4$/, tx(4, "reserve attestation", { seq: 1, reserve: 6000000, reference: "stmt" })],
  [/^tx\/.*-k2$/, tx(2, "mint", { account: ADDR, amount: 5000000, reference: "dep-1" })],
  [/^tx\/.*-k22$/, tx(22, "validator change", { action: "add", validator: ADDR, effective_height: 9 })],
  [/^tx\//, tx(1, "transfer", { to: ADDR, amount: 1500000 })],
  [/^account\//, { address: ADDR, balance: 1234560000, nonce: 3, is_validator: true, credentials: [{ id: "cnh-1", type: "cnh", status: "pending", valid: false, privacy: "public", data_hex: "6361743d42" }], tx_count: 1, txs: [tx(1, "transfer", { to: ADDR, amount: 1500000 })] }],
  [/^validators/, { epoch: 1, validators: [ADDR], pending_change: { action: "remove", validator: ADDR, effective_height: 9 }, evidence: [{ voter: ADDR, height: 2, round: 0 }], evidence_note: "reported by the node; not verifiable here" }],
  [/^search/, { found: false, hint: "address has a typo (checksum mismatch)" }],
];

const els = {};
const windowListeners = {};
globalThis.Node = FakeNode;
globalThis.window = { addEventListener: (t, f) => (windowListeners[t] ??= []).push(f), matchMedia: () => ({ matches: true }) };
globalThis.location = { hash: "#/" };
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = () => {};
globalThis.MutationObserver = class { observe() {} takeRecords() { return []; } };
globalThis.document = {
  body: new FakeEl("body"),
  querySelector: (s) => (els[s] ??= new FakeEl(s)),
  querySelectorAll: () => [],
  createElement: (t) => new FakeEl(t),
  createTextNode: (d) => ({ data: d }),
};
let lastPath = "";
globalThis.fetch = async (url) => {
  const path = String(url).replace(/^\/api\//, "");
  lastPath = path;
  const hit = replies.find(([re]) => re.test(path));
  return { ok: true, status: 200, json: async () => hit?.[1] ?? {} };
};

const settle = () => new Promise((r) => setTimeout(r, 30));

test("every page renders without errors, with the Portuguese labels and translated codes", async () => {
  await import("./app.js");
  await settle();
  const main = els["#main"];
  const goto = async (hash) => {
    globalThis.location.hash = hash;
    for (const f of windowListeners.hashchange ?? []) f();
    await settle();
    assert.ok(!main.hasClass("alert"), `${hash} rendered an error: ${main.textContent}`);
    return main.textContent;
  };
  assert.ok((await goto("#/")).includes("Oferta em circulação"));
  assert.ok((await goto("#/blocks")).includes("Blocos"));
  assert.ok((await goto("#/block/2")).includes("Certificado de commit"));
  const t1 = await goto("#/tx/" + HASH);
  assert.ok(t1.includes("transferência"), t1);
  assert.ok((await goto("#/tx/" + HASH + "-k2")).includes("emissão (mint)"));
  assert.ok((await goto("#/tx/" + HASH + "-k4")).includes("atestado de reserva"));
  const t22 = await goto("#/tx/" + HASH + "-k22");
  assert.ok(t22.includes("troca de validador") && t22.includes("entrada"), t22);
  const acc = await goto("#/account/" + ADDR);
  assert.ok(acc.includes("Saldo") && acc.includes("Credenciais") && acc.includes("cnh-1") && acc.includes("pendente"), acc);
  const v = await goto("#/validators");
  assert.ok(v.includes("saída") && v.includes("informado pelo nó"), v);
  assert.ok((await goto("#/nowhere")).includes("não encontrada"));
});

test("a failing search shows the translated hint", async () => {
  const hint = els["#hint"];
  const input = (els["#q"] ??= new FakeEl("#q"));
  input.value = "brdu1typo";
  const form = els["#search"];
  for (const f of form.listeners.submit ?? []) await f({ preventDefault() {} });
  await settle();
  assert.equal(hint.hidden, false);
  assert.ok(hint.textContent.includes("erro de digitação"), hint.textContent);
  assert.ok(lastPath.startsWith("search?q="));
});
