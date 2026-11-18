# Fluxo 09 — Simuladores, `docs-check` e menu de desenvolvimento

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
As ferramentas de dev que fecham o ciclo de testar o BRDU: simular falhas (equivocação, blocos sob demanda, queda de nó), conferir a documentação e navegar tudo por um menu com setas. **Só dev**: chaves de teste, nunca na imagem de produção.

## Quem participa
- **`brdu-sim`** e `brdu-dev`: simulam e mostram o resultado.
- **Nós** (locais ou do Docker): reagem como em produção.
- **`docs-check`:** lê a árvore do repositório; **`cargo menu`:** chama os comandos.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev stop 4                    # (ou: cargo dev compose kill 4) libera a chave do validador 4
cargo dev equivocate --as 4         # 2 prevotes conflitantes; mostra a evidência em cada nó honesto
cargo dev start 4                   # religa o nó
cargo dev blocks-sim --count 5      # 5 blocos pequenos
cargo dev docs-check                # links, imagens, crates, ADRs, fluxos, títulos, segredos
cargo dev check                     # fmt + clippy + testes + docs-check
cargo menu                          # menu com setas sobre tudo isso
```
1. **Equivocação:** o simulador abre conexões Noise com a chave do validador parado a **todos** os outros de uma vez e manda os dois votos; o motor de cada nó guarda o primeiro, vê o segundo diferente e emite `Evidence`.
2. **Blocos sob demanda:** transferências mínimas de uma carteira de teste (`sim`), esperando a altura subir.
3. **`docs-check`:** percorre os `.md` (ignora `target`, `net`, `compose`, `.agents`…): links relativos e âncoras, imagens com alt, todo crate no README, todo ADR e fluxo no índice, fluxos com as 9 seções do molde, títulos sem inglês, arquivos de segredo fora das pastas ignoradas.
4. **Menu:** catálogo de comandos; pede os parâmetros e roda o `brdu-dev` por baixo, mostrando a saída.

## O que é verificado e por quem
Os simuladores só **provocam**; quem verifica é a rede (assinaturas, evidência). O `docs-check` verifica a documentação contra o código (crates) e contra o molde.

## O que pode dar errado
- `equivocate` com o nó ainda no ar: o CLI recusa (a chave estaria em uso).
- `docs-check` usa heurísticas (título em inglês por palavras-chave): pode dar falso positivo; ajuste o título.

## Onde está no código
`tools/brdu-sim` (equivocação), `tools/brdu-dev/src/{main,docs_check}.rs`, `tools/brdu-menu`.

## Testes que cobrem
`tools/brdu-sim/tests/equivocation.rs` (4 nós em processo: **evidência em 3 de 3**, nomeando o equivocador e os dois blocos); `docs_check.rs` (6 testes com árvores temporárias: árvore correta, link quebrado, âncora, imagem sem alt, código em linha não vira link, crate/ADR/fluxo fora do índice, seção faltando, título em inglês, segredo); verificado com processos reais.

## Limites honestos
Evidência sem penalidade (ADR-0016); o menu não substitui os comandos (é um atalho); `docs-check` não confere se o **conteúdo** dos fluxos está certo.
