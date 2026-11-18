# ADR-0011 — A moeda dentro do `brdu-ledger`, e atomicidade por diário de desfazer

**Status:** Aceito (escolha do plano, 2026-11-18) · **Origem:** novo

## Contexto
O plano previa uma crate `brdu-coin` separada, como o Teko tem `teko-coin`. Mas a moeda (tipos 1 a 4: transferência, mint, burn, atestado) é o **caminho quente** (toda transação mexe em contas e taxa) e o estado dela é **tipado** (contas, oferta, referências, atestado). O Teko guardava tudo em chave-valor com Merkle; isso custa codificar e decodificar a cada acesso.

## Decisão
- **A moeda vive em `brdu-ledger`**, com estado tipado (`BTreeMap` de contas, contadores, conjunto de referências). **Bens, credenciais e contratos (tipos 5 a 21)** entram como **módulos** (`trait Module`) de outras crates (M9), com armazenamento chave-valor próprio (`ext`) dentro do mesmo estado e da mesma raiz.
- **Atomicidade por diário de desfazer** (`State::atomically`, `State::speculate`): cada mudança registra o valor antigo; se algo falha, volta-se tudo, **sem copiar o estado inteiro**. Um bloco vale por inteiro ou nada; montar um bloco calcula a raiz com `speculate` e desfaz.
- **Raiz de estado:** Merkle sobre contas, referências, escalares e `ext`, **em ordem** (não depende da ordem de montagem); conta com saldo e nonce zero não existe (forma canônica). A raiz **não inclui** o hash do último bloco, para que a raiz do estado depois do bloco seja exatamente o `state_root` do cabeçalho.

## Alternativas
- `brdu-coin` separada: obriga o estado de contas a ser genérico (chave-valor) e mais lento.
- Copiar o estado a cada bloco (como o `pending_state` do BRDC): O(contas) por bloco; o diário custa O(mudanças).

## Consequências
Menos alocação e menos CPU por transação (a medir no M10). O custo: o módulo precisa registrar mudanças pelo `State` (nunca por fora), e a raiz ainda é recalculada por inteiro a cada bloco (O(contas)); uma árvore incremental fica para quando a medição pedir.
