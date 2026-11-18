# Fluxo 01 — Da frase ao endereço

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como uma frase de 24 palavras (mais a passphrase opcional, a "25ª palavra") vira a chave que assina e o endereço `brdu1…` que recebe BRDU. Tudo é determinístico: quem tem a frase e a passphrase recupera a mesma carteira em qualquer lugar, e o código em Rust reproduz **bit a bit** os vetores do BRDC.

## Quem participa
- **Usuário:** guarda a frase e a passphrase.
- **Carteira (M8):** o backend Rust do app Tauri deriva e assina; a chave nunca vai ao webview.
- **`brdu-wallet` e `brdu-crypto`:** a lógica, usada pela carteira, pelo nó e pelas ferramentas de teste.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
1. A frase é validada (palavras da lista de 2048, 12 ou 24 palavras, checksum SHA-256). Maiúsculas e espaços sobrando são perdoados.
2. Semente = `PBKDF2-HMAC-SHA512(frase, "mnemonic" + passphrase, 2048, 64 bytes)`. A passphrase é só ASCII imprimível, até 64 caracteres.
3. Chave mestra = `HMAC-SHA512("ed25519 seed", semente)` (SLIP-10): 32 bytes de chave e 32 de código de cadeia.
4. Filhas **endurecidas** pelo caminho `m/44'/1112687701'/conta'/0'/índice'` (`1112687701` é "BRDU" em ASCII e não está registrado no SLIP-44): `HMAC-SHA512(código_de_cadeia, 0x00 ‖ chave ‖ índice)`.
5. Os 32 bytes da chave filha são a semente da chave privada Ed25519; a chave pública tem 32 bytes.
6. Endereço = `bech32m("brdu", SHA3-256(tag "brdu/addr", chave pública))`.

## O que é verificado e por quem
- **A frase:** o checksum detecta palavra trocada ou faltando (`Phrase::parse`).
- **O endereço:** o checksum bech32m detecta erro de digitação; **todo erro de um caractere** é detectado (propriedade testada).
- **A compatibilidade:** o BIP-39 com 3 passphrases, o SLIP-10 (vetor oficial) e o hash etiquetado batem com os vetores do BRDC.
- **A passphrase não é verificável:** qualquer uma gera uma carteira válida.

## O que pode dar errado
- Passphrase esquecida ou digitada diferente: abre **outra carteira, vazia**, sem erro. Por isso a importação mostra o endereço antes de gravar (M8).
- Frase fotografada ou guardada em nuvem: quem a vê esvazia a carteira.
- Caminho não endurecido: o Ed25519 não tem derivação pública, e o código recusa.

## Onde está no código
`crates/brdu-wallet/src/lib.rs` (`Phrase`, `Wallet`, `Account`, `slip10_derive`); `crates/brdu-crypto/src/{hash,sign,address,bech32m}.rs`.

## Testes que cobrem
`crates/brdu-wallet/tests/{vectors,wallet}.rs` (SLIP-10 oficial, 3 passphrases do BRDC, frases inválidas, passphrase, caminhos, `Debug` redigido, propriedades) e `crates/brdu-crypto/tests/{vectors,sign,address}.rs` (RFC 8032, endereços do BRDC, todo erro de um caractere detectado).

## Limites honestos
- Implementações sobre bibliotecas RustCrypto, **sem auditoria**.
- O `coin_type` não está registrado no SLIP-44.
- Em Rust os segredos são zerados ao sair da memória (`zeroize`); isso reduz, mas não elimina, a exposição (paginação, cópias do compilador).
