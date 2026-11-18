// "Decrypting" effect: the text of [data-decrypt] elements scrambles and resolves from left to right, in green.
// Respects "reduced motion" and leaves the real text for screen readers (.sr).
import { scrambleFrame, durationFor, ease } from "./scramble.js";

const reduced = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
const state = new WeakMap();
let observer = null;

const quiet = (fn) => {
  fn();
  observer?.takeRecords();
};
const span = (cls) => {
  const s = document.createElement("span");
  if (cls) s.className = cls;
  return s;
};

export function decrypt(el, text = realText(el)) {
  const prev = state.get(el);
  if (prev?.raf) cancelAnimationFrame(prev.raf);
  const st = { text, raf: 0 };
  state.set(el, st);
  if (reduced() || text.trim() === "") {
    quiet(() => (el.textContent = text));
    return;
  }
  const done = span();
  const rest = span("scr");
  const real = span("sr");
  done.setAttribute("aria-hidden", "true");
  rest.setAttribute("aria-hidden", "true");
  real.textContent = text;
  quiet(() => el.replaceChildren(done, rest, real));
  const total = durationFor(text);
  const start = performance.now();
  let last = -1000;
  const frame = (now) => {
    const p = (now - start) / total;
    if (p >= 1) {
      quiet(() => (el.textContent = text));
      st.raf = 0;
      return;
    }
    if (now - last >= 45) {
      last = now;
      const f = scrambleFrame(text, ease(p));
      quiet(() => {
        done.textContent = f.done;
        rest.textContent = f.rest;
      });
    }
    st.raf = requestAnimationFrame(frame);
  };
  frame(start);
}

function realText(el) {
  const real = el.querySelector(":scope > .sr");
  return real ? real.textContent : el.textContent;
}

function reveal(el) {
  const text = realText(el);
  if (state.get(el)?.text === text) return;
  decrypt(el, text);
}

function scan(root) {
  if (root.matches?.("[data-decrypt]")) reveal(root);
  root.querySelectorAll?.("[data-decrypt]").forEach(reveal);
}

export function installEffects() {
  if (observer) return;
  observer = new MutationObserver((records) => {
    const changed = new Set();
    for (const r of records) {
      const node = r.target.nodeType === 1 ? r.target : r.target.parentElement;
      const el = node?.closest?.("[data-decrypt]");
      if (el) changed.add(el);
      for (const n of r.addedNodes) if (n.nodeType === 1) scan(n);
    }
    changed.forEach(reveal);
  });
  observer.observe(document.body, { subtree: true, childList: true, characterData: true });
  scan(document.body);
}
