# Fluxo 10 — O explorador

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Uma página web (tema do BRDC) e uma API JSON somente leitura para ver blocos, transações, contas, validadores, oferta e cobertura da reserva. O explorador **reverifica cada bloco** antes de mostrá-lo: um nó que mente é recusado e denunciado na própria tela.

## Quem participa
- **Nó (`brdu-node`):** entrega blocos finais codificados (`GET /blocks/raw`) e as evidências.
- **Explorador (`brdu-explorer`):** réplica verificada, índices, API e página.
- **Você:** abre `http://127.0.0.1:8545`; a gênese fixada por quem roda o explorador.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev explorer                  # rede local: abre http://127.0.0.1:8545 (cargo dev explorer stop derruba)
cargo dev compose up                # Docker: o serviço `explorer` já sobe em http://127.0.0.1:8545
```
1. O explorador lê `/status` e `/blocks/raw?from=H+1` do nó em laço (a cada 400 ms, sem pausa enquanto houver blocos).
2. Cada bloco: decodifica, confere o certificado (mais de 2/3 do conjunto **daquela** época), o proponente da rodada, os signatários, executa e confere a raiz de estado. Só então entra.
3. Indexa: transação por id, conta (remetente e contraparte), bloco por hash.
4. A API (`/api/overview`, `/blocks?before&limit`, `/block/N`, `/tx/ID`, `/account/ADDR?skip&limit`, `/validators`, `/search?q=`) responde só com o que foi verificado; a página atualiza a cada 2,5 s.
5. **Página:** Início (oferta em destaque, cobertura, altura, época, validadores, últimos blocos), Blocos (paginados), Bloco (certificado e transações), Transação, Conta (saldo, nonce, movimentações) e Validadores (conjunto em ordem, troca pendente, evidências). Busca por altura, hash, id ou endereço.

## O que é verificado e por quem
O próprio explorador, sozinho (ADR-0017). O rodapé mostra a altura do nó, até onde foi verificado e há quanto tempo sincronizou; se o nó entregar um bloco inválido, o início mostra um alerta com o motivo.

## O que pode dar errado
- **Nó fora do ar:** o rodapé mostra o erro; o explorador continua servindo o que já verificou.
- **Nó que mente** (bloco adulterado, fora de ordem, outra rede): recusado, contado em `rejected_blocks`.
- **Nó removido por rotação:** o explorador para de avançar; aponte para outro (`--node`).
- **Endereço com erro de digitação:** a busca avisa que o checksum não confere.

## Onde está no código
`tools/brdu-explorer/src/{index,sync,view,server,http}.rs`, `tools/brdu-explorer/ui/` (HTML, CSS, JS), endpoint `GET /blocks/raw` em `crates/brdu-node/src/api.rs`, `cargo dev explorer` e o serviço `explorer` do compose em `tools/brdu-dev`.

## Testes que cobrem
`tools/brdu-explorer/tests/explorer.rs`: contra **4 validadores reais** o explorador sincroniza, indexa e serve (visão geral, bloco com certificado, transação, as duas pontas da transferência, paginação, busca incluindo erro de digitação, página com **CSP estrita e sem inline**, `POST` = 405); contra **nós que mentem** (bloco adulterado, outra gênese, fora de ordem) ele recusa e denuncia, e com o nó honesto tudo entra; os testes da parte pura da interface (`ui/ui.test.mjs`, rodados com `node --test`: valores sem ponto flutuante, hashes, efeito de letras e o dicionário de tradução dos códigos da API) e um **teste de fumaça** (`ui/smoke.test.mjs`) que roda o `app.js` de verdade, com um DOM falso, em **todas** as páginas (início, blocos, bloco, transações de cada tipo, conta, validadores, busca): ele pega nomes indefinidos e erros de renderização que só a checagem de sintaxe não pega. Verificado também no navegador (tema, busca, certificado, efeito).

## Limites honestos
A verificação é no explorador, não no navegador (ADR-0017). Sem persistência (ressincroniza ao reiniciar). Um nó só. Os diagramas continuam "a gerar".
