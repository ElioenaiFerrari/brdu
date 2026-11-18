# ADR-0014 — Runtime do nó: fofoca, mempool O(1), diário e blocos sem vazio

**Status:** Aceito (2026-11-18) · **Origem:** lições dos testes de carga do BRDC (ADR-0026, 0029) e `teko-node`

## Decisão
- **Uma thread de núcleo** é dona do consenso, do mempool e da cadeia; rede, API e aceitação conversam com ela por canais. O motor BFT segue sem E/S.
- **Mempool:** dedupe **O(1)** (mapa de ids; no BRDC refazer o hash de todas as pendentes custava 0,5 → 15 ms com 7.500 pendentes), limite em transações **e em bytes**, nonce contíguo por remetente; até 16 nonces à frente ficam **em espera** e são promovidos quando a anterior chega (envio rápido a nós diferentes chega fora de ordem).
- **Assinaturas conferidas fora do núcleo**, na thread de quem recebeu (API ou par), em paralelo; no bloco, em lote.
- **Fofoca sem eco:** uma transação nova vai a todos os pares **menos a quem a mandou**; ids vistos ficam num conjunto limitado (262 144).
- **Ao (re)conectar:** o nó envia seu estado, **reenvia o mempool** e o que o consenso sabe das últimas rodadas (BRDC ADR-0029).
- **Diário antes de transmitir** (BRDC ADR-0026): proposta ou voto próprio é gravado com `fsync` **antes** de sair; sem diário não se assina. Ao reiniciar, `Engine::restore` retoma sem contradizer o que já saiu. O log de blocos só acrescenta e **corta o rabo incompleto** depois de uma queda.
- **Sem blocos vazios** (padrão): a rede dorme até haver transação; um **intervalo mínimo entre blocos** (padrão 500 ms) agrupa transações. Qualquer mensagem de consenso da altura também acorda o nó.
- **Sincronização:** quem vê um par à frente pede blocos (`GetBlocks`), e cada bloco recebido passa por **toda** a verificação da réplica (certificado, proponente, conjunto, execução).
- **Votar só no que dá para aplicar:** `Chain::validate` ensaia o bloco inteiro (inclusive a raiz); o aprovado guarda o hash para o `apply` **não reconferir assinaturas**. O ensaio entra no cache por (rodada, hash).

## Consequências e limites honestos
- A transação com nonce com buraco maior que 16 é recusada; o saldo só é conferido para a taxa na admissão (o valor é conferido ao montar o bloco, e a que falha sai do mempool).
- O `purge` depois de cada bloco é O(mempool). Com dezenas de milhares de pendentes vira custo; a medição do M6 diz quando indexar.
