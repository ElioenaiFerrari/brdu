#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_wallet::{Phrase, Wallet, WalletError, slip10_derive};
use proptest::prelude::*;

const ABOUT: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn gera_24_palavras_validas_e_diferentes_a_cada_vez() {
    let (a, b) = (Phrase::generate().unwrap(), Phrase::generate().unwrap());
    assert_eq!(a.len(), 24);
    let joined = a.words().join(" ");
    assert!(
        Phrase::parse(&joined).is_ok(),
        "a frase gerada tem de passar no checksum"
    );
    assert_ne!(a.words(), b.words());
}

#[test]
fn entropia_zero_da_a_frase_conhecida() {
    let p = Phrase::from_entropy(&[0u8; 32]).unwrap();
    let w = p.words();
    assert_eq!(w.len(), 24);
    assert!(w[..23].iter().all(|x| *x == "abandon"));
    assert_eq!(w[23], "art");
}

#[test]
fn a_passphrase_e_a_25a_palavra_outra_passphrase_outra_carteira() {
    let p = Phrase::parse(ABOUT).unwrap();
    let a = |pass: &str| {
        p.wallet(pass)
            .unwrap()
            .account(0, 0)
            .unwrap()
            .address()
            .encode()
    };
    assert_eq!(a(""), a(""), "determinística");
    assert_ne!(a(""), a("segredo"));
    assert_ne!(a("segredo"), a("Segredo"), "maiúscula conta");
    assert_ne!(a("segredo"), a("segredo "), "espaço conta");
}

#[test]
fn contas_e_indices_diferentes_dao_enderecos_diferentes() {
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
fn caminho_do_brdu_usa_a_moeda_brdu() {
    assert_eq!(Wallet::path(0, 0), "m/44'/1112687701'/0'/0'/0'");
    assert_eq!(Wallet::path(2, 5), "m/44'/1112687701'/2'/0'/5'");
    assert_eq!(0x4252_4455u32, u32::from_be_bytes(*b"BRDU"));
}

#[test]
fn a_conta_assina_e_o_endereco_confere_com_a_chave() {
    let acc = Phrase::parse(ABOUT)
        .unwrap()
        .wallet("")
        .unwrap()
        .account(0, 0)
        .unwrap();
    let sig = acc.secret_key().sign_tagged("brdu/tx", b"pagar");
    assert!(
        acc.public_key()
            .verify_tagged("brdu/tx", b"pagar", &sig)
            .is_ok()
    );
    assert!(acc.address().matches(acc.public_key()));
}

#[test]
fn frases_invalidas_sao_recusadas() {
    let bad = [
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon", // checksum
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon", // 11
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", // 13
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zzzzzz", // fora da lista
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
fn maiusculas_e_espacos_sobrando_sao_perdoados() {
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
fn passphrase_invalida_e_recusada() {
    let p = Phrase::parse(ABOUT).unwrap();
    for bad in ["açaí", "senha\n", "tab\t", &"x".repeat(65)] {
        assert_eq!(
            p.wallet(bad).err(),
            Some(WalletError::InvalidPassphrase),
            "{bad:?}"
        );
    }
    assert!(p.wallet(&"x".repeat(64)).is_ok());
}

#[test]
fn caminhos_nao_endurecidos_ou_malformados_sao_recusados() {
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
fn nenhum_segredo_aparece_no_debug() {
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
        assert!(!shown.contains("abandon"), "vazou palavra: {shown}");
        assert!(!shown.contains(&hex_of(&node.key)), "vazou chave: {shown}");
    }
}

fn hex_of(k: &[u8; 32]) -> String {
    brdu_crypto::hex::encode(k)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn entropia_vira_frase_que_volta_igual_e_deriva_a_mesma_conta(entropy in any::<[u8; 32]>()) {
        let p = Phrase::from_entropy(&entropy).unwrap();
        prop_assert_eq!(p.len(), 24);
        let again = Phrase::parse(&p.words().join(" ")).unwrap();
        let a = p.wallet("").unwrap().account(0, 0).unwrap();
        let b = again.wallet("").unwrap().account(0, 0).unwrap();
        prop_assert_eq!(a.address(), b.address());
    }

    #[test]
    fn passphrases_diferentes_nunca_dao_a_mesma_conta(entropy in any::<[u8; 32]>(), p1 in "[a-zA-Z0-9]{0,12}", p2 in "[a-zA-Z0-9]{0,12}") {
        prop_assume!(p1 != p2);
        let p = Phrase::from_entropy(&entropy).unwrap();
        let a = p.wallet(&p1).unwrap().account(0, 0).unwrap();
        let b = p.wallet(&p2).unwrap().account(0, 0).unwrap();
        prop_assert_ne!(a.address(), b.address());
    }
}
