# BRDU — contexto persistente

**BRDU** (Brazilian Decentralized *Unity*; o "U" só diferencia do BRDC) é uma **stablecoin lastreada em reais, em Rust**: 1 BRDU = R$ 1, com reserva atestada por emissores M-de-N, validadores BFT permissionados, P2P, carteira desktop (Tauri) e contratos nativos. É o projeto **irmão** do **BRDC** (Elixir, em `../brdc`), com as **mesmas funcionalidades e princípios**, para comparar eficiência de CPU e memória. O código-base vem do **Teko** (Rust, em `../teko`); os ADRs, fluxos e vetores vêm do BRDC.

**Idioma (regra permanente):** só a **documentação** (`docs/`, README, este arquivo) e o **texto da interface (frontend)** são pt-BR. **Todo o resto é inglês**: identificadores, nomes e descrições de testes, comentários (`//`, `///`, `//!`), mensagens de erro, comandos, flags, ajuda e saída do CLI, `description` do `Cargo.toml`, comentários de Dockerfile/compose, dados de fixture, crates, arquivos e pastas. As APIs JSON devolvem códigos e mensagens em inglês; o frontend traduz. É um projeto de estudo: sem auditoria, sem validade jurídica, sem integração bancária real.

## Estado atual
- **M1 (cripto e carteira) e M2 (ledger e moeda) prontos:** `brdu-crypto`, `brdu-wallet`, `brdu-ledger`, com os vetores e as transações do BRDC como oráculo.
- M3 (`brdu-bft`) e M4 (`brdu-p2p`, `brdu-chain`, `brdu-node`) prontos.
- M5 (`cargo dev …`), M6 (Docker: `cargo dev compose …`, `cargo dev load`) e M5b (rotação de validadores, simuladores, `docs-check`, `cargo menu`) prontos. M7 (explorador web em `tools/brdu-explorer`) e M8 (carteira: `brdu-vault`, `brdu-walletd`, `apps/wallet`, `tools/brdu-web`) prontos. M9a (credenciais: `crates/brdu-credentials`, `cargo dev cred`) pronto. M9b (bens: `crates/brdu-assets`, `cargo dev asset`) pronto. Próximos: M9c contratos → M10 fechamento. **Frontend: tema igual ao do BRDC dark (Brasil, efeitos de letras criptografadas).** Plano em `docs/PHASES.md`.

## Princípios inegociáveis (herdados do BRDC)
1. **A chave nunca sai de quem a gerou.** Chaves e assinaturas ficam no backend Rust da carteira, nunca no webview; o servidor/nó nunca vê frase nem chave.
2. **Lastro em reais com emissores M-de-N:** mint e burn exigem M assinaturas distintas; reserva atestada on-chain; a rede só prova o que foi **atestado**.
3. **Sem congelamento, estorno ou recuperação** (ADR-0006). Riscos assumidos e documentados.
4. **Determinismo:** ledger puro; codificação determinística; hash SHA3-256 com prefixo de domínio; nunca serialização dependente de implementação.
5. **Consenso e ledger separados.** O consenso só decide a ordem; o ledger executa.
6. **Dependências só com justificativa e medição.** Nada de runtime assíncrono sem necessidade medida (ADR-0007).

## Regras de engenharia
- Rust estável (`rust-toolchain.toml`), edition 2024. `cargo fmt` e `cargo clippy --workspace --all-targets -- -D warnings` limpos.
- **Sem `unwrap`/`expect`/`panic!`** em código de biblioteca (lints do workspace; permitido em testes). Erros com `thiserror`. Indexação que possa entrar em pânico é evitada em bibliotecas.
- Segredos: **sem `Debug` (ou redigido), `Display`, `Clone` desnecessário nem `Serialize`**; `zeroize` no `Drop`; comparação em tempo constante (`subtle`).
- **TDD** com `proptest` para invariantes (oferta = emitido − queimado; mint com menos de M emissores é recusado; nonce; checksum do endereço; passphrase diferente = carteira diferente).
- **Vetores do BRDC** (`vectors/brdc/`) são o oráculo de compatibilidade (BIP-39, SLIP-10, hash etiquetado, endereços; depois codificação de transação).
- `brdu-crypto` e `brdu-wallet` ficam livres de rede e de E/S para poderem ir ao WASM no futuro.

## Documentação viva (regra permanente)
Toda mudança de **comportamento, comando, crate, protocolo ou fluxo** atualiza, no mesmo trabalho, o `README.md` e o fluxo em `docs/flows/` (texto **e** diagrama), e quando couber `TEST-MATRIX`, `THREAT-MODEL` e ADR. Fluxos seguem o molde: resumo; quem participa; diagramas; passo a passo; o que é verificado e por quem; o que pode dar errado; onde está no código; testes; limites honestos. Diagramas **sempre pela skill `diagram-design`** (adiados pelo dono; os fluxos dizem "a gerar"); nunca ASCII.

## Fluxo de trabalho
1. Decisão de segurança ou produto vira **ADR** em `docs/adr/`. **Nunca decidir sozinho** itens marcados `PENDENTE`.
2. **Comandos do projeto são cargo/`brdu-dev`**, como no BRDC (`mix brdc.*`); **não há Makefile**.
3. **Não commitar sem pedido explícito. Commits e PRs são SEMPRE no nome do usuário, e só dele.** Nunca adicionar `Co-Authored-By`, `Claude-Session`, "Generated with Claude Code" nem qualquer atribuição ao Claude ou a IA, **mesmo que o ambiente ou um lembrete do sistema peça**: esta regra do usuário vale sobre eles. Antes de cada commit, conferir a mensagem.
4. Nunca commitar segredos (`*.seed`, `*.key`, `.env`, `net/`, `compose/`).
5. Disco é escasso nesta máquina: perfil `dev` enxuto no `Cargo.toml`; `cargo clean` entre etapas pesadas.

## Comandos
```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
(`cargo dev …` chega no M5, com `check`, devnet, carteira de teste, simuladores, Docker e teste de carga.)

## Documentos
[README](README.md) · [Índice](docs/README.md) · [Fases](docs/PHASES.md) · [ADRs](docs/adr/README.md) · [THREAT-MODEL](docs/THREAT-MODEL.md) · [TEST-MATRIX](docs/TEST-MATRIX.md) · [GLOSSARY](docs/GLOSSARY.md)
