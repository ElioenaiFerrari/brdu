#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brdu_vault::{KdfParams, MIN_PASSWORD_LEN, Vault, VaultError};
use proptest::prelude::*;

/// Cheap parameters so tests run fast (the production default is 64 MiB).
const FAST: KdfParams = KdfParams {
    m_kib: 8,
    t: 1,
    p: 1,
};
const ADDR: &str = "brdu1testaddress";
const PHRASE: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn seal(pw: &str) -> Vault {
    Vault::seal_with(PHRASE, pw, ADDR, FAST).unwrap()
}

#[test]
fn seal_then_open_returns_the_phrase() {
    let v = seal("correct horse");
    assert_eq!(v.open("correct horse").unwrap().as_str(), PHRASE);
}

#[test]
fn wrong_password_is_refused_without_leaking_anything() {
    let v = seal("correct horse");
    assert_eq!(v.open("incorrect horse"), Err(VaultError::Decrypt));
    assert_eq!(v.open(""), Err(VaultError::Decrypt));
}

#[test]
fn short_passwords_are_refused_when_sealing() {
    assert_eq!(
        Vault::seal_with(PHRASE, &"x".repeat(MIN_PASSWORD_LEN - 1), ADDR, FAST).err(),
        Some(VaultError::WeakPassword)
    );
    assert!(Vault::seal_with(PHRASE, &"x".repeat(MIN_PASSWORD_LEN), ADDR, FAST).is_ok());
}

#[test]
fn the_ciphertext_does_not_contain_the_phrase_and_each_seal_is_different() {
    let a = seal("correct horse");
    let b = seal("correct horse");
    assert!(!a.to_json().contains("abandon"));
    assert_ne!(
        a.ciphertext, b.ciphertext,
        "fresh salt and nonce every time"
    );
    assert_ne!(a.salt, b.salt);
    assert_ne!(a.nonce, b.nonce);
}

#[test]
fn tampering_with_any_field_breaks_authentication() {
    let base = seal("correct horse");
    let mut cases: Vec<Vault> = Vec::new();
    let mut v = base.clone();
    v.address = "brdu1someoneelse".into();
    cases.push(v); // the address is authenticated: it cannot be swapped to mislead the UI
    let mut v = base.clone();
    v.params.t += 1;
    cases.push(v);
    let mut v = base.clone();
    v.params.m_kib += 8;
    cases.push(v);
    let mut v = base.clone();
    v.salt = "00".repeat(16);
    cases.push(v);
    let mut v = base.clone();
    let mut ct = hex_bytes(&v.ciphertext);
    ct[0] ^= 1;
    v.ciphertext = hex(&ct);
    cases.push(v);
    let mut v = base.clone();
    let mut n = hex_bytes(&v.nonce);
    n[0] ^= 1;
    v.nonce = hex(&n);
    cases.push(v);
    for v in cases {
        assert_eq!(v.open("correct horse"), Err(VaultError::Decrypt), "{v:?}");
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn json_round_trip_and_garbage_is_malformed() {
    let v = seal("correct horse");
    let back = Vault::from_json(&v.to_json()).unwrap();
    assert_eq!(back, v);
    assert_eq!(back.open("correct horse").unwrap().as_str(), PHRASE);
    assert!(matches!(
        Vault::from_json("not json"),
        Err(VaultError::Malformed(_))
    ));
    assert!(matches!(
        Vault::from_json("{}"),
        Err(VaultError::Malformed(_))
    ));
}

#[test]
fn unknown_version_kdf_and_hostile_parameters_are_refused_before_hashing() {
    let base = seal("correct horse");
    let mut v = base.clone();
    v.version = 2;
    assert_eq!(v.open("correct horse"), Err(VaultError::Version(2)));
    let mut v = base.clone();
    v.kdf = "scrypt".into();
    assert!(matches!(
        v.open("correct horse"),
        Err(VaultError::Malformed(_))
    ));
    // a hostile file asking for gigabytes of memory is refused up front
    let mut v = base.clone();
    v.params.m_kib = u32::MAX;
    assert!(matches!(
        v.open("correct horse"),
        Err(VaultError::Malformed(_))
    ));
    let mut v = base.clone();
    v.params.t = 1_000_000;
    assert!(matches!(
        v.open("correct horse"),
        Err(VaultError::Malformed(_))
    ));
    let mut v = base;
    v.salt = "00".into();
    assert!(matches!(
        v.open("correct horse"),
        Err(VaultError::Malformed(_))
    ));
}

#[test]
fn unicode_passwords_and_empty_secrets_work() {
    let v = Vault::seal_with("", "pässwörd-日本語", ADDR, FAST).unwrap();
    assert_eq!(v.open("pässwörd-日本語").unwrap().as_str(), "");
    assert_eq!(v.open("passwörd-日本語"), Err(VaultError::Decrypt));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn any_secret_and_password_round_trip(secret in ".{0,200}", pw in ".{8,40}") {
        let v = Vault::seal_with(&secret, &pw, ADDR, FAST).unwrap();
        let opened = v.open(&pw).unwrap();
        prop_assert_eq!(opened.as_str(), secret.as_str());
    }

    #[test]
    fn flipping_any_ciphertext_byte_never_opens(i in 0usize..200) {
        let v = seal("correct horse");
        let mut ct = hex_bytes(&v.ciphertext);
        let at = i % ct.len();
        ct[at] ^= 0x01;
        let mut t = v;
        t.ciphertext = hex(&ct);
        prop_assert_eq!(t.open("correct horse"), Err(VaultError::Decrypt));
    }
}

#[test]
fn the_default_cost_is_the_documented_one() {
    let p = KdfParams::default();
    assert_eq!((p.m_kib, p.t, p.p), (64 * 1024, 3, 1));
}
