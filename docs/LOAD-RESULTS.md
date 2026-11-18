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

## Rodada com a mesma memória do BRDC (512 MiB por validador)

Pedido do dono: repetir a escada do BRDU com **512 MiB** e 0,5 CPU por validador, como o BRDC (2026-11-18, mesma máquina, 3 repetições, carga aberta de 20 s por degrau, aquecimento de 10 s descartado).

| Oferecida | Aceita/s | Confirmação p50 | p95 | Esvaziou | tx/bloco | CPU máx | Throttle | Memória pico |
|---|---|---|---|---|---|---|---|---|
| 50/s | 50,0 | 160 ms | 264 ms | 0,1 s | 11,5 | 0,101 | 0,0% | 9 MiB |
| 100/s | 100,0 | 167 ms | 273 ms | 0,2 s | 23,3 | 0,131 | 0,0% | 12 MiB |
| 200/s | 199,9 | 179 ms | 289 ms | 0,3 s | 47,6 | 0,156 | 0,5% | 18 MiB |
| 400/s | 400,0 | 179 ms | 289 ms | 0,2 s | 96,4 | 0,164 | 0,0% | 27 MiB |
| 800/s | 800,0 | 191 ms | 303 ms | 0,2 s | 195,1 | 0,183 | 0,5% | 46 MiB |
| 1.200/s | 1.199,9 | 216 ms | 338 ms | 0,2 s | 311,7 | 0,254 | 4,8% | 79 MiB |
| 2.000/s | 1.992,9 | 1.147 ms | 2.129 ms | 1,6 s | 930,2 | 0,374 | 47,5% | 134 MiB |

Nenhuma transação aceita foi perdida e o Docker não registrou `die` nem `oom`. O limite de memória não foi o fator: o resultado é praticamente o da rodada de 256 MiB (a CPU em 0,5 é o que limita). A memória de pico é cumulativa entre degraus (ver aviso acima): os 134 MiB do último degrau incluem o histórico dos anteriores.

Com a mesma memória e a mesma CPU, a comparação direta fica: BRDC saturou em ~700 tx/s (p50 de 12,5 s a 800/s, 292 MiB); o BRDU sustentou **1.200 tx/s com p50 de 216 ms** e só começou a enfileirar perto de 2.000 tx/s, com **134 MiB** no pior caso. Valem as mesmas ressalvas da seção anterior (gerador no mesmo computador; o BRDC foi medido uma vez).

## Comparação final BRDU (Rust) × BRDC (Elixir): mesma escada, mesmos limites

Medido em 2026-11-18, **na mesma máquina e no mesmo dia**, com a mesma configuração: 4 validadores, **0,5 CPU cada**, degraus de 20 s de transferências simples com carga **aberta**, aquecimento de 10 s descartado, 8 contas remetentes. As duas ferramentas de carga são a de cada projeto (`cargo dev load` e `mix brdc.compose.load --no-stop`). Saídas brutas em [`load-runs/`](load-runs/).

### Memória de 512 MiB por validador (BRDU e BRDC com 3 repetições; mediana)

| Oferecida | Aceita/s BRDU | Aceita/s BRDC | Confirmação p50 BRDU | p50 BRDC | p95 BRDU | p95 BRDC | Memória pico BRDU | Memória pico BRDC |
|---|---|---|---|---|---|---|---|---|
| 50/s | 50,0 | 50,0 | 160 ms | 752 ms | 264 ms | 1.236 ms | 9 MiB | 147 MiB |
| 100/s | 100,0 | 100,0 | 167 ms | 1.505 ms | 273 ms | 3.824 ms | 12 MiB | 180 MiB |
| 200/s | 199,9 | 200,0 | 179 ms | 1.627 ms | 289 ms | 2.509 ms | 18 MiB | 239 MiB |
| 400/s | 400,0 | 400,0 | 179 ms | 6.135 ms | 289 ms | 9.608 ms | 27 MiB | 311 MiB |
| 800/s | **800,0** | **799,3** (saturou nas 3) | 191 ms | **36.729 ms** | 303 ms | 58.009 ms | 46 MiB | 398 MiB |

Tempo para esvaziar o que ficou pendente em 800/s: BRDU 0,2 s; BRDC 58,7 s (nas três repetições passou do limite de saturação). CPU máxima por validador em 800/s: BRDU 0,183 núcleo (1% de throttle); BRDC 0,292 (56% de throttle).

