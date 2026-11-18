# Fluxo 03 — Produção da moeda: reserva, mint e burn

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
O BRDU vale R$ 1 e nasce **lastreado**: o banco recebe reais, os emissores **atestam a reserva** e então **emitem** BRDU (*mint*); quem quer sair **resgata** (*burn*), o banco devolve os reais e os emissores **reduzem o lastro**. Tudo exige **M de N emissores**. A rede **só informa a cobertura** (reserva atestada contra o que circula): não prova os reais e **não congela nada** ([ADR-0006](../adr/0006-no-freezing-uncensored-brdu.md)).

## Quem participa
- **Emissores** (chaves de teste nas redes de teste): assinam atestados, mints e burns. A rede conhece o conjunto e o limiar M pelas regras da gênese.
- **Titular** (uma conta): recebe, paga e pede o resgate (só ele queima o próprio saldo).
- **Banco** (fora da rede): recebe e devolve os reais; aqui é só texto na referência.
- **Qualquer um pode enviar** a transação de mint ou atestado, desde que traga as aprovações: quem envia não é quem autoriza.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
1. **Lastro (tipo 4):** M emissores assinam o documento `brdu/attestation` com `{rede, sequência, reserva, referência}`. A sequência é `último + 1` (começa em 1); repetir é recusado.
2. **Emissão (tipo 2):** os emissores assinam `brdu/issuance/mint` com `{rede, conta, valor, referência}`; a carga leva as aprovações (96 bytes cada: chave + assinatura). A conta recebe o valor e `emitido` sobe. **Cada referência de depósito vale uma vez.**
3. **Uso (tipo 1):** transferência comum, com taxa; a oferta **não muda**.
4. **Resgate (tipo 3):** o **titular** envia a transação e os emissores assinam `brdu/issuance/burn`; o saldo cai e `queimado` sobe.
5. **Baixa do lastro:** depois de pagar os reais, os emissores atestam a reserva nova.
6. **Cobertura:** `reserva − (emitido − queimado)`: *coberta* (sobra), *exata* ou *abaixo* (faltam reais). **Só informação.**

## O que é verificado e por quem
- **O ledger**, em toda transação: quantas aprovações **válidas e de emissores distintos** (o mesmo emissor duas vezes conta uma; quem não é emissor é ignorado; a aprovação só vale para o **documento exato**: outro valor, referência, rede, conta ou tipo invalida); valor diferente de zero; referência única e não vazia (até 64 bytes); saldo; estouro de 64 bits.
- **Mint e burn têm referências separadas:** a mesma referência pode existir uma vez como mint e uma como burn.

## O que pode dar errado
- **Reserva abaixo da oferta:** a rede **mostra** "abaixo", mas não impede nada. A confiança nos emissores é o risco de qualquer stablecoin.
- **Atestado falso:** a rede só prova que M emissores **assinaram**, não que os reais existem.
- **Emissores em conluio:** M deles criam moeda sem lastro. A mitigação é transparência (atestados públicos, auditoria fora da cadeia) e o limiar M.
- **Sem congelamento nem recuperação:** fundos roubados ou enviados por engano não voltam.

## Onde está no código
`crates/brdu-ledger/src/coin.rs` (cargas, documentos e regras), `state.rs` (oferta, referências, atestado, `coverage`).

## Testes que cobrem
`tests/coin.rs` (18): mint com 2 de 3, menos de M, mesmo emissor repetido, não emissor, documento exato (5 variantes), referência única, zero, vazia, grande e estouro (que desfaz o nonce); burn só do dono, com aprovação, maior que o saldo, referências separadas; atestado (sequência, aprovações, replay) e cobertura; transferência, taxa mínima exata, replay, nonce futuro, rede errada, saldo para valor + taxa, cargas malformadas. `tests/invariants.rs` mistura tudo ao acaso.

## Limites honestos
- **Emissores de teste:** conjunto e limiar fixos na gênese (M2); a troca por mais de 2/3 chega com a rotação (M4). A governança real é PENDENTE (ADR-0010).
- Não há banco nem Pix: os "reais" são só o número do atestado.
