# Contratos nativos: módulos, janelas e custódia

## Resumo
Não há máquina virtual. Os comportamentos possíveis são **módulos de Rust revisados**; acordos entre partes são **modelos declarativos** interpretados por um vocabulário fechado. Poucos comportamentos, todos auditáveis.

| Módulo | Tipos | Ideia |
|---|---|---|
| Credenciais | 14–19 | um **órgão** (M-de-N) declara algo sobre uma pessoa; **janela de contestação**, validade, veto, revogação |
| Bens | 5–13 | classes com órgão, **negociação com custódia (escrow)** e **janela de veto**, gravames |
| Contratos | 20–21 | compra e venda, aluguel, empréstimo como **máquinas de estado** |

## Três padrões que se repetem
1. **Aprovação do órgão sobre um documento exato.** Inclui rede, ids e uma **revisão** que sobe a cada ação; assim uma aprovação nunca vale duas vezes nem para outra coisa.
2. **Janela de tempo pela hora do bloco.** "Antes do prazo" e "a partir do prazo" são comparações com a hora do bloco; o limite exato está testado.
3. **Custódia.** O dinheiro sai do saldo, fica no módulo e só sai com a transição certa. Invariante, testado por propriedade: `saldos + custódias = emitido − queimado`.

Um modelo de contrato é **dado**:

```rust
tr("accept", "open", Actor::OnlyB, vec![],
   vec![Effect::Lock(Party::B, e_term("price")),
        Effect::SetVar("deadline", add(Expr::Time, e_term("duration")))], "funded")
```

## O que NÃO garante
Que o bem existe, que a credencial é verdadeira ou que o acordo é justo: a rede confia no **órgão** e executa só o que as partes autorizadas assinaram. Sem arbitragem.

## Onde está no código
`crates/brdu-credentials`, `crates/brdu-assets`, `crates/brdu-contracts`; fluxos [`12`](../flows/12-credentials.md), [`13`](../flows/13-assets.md), [`14`](../flows/14-contracts.md); ADRs 0019, 0020, 0021.
