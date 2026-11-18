#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_wallet::{Phrase, Wallet, WalletError, slip10_derive};
use proptest::prelude::*;

const ABOUT: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn generates_24_valid_words_that_differ_each_time() {
    let (a, b) = (Phrase::generate().unwrap(), Phrase::generate().unwrap());
    assert_eq!(a.len(), 24);
    let joined = a.words().join(" ");
    assert!(
        Phrase::parse(&joined).is_ok(),
        "the generated phrase must pass the checksum"
    );
    assert_ne!(a.words(), b.words());
}

#[test]
fn zero_entropy_gives_the_known_phrase() {
    let p = Phrase::from_entropy(&[0u8; 32]).unwrap();
    let w = p.words();
    assert_eq!(w.len(), 24);
    assert!(w[..23].iter().all(|x| *x == "abandon"));
    assert_eq!(w[23], "art");
}

#[test]
fn passphrase_is_the_25th_word_another_passphrase_another_wallet() {
    let p = Phrase::parse(ABOUT).unwrap();
    let a = |pass: &str| {
        p.wallet(pass)
            .unwrap()
            .account(0, 0)
            .unwrap()
            .address()
            .encode()
    };
    assert_eq!(a(""), a(""), "deterministic");
    assert_ne!(a(""), a("secret"));
    assert_ne!(a("secret"), a("Secret"), "upper case counts");
    assert_ne!(a("secret"), a("secret "), "space counts");
}

#[test]
fn different_accounts_and_indices_give_different_addresses() {
    let w = Phrase::parse(ABOUT).unwrap().wallet("").unwrap();
    let all: Vec<_> = [(0, 0), (0, 1), (1, 0), (1, 1)]
        .iter()
        .map(|(a, i)| w.account(*a, *i).unwrap().address().encode())
        .collect();
    let unique: std::collections::HashSet<_> = all.iter().collect();
    assert_eq!(unique.len(), 4);
    assert!(all.iter().all(|a| a.starts_with("brdu1")));
}

#[test]
fn brdu_path_uses_the_brdu_coin_type() {
    assert_eq!(Wallet::path(0, 0), "m/44'/1112687701'/0'/0'/0'");
    assert_eq!(Wallet::path(2, 5), "m/44'/1112687701'/2'/0'/5'");
    assert_eq!(0x4252_4455u32, u32::from_be_bytes(*b"BRDU"));
}

#[test]
fn account_signs_and_address_matches_the_key() {
    let acc = Phrase::parse(ABOUT)
        .unwrap()
        .wallet("")
        .unwrap()
        .account(0, 0)
        .unwrap();
    let sig = acc.secret_key().sign_tagged("brdu/tx", b"pay");
    assert!(
        acc.public_key()
            .verify_tagged("brdu/tx", b"pay", &sig)
            .is_ok()
    );
    assert!(acc.address().matches(acc.public_key()));
}

#[test]
fn invalid_phrases_are_rejected() {
    let bad = [
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon", // checksum
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon", // 11
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", // 13
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zzzzzz", // not in the list
        "",
    ];
    for b in bad {
        assert_eq!(
            Phrase::parse(b).err(),
            Some(WalletError::InvalidPhrase),
            "{b:?}"
        );
    }
}

#[test]
fn upper_case_and_extra_spaces_are_forgiven() {
    let sloppy = format!("  {}  ", ABOUT.to_uppercase().replace(' ', "   "));
    let a = Phrase::parse(&sloppy)
        .unwrap()
        .wallet("")
        .unwrap()
        .account(0, 0)
        .unwrap();
    let b = Phrase::parse(ABOUT)
        .unwrap()
        .wallet("")
        .unwrap()
        .account(0, 0)
        .unwrap();
    assert_eq!(a.address(), b.address());
}

#[test]
fn invalid_passphrase_is_rejected() {
    let p = Phrase::parse(ABOUT).unwrap();
    for bad in ["pass世", "pass\n", "tab\t", &"x".repeat(65)] {
        assert_eq!(
            p.wallet(bad).err(),
            Some(WalletError::InvalidPassphrase),
            "{bad:?}"
        );
    }
    assert!(p.wallet(&"x".repeat(64)).is_ok());
}

#[test]
fn non_hardened_or_malformed_paths_are_rejected() {
    let seed = [7u8; 16];
    for bad in [
        "",
        "m/44/0'",
        "44'/0'",
        "x/0'",
        "m/0'/",
        "m//0'",
        "m/a'",
        "m/-1'",
        "m/2147483648'",
        "m/0'/1",
    ] {
        assert_eq!(
            slip10_derive(&seed, bad).err(),
            Some(WalletError::BadPath),
            "{bad:?}"
        );
    }
    assert!(slip10_derive(&seed, "m").is_ok());
    assert!(slip10_derive(&seed, "m/2147483647'").is_ok());
}

#[test]
fn no_secret_appears_in_debug() {
    let p = Phrase::parse(ABOUT).unwrap();
    let w = p.wallet("").unwrap();
    let acc = w.account(0, 0).unwrap();
    let node = w.master().unwrap();
    for shown in [
        format!("{p:?}"),
        format!("{w:?}"),
        format!("{acc:?}"),
        format!("{node:?}"),
    ] {
        assert!(!shown.contains("abandon"), "leaked word: {shown}");
        assert!(!shown.contains(&hex_of(&node.key)), "leaked key: {shown}");
    }
}

fn hex_of(k: &[u8; 32]) -> String {
    brdu_crypto::hex::encode(k)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn entropy_becomes_a_phrase_that_round_trips_and_derives_the_same_account(entropy in any::<[u8; 32]>()) {
        let p = Phrase::from_entropy(&entropy).unwrap();
        prop_assert_eq!(p.len(), 24);
        let again = Phrase::parse(&p.words().join(" ")).unwrap();
        let a = p.wallet("").unwrap().account(0, 0).unwrap();
        let b = again.wallet("").unwrap().account(0, 0).unwrap();
        prop_assert_eq!(a.address(), b.address());
    }

    #[test]
    fn different_passphrases_never_give_the_same_account(entropy in any::<[u8; 32]>(), p1 in "[a-zA-Z0-9]{0,12}", p2 in "[a-zA-Z0-9]{0,12}") {
        prop_assume!(p1 != p2);
        let p = Phrase::from_entropy(&entropy).unwrap();
        let a = p.wallet(&p1).unwrap().account(0, 0).unwrap();
        let b = p.wallet(&p2).unwrap().account(0, 0).unwrap();
        prop_assert_ne!(a.address(), b.address());
    }
}
