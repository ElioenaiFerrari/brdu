# ADR-0004 — Modelo de conta, endereço e casas decimais

**Status:** Aceito (dono do produto, 2026-11-18) · **Origem:** BRDC ADR-0010

## Decisão
- **Modelo de conta** (saldo e nonce por endereço), não UTXO: mais simples para contratos, bens e credenciais.
- **Endereço** `brdu1…` em **bech32m**: `bech32m("brdu", SHA3-256(tag "brdu/addr", chave pública))`. O checksum detecta erro de digitação; o endereço carrega o **hash** da chave, então não revela a chave até a primeira assinatura.
- **6 casas decimais**: 1 BRDU = 1.000.000 unidades; valores sempre inteiros de 64 bits, **nunca ponto flutuante**.
- Entrada de valor no formato brasileiro: vírgula decimal e ponto **só** como milhar (`1.000,50`); `1.5` é recusado por ambíguo.

## Alternativas
UTXO (privacidade e paralelismo, mas estado de contratos pior); 2 casas (sem folga para taxas pequenas); 18 casas (excesso para uma moeda em reais).

## Consequências
Mudar depois custa caro (estado, vetores, carteira). Taxas pequenas aparecem sem perder precisão (`0,001172`).
