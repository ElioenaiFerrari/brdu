# ADR-0021 — Contratos declarativos entre partes (sem VM)

**Status:** Aceito (2026-11-18) · **Origem:** BRDC ADR-0004 e ADR-0017

## Contexto
Acordos entre duas partes com dinheiro (compra e venda, aluguel, empréstimo) precisam rodar sem juiz e sem código de terceiros. Uma máquina virtual (WASM, EVM) traria determinismo, medição de custo e sandbox: um projeto enorme e uma classe própria de bugs.

## Decisão
1. **Sem VM.** Os contratos são **modelos declarativos** (dados): estados, transições, quem age, condições de tempo e efeitos sobre dinheiro, executados por um interpretador com **vocabulário fechado** (`brdu-contracts`).
2. **Vocabulário:** atores `A`, `B`, qualquer um; condições `antes de`, `a partir de`, `variável < termo`, `variável = termo`; efeitos `travar` (conta → custódia), `liberar` (custódia → conta, no máximo o saldo da custódia), `pagar` (direto), `definir` e `incrementar` variável; expressões com termos, variáveis, constante, hora do bloco, custódia, soma, subtração sem negativo, produto e mínimo. **Aritmética em 128 bits com recusa de estouro de 64 bits.**
3. **Tipos 20 (criar) e 21 (agir)**; taxa mínima de cada passo `contract_fee` (0,01 BRDU). Quem cria é a parte A; a parte B é outra conta. Cada transição é **tudo ou nada**.
4. **Três modelos:** `compra-venda` (comprador deposita, confirma ou reembolso por prazo), `aluguel` (caução, parcelas, acerto que desconta as parcelas não pagas da caução) e `empréstimo` (principal travado na criação, entregue no aceite, devolução do valor combinado, **sem juros automáticos**).
5. **Custódia e conservação:** a custódia de cada contrato entra na oferta (`saldos + custódias = emitido − queimado`) e contratos em estado final têm custódia zero (teste por propriedade).
6. **Compatibilidade:** os payloads de criação e ação reproduzem **byte a byte** os do BRDC (vetores `contract_txs`).

## Consequências e limites honestos
- **Sem arbitragem:** em disputa, a rede só executa as transições que a parte autorizada assinou; quem está certo se decide fora da rede.
- Sem juros, correção, multas automáticas nem oráculos. Modelo novo exige código revisado e atualização da rede.
- Contrato aqui **não substitui** o contrato jurídico.
- Qualquer um pode acionar `refund_expired`, `settle` e `mark_default` depois do prazo (as condições são de tempo, não de quem pede).
