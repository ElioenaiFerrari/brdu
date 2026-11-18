# ADR-0001 — Objetivo e escopo do BRDU

**Status:** Aceito (dono do produto, 2026-11-18) · **Origem:** BRDC ADR-0001

## Contexto
O BRDC (Elixir) é uma stablecoin lastreada em reais com contratos nativos. Os testes de carga dele mostraram que o BEAM usa ~135 MiB por validador em repouso e satura a ~800 tx/s a 0,5 CPU. O dono quer **testar se Rust é mais eficiente em CPU e memória**, com as mesmas funcionalidades.

## Decisão
- O **BRDU** é uma **stablecoin em reais** (1 BRDU = R$ 1), **inteiramente em Rust**, com as **mesmas funcionalidades e princípios do BRDC**: lastro em reais com emissores M-de-N, reserva atestada, validadores BFT permissionados, P2P, carteira, explorador, contratos nativos (bens, credenciais, contratos entre partes).
- A base de código é o **Teko** (Rust), adaptada (P-256 → Ed25519, domínio de votos → moeda); ADRs, fluxos e vetores vêm do BRDC.
- **Fora do escopo por ora:** votos e eleições (Teko), integração bancária real (Pix/custódia), validade jurídica, hardware wallet, validadores abertos e a carteira no navegador sem daemon (WASM).
- O resultado final inclui um **comparativo Rust × Elixir** com a mesma escada de carga.

## Consequências
Duas implementações do mesmo produto, comparáveis. A documentação é parte da entrega de cada marco.

## Limites honestos
Protótipo: sem auditoria, sem validade jurídica, sem integração bancária. O comparativo só vale com a mesma carga e os mesmos limites de recurso.
