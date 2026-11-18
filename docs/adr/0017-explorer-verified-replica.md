# ADR-0017 — Explorador com réplica verificada (não confia no nó)

**Status:** Aceito (2026-11-18) · **Origem:** BRDC (explorador LiveView) e `explorer-server` do Teko (ADR-0031)

## Decisão
- O explorador é um processo próprio (`brdu-explorer`): mantém uma **réplica verificada** da cadeia (`brdu-chain::Chain`) e a serve como **página web** e **API JSON somente leitura**. Nenhum segredo, nenhuma escrita (`POST` dá 405).
- Ele **não confia no nó** que o alimenta: cada bloco final (`GET /blocks/raw`) passa por toda a verificação da réplica (certificado de mais de 2/3 do conjunto da época, proponente da rodada, signatários, execução e raiz). Bloco adulterado, fora de ordem ou de outra rede é **recusado**, contado em `rejected_blocks` e **denunciado na tela**.
- **A gênese é fixada por quem roda** (`--genesis`): o nó não escolhe a rede.
- Índices em memória: transação por id, conta (remetente **e** contraparte), bloco por hash. Busca por altura, hash de bloco, id de transação ou endereço (o checksum do endereço denuncia erro de digitação).
- **Interface:** HTML, CSS e JS puros (sem framework, sem fonte de terceiros), **CSP estrita** (`default-src 'none'; script-src 'self'; style-src 'self'`), DOM montado só com `textContent`. **Tema do BRDC** (fundo quase preto com brilho verde e azul, cartões arredondados, decimais apagados) e o efeito de letras "descriptografando" (`scramble`), com texto real para leitores de tela e respeito a "reduzir movimento".
- As **evidências de equivocação** vêm do nó (não são verificáveis aqui) e aparecem rotuladas como tal.

## Consequências e limites honestos
- A verificação é **no servidor do explorador**, não no navegador: quem abre a página confia no explorador que roda (local, por padrão). Verificar o certificado no navegador (WebCrypto Ed25519 + SHA3 em JS) fica planejado.
- Tudo em memória: ao reiniciar, ressincroniza do bloco 1 (a verificação de cada bloco custa CPU).
- Alimentado por **um** nó; se ele sair do conjunto o explorador fica parado (aponte para outro com `--node`).
