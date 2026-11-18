# BRDU

**Stablecoin brasileira lastreada em reais, em Rust.** 1 BRDU = R$ 1. O irmão em Rust do BRDC (Elixir), com as mesmas funcionalidades e princípios, usando o Teko (Rust) como base de código. O "U" é de *unity*, só para diferenciar os projetos.

> Projeto de estudo. **Sem auditoria, sem validade jurídica, sem integração bancária real.** As chaves das redes de teste ficam em texto puro em pastas ignoradas pelo git.

## Ideia
- **Lastro em reais:** emissores autorizados (decisão M-de-N) criam BRDU (*mint*) quando recebem reais e destroem (*burn*) quando devolvem; a reserva é **atestada** na cadeia e a rede mostra a cobertura. A cadeia prova só o que foi atestado, não que os reais existem.
- **Sem token volátil e sem congelamento:** taxas em BRDU, repartidas entre validadores; ninguém congela, estorna ou recupera contas.
- **BFT permissionado:** validadores autorizados, finalidade imediata (mais de 2/3), sem blocos vazios.
- **Carteira não custodial:** frase BIP-39 de 24 palavras com passphrase como 25ª palavra; as chaves ficam no backend Rust (Tauri), nunca no webview.
- **Contratos nativos** (bens, credenciais, contratos entre partes), sem máquina virtual.

## Estado atual
| Marco | O quê | Estado |
|---|---|---|
| M1 | `brdu-crypto` e `brdu-wallet` (hash, Ed25519, endereço, codificação, valor, Merkle, BIP-39, SLIP-10) | **pronto** |
| M2 | `brdu-ledger`: transação, estado com raiz de Merkle, bloco atômico, taxas, mint/burn M-de-N, atestado de reserva, cobertura | **pronto** |
| M3 | `brdu-bft`: motor BFT sem E/S, certificado de commit, simulador determinístico | **pronto** |
| M4 | `brdu-p2p`, `brdu-chain`, `brdu-node` e o binário `brdu-node`: validador com Noise, disco, diário e API HTTP | **pronto** |
| M5 | `brdu-dev`: `cargo dev init/up/down/status/wallet/send/fund/mint/burn/reserve/supply/blocks/tx` | **pronto** |
| M5b | rotação de validadores (tipo 22, ADR-0015), simuladores (`equivocate`, `rotate`, `blocks-sim`, `stop/start`), `cargo dev docs-check` e `check`, menu `cargo menu` | **pronto** |
| M6 | Docker: imagem única, `cargo dev compose`, `cargo dev load` ([resultados](docs/LOAD-RESULTS.md): ~2.000 tx/s a 0,5 CPU, 2,5 MiB em repouso) | **pronto** |
| M7 | explorador web (tema do BRDC) que **reverifica cada bloco**: `cargo dev explorer` ou o serviço do Docker em `http://127.0.0.1:8545` | **pronto** |
| M8 | carteira: cofre criptografado (Argon2id + ChaCha20-Poly1305), serviço, interface no tema do BRDC, app desktop Tauri v2 e daemon web (`cargo dev wallet-web`) | **pronto** |
| M9a | credenciais nativas (tipos 14 a 19): `brdu-credentials`, `cargo dev cred`, explorador e carteira | **pronto** |
| M9b | bens nativos (tipos 5 a 13): `brdu-assets`, `cargo dev asset`, explorador e carteira | **pronto** |
| M9c a M10 | contratos declarativos, fechamento | planejado ([fases](docs/PHASES.md)) |

## Mapa de crates
| Crate | Papel | Marco |
|---|---|---|
| `brdu-crypto` | SHA3-256 com prefixo de domínio, Ed25519 estrito (e em lote), endereços `brdu1…` (bech32m), codificação determinística, valor com 6 casas, Merkle. `no_std` + `alloc`. | M1 |
| `brdu-wallet` | frase BIP-39 (12 ou 24 palavras), passphrase, SLIP-10 Ed25519, contas. | M1 |
| `brdu-ledger` | transação assinada (formato do BRDC), estado com raiz de Merkle e desfazer atômico, bloco inteiro ou nada, taxas, a moeda (transferência, mint, burn, atestado) e a governança do conjunto de validadores (`gov`, tipo 22). Puro, sem E/S. | M2, M5b |
| `brdu-bft` | motor BFT sem E/S (estilo Tendermint), votos, propostas, certificado de commit, evidência de equivocação, restauração do diário. | M3 |
| `brdu-p2p` | Noise XX ligado à chave de consenso Ed25519, sessão cifrada com rekey, limites de conexão e mensagem, registros de endereço assinados. | M4 |
| `brdu-chain` | gênese, bloco final (bloco + certificado), réplica verificada, seleção de transações, log só de acrescentar. | M4 |
| `brdu-node` | runtime do validador: consenso, mempool O(1), fofoca sem eco, diário, sincronização, rede e API HTTP. | M4 |
| `tools/brdu-nodebin` | o binário `brdu-node` (`keygen`, `genesis`, `run`). | M4 |
| `docker/` | `Dockerfile` único (validador e ferramentas) usado pelo laboratório. | M6 |
| `tools/brdu-dev` | CLI de desenvolvimento: devnet local, carteira de teste, envio, mint, burn, reserva, consultas (`cargo dev`). | M5 |
| `tools/brdu-sim` | simulador só de dev: equivocação de um validador de teste (a rede expõe a evidência). | M5b |
| `tools/brdu-explorer` | explorador: réplica verificada + página web (tema BRDC) + API JSON somente leitura. | M7 |
| `brdu-assets` | bens: classes com órgão M-de-N, registro, negociação com escrow e janela de veto, gravames (tipos 5 a 13). | M9b |
| `brdu-credentials` | credenciais: tipos com órgão M-de-N, janela de contestação, validade, veto, revogação (tipos 14 a 19). | M9a |
| `brdu-vault` | cofre da frase de recuperação: Argon2id + ChaCha20-Poly1305, cabeçalho autenticado, passphrase nunca guardada. | M8 |
| `brdu-walletd` | serviço da carteira (criar, importar, desbloquear, saldo, prévia e envio, histórico); a chave nunca sai do processo. | M8 |
| `tools/brdu-web` | daemon web local da carteira (interface + `/rpc`), só loopback, Host e Origin conferidos; chaves de TESTE. | M8 |
| `brdu-wallet-app` | app desktop Tauri v2 (`apps/wallet/src-tauri`), mesma interface (`apps/wallet/ui`); fora do workspace, com lockfile próprio. | M8 |
| `tools/brdu-menu` | menu de desenvolvimento com setas (ratatui) sobre o `brdu-dev`; só dev. | M5b |

## Início rápido
```
cargo test --workspace      # 230 testes (Rust + JS): RFC 8032, vetores e transações do BRDC, consenso, propriedades
cargo dev init && cargo dev up   # 4 validadores locais (+ cargo dev explorer, cargo dev wallet-web) (depois: cargo dev wallet new alice, cargo dev fund alice 1000…)
cargo clippy --workspace --all-targets -- -D warnings
```
Requisitos: Rust 1.100 (`rust-toolchain.toml`). Para a carteira desktop (M8): `cargo install tauri-cli --version "^2"`.

## Documentação
[Índice](docs/README.md) · [Fluxos](docs/README.md#fluxos) · [ADRs](docs/adr/README.md) · [Ameaças](docs/THREAT-MODEL.md) · [Testes](docs/TEST-MATRIX.md) · [Glossário](docs/GLOSSARY.md)

Projetos irmãos: [BRDC](../brdc) (Elixir) e [Teko](../teko) (identidade cívica e votação, Rust).
