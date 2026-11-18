# ADR-0006 — Sem congelamento de contas: BRDU sem censura

**Status:** Aceito (dono do produto, 2026-11-18), com riscos assumidos · **Origem:** BRDC ADR-0009

## Contexto
O USDC e o USDT permitem que o emissor **congele** contas e bloqueie endereços. O dono do produto decidiu que o BRDU (como o BRDC) **não** terá esse controle.

## Decisão
**Não existe operação de congelamento, bloqueio, estorno ou recuperação.** Os emissores só podem **mint, burn e atestar reservas**; ninguém move fundos de uma conta que não assinou.

## Riscos assumidos (honestos)
- **Fundos roubados ou enviados por engano não podem ser recuperados** por ninguém.
- Uma stablecoin lastreada em reais sem controle de conformidade **pode ser inviável para emissores regulados** e atrair uso ilícito.
- A decisão pode ser **revista por novo ADR**; o código **não prepara** esse controle.

## Consequências
Simplicidade e previsibilidade para o usuário; maior risco para os emissores. Documentado para ninguém descobrir isso tarde.
