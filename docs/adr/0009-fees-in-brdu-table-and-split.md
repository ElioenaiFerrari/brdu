# ADR-0009 — Taxas em BRDU: tabela, repartição e ausência de halving

**Status:** Aceito (dono do produto, 2026-11-18) · **Origem:** BRDC ADR-0008, ADR-0014 e ADR-0027

## Contexto
Toda rede precisa de defesa contra spam e de incentivo a validar. Como 1 BRDU = R$ 1, as taxas precisam ser minúsculas, mas não zero. Não há token nativo volátil.

## Decisão
| Operação | Taxa mínima | Em reais |
|---|---|---|
| Transferência | 0,001 BRDU + 0,000001 BRDU por byte da transação codificada | cerca de R$ 0,0012 |
| Mint, burn e atestado de reserva | 0 (já exigem M emissores) | — |
| Bens, credenciais, contratos (M9) | definida pelo módulo: 0,01 a 0,05 BRDU | R$ 0,01 a R$ 0,05 |

- **Taxa em BRDU**, cobrada do remetente na própria transação (`fee`); a taxa paga pode ser maior que a mínima.
- **Repartição por bloco:** a soma das taxas é dividida **igualmente entre os validadores do cabeçalho** (`signers` = o conjunto em vigor, ordenado); **o resto da divisão vai ao proponente**. Quem assinou de fato o certificado fica no certificado (BRDC ADR-0027); repartir só entre quem assinou é evolução para quando houver stake.
- **Sem queima de taxa:** preserva a invariante `Σ saldos = emitido − queimado` (testada por propriedade).
- **Sem halving e sem teto fixo:** não há emissão automática. A oferta só muda por mint e burn dos emissores, acompanhando a reserva; um teto fixo quebraria o lastro.
- Parâmetros (`transfer_base`, `per_byte`, limites de bloco) ficam nas **regras da gênese** (`Rules`).

## Consequências e limites honestos
- A recompensa dos validadores vem **só das taxas**; com pouco uso rende pouco. Na fase institucional os órgãos arcam com os nós.
- Os valores são estimativas de estudo; só carga real mostra se estão altos ou baixos (M6).
- Num estado "pendente" do mempool a taxa simplesmente sai da conta; só o estado de um bloco a reparte (teste cobre a diferença).