### Memória de 256 MiB por validador (BRDC 1 repetição; BRDU, mediana de 3 da primeira seção)

| Oferecida | Aceita/s BRDU | Aceita/s BRDC | p50 BRDU | p50 BRDC | p95 BRDU | p95 BRDC | Memória pico BRDU | Memória pico BRDC |
|---|---|---|---|---|---|---|---|---|
| 50/s | 50,0 | 50,0 | 169 ms | 746 ms | 275 ms | 1.228 ms | 9 MiB | 155 MiB |
| 100/s | 100,0 | 100,0 | 175 ms | 2.912 ms | 283 ms | 11.223 ms | 11 MiB | 167 MiB |
| 200/s | 200,0 | 200,0 | 191 ms | 1.522 ms | 305 ms | 3.441 ms | 13 MiB | 198 MiB |
| 400/s | 400,0 | 400,0 | 197 ms | 1.870 ms | 312 ms | 2.900 ms | 18 MiB | 236 MiB |
| 800/s | **800,0** | **695,4** (saturou) | 204 ms | 8.859 ms | 324 ms | 19.433 ms | 32 MiB | **256 MiB (no teto)** |

Nenhum dos dois teve `oom` nem reinício de contêiner. O BRDC encostou no limite de 256 MiB em 800 tx/s (o pico de um validador chegou a 256,0/256,0 MiB) sem ser morto, mas ficou saturado: aceitou 695/s e levou 60 s para esvaziar. O BRDU usou 32 MiB no mesmo degrau.

### Com o mesmo intervalo de bloco: BRDC também com 200 ms

A principal ressalva da comparação acima era que o BRDC fechava blocos a cada ~1,1 s e o BRDU a cada 200 ms. Para igualar, o laboratório do BRDC ganhou `mix brdc.compose.init --interval 200ms` (vai em `BRDC_INTERVAL_MS` de cada validador) e a escada foi repetida **com o mesmo intervalo de 200 ms**, 0,5 CPU e a mesma memória. Saídas brutas em [`load-runs/`](load-runs/) (`brdc-200ms-*`).

**512 MiB por validador, 3 repetições (mediana):**

| Oferecida | Aceita/s BRDU | Aceita/s BRDC (200 ms) | p50 BRDU | p50 BRDC | p95 BRDU | p95 BRDC | Memória pico BRDU | Memória pico BRDC |
|---|---|---|---|---|---|---|---|---|
| 50/s | 50,0 | 50,0 | 160 ms | 285 ms | 264 ms | 434 ms | 9 MiB | 168 MiB |
| 100/s | 100,0 | 100,0 | 167 ms | 909 ms | 273 ms | 1.804 ms | 12 MiB | 202 MiB |
| 200/s | 199,9 | 200,0 | 179 ms | 1.003 ms | 289 ms | 1.433 ms | 18 MiB | 260 MiB |
| 400/s | 400,0 | 400,0 | 179 ms | 3.001 ms | 289 ms | 4.327 ms | 27 MiB | 319 MiB |
| 800/s | **800,0** | 800,0 (740 a 800; **saturou nas 3**) | 191 ms | **18.592 ms** | 303 ms | 32.396 ms | 46 MiB | 415 MiB |

Em 800/s o BRDC levou 57 s para esvaziar a fila (BRDU: 0,2 s) e a CPU dele ficou em ~0,31 a 0,34 núcleo com 63 a 70% de throttle; o BRDU usou 0,18 núcleo com 1%.

**256 MiB por validador (BRDC 1 repetição):**

| Oferecida | Aceita/s BRDC (200 ms) | p50 BRDC | p95 BRDC | Memória pico BRDC | BRDU (mediana de 3): aceita, p50, memória |
|---|---|---|---|---|---|
| 50/s | 50,0 | 270 ms | 429 ms | 169 MiB | 50,0 · 169 ms · 9 MiB |
| 100/s | 100,0 | 1.725 ms | 7.553 ms | 169 MiB | 100,0 · 175 ms · 11 MiB |
| 200/s | 200,0 | 2.119 ms | 9.122 ms | 201 MiB | 200,0 · 191 ms · 13 MiB |
| 400/s | 400,0 | 907 ms | 1.524 ms | 236 MiB | 400,0 · 197 ms · 18 MiB |
| 800/s | **677,3 (saturou)** | 3.205 ms | 23.219 ms | **256 MiB (no teto)** | 800,0 · 204 ms · 32 MiB |

