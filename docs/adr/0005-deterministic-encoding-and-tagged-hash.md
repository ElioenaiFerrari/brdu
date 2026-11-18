# ADR-0005 — Codificação determinística e hash etiquetado

**Status:** Aceito (dono do produto, 2026-11-18) · **Origem:** BRDC ADR-0011

## Contexto
O estado e os blocos têm de ser **idênticos byte a byte** em todos os nós, e a carteira precisa produzir exatamente os mesmos bytes para assinar.

## Decisão
- **Hash:** SHA3-256 com **prefixo de domínio**: `tagged(tag, dados) = SHA3-256(tamanho da tag (4 bytes) ‖ tag ‖ dados)`, com uma tag por uso (`brdu/tx`, `brdu/block`, `brdu/state`, `brdu/addr`…), para o hash de uma coisa nunca valer como o de outra.
- **Codificação:** formato próprio, simples e documentado: campos em ordem fixa, inteiros big-endian de tamanho fixo, bytes e textos com prefixo de tamanho de 4 bytes. **Sem Protobuf nem serialização dependente de implementação.**
- Todo campo variável tem **limite**, conferido **antes** de alocar; uma contagem maior que os bytes restantes é recusada.
- **Merkle** com domínios separados para folha e nó e divisão estilo RFC 6962 (sem duplicar a última folha).
- A assinatura cobre o `chain_id`, o tipo, o nonce e todos os campos.

## Alternativas
Protobuf (usado no Teko: bom ecossistema, mas a canonicidade exige cuidado extra); a escolha mantém os **mesmos bytes do BRDC**, o que permite usar os vetores dele como oráculo (com o prefixo de domínio parametrizado).

## Consequências
Um codificador e um decodificador pequenos, fáceis de revisar. Nenhum decodificador entra em pânico, seja qual for a entrada (propriedade testada).
