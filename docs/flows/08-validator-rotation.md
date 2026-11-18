# Fluxo 08 — Rotação de validadores

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como uma instituição entra ou sai do conjunto de validadores **sem recriar a rede**: mais de 2/3 do conjunto em vigor aprovam uma troca, ela fica pendente e passa a valer numa altura futura, quando o conjunto, a ordem do rodízio e a época mudam em todos os nós ao mesmo tempo.

## Quem participa
- **Validadores em vigor:** assinam a aprovação (mais de 2/3).
- **Retransmissor:** qualquer um que envia a transação (taxa zero; quem envia não autoriza).
- **Novo nó:** sobe com a chave nova, fora do conjunto; sincroniza como **observador**.
- **Nó removido:** vira observador e é desconectado.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev init --spare 1 && cargo dev up   # v5 sobe como observador (recusado até haver troca pendente)
cargo dev rotate --in 5 --out 3            # entra o 5, depois sai o 3
cargo dev status                           # v5 com pares, v3 isolado
curl localhost:7201/validators             # época, conjunto, troca pendente
```
1. As aprovações assinam `brdu/validators/change` com rede, ação, chave, **altura efetiva** e **época atual**.
2. A transação `VALIDATOR_CHANGE` entra num bloco: o ledger confere mais de 2/3 do conjunto em vigor, tamanho mín./máx., altura efetiva ≥ altura + 2 e **uma só troca pendente**; grava `val/pending`.
3. Com a troca pendente, a política de conexão admite a chave que entra: o novo nó conecta e **sincroniza** os blocos (cada um verificado com o conjunto da sua época).
4. Ao fim do bloco `efetiva − 1`, o conjunto novo e a época nova entram em vigor. O proponente, o certificado e o `signers` do bloco seguinte já usam o conjunto novo.
5. O nó que entrou começa a votar; o que saiu vira observador e é desconectado pelos demais.
6. `cargo dev rotate` manda transferências mínimas para a rede produzir blocos (sem transação ela dorme) até a época virar.

## O que é verificado e por quem
Cada nó, sozinho: aprovações (distintas, do conjunto em vigor, sobre o documento exato), limites, altura, pendência, e depois o certificado de cada bloco contra o conjunto **daquela** altura.

## O que pode dar errado
- **Aprovações de menos de 2/3, repetidas, de estranhos ou de outra época/rede:** a troca é recusada e nunca fica pendente.
- **Segunda troca com uma pendente:** recusada. **Entrada acima do máximo / saída abaixo do mínimo:** recusada.
- **Altura efetiva muito próxima:** recusada (precisa ser futura).
- **Conluio de mais de 2/3:** controla o conjunto. **Mais de 1/3 parado:** a rede não decide.

## Onde está no código
`crates/brdu-ledger/src/gov.rs`, gancho `Module::end_block` em `exec.rs`; `crates/brdu-chain/src/{chain,genesis}.rs`; `crates/brdu-node/src/{core,net,api}.rs`; `tools/brdu-dev/src/main.rs` (`rotate`).

## Testes que cobrem
`crates/brdu-ledger/tests/gov.rs` (9, incluindo propriedade: o conjunto nunca sai de [mín, máx]); `crates/brdu-chain/tests/chain.rs` (o certificado do conjunto velho não vale depois da troca; removido não conta; réplica refaz o mesmo conjunto); `crates/brdu-node/tests/cluster.rs` (5º nó entra e vota, removido vira observador e a rede segue, troca fraca nunca fica pendente, reinício com troca pendente). Verificado também com 5 processos reais (época 2 com 4 validadores).

## Limites honestos
Sem stake nem penalidade; governança real (quem são os órgãos) é decisão de implantação. A política de conexão aceita a chave que entra **antes** de valer (necessário para sincronizar), o que permite a ela abrir conexões de observador.
