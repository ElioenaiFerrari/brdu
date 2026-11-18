#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_bft::{CertError, Certificate, Proposal, Vote, VoteType, quorum};
use brdu_crypto::{Hash32, PublicKey, SecretKey};
use proptest::prelude::*;

const CHAIN: &str = "brdu-types";

fn ks() -> Vec<SecretKey> {
    (1..=4u8).map(|i| SecretKey::from_bytes([i; 32])).collect()
}

fn pubs(k: &[SecretKey]) -> Vec<PublicKey> {
    k.iter().map(SecretKey::public).collect()
}

fn cert(k: &[SecretKey], n: usize) -> Certificate {
    let h = Hash32([5; 32]);
    Certificate {
        height: 3,
        round: 1,
        block_hash: h,
        votes: k
            .iter()
            .take(n)
            .map(|s| Vote::sign(CHAIN, VoteType::Precommit, 3, 1, Some(h), s))
            .collect(),
    }
}

#[test]
fn quorum_is_more_than_two_thirds() {
    assert!(!quorum(2, 4) && quorum(3, 4));
    assert!(!quorum(2, 3) && quorum(3, 3));
    assert!(!quorum(4, 6) && quorum(5, 6));
    assert!(quorum(1, 1) && !quorum(0, 1));
}

#[test]
fn certificate_with_3_of_4_checks_out_and_with_2_does_not() {
    let k = ks();
    assert_eq!(cert(&k, 3).verify(CHAIN, &pubs(&k)), Ok(()));
    assert_eq!(
        cert(&k, 2).verify(CHAIN, &pubs(&k)),
        Err(CertError::NoQuorum)
    );
    assert_eq!(cert(&k, 3).signers().len(), 3);
}

#[test]
fn certificate_rejects_every_tampering() {
    let k = ks();
    let v = pubs(&k);
    let mut c = cert(&k, 4);
    c.votes.push(c.votes[0].clone());
    assert_eq!(c.verify(CHAIN, &v), Err(CertError::DuplicateVoter));

    let mut c = cert(&k, 4);
    c.round = 9;
    assert_eq!(c.verify(CHAIN, &v), Err(CertError::MismatchedVotes));

    let mut c = cert(&k, 4);
    c.votes[0] = Vote::sign(CHAIN, VoteType::Prevote, 3, 1, Some(c.block_hash), &k[0]);
    assert_eq!(c.verify(CHAIN, &v), Err(CertError::MismatchedVotes));

    let mut c = cert(&k, 4);
    c.votes[0] = Vote::sign(CHAIN, VoteType::Precommit, 3, 1, None, &k[0]);
    assert_eq!(c.verify(CHAIN, &v), Err(CertError::DifferentBlock));

    let mut c = cert(&k, 4);
    c.votes[0].signature.0[3] ^= 1;
    assert_eq!(c.verify(CHAIN, &v), Err(CertError::BadSignature));

    let c = cert(&k, 4);
    assert_eq!(
        c.verify(CHAIN, &v[..3]),
        Err(CertError::UnknownVoter),
        "the 4th is not in the set"
    );
    assert_eq!(
        c.verify("other-network", &v),
        Err(CertError::BadSignature),
        "valid only on its own network"
    );
}

#[test]
fn vote_and_proposal_round_trip_and_the_signature_covers_everything() {
    let k = ks();
    let v = Vote::sign(
        CHAIN,
        VoteType::Precommit,
        5,
        2,
        Some(Hash32([1; 32])),
        &k[0],
    );
    assert_eq!(Vote::decode(&v.encode()).unwrap(), v);
    let nil = Vote::sign(CHAIN, VoteType::Prevote, 5, 2, None, &k[0]);
    assert_eq!(Vote::decode(&nil.encode()).unwrap(), nil);
    assert!(v.valid(CHAIN) && !v.valid("x"));
    let mut t = v.clone();
    t.round += 1;
    assert!(!t.valid(CHAIN));
    let mut t = v.clone();
    t.vote_type = VoteType::Prevote;
    assert!(!t.valid(CHAIN));

    for vr in [-1i64, 0, 4] {
        let p = Proposal::sign(CHAIN, 5, 6, vr, vec![1, 2, 3], Hash32([2; 32]), &k[1]);
        assert_eq!(Proposal::decode(&p.encode(), 100).unwrap(), p);
        assert!(p.valid(CHAIN));
        assert!(
            Proposal::decode(&p.encode(), 2).is_err(),
            "block above the limit"
        );
    }
    let c = cert(&k, 3);
    assert_eq!(Certificate::decode(&c.encode()).unwrap(), c);
}

proptest! {
    #[test]
    fn garbage_never_panics(data in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = Vote::decode(&data);
        let _ = Proposal::decode(&data, 1000);
        let _ = Certificate::decode(&data);
    }

    #[test]
    fn flipping_a_vote_bit_invalidates_it_or_it_fails_to_decode(i in 0usize..200) {
        let k = ks();
        let v = Vote::sign(CHAIN, VoteType::Precommit, 5, 2, Some(Hash32([1; 32])), &k[0]);
        let mut b = v.encode();
        let at = i % b.len();
        b[at] ^= 1 << (i % 8);
        if let Ok(d) = Vote::decode(&b) {
            prop_assert!(d == v || !d.valid(CHAIN) || d.voter != v.voter);
        }
    }
}
