//! Vetores: o padrão SLIP-10 para Ed25519 e os do BRDC (BIP-39 com passphrase → SLIP-10 → chave pública).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brdu_crypto::hex;
use brdu_wallet::{Phrase, slip10_derive};
use serde_json::Value;

fn load() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vectors/brdc/crypto.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Vetor 1 do SLIP-10 para ed25519 (do próprio documento do SLIP-10).
#[test]
fn slip10_vetor_oficial_1() {
    let seed = hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
    let cases = [
        (
            "m",
            "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7",
            "90046a93de5380a72b5e45010748567d5ea02bbf6522f979e05c0d8d8ca9fffb",
        ),
        (
            "m/0'",
            "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3",
            "8b59aa11380b624e81507a27fedda59fea6d0b779a778918a2fd3590e16e9c69",
        ),
        (
            "m/0'/1'",
            "b1d0bad404bf35da785a64ca1ac54b2617211d2777696fbffaf208f746ae84f2",
            "a320425f77d1b5c2505a6b1b27382b37368ee640e3557c315416801243552f14",
        ),
    ];
    for (path, key, chain) in cases {
        let n = slip10_derive(&seed, path).unwrap();
        assert_eq!(hex::encode(n.key.as_slice()), key, "chave de {path}");
        assert_eq!(
            hex::encode(n.chain_code.as_slice()),
            chain,
            "código de cadeia de {path}"
        );
    }
}

#[test]
fn slip10_do_brdc_bate() {
    let v = &load()["slip10"];
    let seed = hex::decode(v["seed"].as_str().unwrap()).unwrap();
    let n = slip10_derive(&seed, v["path"].as_str().unwrap()).unwrap();
    assert_eq!(hex::encode(n.key.as_slice()), v["key"].as_str().unwrap());
    assert_eq!(
        hex::encode(n.chain_code.as_slice()),
        v["chain_code"].as_str().unwrap()
    );
}

/// Mesma frase, três passphrases (inclusive o clássico "TREZOR" do BIP-39): as chaves públicas do BRDC.
/// O caminho do BRDC usa o tipo de moeda do BRDC (`1112687683`); o do BRDU usa o dele, então aqui o caminho
/// vem do vetor.
#[test]
fn bip39_e_slip10_do_brdc_batem_com_tres_passphrases() {
    let v = load();
    let phrase = Phrase::parse(v["mnemonic"].as_str().unwrap()).unwrap();
    let accounts = v["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 3);
    for a in accounts {
        let w = phrase.wallet(a["passphrase"].as_str().unwrap()).unwrap();
        let acc = w.derive(a["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            acc.public_key().to_hex(),
            a["public_key"].as_str().unwrap(),
            "passphrase {:?}",
            a["passphrase"]
        );
    }
}
