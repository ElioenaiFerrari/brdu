#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{CryptoError, PublicKey, SecretKey, Signature, hex, sign::verify_all};
use proptest::prelude::*;

fn key(h: &str) -> [u8; 32] {
    hex::decode_array::<32>(h).unwrap()
}

/// RFC 8032, section 7.1: official Ed25519 test vectors.
#[test]
fn rfc8032_test_1_empty_message() {
    let sk = SecretKey::from_bytes(key(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    ));
    assert_eq!(
        sk.public().to_hex(),
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    let sig = sk.sign(b"");
    assert_eq!(
        hex::encode(sig.as_bytes()),
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );
    assert!(sk.public().verify(b"", &sig).is_ok());
}

#[test]
fn rfc8032_test_2_one_byte() {
    let sk = SecretKey::from_bytes(key(
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
    ));
    assert_eq!(
        sk.public().to_hex(),
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c"
    );
    let sig = sk.sign(&[0x72]);
    assert_eq!(
        hex::encode(sig.as_bytes()),
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"
    );
}

#[test]
fn private_key_does_not_appear_in_debug() {
    let sk = SecretKey::from_bytes([7u8; 32]);
    let shown = format!("{sk:?}");
    assert!(!shown.contains("07070707"), "leaked the key: {shown}");
    assert!(shown.contains("redacted"));
}

#[test]
fn tagged_signature_is_only_valid_with_the_same_tag() {
    let sk = SecretKey::from_bytes([1u8; 32]);
    let sig = sk.sign_tagged("brdu/tx", b"dados");
    assert!(sk.public().verify_tagged("brdu/tx", b"dados", &sig).is_ok());
    assert_eq!(
        sk.public().verify_tagged("brdu/block", b"dados", &sig),
        Err(CryptoError::BadSignature)
    );
}

#[test]
fn invalid_public_key_is_rejected() {
    // y = 2 is not a point on the curve (no square root): it does not decode
    let mut bad = [0u8; 32];
    bad[0] = 2;
    let pk = PublicKey(bad);
    let sig = Signature([0u8; 64]);
    assert!(pk.verify(b"x", &sig).is_err());
}

#[test]
fn batch_verification_requires_all_valid() {
    let a = SecretKey::from_bytes([1u8; 32]);
    let b = SecretKey::from_bytes([2u8; 32]);
    let (pa, pb) = (a.public(), b.public());
    let (sa, sb) = (a.sign(b"one"), b.sign(b"two"));
    assert!(verify_all(&[(&pa, b"one", &sa), (&pb, b"two", &sb)], true).is_ok());
    // one bad among good ones: the whole batch fails
    assert!(verify_all(&[(&pa, b"one", &sa), (&pb, b"swapped", &sb)], true).is_err());
    assert!(verify_all(&[], true).is_ok());
}

proptest! {
    #[test]
    fn signing_and_verifying_always_round_trips(seed in any::<[u8; 32]>(), msg in proptest::collection::vec(any::<u8>(), 0..300)) {
        let sk = SecretKey::from_bytes(seed);
        let sig = sk.sign(&msg);
        prop_assert!(sk.public().verify(&msg, &sig).is_ok());
    }

    #[test]
    fn other_message_or_other_key_never_verifies(
        seed in any::<[u8; 32]>(), other in any::<[u8; 32]>(),
        msg in proptest::collection::vec(any::<u8>(), 1..100), flip in 0usize..100
    ) {
        let sk = SecretKey::from_bytes(seed);
        let sig = sk.sign(&msg);
        let mut tampered = msg.clone();
        let i = flip % tampered.len();
        tampered[i] ^= 1;
        prop_assert!(sk.public().verify(&tampered, &sig).is_err());
        prop_assume!(other != seed);
        prop_assert!(SecretKey::from_bytes(other).public().verify(&msg, &sig).is_err());
    }

    #[test]
    fn a_flipped_bit_in_the_signature_never_verifies(seed in any::<[u8; 32]>(), bit in 0usize..512) {
        let sk = SecretKey::from_bytes(seed);
        let mut sig = sk.sign(b"message").0;
        sig[bit / 8] ^= 1 << (bit % 8);
        prop_assert!(sk.public().verify(b"message", &Signature(sig)).is_err());
    }
}
