// Pure money helpers (no DOM): parsing what a person types, formatting for display, and the on-screen keypad.
// Amounts are BigInt in base units (1 BRDU = 1,000,000): `Number` would lose precision above 2^53.
const UNIT = 1_000_000n;
const MAX = (1n << 64n) - 1n;
const MAX_INT_DIGITS = 13; // fits in 2^64 - 1 together with 6 decimals
const MAX_DECIMALS = 6;

/**
 * "12,5" -> 12500000n; null if invalid. Brazilian format: decimal comma, and the period ONLY as a well-placed
 * thousands separator ("1.234,56"). "1.5" is REJECTED instead of silently becoming 15.
 */
export function parseAmount(text) {
  if (typeof text !== "string") return null;
  const parts = text.trim().split(",");
  if (parts.length > 2) return null;
  const [whole, frac = ""] = parts;
  if (!/^(\d+|\d{1,3}(\.\d{3})+)$/.test(whole)) return null;
  if (frac !== "" && !/^\d{1,6}$/.test(frac)) return null;
  const units = BigInt(whole.replaceAll(".", "")) * UNIT + BigInt(frac.padEnd(6, "0"));
  return units <= MAX ? units : null;
}

const thousands = (n) => n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ".");

/** Integer part with thousands separators, and the six decimals separately (the UI dims the decimals). */
export function splitAmount(units) {
  const u = BigInt(units ?? 0);
  return { int: thousands(u / UNIT), dec: (u % UNIT).toString().padStart(6, "0") };
}

/** 12480570000n -> "12.480,570000". */
export function formatFull(units) {
  const { int, dec } = splitAmount(units);
  return `${int},${dec}`;
}

/**
 * The keypad: applies one key ("0"-"9", ",", "." or "back") to the amount text typed so far.
 * - "." is treated as the decimal comma (never as a thousands separator); a second comma is ignored;
 * - at most 13 integer digits and 6 decimals; no leading zeros ("007" -> "7"); "," first gives "0,".
 * The result is always empty or accepted by `parseAmount` (after trimming a trailing comma).
 */
export function pressKey(text, key) {
  let t = text.replaceAll(".", "");
  if (key === "back") return t.slice(0, -1) === "" ? "" : t.slice(0, -1);
  if (key === "," || key === ".") {
    if (t.includes(",")) return t;
    return t === "" ? "0," : `${t},`;
  }
  if (!/^\d$/.test(key)) return t;
  const [int, dec] = t.split(",");
  if (dec !== undefined) return dec.length >= MAX_DECIMALS ? t : `${t}${key}`;
  const next = (int + key).replace(/^0+(?=\d)/, "");
  return next.length > MAX_INT_DIGITS ? t : next;
}

/** Groups the integer part with dots for display while typing: "1234,5" -> "1.234,5". */
export function display(text) {
  if (text === "") return "0";
  const [int, dec] = text.split(",");
  const grouped = int.replace(/\B(?=(\d{3})+(?!\d))/g, ".");
  return dec === undefined ? grouped : `${grouped},${dec}`;
}

/** What `parseAmount` should receive for the typed text (a trailing comma is dropped). */
export function typedToAmount(text) {
  const clean = text.endsWith(",") ? text.slice(0, -1) : text;
  return clean === "" ? null : parseAmount(clean.replace(/\B(?=(\d{3})+(?!\d))/g, "."));
}
