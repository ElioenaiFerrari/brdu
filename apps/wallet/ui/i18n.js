// Portuguese text shown for the stable error codes of the wallet service (the service speaks English codes).
const ERRORS = {
  locked: "A carteira está bloqueada.",
  no_vault: "Ainda não existe uma carteira aqui.",
  vault_exists: "Já existe uma carteira aqui.",
  wrong_password: "Senha incorreta.",
  too_many_attempts: "Muitas tentativas erradas. Aguarde um pouco e tente de novo.",
  wrong_passphrase: "A passphrase não confere com esta carteira.",
  weak_password: "A senha precisa ter pelo menos 8 caracteres.",
  invalid_phrase: "Frase de recuperação inválida. Confira as palavras e a ordem.",
  address_mismatch: "O endereço não é o que foi confirmado.",
  invalid_address: "Endereço inválido: confira se há erro de digitação.",
  invalid_amount: "Valor inválido.",
  insufficient_funds: "Saldo insuficiente: o valor mais a taxa precisam caber no saldo.",
  unknown_prepared_send: "Esta prévia expirou ou já foi usada. Revise o envio de novo.",
  node_unreachable: "Não consegui falar com a rede. Tente de novo.",
  rejected: "A rede recusou a operação.",
  no_explorer: "Histórico indisponível: nenhum explorador configurado.",
  bad_request: "Pedido inválido.",
  unknown_method: "Operação desconhecida.",
  internal: "Erro interno.",
};

const STATUS = { pending: "pendente", active: "ativa", contested: "contestada", vetoed: "vetada", revoked: "revogada" };

/** Portuguese label of a credential status (unknown values are shown as they are). */
export const statusText = (s) => STATUS[s] ?? s;

/** Text for an error code; unknown codes fall back to the raw message so nothing is hidden. */
export const errorText = (code, fallback = "") => ERRORS[code] ?? (fallback || "Erro inesperado.");
