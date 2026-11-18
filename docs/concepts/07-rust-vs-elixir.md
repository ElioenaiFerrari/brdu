# Rust × Elixir: o que mudou no desempenho e por quê

## Resumo
Mesmo desenho, mesma escada de carga, mesmos limites (4 validadores, 0,5 CPU cada): o BRDU (Rust) sustentou **800 tx/s** com confirmação de ~190 ms e só saturou perto de **2.000 tx/s**; o BRDC (Elixir) saturou entre **400 e 800 tx/s** (aceitou ~700/s, p50 de 8 a 36 s a 800/s). A memória em 800 tx/s foi de **46 MiB contra 398 MiB** (512 MiB de limite). Números e ressalvas em [`LOAD-RESULTS.md`](../LOAD-RESULTS.md).

## Por que a diferença (hipóteses, não medições isoladas)
- **Sem *runtime* gerenciado:** a BEAM reserva memória por processo e coleta lixo; em Rust a memória é a do dado.
- **Assinaturas em lote e cache de validação:** o BRDU verifica as assinaturas de um bloco em lote e não repete verificações já feitas no mempool.
- **Blocos mais frequentes** (`min-block-ms 200`): parte da latência menor com pouca carga era **desenho**, não linguagem. Igualando o intervalo do BRDC a 200 ms, a latência dele em 50 tx/s caiu de 752 para 285 ms, mas ele continuou saturando a partir de 400 a 800 tx/s e gastando ~9 vezes mais memória: a diferença de vazão e memória não vem do intervalo.

## O que o Elixir faz melhor (e o BRDU não tem)
Tolerância a falhas por supervisão, atualização a quente e inspeção em produção. No BRDU um pânico derrubaria o nó; por isso as bibliotecas proíbem `unwrap`/`panic` (regra do clippy).

## Como ler com honestidade
Não é benchmark controlado: gerador no mesmo computador, poucas repetições, um dia, uma máquina; as implementações diferem em intervalo de bloco. Para decidir algo de verdade, repetir em máquinas dedicadas.
