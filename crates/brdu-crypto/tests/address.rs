#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Address, AddressError, SecretKey, bech32m};
use proptest::prelude::*;

fn addr(seed: u8) -> Address {
    Address::from_public_key(&SecretKey::from_bytes([seed; 32]).public())
}

#[test]
fn starts_with_brdu1_and_round_trips() {
    let a = addr(1);
    let text = a.encode();
    assert!(text.starts_with("brdu1"), "{text}");
    assert_eq!(Address::decode(&text), Ok(a));
    assert_eq!(a.to_string(), text);
}

#[test]
fn address_matches_the_key_that_produced_it_and_no_other() {
    let (a, b) = (
        SecretKey::from_bytes([1; 32]).public(),
        SecretKey::from_bytes([2; 32]).public(),
    );
    let address = Address::from_public_key(&a);
    assert!(address.matches(&a));
    assert!(!address.matches(&b));
}

#[test]
fn prefix_of_another_currency_is_rejected() {
    let hash = *addr(1).hash();
    let brdc = bech32m::encode("brdc", hash.as_bytes()).unwrap();
    assert_eq!(Address::decode(&brdc), Err(AddressError::WrongPrefix));
}

#[test]
fn wrong_size_is_rejected() {
    let short = bech32m::encode("brdu", &[1u8; 20]).unwrap();
    assert_eq!(Address::decode(&short), Err(AddressError::BadSize));
}

#[test]
fn garbage_is_rejected() {
    for bad in ["", "brdu1", "brdu1qqqq", "not an address", "BRDU1abc"] {
        assert!(Address::decode(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn mixed_upper_and_lower_case_is_rejected() {
    let text = addr(3).encode();
    let mut chars: Vec<char> = text.chars().collect();
    let i = chars.iter().position(|c| c.is_ascii_alphabetic()).unwrap();
    chars[i] = chars[i].to_ascii_uppercase();
    let mixed: String = chars.into_iter().collect();
    assert!(Address::decode(&mixed).is_err());
}

proptest! {
    // bech32m detects EVERY single-character substitution (a guarantee of the code, not luck)
    #[test]
    fn single_character_typo_is_always_detected(seed in any::<u8>(), pos in 0usize..60, repl in 0usize..32) {
        const ALPHABET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
        let text = addr(seed).encode();
        let body_start = "brdu1".len();
        let i = body_start + pos % (text.len() - body_start);
        let mut bytes = text.clone().into_bytes();
        let new = ALPHABET[repl % 32];
        prop_assume!(bytes[i] != new);
        bytes[i] = new;
        let broken = String::from_utf8(bytes).unwrap();
        prop_assert!(Address::decode(&broken).is_err(), "did not detect: {broken}");
    }

    #[test]
    fn any_hash_becomes_an_address_and_back(bytes in any::<[u8; 32]>()) {
        let a = Address(brdu_crypto::Hash32(bytes));
        prop_assert_eq!(Address::decode(&a.encode()), Ok(a));
    }
}
