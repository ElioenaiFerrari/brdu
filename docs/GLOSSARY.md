# Glossário

- **BRDU:** a moeda (1 BRDU = R$ 1) e o projeto. Unidade mínima: 0,000001 (1 milionésimo).
- **Unidade mínima / `Amount`:** inteiro de 64 bits; 1 BRDU = 1.000.000 unidades. Nunca ponto flutuante.
- **Endereço:** `brdu1…` em bech32m; carrega o **hash** da chave pública, não a chave.
- **Hash etiquetado:** `SHA3-256(tamanho da tag ‖ tag ‖ dados)`; a tag (`brdu/tx`, `brdu/block`…) impede um hash valer como outro.
- **Frase de recuperação:** 24 palavras BIP-39 (12 também é aceita ao importar).
- **Passphrase (25ª palavra):** segredo extra que muda a carteira inteira; qualquer passphrase gera uma carteira válida, e **não há como saber se está certa**.
- **SLIP-10:** derivação hierárquica para Ed25519, só com índices endurecidos (`'`).
- **Emissor:** quem faz *mint*/*burn* e atesta a reserva, em decisão M-de-N.
- **Mint / burn:** criar BRDU ao receber reais / destruir ao devolver reais.
- **Atestado de reserva:** declaração assinada por M emissores do total de reais em caixa.
- **Cobertura:** reserva atestada menos o que circula (exata, coberta ou abaixo). Só informação: a rede não bloqueia nada.
- **M-de-N:** M assinaturas distintas de N autorizados.
- **Validador / BFT:** nó que vota; o bloco é final com mais de 2/3 dos validadores.
- **Estrito (Ed25519):** verificação que recusa assinaturas não canônicas e chaves de ordem pequena.
