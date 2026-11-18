# Fluxo 05 — Da carteira ao bloco: envio, fofoca, consenso e sincronização

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como uma transação assinada chega a **todos** os validadores, entra num bloco decidido por mais de 2/3 e é replicada por quem estava fora do ar. A carteira só envia a transação já assinada; o nó nunca tem chave de usuário.

## Quem participa
- **Carteira/cliente:** assina e faz `POST /tx` em qualquer validador.
- **Validadores** (`brdu-node`): mempool, consenso, log de blocos, API.
- **Par atrasado:** um validador que caiu ou chegou depois.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
1. `POST /tx` (corpo = hex da transação): **assinatura conferida na thread da API** (em paralelo); o núcleo confere rede, taxa mínima, saldo para a taxa e nonce, e admite no mempool (dedupe O(1)).
2. **Fofoca sem eco:** a nova transação vai a todos os pares, **menos** a quem a mandou; quem já viu o id ignora.
3. **Acordar:** sem blocos vazios, o consenso dorme até haver transação (ou mensagem de consenso). O intervalo mínimo entre blocos agrupa o que chegou.
4. **Proponente da rodada** (`validadores[(altura + rodada) % n]`) escolhe as transações que valem juntas (ensaio sobre o estado), monta o bloco e propõe.
5. **Validadores** ensaiam o bloco inteiro (`Chain::validate`: proponente, conjunto de signatários, execução e raiz) antes de votar; diário gravado antes de cada voto sair.
6. Mais de 2/3 de precommits → **bloco final com certificado**; cada nó aplica, grava no log (`fsync`), limpa o diário, tira do mempool o que passou do nonce e anuncia o novo estado.
7. **Quem está atrasado** vê `Status` de altura maior (ou mensagem de uma altura futura) e pede `GetBlocks`; cada bloco recebido passa pela verificação completa da réplica.
8. **Proponente fora do ar:** o prazo de proposta estoura, a rodada avança e outro proponente decide (o bloco guarda a rodada no certificado).

## O que é verificado e por quem
Cada nó, sozinho: assinatura da transação, certificado (mais de 2/3), proponente da rodada, signatários, cada transação, a raiz de estado. Nada vale por "vir de um validador".

## O que pode dar errado
- **Mais de 1/3 fora:** a rede não decide (e não diverge). 
- **Par lento:** a fila de saída é limitada; a mensagem perdida é reposta pela sincronização.
- **Nonce com buraco maior que 16:** recusado. **Mempool cheio:** recusado (transações ou bytes).
- **Queda no meio de um voto:** o diário garante que não se contradiz; o log corta o rabo incompleto.

## Onde está no código
`crates/brdu-node/src/{core,net,api,mempool,wire}.rs`, `crates/brdu-chain/src/{chain,store}.rs`.

## Testes que cobrem
`crates/brdu-node/tests/cluster.rs` (7, TCP e Noise reais em loopback): 4 validadores decidem e chegam à mesma raiz; mesma transação a dois nós entra uma vez; um validador cai e volta (disco + sincronização); a rede toda reinicia do mesmo ponto; API HTTP; **proponente da vez fora do ar muda a rodada**; rajada de 300 transações de 20 contas sem perder nem duplicar e sem eco de fofoca. `tests/mempool.rs` (5): duplicata, nonce vencido, buraco e promoção, limites em transações e em bytes, purge, dedupe com 5.000 pendentes. Teste manual com **4 processos** do binário `brdu-node` (malha de 3 pares em cada).

## Limites honestos
Mempool: depois de cada bloco a cabeça de cada fila que deixou de valer (por exemplo, um mint com referência já usada) sai junto com as seguintes do remetente; uma transação que sai por falha devolve o nonce.
Endereços de pares estáticos (a descoberta por registros assinados existe na crate e não está ligada); sem limitação de taxa por par além do tamanho e das conexões; mempool é O(n) no `purge`; o teste de carga preciso vem no M6.
