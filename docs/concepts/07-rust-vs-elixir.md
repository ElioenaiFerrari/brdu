# Rust × Elixir: o que mudou no desempenho e por quê

## Resumo
Mesmo desenho, mesma escada de carga, mesmos limites (4 validadores, 0,5 CPU cada): o BRDU (Rust) saturou entre **2.000 e 3.000 tx/s** e o BRDC (Elixir) em **~700 tx/s**; a memória em carga foi de ~9–32 MiB contra ~137–292 MiB. Números e ressalvas em [`LOAD-RESULTS.md`](../LOAD-RESULTS.md).

## Por que a diferença (hipóteses, não medições isoladas)
- **Sem *runtime* gerenciado:** a BEAM reserva memória por processo e coleta lixo; em Rust a memória é a do dado.
- **Assinaturas em lote e cache de validação:** o BRDU verifica as assinaturas de um bloco em lote e não repete verificações já feitas no mempool.
- **Blocos mais frequentes** (`min-block-ms 200`): parte da latência menor é **desenho**, não linguagem.

## O que o Elixir faz melhor (e o BRDU não tem)
Tolerância a falhas por supervisão, atualização a quente e inspeção em produção. No BRDU um pânico derrubaria o nó; por isso as bibliotecas proíbem `unwrap`/`panic` (regra do clippy).

## Como ler com honestidade
Não é benchmark controlado: gerador no mesmo computador, poucas repetições, um dia, uma máquina; as implementações diferem em intervalo de bloco. Para decidir algo de verdade, repetir em máquinas dedicadas.
