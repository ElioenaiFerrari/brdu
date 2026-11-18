// Pure formatting (no DOM): amounts in minimum units (1 BRDU = 1,000,000), shortened hashes and addresses.
export const UNIT = 1_000_000n;

/** `units` (number or string) -> { int: "1.234", dec: "560000" } (pt-BR style, 6 decimals). */
export function splitAmount(units) {
  const v = BigInt(units ?? 0);
  const int = (v / UNIT).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ".");
  const dec = (v % UNIT).toString().padStart(6, "0");
  return { int, dec };
}

export const shortHash = (h, n = 8) => (h ? `${h.slice(0, n)}…${h.slice(-4)}` : "—");
export const shortAddr = (a, n = 10) => (a ? `${a.slice(0, n)}…${a.slice(-5)}` : "—");

/** "há 5 s", "há 3 min"... from seconds (the output is UI text, in Portuguese). */
export function ago(secs) {
  if (secs == null) return "nunca";
  if (secs < 2) return "agora";
  if (secs < 60) return `há ${secs} s`;
  if (secs < 3600) return `há ${Math.floor(secs / 60)} min`;
  return `há ${Math.floor(secs / 3600)} h`;
}

/** Local date/time of a Unix instant in seconds. */
export const when = (unix) => new Date(unix * 1000).toLocaleString("pt-BR");
