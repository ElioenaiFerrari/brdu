# Roteiro dos marcos

Cada marco só começa com o anterior verde (`cargo fmt`, `clippy -D warnings`, `cargo test --workspace`) e documentado.

| Marco | Entrega | Base de código | Estado |
|---|---|---|---|
| **M1** | `brdu-crypto`, `brdu-wallet`: hash etiquetado, Ed25519 estrito, endereço, codificação, valor, Merkle, BIP-39, SLIP-10; vetores do BRDC | `teko-crypto`, `teko-wallet` (P-256 → Ed25519) | **pronto** |
| **M2** | `brdu-ledger` (a moeda entrou nele, ADR-0011): Tx, estado, bloco atômico, taxas; mint/burn M-de-N, atestado de reserva, cobertura; invariantes por proptest | `teko-ledger`, padrão de `teko-coin` | **pronto** |
| **M3** | `brdu-bft`: motor Tendermint sem I/O e simulador determinístico | `teko-bft` + lições do BRDC (ADR-0029) | **pronto** |
| **M4** | `brdu-p2p`, `brdu-chain`, `brdu-node`: Noise, validador, disco, WAL, API HTTP; cluster de 4 nós nos testes | `teko-p2p`, `teko-economy`, `teko-node` | **pronto** |
| **M5** (núcleo) | `brdu-dev`: devnet, carteira de teste, `send`, `mint`, `burn`, `reserve`, `supply` | `teko-dev` | **pronto** |
| **M5b** | **rotação de validadores** (ADR-0015), simuladores (equivocação, blocos sob demanda, queda de nó), `docs-check` em Rust, menu TUI | `teko-dev`, `devnet-sim`, ADR-0045 do Teko | **pronto** |
| **M6** | Docker: imagem única, `compose`, **teste de carga preciso** (equivocação no M5b) | ADR-0028 e fluxos 22 a 24 do BRDC | **pronto** ([resultados](LOAD-RESULTS.md)) |
| **M7** | Explorador: índice no nó, página, `blocks`/`tx`, certificado conferido | `teko-explorer`, `explorer-server` | planejado |
| **M8** | Carteira: cofre, app Tauri, `brdu-web` para navegador e Docker | `apps/suite` do Teko | planejado |
| M9 | Contratos nativos: bens, credenciais, contratos entre partes; órgãos M-de-N | `teko-assets`, `teko-credentials`, porte do BRDC | planejado |
| M10 | Fechamento: ameaças, matriz de testes, revisão, **comparativo Rust × Elixir** com a mesma escada de carga | — | planejado |
