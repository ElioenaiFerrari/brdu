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
  "contract creation": "criação de contrato",
  "contract action": "ação em contrato",
  "asset class": "classe de bens",
  "asset registration": "registro de bem",
  "asset offer": "oferta de bem",
  "asset accept": "aceite de oferta",
  "asset cancel": "cancelamento de oferta",
  "asset veto": "veto de negociação",
  "asset finalization": "finalização de negociação",
  "asset lien": "gravame",
  "asset lien removal": "remoção de gravame",
  "credential type": "tipo de credencial",
  "credential issue": "emissão de credencial",
  "credential contest": "contestação de credencial",
  "credential veto": "veto de credencial",
  "credential activation": "ativação de credencial",
  "credential revocation": "revogação de credencial",
  contracts: "contratos",
  unknown: "desconhecido",
};

const STATUS = { pending: "pendente", active: "ativa", contested: "contestada", vetoed: "vetada", revoked: "revogada" };

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

export const status = (s) => STATUS[s] ?? s;
export const kindName = (k) => KINDS[k] ?? k;
export const action = (a) => ACTIONS[a] ?? a;
export const message = (m) => (typeof m === "string" ? MESSAGES[m] ?? m : "");
