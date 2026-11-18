# ADR-0020 — Bens nativos: classes com órgão, negociação com escrow e janela de veto, gravames

**Status:** Aceito (2026-11-18) · **Origem:** BRDC ADR-0018

## Contexto
A rede representa **bens** (imóveis, veículos…) e a **transferência de propriedade** com pagamento em BRDU, sem juiz na rede. Quem garante que o bem existe e quem é o dono é um **órgão** (cartório, Detran…), não a rede.

## Decisão
1. **Classe** (tipo 5): criada pelas autoridades raiz (M-de-N da gênese). Fixa o **órgão** (1 a 16 chaves distintas, limite M) e a **janela de veto** (entre `min_window` e `max_window`).
2. **Registro** (6): o órgão aprova (M-de-N) um `asset_id` único, o dono inicial e o metadado (até 256 bytes, normalmente o hash do título).
3. **Negociação direta**, uma transação por etapa:
   - **oferta** (7): só o dono, sem gravame e sem negociação em curso; comprador e preço;
   - **aceite** (8): só o comprador; o preço sai do saldo dele para o **escrow** e a janela começa (`prazo = hora do bloco + janela da classe`); saldo insuficiente falha a transação inteira;
   - **veto** (10): órgão M-de-N, só **antes** do prazo; o escrow volta ao comprador;
   - **finalização** (11): qualquer um, só **a partir** do prazo; a propriedade passa ao comprador e o escrow ao vendedor;
   - **cancelamento** (9): só o dono, só antes do aceite.
4. **Gravames** (12 e 13): o órgão adiciona e remove (M-de-N). Bem com gravame não pode ser ofertado e uma negociação em janela **não finaliza** enquanto houver gravame (o dinheiro continua no escrow).
5. **Aprovações não se repetem:** cada bem tem uma **revisão** que sobe a cada ação do órgão e a cada troca de dono, e entra no documento aprovado. O veto amarra ainda comprador, preço e prazo.
6. **Taxas:** classe e registro `registration_fee` (0,05 BRDU); oferta, aceite, cancelamento e finalização `step_fee` (0,02); veto e gravames sem mínimo.
7. **Conservação:** o escrow é parte da oferta (`saldos + escrow = emitido − queimado`) e tem teste por propriedade; o escrow é sempre exatamente o preço de uma negociação aceita.
8. **Compatibilidade:** documentos e payloads têm o **mesmo formato do BRDC**; só o prefixo (`brdu/asset`) muda. Com o prefixo do BRDC os vetores `assets` e `asset_ops` são reproduzidos **byte a byte**.

## Consequências e limites honestos
- A rede **não sabe** se o bem existe nem quem é o dono de verdade: confia no órgão. Órgão comprometido (M chaves) registra bens falsos ou veta à vontade.
- **Sem arbitragem:** disputas se resolvem fora da rede; o órgão só tem veto e gravames.
- O comprador fica com o dinheiro **preso** durante a janela.
- O veto é só do órgão (nem vendedor nem comprador depois do aceite).
- Sem leilão, sem várias ofertas, sem condições além do veto.
- Órgão e autoridades de **teste** têm chaves em texto puro (`keys/organ*`, `keys/authority*`).
