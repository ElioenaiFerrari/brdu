# Moeda lastreada, emissores M-de-N e reserva

## Resumo
1 BRDU = R$ 1. Emissores **criam** BRDU (*mint*) ao receber reais e **destroem** (*burn*) ao devolver. A decisão exige **M de N** assinaturas de emissores independentes, sobre um documento que prende conta, valor e referência.

```rust
use brdu_ledger::approvals;
// o documento é assinado por cada emissor; o ledger exige "mais que M" distintos
approvals::require(&doc, &lista, &chaves_dos_emissores, limite)?;
```

- **Atestado de reserva:** os emissores publicam na cadeia o quanto há em reais; a rede mostra a **cobertura** (reserva ÷ circulação). É uma declaração assinada, não uma prova bancária.
- **Sem congelamento:** nenhuma função congela, estorna ou recupera contas.
- **Valor com 6 casas** (inteiros em unidades mínimas), sem ponto flutuante.

## O que NÃO garante
Que os reais existem. A rede prova **quem declarou**; o lastro depende da honestidade de M emissores e de auditoria externa.

## Onde está no código
`crates/brdu-ledger/src/{coin,approvals}.rs`; fluxo [`03-coin-issuance.md`](../flows/03-coin-issuance.md).
