# Fluxo 06 — Rede de teste local e o CLI `cargo dev`

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como subir 4 validadores na sua máquina, criar carteiras de teste, emitir moeda com os emissores de teste, enviar, atestar a reserva e olhar os blocos, tudo por `cargo dev …` (alias em `.cargo/config.toml`). **Só dev**: chaves e frases de teste em texto puro.

## Quem participa
- **`brdu-dev`**: gera a rede, sobe/derruba processos, assina e envia pelas APIs.
- **4 processos `brdu-node`** (validadores) em `127.0.0.1:7101..` (pares) e `:7201..` (API).
- **Emissores de teste** (3, M = 2) e um **retransmissor** (paga o nonce de mint/burn/atestado; quem envia não autoriza).

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev init                      # chaves, gênese e devnet.json em net/
cargo dev up                        # sobe os validadores (logs em net/logs)
cargo dev status                    # altura, pares, mempool, raiz de cada nó
cargo dev wallet new alice          # frase de TESTE em net/wallets; mostra o endereço brdu1…
cargo dev fund alice 1000           # mint assinado pelos emissores de teste
cargo dev send --from alice --to bob --amount 12,5 --count 3
cargo dev reserve --amount 1000     # atestado de reserva (M emissores)
cargo dev burn --from alice --amount 10 --ref saque-1
cargo dev supply                    # emitido, queimado, oferta, reserva, cobertura
cargo dev blocks                    # últimos blocos, rodada e quantos assinaram
cargo dev down                      # derruba
```
1. `init` cria as chaves (0600), a gênese e `devnet.json`; os nomes e portas ficam ali (o laboratório Docker usa o mesmo formato, com nomes de serviço).
2. `up` sobe cada nó discando aos anteriores (malha completa) com `--min-block-ms 200`.
3. Os comandos que assinam leem a conta na API (nonce), calculam a **taxa mínima** (`/fees` × tamanho), assinam e fazem `POST /tx`.
4. `fund`/`mint`/`burn`/`reserve` juntam as aprovações de M emissores sobre o documento exato.

## O que é verificado e por quem
Quem verifica é a rede, não o CLI: assinatura, taxa, nonce, M emissores distintos, referência única (fluxos 02, 03 e 05). O CLI só mostra o que a API devolve.

## O que pode dar errado
- Porta em uso: `up` falha no nó; veja `net/logs/vN.log`.
- `reset` apaga **só** o que o `init` criou na pasta indicada (`keys`, `wallets`, `data`, `logs`, `devnet.json`, `genesis.hex`, `pids`).
- As chaves e frases em texto puro **nunca** devem ser usadas fora de redes de teste.

## Onde está no código
`tools/brdu-dev/src/{main,net,http}.rs`, `tools/brdu-nodebin`, `.cargo/config.toml`.

## Testes que cobrem
Verificado ponta a ponta com 4 processos reais (init, up, fund, send ×3, reserve, burn, supply, blocks): mesma raiz nos 4 nós. A lógica de cada comando está coberta pelos testes do ledger e do cluster (M2 e M4). **Planejado:** testes automatizados do próprio CLI.

## Limites honestos
Sem menu TUI, sem `docs-check` em Rust e sem os simuladores (equivocação, rotação, blocos sob demanda): ficam para depois do Docker (M5b). A rotação de validadores ainda não existe no nó.
