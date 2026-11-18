# ADR-0012 — Motor BFT sem E/S, com as lições do BRDC

**Status:** Aceito (2026-11-18) · **Origem:** BRDC ADR-0003, 0026, 0029; `teko-bft`

## Decisão
- **Estilo Tendermint, poder igual**, validadores permissionados (ADR-0010). Quórum = **mais de 2/3** (`count*3 > 2n`); proponente da rodada = `validators[(altura + rodada) % n]`.
- **Sem E/S:** `Engine::handle(Event) -> Vec<Action>`. Nada de soquete, relógio ou acaso dentro do motor; quem o roda entrega mensagens e dispara prazos. Isso permite o **simulador determinístico** dos testes (ordem de entrega sorteada por semente).
- **Votos e propostas** assinados sobre hash etiquetado (`brdu/vote`, `brdu/proposal`) com rede, altura, rodada, tipo e hash: nada vale em outro contexto. Voto `None` = nulo.
- **Certificado de commit** = precommits de mais de 2/3, verificável por quem não participou (nó que sincroniza, carteira, explorador), e diz **quem assinou** (entra no cabeçalho para repartir a taxa, ADR-0009).
- **Evidência de equivocação** (dois votos do mesmo validador, mesma rodada e tipo, blocos diferentes) sai como `Action::Evidence`.
- **Lições do BRDC embutidas:** o prazo de `propose` também é agendado para o **proponente** (se o bloco nunca vier, a rodada segue); `known_messages` permite **reenviar a quem (re)conecta**; `restore` retoma do **diário** das mensagens que este validador assinou, sem contradizê-lo (quem roda grava o diário **antes** de transmitir).
- Rodada pula quando mais de 1/3 dos validadores já está numa à frente.

## Limites honestos
Sem auditoria formal; a prova de segurança é a do Tendermint, o que existe aqui são testes (simulador, propriedades). Poder igual (sem stake) por ora. Troca do conjunto de validadores (rotação) chega com o nó (M4).
