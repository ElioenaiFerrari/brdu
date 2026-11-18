// Translation of the English codes/messages returned by the API into the Portuguese text shown to people.
// Every lookup falls back to the raw value, so an unknown code is still displayed (never lost).

const KINDS = {
  transfer: "transferência",
  mint: "emissão (mint)",
  burn: "resgate (burn)",
  "reserve attestation": "atestado de reserva",
  "validator change": "troca de validador",
  assets: "bens",
  credentials: "credenciais",
  contracts: "contratos",
  unknown: "desconhecido",
};

const ACTIONS = { add: "entrada", remove: "saída" };

const MESSAGES = {
  "block not found": "Bloco não encontrado.",
  "transaction not found": "Transação não encontrada.",
  "invalid address": "Endereço inválido.",
  "read-only": "Somente leitura.",
  "not found": "Não encontrado.",
  busy: "Ocupado, tente de novo.",
  "height does not exist yet": "Essa altura ainda não existe.",
  "address has a typo (checksum mismatch)": "Endereço com erro de digitação (o checksum não confere).",
  "reported by the node; not verifiable here": "informado pelo nó; não verificável aqui",
};

export const kindName = (k) => KINDS[k] ?? k;
export const action = (a) => ACTIONS[a] ?? a;
export const message = (m) => (typeof m === "string" ? MESSAGES[m] ?? m : "");
