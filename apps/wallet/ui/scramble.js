// PURE core of the "decrypting" effect (letters that scramble and resolve, in the style of Proton's Lumo).
// No DOM and no clock: randomness comes in as a function (`rand`), so everything is testable. The DOM lives in `effects.js`.
const UP = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOW = "abcdefghijklmnopqrstuvwxyz";
const DIG = "0123456789";

const pick = (set, rand) => set[Math.min(Math.floor(rand() * set.length), set.length - 1)];

/** A scrambled character of the same "class": digit becomes digit, letter becomes letter (upper/lower case), the rest stays. */
export function glyphFor(ch, rand = Math.random) {
  if (/\d/.test(ch)) return pick(DIG, rand);
  if (/\p{Lu}/u.test(ch)) return pick(UP, rand);
  if (/\p{L}/u.test(ch)) return pick(LOW, rand);
  return ch;
}

/**
 * Reveal frame: `progress` from 0 to 1. The already resolved part (`done`) grows from left to right; the
 * rest (`rest`) is scrambled glyphs of the same length. Spaces and punctuation never move.
 */
export function scrambleFrame(text, progress, rand = Math.random) {
  const chars = Array.from(text);
  const k = Math.min(chars.length, Math.max(0, Math.floor(progress * chars.length)));
  return {
    done: chars.slice(0, k).join(""),
    rest: chars.slice(k).map((c) => glyphFor(c, rand)).join(""),
  };
}

/** "Loading" frame: the text stays readable and a fraction of the characters flickers as glyphs. */
export function loadingFrame(text, rand = Math.random, density = 0.3) {
  return Array.from(text)
    .map((c) => (glyphFor(c, () => 0) !== c || /\p{L}|\d/u.test(c)) && rand() < density ? glyphFor(c, rand) : c)
    .join("");
}

/** Reveal duration (ms): about 35 ms per character, between 350 and 900. */
export const durationFor = (text) => Math.min(900, Math.max(350, Array.from(text).length * 35));

/** Eases the progress (starts fast and slows down as it resolves). */
export const ease = (p) => 1 - (1 - Math.min(1, Math.max(0, p))) ** 2;
