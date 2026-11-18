# Fluxo 07 — Laboratório Docker e teste de carga

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Uma **imagem só** (`brdu-lab:dev`, ~100 MB) sobe N validadores com **limites de CPU e memória** (padrão 0,5 CPU e 256 MiB cada), em malha completa, e um teste de carga **aberto** mede o que a rede aguenta, com CPU e memória lidos do **cgroup** de cada contêiner. É o ambiente para testar P2P, falhas e desempenho sem travar a máquina.

## Quem participa
- **`cargo dev compose …`**: gera `compose/` (chaves, gênese, `docker-compose.yml`) e embrulha o `docker compose`.
- **Contêineres `brdu-lab-vN-1`**: o mesmo `brdu-node`, com a própria chave montada só-leitura e um volume de dados.
- **`cargo dev load`**: gera a carga, mede latência e lê os cgroups.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev compose up                    # TUDO de uma vez: cria o lab se não existir, constrói a imagem (com cache) e sobe
                                        # 4 validadores + explorador (8545) + carteira web (8547), esperando ficarem saudáveis
cargo dev compose init [--nodes 4] [--cpus 0.5] [--mem 256]   # (opcional) gera compose/ com outros parâmetros antes do up
export BRDU_DIR=compose                 # os outros comandos passam a falar com o laboratório
cargo dev status | wallet new alice | fund alice 500 | send … | blocks
cargo dev compose pause 3 | unpause 3 | kill 2 | start 2 | logs 1 | stats   # falhas e diagnóstico
cargo dev load --rates 50,100,200,400,800 --secs 20 --warmup 10 --repeats 3
cargo dev compose down | clean          # clean apaga só este laboratório (projeto brdu-lab e seus volumes)
```
0. **Um compose só:** o `docker-compose.yml` gerado tem os validadores, o **explorador** (`http://127.0.0.1:8545`) e a **carteira web** (`http://127.0.0.1:8547`, o `brdu-web` com o cofre no volume `data-wallet`). A carteira escuta em todas as interfaces **dentro** do contêiner (`--allow-remote`), mas a porta só é publicada no loopback do computador; é a 8547 para não colidir com o `cargo dev wallet-web` local (8546). Chaves de **teste**. Para ter saldo: `cargo dev fund ENDEREÇO 1000 --dir compose`.
1. `init` cria 4 chaves, gênese e o compose; cada serviço monta **só a própria chave**; os nós discam aos anteriores (malha completa) pelos nomes `vN:7000`; a API sai em `127.0.0.1:820N`.
2. Limites por serviço: `cpus`, `mem_limit`, `memswap_limit` (igual, sem swap), `pids_limit`; `healthcheck` pelo subcomando `brdu-node health`.
3. **Teste de carga** (lições do BRDC embutidas):
   - **Aberto:** cada conta envia na hora agendada **mesmo que a rede atrase**; a latência conta a partir da hora agendada (sem "omissão coordenada").
   - **Preparação fora da medição:** contas financiadas antes; todas as transações **pré-assinadas** antes de cada degrau.
   - **Aquecimento** na taxa do 1º degrau, descartado; **repetições** com mediana e faixa [mín–máx].
   - **Confirmação** = do envio até a primeira vez que o bloco com a transação aparece (poller a cada ~15 ms).
   - **CPU e memória por cgroup** (`cpu.stat`, `memory.peak`, `memory.events`), não `docker stats`; **eventos `die`/`oom` do Docker** capturados no fim.
   - Conexões HTTP **persistentes** (o BRDC esgotou portas efêmeras com `Connection: close` a 800 tx/s).
   - A carga vai **só aos nós que votam** (`/validators`: `me_in_set`): observadores e removidos não propõem nem repassam e distorceriam a medição.
   - Degrau marcado **SATURADO** se aceitou menos de 95% do oferecido, esvaziou em mais de 10 s ou perdeu confirmações.

## O que é verificado e por quem
O teste só confia no que a rede confirma: toda transação aceita tem de aparecer em bloco; as perdidas são contadas. CPU e memória vêm do kernel (cgroup).

## O que pode dar errado
- **Cache de build:** a 1ª construção compila tudo (minutos); depois o BuildKit reaproveita dependências.
- **Memória:** o limite é de 256 MiB por nó; o pico medido aparece no resumo (a escolha do padrão é por medição, não por suposição).
- **Conflito de portas** 8201..: outro serviço local.
- `compose clean` **não** remove a imagem nem volumes de outros projetos.

## Onde está no código
`docker/Dockerfile`, `.dockerignore`, `tools/brdu-dev/src/{compose,load,http}.rs`, subcomando `health` em `tools/brdu-nodebin`.

## Testes que cobrem
Verificado de ponta a ponta no Docker (4 contêineres saudáveis, fund, send, blocos, mesma raiz). A escada de carga está em [`X. **Planejado:** automatizar o laboratório num teste de CI.

## Limites honestos
Só mede o que roda na sua máquina (CPUs e VM do Docker compartilhadas); a mediana de poucas repetições não é um benchmark científico; o gerador de carga roda no host e disputa CPU com os contêineres.
