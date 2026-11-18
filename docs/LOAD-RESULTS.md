# Resultados do teste de carga (BRDU, Rust) e comparativo com o BRDC (Elixir)

Medido em 2026-11-18 no laboratório Docker (fluxo 07): **4 validadores, 0,5 CPU e 256 MiB cada**, OrbStack (11 CPUs e 9,4 GB na VM), gerador de carga no host. Carga **aberta**, 20 s por degrau, aquecimento de 10 s descartado, **3 repetições** (mediana). Transferências simples de 8 contas para um destino, enviadas em round-robin aos 4 nós.

## Escada oficial (mediana de 3)

| Oferecida | Aceita/s | Confirmação p50 | p95 | Esvaziou | tx/bloco | CPU máx (núcleos) | Throttle |
|---|---|---|---|---|---|---|---|
| 50/s | 50,0 | 169 ms | 275 ms | 0,2 s | 11,6 | 0,104 | 0,0% |
| 100/s | 100,0 | 175 ms | 283 ms | 0,2 s | 23,5 | 0,139 | 0,0% |
| 200/s | 200,0 | 191 ms | 305 ms | 0,2 s | 49,4 | 0,174 | 0,5% |
| 400/s | 400,0 | 197 ms | 312 ms | 0,2 s | 100,0 | 0,194 | 1,4% |
| 800/s | 800,0 | 204 ms | 324 ms | 0,2 s | 200,0 | 0,199 | 1,4% |
| 1.200/s | 1.200,0 | 262 ms | 810 ms | 0,2 s | 352,9 | 0,291 | 16,7% |
| 2.000/s | 1.995,3 | 940 ms | 1.708 ms | 1,3 s | 930,2 | 0,364 | 42,4% |

Nenhuma transação aceita foi perdida; nenhum evento `die`/`oom` do Docker. Em exploração anterior (1 repetição): **3.000/s** já saturou (p50 15 s, esvaziou em 17,7 s) e **4.500/s** aceitou só 2.405/s (CPU estrangulada em 0,5 e bloco no limite de 1.000 transações).

**Memória:** em repouso, **2,5 MiB** por validador. Em carga sustentada (primeira execução, contêineres novos): 9 MiB a 50/s, 18 MiB a 400/s, 32 MiB a 800/s, 43 MiB a 1.200/s, 78 MiB a 2.000/s. **Atenção:** o `memory.peak` do cgroup é cumulativo desde o início do contêiner; na tabela da escada oficial o pico aparecia como 145 MiB só porque herdou o teste de sobrecarga (4.500/s) feito antes, então esse número **não** é por degrau e foi omitido acima.

## Comparativo com o BRDC (mesma escada, mesmos limites, mesmo desenho de carga)

| Oferecida | BRDC (Elixir) confirmação p50 | BRDU (Rust) p50 | BRDC CPU máx | BRDU CPU máx | BRDC memória pico | BRDU memória pico |
|---|---|---|---|---|---|---|
| 50/s | 746 ms | 169 ms | 0,09 | 0,104 | 137 MiB | 9 MiB |
| 400/s | 1.940 ms | 197 ms | 0,287 | 0,194 | 219 MiB | 18 MiB |
| 800/s | saturou: 702/s aceitas, p50 12,5 s | 204 ms, 800/s | 0,264 (44% de throttle) | 0,199 (1,4%) | 292 MiB | 32 MiB |

Ponto de saturação: **BRDC ~700 tx/s; BRDU entre 2.000 e 3.000 tx/s** (≈ 3 a 4 vezes mais).

## Como ler isto com honestidade
- **Não é um benchmark controlado.** O gerador de carga roda no host e disputa CPU; poucas repetições; um só dia e máquina.
- **As implementações não são idênticas:** o BRDC tinha intervalo médio de bloco ~1,1 s e o BRDU usa `min-block-ms 200` e confirma mais cedo; parte do ganho de latência vem do **desenho** (blocos mais frequentes, assinaturas em lote, validação com cache), não só da linguagem. A memória em repouso (2,5 MiB × 135 MiB) e o ponto de saturação, porém, apontam para um ganho real de Rust, como você previa.
- A medição do BRDC é a do `/tmp/brdc-load-run2.txt` do mesmo dia, com os 4 validadores a 0,5 CPU e 512 MiB; não foi repetida com 3 execuções.
- **O primeiro resultado do BRDU em 800/s estava errado** por um defeito do meu medidor (cliente HTTP sem suporte a resposta `chunked`); corrigido e refeito. A rede nunca travou.
