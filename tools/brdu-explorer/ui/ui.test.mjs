// Tests for the pure part of the UI (no DOM): `node --test tools/brdu-explorer/ui`
import assert from "node:assert/strict";
import test from "node:test";
import { splitAmount, shortHash, shortAddr, ago } from "./format.js";
import { scrambleFrame, glyphFor, durationFor, ease } from "./scramble.js";

test("amount in minimum units becomes an integer with thousands separators and 6 decimals", () => {
  assert.deepEqual(splitAmount(0), { int: "0", dec: "000000" });
  assert.deepEqual(splitAmount(1_234_560_000), { int: "1.234", dec: "560000" });
  assert.deepEqual(splitAmount("999999"), { int: "0", dec: "999999" });
  // 64 bits without losing precision (no floating point)
  assert.deepEqual(splitAmount("18446744073709551615"), { int: "18.446.744.073.709", dec: "551615" });
});

test("shortened hashes and addresses", () => {
  assert.equal(shortHash("a".repeat(64), 8), "aaaaaaaa…aaaa");
  assert.equal(shortHash(null), "—");
  assert.equal(shortAddr("brdu1" + "q".repeat(58), 10), "brdu1qqqqq…qqqqq");
});

test("relative time", () => {
  assert.equal(ago(null), "nunca");
  assert.equal(ago(0), "agora");
  assert.equal(ago(5), "há 5 s");
  assert.equal(ago(120), "há 2 min");
  assert.equal(ago(7200), "há 2 h");
});

test("letter effect: same character class, fixed spaces and punctuation, and the real text arrives at the end", () => {
  const rand = () => 0.5;
  assert.match(glyphFor("7", rand), /\d/);
  assert.match(glyphFor("a", rand), /[a-z]/);
  assert.match(glyphFor("A", rand), /[A-Z]/);
  assert.equal(glyphFor("-", rand), "-");
  const f = scrambleFrame("Block 12", 0.5, rand);
  assert.equal(f.done, "Bloc"); // half progress: 4 of 8 characters already resolved, from the left
  assert.match(f.rest, /^[a-z]{2}\s\d{2}$|^.{4}$/u);
  assert.equal(Array.from(f.done + f.rest).length, 8);
  assert.equal(scrambleFrame("Block 12", 1, rand).done, "Block 12");
  assert.equal(scrambleFrame("Block 12", 0, rand).done, "");
  assert.ok(durationFor("x") >= 350 && durationFor("x".repeat(500)) <= 900);
  assert.ok(ease(0) === 0 && ease(1) === 1 && ease(0.5) > 0.5);
});
