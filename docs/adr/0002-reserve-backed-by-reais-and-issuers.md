# ADR-0002 — Lastro em reais e emissores autorizados

**Status:** Aceito (dono do produto, 2026-11-18) · **Origem:** BRDC ADR-0002

## Contexto
Uma stablecoin precisa de um mecanismo que mantenha 1 BRDU = R$ 1. Opções: lastro em reais guardados por emissores (USDC, USDT); colateral em cripto (DAI); algorítmica.

## Decisão
**Lastro em reais, com emissores autorizados.** Um conjunto de emissores, em decisão **M-de-N**, **cria (mint)** BRDU quando recebe reais e **destrói (burn)** quando devolve. As reservas são **atestadas na cadeia** (total em reais, com número de sequência), e a rede mostra a **cobertura** (reserva menos o que circula).

## Alternativas descartadas
Colateral em cripto (oráculos de preço e liquidações, muito mais complexo e arriscado); algorítmica (historicamente instável).

## Consequências
- Mint e burn são transações privilegiadas, válidas só com M assinaturas distintas de emissores sobre um documento amarrado à rede, conta, valor e referência (cada referência de depósito vale uma vez).
- A cadeia **não prova** que os reais existem: prova só o que foi **atestado**. É o mesmo risco de confiança do USDC/USDT; a mitigação é transparência (atestados públicos, auditoria externa fora da cadeia).
- A cobertura é **só informação**: a rede não bloqueia nada (ADR-0006).
- Quem são os emissores iniciais e como o conjunto muda: nas redes de teste, 2 de 2 com chaves de teste (como no BRDC, ADR-0013); governança real é **PENDENTE**.
