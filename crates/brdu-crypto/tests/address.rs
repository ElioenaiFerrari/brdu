#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Address, AddressError, SecretKey, bech32m};
use proptest::prelude::*;

fn addr(seed: u8) -> Address {
    Address::from_public_key(&SecretKey::from_bytes([seed; 32]).public())
}

#[test]
fn comeca_com_brdu1_e_volta_igual() {
    let a = addr(1);
    let text = a.encode();
    assert!(text.starts_with("brdu1"), "{text}");
    assert_eq!(Address::decode(&text), Ok(a));
    assert_eq!(a.to_string(), text);
}

#[test]
fn o_endereco_corresponde_a_chave_que_o_gerou_e_a_nenhuma_outra() {
    let (a, b) = (
        SecretKey::from_bytes([1; 32]).public(),
        SecretKey::from_bytes([2; 32]).public(),
    );
    let address = Address::from_public_key(&a);
    assert!(address.matches(&a));
    assert!(!address.matches(&b));
}

#[test]
fn prefixo_de_outra_moeda_e_recusado() {
    let hash = *addr(1).hash();
    let brdc = bech32m::encode("brdc", hash.as_bytes()).unwrap();
    assert_eq!(Address::decode(&brdc), Err(AddressError::WrongPrefix));
}

#[test]
fn tamanho_errado_e_recusado() {
    let curto = bech32m::encode("brdu", &[1u8; 20]).unwrap();
    assert_eq!(Address::decode(&curto), Err(AddressError::BadSize));
}

#[test]
fn lixo_e_recusado() {
    for ruim in ["", "brdu1", "brdu1qqqq", "nao e um endereco", "BRDU1abc"] {
        assert!(Address::decode(ruim).is_err(), "{ruim:?}");
    }
}

#[test]
fn maiusculas_e_minusculas_misturadas_sao_recusadas() {
    let text = addr(3).encode();
    let mut chars: Vec<char> = text.chars().collect();
    let i = chars.iter().position(|c| c.is_ascii_alphabetic()).unwrap();
    chars[i] = chars[i].to_ascii_uppercase();
    let mixed: String = chars.into_iter().collect();
    assert!(Address::decode(&mixed).is_err());
}

proptest! {
    // o bech32m detecta TODA troca de um único caractere (garantia do código, não sorte)
    #[test]
    fn erro_de_digitacao_em_um_caractere_e_sempre_detectado(seed in any::<u8>(), pos in 0usize..60, repl in 0usize..32) {
        const ALPHABET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
        let text = addr(seed).encode();
        let body_start = "brdu1".len();
        let i = body_start + pos % (text.len() - body_start);
        let mut bytes = text.clone().into_bytes();
        let new = ALPHABET[repl % 32];
        prop_assume!(bytes[i] != new);
        bytes[i] = new;
        let broken = String::from_utf8(bytes).unwrap();
        prop_assert!(Address::decode(&broken).is_err(), "não detectou: {broken}");
    }

    #[test]
    fn qualquer_hash_vira_endereco_e_volta(bytes in any::<[u8; 32]>()) {
        let a = Address(brdu_crypto::Hash32(bytes));
        prop_assert_eq!(Address::decode(&a.encode()), Ok(a));
    }
}
