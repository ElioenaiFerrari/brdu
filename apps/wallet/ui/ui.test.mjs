// Pure-logic tests for the wallet UI: `node --test apps/wallet/ui/ui.test.mjs`
import assert from "node:assert/strict";
import test from "node:test";
import { parseAmount, splitAmount, formatFull, pressKey, display, typedToAmount } from "./money.js";
import { errorText } from "./i18n.js";

test("amounts are parsed in the Brazilian format and never as floating point", () => {
  assert.equal(parseAmount("12,5"), 12_500_000n);
  assert.equal(parseAmount("1.234,56"), 1_234_560_000n);
  assert.equal(parseAmount("0,000001"), 1n);
  assert.equal(parseAmount("18.446.744.073.709,551615"), 18_446_744_073_709_551_615n);
  for (const bad of ["1.5", "1.23", "1,2,3", "-1", "1,0000001", "", " ", "abc", "18.446.744.073.710,000000", "1..000", "1.0000"]) {
    assert.equal(parseAmount(bad), null, JSON.stringify(bad));
  }
  assert.equal(parseAmount(undefined), null);
});

test("amounts split into integer and six decimals, and format in full", () => {
  assert.deepEqual(splitAmount(0), { int: "0", dec: "000000" });
  assert.deepEqual(splitAmount(1_234_560_000n), { int: "1.234", dec: "560000" });
  assert.equal(formatFull("999999"), "0,999999");
  assert.deepEqual(splitAmount(null), { int: "0", dec: "000000" });
});

test("the keypad builds valid amounts and refuses what would be invalid", () => {
  const type = (keys) => [...keys].reduce((t, k) => pressKey(t, k), "");
  assert.equal(type("1234"), "1234");
  assert.equal(display(type("1234")), "1.234");
  assert.equal(type("12,5"), "12,5");
  assert.equal(type("12.5"), "12,5", "a typed period is the decimal comma, never thousands");
  assert.equal(type("007"), "7", "no leading zeros");
  assert.equal(type(",5"), "0,5");
  assert.equal(type("1,2,3"), "1,23", "a second comma is ignored");
  assert.equal(type("0,1234567"), "0,123456", "at most 6 decimals");
  assert.equal(type("12345678901234"), "1234567890123", "at most 13 integer digits");
  assert.equal(pressKey("12,5", "back"), "12,");
  assert.equal(pressKey("1", "back"), "");
  assert.equal(pressKey("", "back"), "");
  assert.equal(pressKey("12", "x"), "12", "unknown keys are ignored");
  assert.equal(display(""), "0");
  assert.equal(display("1234567,5"), "1.234.567,5");
});

test("whatever the keypad produces is accepted by the parser (or is empty / a trailing comma)", () => {
  const alphabet = ["0", "1", "2", "5", "9", ",", "back", "."];
  let seed = 7;
  const next = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff);
  for (let i = 0; i < 300; i++) {
    let t = "";
    for (let j = 0; j < 1 + (next() % 25); j++) t = pressKey(t, alphabet[next() % alphabet.length]);
    const amount = typedToAmount(t);
    if (t === "" || t === "0,") assert.ok(amount === null || amount === 0n);
    else assert.notEqual(amount, null, `typed ${JSON.stringify(t)} must be a valid amount`);
  }
  assert.equal(typedToAmount("12,"), 12_000_000n);
  assert.equal(typedToAmount("1234,5"), 1_234_500_000n);
});

test("error codes are shown in Portuguese, unknown ones fall back to the raw text", () => {
  assert.equal(errorText("wrong_password"), "Senha incorreta.");
  assert.equal(errorText("insufficient_funds").startsWith("Saldo insuficiente"), true);
  assert.equal(errorText("brand_new_code", "details"), "details");
  assert.equal(errorText("brand_new_code"), "Erro inesperado.");
});
