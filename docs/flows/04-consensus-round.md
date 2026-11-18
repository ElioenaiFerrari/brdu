# Fluxo 04 — Uma rodada de consenso BFT

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como 4 validadores concordam em **um** bloco por altura, mesmo com um deles caído, mudo ou mentindo. Cada altura tem rodadas: proposta → prevote → precommit. Mais de 2/3 de precommits no mesmo bloco o tornam **final**, com um **certificado** que qualquer um confere sozinho.

## Quem participa
- **Proponente da rodada:** `validators[(altura + rodada) % n]`; monta e assina a proposta.
- **Validadores:** votam; guardam o bloqueio; assinam um diário antes de transmitir.
- **Quem roda o motor** (nó ou simulador): entrega mensagens, dispara prazos, executa as ações.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
1. `start`: rodada 0; se sou o proponente, pede bloco (`NeedBlock`) e **agenda o prazo de proposta** (também para mim).
2. **Proposta** válida (assinatura, proponente certo, hash confere) → prevote no bloco se a validação do ledger passar e o bloqueio permitir; senão prevote **nulo**.
3. Mais de 2/3 de prevotes no mesmo bloco → **trava** nele e dá precommit. Mais de 2/3 de prevotes nulos → precommit nulo.
4. Mais de 2/3 de precommits no mesmo bloco → `Decide(bloco, certificado)`.
5. Prazo estourado sem progresso → prevote/precommit nulo e, ao fim, **próxima rodada** com outro proponente. Mais de 1/3 já numa rodada à frente → pula para ela.
6. **Queda e volta:** `restore` reconstrói rodada, passo e bloqueio do diário; `known_messages` reenvia o estado a quem reconecta.

## O que é verificado e por quem
Cada validador, sozinho: assinatura, rede, altura, pertencer ao conjunto, proponente certo, hash do bloco. **Certificado** (`Certificate::verify`): mesma altura, rodada e bloco; só precommits; votantes distintos e conhecidos; assinaturas válidas; mais de 2/3.

## O que pode dar errado
- Mais de 1/3 parado: **não há bloco final** (vivacidade), mas **nunca dois** (segurança): testado.
- Mais de 1/3 mentindo (bizantinos): fora da garantia.
- Equivocação é **detectada** (evidência), mas a penalidade não existe sem stake.

## Onde está no código
`crates/brdu-bft/src/{types,engine}.rs`.

## Testes que cobrem
`tests/types.rs` (6: quórum, certificado com cada adulteração, codec, lixo sem pânico, bit trocado) e `tests/sim.rs` (12): 4 honestos na rodada 0; 1 caído (4 posições); proponente mudo; 2 caídos (não decide, não diverge); partição 2 a 2 trava e, curada com reenvio, decide; evidência de equivocação; mensagens inválidas ignoradas; bloco inválido → prevote nulo; proponente errado; restauração do diário; **propriedades** (até f falhas: seguro e vivo; mais que f: nunca diverge), 1.500 casos sem falha.

## Limites honestos
Poder igual, sem stake; sem rotação de validadores ainda (M4); simulador sem relógio real (os prazos são disparados quando nada mais anda).
