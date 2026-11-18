# Consenso BFT e certificado de commit

## Resumo
Os validadores são **autorizados** e concordam sobre cada bloco por um protocolo no estilo **Tendermint**: proposta → *prevote* → *precommit*, em rodadas, com proponente rotativo. Um bloco é **final** quando mais de **2/3** dos validadores assinam o commit. Não há bifurcação a desfazer.

- **Tolerância:** com 4 validadores, 1 pode falhar ou mentir; precisa de 3 assinaturas.
- **Certificado de commit:** o bloco anda junto com as assinaturas; qualquer réplica (inclusive o explorador) **reverifica** sem confiar no nó que o serviu.
- **Equivocação:** assinar dois votos conflitantes na mesma altura e rodada gera **evidência** pública (sem penalidade automática, pois não há stake).
- **Sem blocos vazios:** a rede espera haver transações.
- **Motor sem E/S:** `brdu-bft` é uma máquina pura (entra mensagem, saem mensagens e eventos), por isso é testável com um simulador determinístico.

## Mudar o conjunto de validadores
É uma transação (tipo 22) aprovada por mais de 2/3 do conjunto **em vigor**, com altura efetiva futura e uma troca pendente por vez ([ADR-0015](../adr/README.md)).

## O que NÃO garante
Mais de 1/3 de validadores maliciosos pode travar ou fraudar a rede. Quem escolhe os validadores é uma decisão humana de governança.

## Onde está no código
`crates/brdu-bft`, `crates/brdu-chain`; fluxos [`04-consensus-round.md`](../flows/04-consensus-round.md) e [`08-validator-rotation.md`](../flows/08-validator-rotation.md).
