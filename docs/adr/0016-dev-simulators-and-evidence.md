# ADR-0016 — Simuladores só de dev e evidência sem penalidade

**Status:** Aceito (2026-11-18) · **Origem:** BRDC ADR-0028 e `tools/devnet-sim` do Teko

## Decisão
- Os simuladores de falha vivem em `tools/brdu-sim` (e nos subcomandos `equivocate`, `rotate`, `blocks-sim` do `brdu-dev`), **fora** da imagem Docker e de qualquer build de produção. Usam **chaves de teste** em texto puro.
- **Equivocação:** com a chave de um validador de teste **parado**, o simulador abre conexões Noise autenticadas como ele e envia dois prevotes conflitantes (mesma altura e rodada). Os nós honestos geram `Evidence`, exposta em `GET /evidence`, e o CLI mostra quantos detectaram.
- **Sem penalidade:** não existe stake para cortar; a evidência é pública e verificável (dois votos assinados), e a decisão de remover o validador é a rotação do ADR-0015.
- A chave do equivocador precisa ser de um nó parado (não disputar a conexão com o nó real): o CLI recusa se o nó ainda responde.

## Consequências
Os testes automáticos cobrem 3 de 3 detecções em processo e o roteiro manual cobre o Docker. Honestidade: o simulador só prova que os nós **detectam**; o que fazer depois (remover) é governança.
