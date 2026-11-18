# ADR-0010 — Governança de emissores e validadores

**Status:** Aceito para as redes de teste (2026-11-18); governança real **PENDENTE** (P1) · **Origem:** BRDC ADR-0013

## Decisão
1. **Dois conjuntos separados:** **validadores** (ordenam blocos) e **emissores** (mint, burn, atestado de reserva). Quem opera o consenso não mexe nas reservas, e o contrário também vale.
2. **Quem entra são órgãos** (pessoas jurídicas), definidos na gênese; pessoas físicas atuam como **signatárias dentro de um órgão** (M-de-N).
3. **Mudar o conjunto** (nomear e remover) vale com **mais de 2/3 do conjunto em vigor do mesmo papel**; no máximo uma troca pendente; só em altura futura. **Validadores:** implementado no M5b ([ADR-0015](0015-validator-rotation.md)). **Emissores:** planejado (hoje fixos na gênese).
4. **Nas redes de teste:** emissores de teste, **M de N configurável** (padrão 2 de 3 nos testes; 2 de 2 no laboratório), chaves de teste em texto puro, **só para dados fictícios**. As regras (`Rules`) guardam o conjunto de emissores e o limiar M; o limiar tem de estar em `1..=N`.
5. **NPoS e stake não entram agora.** Direção futura: stake **em BRDU** (vale 1 real), com a regra de entrada reaberta por ADR.

## Consequências e limites honestos
- Quem nomeia já está dentro: o grupo inicial tende a se perpetuar. Mais de 2/3 em conluio controlam o conjunto; mais de 1/3 podem parar a rede.
- **Quais órgãos entram na gênese real é decisão de implantação, não de código** (PENDENTE, P1).
- Nesta fase o conjunto de emissores é **fixo** na gênese; a troca de emissores por mais de 2/3 é planejada; a de validadores já existe (ADR-0015).