Sem `oom` e sem `die` do Docker nas duas rodadas. **Atenção:** no degrau de 800/s a 256 MiB a ferramenta do BRDC listou a altura de só **3 dos 4 validadores** (`[253, 253, 253]`); o Docker não registrou morte de contêiner, mas esse degrau é de saturação e não deve ser lido com precisão.

**O que o intervalo igual mudou:**
- **A latência com pouca carga do BRDC caiu de 752 para 285 ms em 50/s** (era 2,6 vezes maior que a do BRDU, agora 1,8 vezes): parte da diferença anterior era mesmo configuração de bloco.
- **A saturação não mudou de lugar:** com 200 ms o BRDC ainda enfileira a partir de 400 a 800 tx/s (800/s: p50 de 18,6 s e fila de 57 s) e o BRDU continua em ~190 ms até 800/s. A diferença de **vazão** e de **memória** (46 contra 415 MiB) **não** vem do intervalo de bloco.
- Em blocos de 200 ms o BRDC fecha blocos de ~0,3 s (50/s) a ~3 s (800/s); o BRDU, de 0,2 s sob carga leve e blocos cheios (até 1.000 tx) sob carga alta.

### Conclusão
- **Nenhum dos dois "travou"** (sem perda de transação aceita, sem OOM, mesma altura e mesmo bloco em todos os validadores). A diferença é **quanta carga cada um sustenta antes de enfileirar** e **quanta memória gasta para isso**.
- Com 0,5 CPU por validador: o BRDC saturou entre 400 e 800 tx/s; o BRDU sustentou 800 tx/s com p50 de ~190 ms e só começou a enfileirar perto de 2.000 tx/s (primeira seção).
- Em memória, o BRDU usou de **~10 a 15 vezes menos** em todos os degraus (por exemplo 46 contra 398 MiB em 800 tx/s).
- A latência de confirmação do BRDU é de **4 a 5 vezes menor já em 50 tx/s** (160 contra 752 ms) e fica ~200 ms até 800 tx/s, enquanto a do BRDC cresce com a carga.

### Ressalvas (leia antes de citar estes números)
- **Um só computador, Docker em VM:** a ferramenta de carga roda no host e disputa CPU com os contêineres; um dia só; poucas repetições (BRDC a 256 MiB: uma).
- **Os intervalos de bloco foram igualados (200 ms) na última rodada.** As duas primeiras tabelas usam o padrão do BRDC (1 s); a seção "mesmo intervalo" as refaz. O BRDC mede a confirmação pelo nonce lido a cada 100 ms; o BRDU pelo bloco, a cada ~15 ms.
- **A "memória pico" é cumulativa desde o início do contêiner** em ambos: um degrau herda o pico dos anteriores, na ordem crescente de taxa. Serve para comparar os dois com a mesma escada, não como consumo por degrau.
- **Cada projeto tem seu próprio gerador de carga**, escritos em linguagens diferentes: parecidos, mas não idênticos. O BRDC também carrega 2 contêineres de carteira ociosos no lab (não entram na medição).
- **Transferências simples apenas:** contratos, bens e credenciais custam mais.
- A diferença entre os dois tem causas misturadas (linguagem, coleta de lixo, verificação de assinaturas em lote, cache de validação, intervalo de blocos). Não está medido quanto vem de cada uma.

## Como ler isto com honestidade
- **Não é um benchmark controlado.** O gerador de carga roda no host e disputa CPU; poucas repetições; um só dia e máquina.
- **As implementações não são idênticas:** o BRDC tinha intervalo médio de bloco ~1,1 s e o BRDU usa `min-block-ms 200` e confirma mais cedo; parte do ganho de latência vem do **desenho** (blocos mais frequentes, assinaturas em lote, validação com cache), não só da linguagem. A memória em repouso (2,5 MiB × 135 MiB) e o ponto de saturação, porém, apontam para um ganho real de Rust, como você previa.
- A primeira comparação usava uma só medição do BRDC; ela foi **refeita com 3 repetições** (512 MiB) e com 256 MiB na seção acima.
- **O primeiro resultado do BRDU em 800/s estava errado** por um defeito do meu medidor (cliente HTTP sem suporte a resposta `chunked`); corrigido e refeito. A rede nunca travou.
