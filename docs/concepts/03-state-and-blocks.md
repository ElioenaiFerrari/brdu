# Estado, blocos atômicos e raiz de estado

## Resumo
A rede é uma **máquina de estados replicada**: saldos, nonces e os dados dos módulos. Um **bloco** é uma lista ordenada de transações que muda o estado **tudo ou nada**.

- **Nonce** por conta evita repetir uma transação (replay).
- **Atomicidade:** o estado guarda um **diário de desfazer**; se qualquer transação do bloco for inválida, o bloco inteiro é recusado e o estado volta ao que era.
- **Raiz de estado:** uma árvore de Merkle sobre todo o estado (inclusive os módulos). Dois nós com a mesma raiz têm exatamente o mesmo estado.
- **Hora do bloco:** os prazos dos módulos usam a hora do **bloco em execução**, nunca o relógio local do nó.

## Módulos
Moeda, credenciais, bens e contratos plugam no mesmo livro-razão pelo trait `Module`: cada um declara a faixa de tipos de transação que trata, a taxa mínima e como executa, guardando seus dados num espaço de chaves próprio.

```rust
impl Module for AssetsModule {
    fn kinds(&self) -> core::ops::RangeInclusive<u8> { kind::CLASS..=kind::LIEN_REMOVE }
    fn min_fee(&self, rules: &Rules, tx: &Tx) -> u64 { /* … */ }
    fn execute(&self, state: &mut State, rules: &Rules, ctx: &TxCtx, tx: &Tx)
        -> Result<(), LedgerError> { /* … */ }
}
```

Quem recebe a taxa: os validadores que assinaram o bloco, em partes iguais.

## O que NÃO garante
O estado correto depende de o código dos módulos estar certo; um erro de lógica é replicado em todos os nós. Por isso há testes por propriedade (conservação do dinheiro) e vetores do BRDC.

## Onde está no código
`crates/brdu-ledger/src/{state,exec,tx,block}.rs`; fluxo em [`flows/02-transaction-and-block-execution.md`](../flows/02-transaction-and-block-execution.md).
