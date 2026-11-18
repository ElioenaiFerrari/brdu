# Fluxo 02 — Da transação ao bloco: execução determinística

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como uma transação assinada vira mudança de estado, e como um bloco de transações vale **por inteiro ou nada**. O livro-razão é puro e determinístico: o mesmo bloco, aplicado ao mesmo estado, dá exatamente a mesma raiz em qualquer nó. A taxa de cada transação é cobrada do remetente e **repartida entre os validadores** no fim do bloco.

## Quem participa
- **Remetente:** assina a transação com a chave dele (a carteira, M8).
- **Proponente:** monta o bloco com as transações do mempool e calcula a raiz de estado (`Executor::build_block`).
- **Validadores:** reexecutam o bloco (`Executor::apply_block`) e só votam se a raiz bater (consenso, M3).

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
**Uma transação** (`Executor::apply_tx`, também usada para admitir no mempool):
1. **Forma e assinatura:** tamanho da carga (até 64 KiB), tipo conhecido (1 a 21), assinatura Ed25519 estrita sobre o hash etiquetado `brdu/tx` do corpo.
2. **Rede:** o `chain_id` tem de ser o da rede (impede reaproveitar a transação em outra rede).
3. **Taxa mínima:** transferência = 1.000 + 1 × bytes da transação codificada; mint, burn e atestado = 0.
4. **Nonce:** tem de ser exatamente o próximo da conta (replay e transação fora de ordem são recusados).
5. **Débito da taxa** e nonce + 1.
6. **Regra do tipo:** moeda (fluxo 03) ou módulo (M9). Se **qualquer** passo falha, **nada** do que a transação mudou fica.

**Um bloco** (`Executor::apply_block`):
1. **Ligação:** altura = anterior + 1 e `prev_hash` = hash do último bloco; hora não decresce.
2. **Limites:** até 1.000 transações e 1.000.000 de bytes; nenhuma transação repetida; signatários não vazios, em ordem estrita, com o proponente.
3. **Assinaturas** de todas as transações (a parte mais cara; pode usar várias threads).
4. **Execução** de cada transação, em ordem. Uma inválida **derruba o bloco inteiro**.
5. **Taxas:** a soma é dividida igualmente entre os validadores do cabeçalho; o resto da divisão vai ao proponente. Nenhuma unidade se perde.
6. **Raiz de estado:** a raiz depois da execução tem de ser igual à do cabeçalho; só então o hash do bloco vira o "último hash".

## O que é verificado e por quem
- **Cada nó**, sozinho: tudo acima. Não confia no proponente.
- **Atomicidade:** `State::atomically` desfaz pelo diário (sem copiar o estado); `speculate` calcula a raiz de um bloco novo e desfaz.
- **Determinismo:** repetir todos os blocos do zero dá o mesmo estado (propriedade testada, 500 sequências aleatórias).

## O que pode dar errado
- **Bloco com uma transação ruim:** recusado inteiro; o proponente que a incluiu perde o bloco. Por isso o nó pré-valida cada transação no mempool.
- **Estado "pendente" do mempool × estado do bloco:** no pendente a taxa só sai da conta; no bloco ela é repartida. Os dois não podem ser confundidos.
- **Raiz recalculada a cada bloco** (O(contas)): com milhões de contas vira gargalo; árvore incremental fica para quando a medição pedir.

## Onde está no código
`crates/brdu-ledger/src/{exec,state,block,tx,rules}.rs`.

## Testes que cobrem
`tests/tx.rs` (codec, assinatura cobre todos os campos, **oráculo: 4 transações do BRDC**), `tests/block.rs` (13: bloco inteiro ou nada, taxa repartida, ligação, hora, limites, repetição, signatários, assinatura adulterada, `build` não altera o estado, codec), `tests/invariants.rs` (6: oferta = soma dos saldos, recusada não muda nada, replay dá o mesmo estado, raiz independe da ordem, atomicidade aninhada, limite de 64 bits).

## Limites honestos
- Sem módulos ainda (bens, credenciais, contratos chegam no M9); tipos 5 a 21 dão `UnknownKind` até lá.
- Sem auditoria. A hora do bloco é só conferida contra o estado aqui; a janela "até 15 s à frente do relógio" é do nó (M4).
