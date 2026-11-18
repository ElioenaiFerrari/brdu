# Fluxo 14 — Contratos entre partes

## Resumo
Como duas partes fecham um acordo com dinheiro (compra e venda, aluguel, empréstimo) que a rede executa por **modelos declarativos**, com o dinheiro em **custódia** e prazos pela hora do bloco. Sem máquina virtual e sem juiz.

## Quem participa
- **Parte A** (quem cria) e **parte B**.
- **Qualquer pessoa**, nas transições que dependem só do prazo.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev contract templates                                  # modelos, termos e transições
cargo dev contract create --template compra-venda --id cv-1 --as alice --with bob --terms m:10,3600
cargo dev contract act --id cv-1 --as bob --action accept     # 10 BRDU de bob vão para a custódia; prazo de 3600 s
cargo dev contract act --id cv-1 --as bob --action confirm    # a custódia vai para alice
cargo dev contract act --id cv-1 --as bob --action refund_expired   # (ou, passado o prazo, qualquer um devolve a bob)
cargo dev contract list alice | show cv-1
```
`m:` marca dinheiro (`m:10` = 10 BRDU); o resto são segundos ou contagens. Termos por modelo: compra-venda `price, duration`; aluguel `rent, period, periods, deposit, grace`; empréstimo `principal, repay, duration`.
1. **Criar (20):** valida modelo, id livre, parte B diferente, termos (valores positivos, prazos entre `min_window` e `max_window`, sem estouro). No empréstimo o principal já é travado aqui.
2. **Agir (21):** confere o estado de origem, o ator, as condições de tempo e aplica os efeitos em ordem, tudo ou nada.
3. **Consulta:** `GET /contract-templates`, `/contract/ID`, `/contracts/ENDEREÇO`; o explorador lista os contratos na conta; a carteira mostra um cartão de contratos.

## O que é verificado e por quem
O **ledger**, em todo nó: modelo e transição existem, estado e ator certos, prazo pela hora do **bloco**, saldo suficiente para travar ou pagar, aritmética sem estouro, e a conservação do dinheiro (custódia).

## O que pode dar errado
- **Parte errada agindo** (`not_party`), **cedo demais** (`too_early`), **estado errado** (`bad_state`): recusado.
- **Saldo insuficiente** ao aceitar ou pagar: a transação falha inteira, sem custódia parcial.
- **Disputa:** a rede não decide; só executa o que a parte autorizada assinou.
- **Inadimplência:** `mark_default` só registra o estado; não há cobrança automática.

## Onde está no código
`crates/brdu-contracts` (vocabulário, modelos, interpretador), registro em `crates/brdu-chain/src/chain.rs`, API em `crates/brdu-node/src/api.rs`, `cargo dev contract` em `tools/brdu-dev/src/contract.rs`, explorador e carteira.

## Testes que cobrem
`crates/brdu-contracts/tests/contracts.rs` (8: os três modelos de ponta a ponta, atores, prazos no limite, validação, aceite sem saldo, conservação por propriedade) e `tests/oracle.rs` (payloads do BRDC byte a byte); `tools/brdu-dev/tests/cli.rs` (venda e empréstimo contra nós reais); testes de interface do explorador e da carteira.

## Limites honestos
Só três modelos; sem juros nem oráculos; o índice por conta é uma varredura (O(contratos)); o explorador não tem teste de Rust com contratos reais (só fixture de interface); diagramas "a gerar".
