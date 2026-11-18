# Carteira: frase BIP-39, passphrase e derivação

## Resumo
A conta nasce de uma **frase de 24 palavras** (BIP-39). Dela deriva-se, por **SLIP-10** (a variante do BIP-32 para Ed25519), uma chave por caminho. Uma **passphrase** opcional (a "25ª palavra") muda toda a árvore e **nunca é guardada**.

```rust
use brdu_wallet::Phrase;

let frase = Phrase::generate()?;                 // 24 palavras
let carteira = frase.wallet("minha passphrase")?; // a passphrase entra na semente
let conta = carteira.account(0, 0)?;             // conta 0, índice 0 (caminho SLIP-10)
# Ok::<(), brdu_wallet::WalletError>(())
```

- **Mesma frase + passphrase diferente = outra carteira** (e outro saldo). Esquecer a passphrase é perder o acesso; não há recuperação.
- **Cofre:** no disco, a frase fica cifrada (Argon2id + ChaCha20-Poly1305) em `vault.json`; a senha do cofre **não** é a passphrase.
- **Não custodial:** quem tem a frase tem o dinheiro; ninguém (nem a rede) recupera ou congela contas.

## O que NÃO garante
Não há proteção contra malware no computador que digita a frase ou a senha. No protótipo as redes de teste guardam chaves em texto puro.

## Onde está no código
`crates/brdu-wallet`, `crates/brdu-vault`, `crates/brdu-walletd`; fluxo em [`flows/11-wallet.md`](../flows/11-wallet.md).
