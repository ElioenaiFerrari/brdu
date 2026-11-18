# Hash com prefixo de domínio, assinatura e endereço

## Resumo
Tudo na rede é **hash** (impressão digital de dados), **assinatura** (prova de quem autorizou) e **endereço** (nome público derivado da chave). O BRDU usa SHA3-256, Ed25519 e bech32m.

## Hash com prefixo de domínio
Um hash de `X` em um contexto não pode valer em outro. Por isso cada uso tem um **prefixo** (`brdu/tx`, `brdu/cred/issue`, `brdu/asset/veto`…):

```rust
use brdu_crypto::tagged;

let a = tagged("brdu/cred/issue", &[b"dados"]);
let b = tagged("brdu/asset/veto", &[b"dados"]);
assert_ne!(a, b); // mesmos dados, usos diferentes, hashes diferentes
```

É por isso que uma aprovação de órgão para "emitir" nunca vale como "vetar": o documento assinado é outro. Os vetores do BRDC são reproduzidos trocando só o prefixo (`brdc/…` ↔ `brdu/…`).

## Assinatura
Ed25519 **estrito** (recusa assinaturas maleáveis) e com verificação **em lote** para blocos cheios:

```rust
use brdu_crypto::SecretKey;

let key = SecretKey::from_bytes([7u8; 32]); // só em teste
let sig = key.sign_tagged("brdu/exemplo", b"mensagem");
assert!(key.public().verify_tagged("brdu/exemplo", b"mensagem", &sig).is_ok());
assert!(key.public().verify_tagged("brdu/outro", b"mensagem", &sig).is_err());
```

## Endereço
O endereço é o hash da chave pública em **bech32m** (`brdu1…`): tem prefixo legível e checksum que detecta erro de digitação.

```rust
use brdu_crypto::Address;
let addr = Address::from_public_key(&key.public());
println!("{}", addr.encode()); // brdu1…
```

## O que NÃO garante
- Assinatura prova que **a chave** autorizou, não que a pessoa certa a usou.
- O endereço não é anônimo: é um pseudônimo público.

## Onde está no código
`crates/brdu-crypto/src/{hash,sign,address,bech32m}.rs`.
