// The transport to the wallet service. In the desktop app (Tauri) calls go through `invoke` and never touch the
// network; in the browser they go to the local daemon's `/rpc`. Either way the UI only ever sees public data.
export class WalletError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

export async function rpc(method, params = {}) {
  const tauri = globalThis.__TAURI__?.core?.invoke;
  if (tauri) {
    try {
      return await tauri("rpc", { method, params });
    } catch (e) {
      const err = typeof e === "object" && e ? e : { code: "internal", message: String(e) };
      throw new WalletError(err.code ?? "internal", err.message ?? "");
    }
  }
  const res = await fetch("/rpc", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ method, params }) });
  const body = await res.json().catch(() => ({}));
  if (!res.ok || body.error) throw new WalletError(body.error?.code ?? "internal", body.error?.message ?? `HTTP ${res.status}`);
  return body.result;
}
