#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[macro_use]
mod common;

use brdu_crypto::{Address, Hash32};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Coverage, LedgerError, State, kind};
use common::*;

fn funded(env: &Env, amount: u64) -> State {
    let mut s = State::new();
    let t = mint(env, &s, &addr(&env.alice), amount, "dep-1");
    ok(env, &mut s, &t);
    s
}

// ---------------------------------------------------------------- mint

#[test]
fn mint_with_2_of_3_issuers_creates_balance_and_supply() {
    let env = Env::new();
    let s = funded(&env, 5_000_000);
    assert_eq!(s.balance(&addr(&env.alice)), 5_000_000);
    assert_eq!(
        (s.minted(), s.burned(), s.supply()),
        (5_000_000, 0, 5_000_000)
    );
}

#[test]
fn mint_with_fewer_than_m_approvals_is_rejected() {
    let env = Env::new();
    let mut s = State::new();
    for n in [0, 1] {
        let t = tx(
            &env.relayer,
            kind::MINT,
            0,
            0,
            env.mint_payload(&addr(&env.alice), 10, "r", n),
        );
        assert_eq!(
            err(&env, &mut s, &t),
            LedgerError::NotEnoughApprovals {
                valid: n,
                needed: 2
            }
        );
    }
    assert_eq!(s.supply(), 0);
    assert_eq!(
        s.nonce(&addr(&env.relayer)),
        0,
        "a rejected transaction does not consume the nonce"
    );
}

#[test]
fn the_same_issuer_twice_counts_only_once() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    let doc = coin::issuance_doc(false, CHAIN, &to, 10, "r");
    let one = Approval::new(&doc, &env.issuers[0]);
    let t = tx(
        &env.relayer,
        kind::MINT,
        0,
        0,
        coin::issuance_payload(&to, 10, "r", &[one, one, one]),
    );
    assert_eq!(
        err(&env, &mut s, &t),
        LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        }
    );
}

#[test]
fn approval_from_a_non_issuer_is_ignored() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    let doc = coin::issuance_doc(false, CHAIN, &to, 10, "r");
    let outsiders: Vec<_> = (50..53).map(|n| Approval::new(&doc, &key(n))).collect();
    let t = tx(
        &env.relayer,
        kind::MINT,
        0,
        0,
        coin::issuance_payload(&to, 10, "r", &outsiders),
    );
    assert_eq!(
        err(&env, &mut s, &t),
        LedgerError::NotEnoughApprovals {
            valid: 0,
            needed: 2
        }
    );
}

#[test]
fn approval_only_counts_for_the_exact_document() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    let approved = |doc: &Hash32| env.approvals(doc, 2);
    let variants = [
        coin::issuance_doc(false, CHAIN, &to, 11, "r"), // other amount
        coin::issuance_doc(false, CHAIN, &to, 10, "other"), // other reference
        coin::issuance_doc(false, "other-network", &to, 10, "r"), // other network
        coin::issuance_doc(false, CHAIN, &addr(&env.bob), 10, "r"), // other account
        coin::issuance_doc(true, CHAIN, &to, 10, "r"),  // BURN document used in a mint
    ];
    for doc in variants {
        let t = tx(
            &env.relayer,
            kind::MINT,
            0,
            0,
            coin::issuance_payload(&to, 10, "r", &approved(&doc)),
        );
        assert!(
            matches!(
                err(&env, &mut s, &t),
                LedgerError::NotEnoughApprovals { valid: 0, .. }
            ),
            "{doc}"
        );
    }
}

#[test]
fn deposit_reference_counts_once() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    let t = mint(&env, &s, &addr(&env.bob), 7, "dep-1"); // same reference, different amount and different account
    assert_eq!(err(&env, &mut s, &t), LedgerError::RefUsed);
    okm!(env, s, &mint(&env, &s, &addr(&env.bob), 7, "dep-2"));
}

#[test]
fn mint_zero_empty_or_large_reference_and_overflow_are_rejected() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 0, "z")),
        LedgerError::ZeroAmount
    );
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 5, "")),
        LedgerError::BadRef
    );
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 5, &"x".repeat(65))),
        LedgerError::Malformed
    );
    okm!(env, s, &mint(&env, &s, &to, u64::MAX - 1, "large"));
    let before = s.clone();
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 2, "overflow")),
        LedgerError::Overflow
    );
    assert_eq!(
        s, before,
        "the overflow undoes everything (including the nonce)"
    );
}

// ---------------------------------------------------------------- burn

#[test]
fn burn_only_by_the_owner_and_with_approval() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    // bob tries to burn alice's balance: the sender must be the holder
    let t = tx(
        &env.bob,
        kind::BURN,
        0,
        0,
        env.burn_payload(&addr(&env.alice), 10, "b", 2),
    );
    assert_eq!(err(&env, &mut s, &t), LedgerError::NotOwner);
    // without enough approvals
    let t = tx(
        &env.alice,
        kind::BURN,
        0,
        0,
        env.burn_payload(&addr(&env.alice), 10, "b", 1),
    );
    assert!(matches!(
        err(&env, &mut s, &t),
        LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        }
    ));
    // correct
    okm!(env, s, &burn(&env, &s, &env.alice, 400, "b"));
    assert_eq!(
        (s.balance(&addr(&env.alice)), s.burned(), s.supply()),
        (600, 400, 600)
    );
}

#[test]
fn burn_greater_than_balance_or_zero_is_rejected_and_mint_and_burn_references_are_separate() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    assert_eq!(
        errm!(env, s, &burn(&env, &s, &env.alice, 1_001, "b")),
        LedgerError::InsufficientFunds
    );
    assert_eq!(
        errm!(env, s, &burn(&env, &s, &env.alice, 0, "b")),
        LedgerError::ZeroAmount
    );
    // "dep-1" was already used as a MINT reference; as a BURN reference it is a different thing
    okm!(env, s, &burn(&env, &s, &env.alice, 1, "dep-1"));
    assert_eq!(
        errm!(env, s, &burn(&env, &s, &env.alice, 1, "dep-1")),
        LedgerError::RefUsed
    );
}

// ---------------------------------------------------------------- attestation and coverage

#[test]
fn attestation_requires_sequence_and_approvals_and_coverage_is_information_only() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    assert_eq!(s.coverage(), Coverage::None);
    assert_eq!(
        errm!(env, s, &attest(&env, &s, 2, 1_000, "bank")),
        LedgerError::BadSequence {
            expected: 1,
            got: 2
        }
    );
    let nonce = s.nonce(&addr(&env.relayer));
    let weak = tx(
        &env.relayer,
        kind::ATTESTATION,
        nonce,
        0,
        env.attest_payload(1, 1_000, "bank", 1),
    );
    assert!(matches!(
        err(&env, &mut s, &weak),
        LedgerError::NotEnoughApprovals { .. }
    ));
    okm!(env, s, &attest(&env, &s, 1, 1_000, "bank"));
    assert_eq!(
        s.attestation().map(|a| (a.seq, a.reserve)),
        Some((1, 1_000))
    );
    assert_eq!(s.coverage(), Coverage::Covered(0), "reserve = supply");
    // repeating the same sequence (attestation replay) is rejected
    assert_eq!(
        errm!(env, s, &attest(&env, &s, 1, 9_999, "bank")),
        LedgerError::BadSequence {
            expected: 2,
            got: 1
        }
    );
    // coverage follows the supply, without blocking anything
    okm!(env, s, &mint(&env, &s, &addr(&env.bob), 500, "dep-2"));
    assert_eq!(s.coverage(), Coverage::Under(500));
    okm!(env, s, &attest(&env, &s, 2, 2_000, "bank"));
    assert_eq!(s.coverage(), Coverage::Covered(500));
}

// ---------------------------------------------------------------- transfer, fee and nonce

#[test]
fn transfer_moves_balance_charges_fee_and_bumps_the_nonce() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 2_500_000);
    let fee = t.fee;
    assert_eq!(
        fee,
        1_000 + t.size() as u64,
        "minimum fee = base + 1 per byte"
    );
    ok(&env, &mut s, &t);
    assert_eq!(s.balance(&addr(&env.bob)), 2_500_000);
    assert_eq!(s.balance(&addr(&env.alice)), 10_000_000 - 2_500_000 - fee);
    assert_eq!(s.nonce(&addr(&env.alice)), 1);
}

#[test]
fn replay_future_nonce_and_wrong_network_are_rejected() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 1);
    ok(&env, &mut s, &t);
    assert_eq!(
        err(&env, &mut s, &t),
        LedgerError::BadNonce {
            expected: 1,
            got: 0
        },
        "replay"
    );
    let future = tx(
        &env.alice,
        kind::TRANSFER,
        5,
        10_000,
        coin::transfer_payload(&addr(&env.bob), 1),
    );
    assert_eq!(
        err(&env, &mut s, &future),
        LedgerError::BadNonce {
            expected: 1,
            got: 5
        }
    );
    let other = brdu_ledger::Tx::new_signed(
        &env.alice,
        "other-network",
        kind::TRANSFER,
        1,
        10_000,
        coin::transfer_payload(&addr(&env.bob), 1),
    )
    .unwrap();
    assert_eq!(err(&env, &mut s, &other), LedgerError::WrongChain);
}

#[test]
fn fee_below_minimum_is_rejected_and_the_exact_one_passes() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let probe = tx(
        &env.alice,
        kind::TRANSFER,
        0,
        0,
        coin::transfer_payload(&addr(&env.bob), 1),
    );
    let min = env.exec.min_fee(&probe).unwrap();
    let low = tx(
        &env.alice,
        kind::TRANSFER,
        0,
        min - 1,
        coin::transfer_payload(&addr(&env.bob), 1),
    );
    assert_eq!(
        err(&env, &mut s, &low),
        LedgerError::FeeTooLow { min, got: min - 1 }
    );
    ok(
        &env,
        &mut s,
        &tx(
            &env.alice,
            kind::TRANSFER,
            0,
            min,
            coin::transfer_payload(&addr(&env.bob), 1),
        ),
    );
}

#[test]
fn insufficient_balance_for_amount_plus_fee_undoes_everything() {
    let env = Env::new();
    let mut s = funded(&env, 1_000_000);
    let before = s.clone();
    // the balance covers the amount, but not amount + fee
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 1_000_000);
    assert_eq!(err(&env, &mut s, &t), LedgerError::InsufficientFunds);
    assert_eq!(s, before, "the fee and the nonce are also rolled back");
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 0);
    assert_eq!(err(&env, &mut s, &t), LedgerError::ZeroAmount);
    assert_eq!(s, before);
}

#[test]
fn transferring_to_oneself_only_spends_the_fee() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = transfer(&env, &s, &env.alice, &addr(&env.alice), 3_000_000);
    ok(&env, &mut s, &t);
    assert_eq!(s.balance(&addr(&env.alice)), 10_000_000 - t.fee);
}

#[test]
fn payload_with_wrong_size_or_leftovers_is_malformed() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let good = coin::transfer_payload(&addr(&env.bob), 5);
    for bad in [
        good[..39].to_vec(),
        [good.clone(), vec![0]].concat(),
        vec![],
    ] {
        let t = tx(&env.alice, kind::TRANSFER, 0, 50_000, bad);
        assert_eq!(err(&env, &mut s, &t), LedgerError::Malformed);
    }
}

#[test]
fn module_kind_without_a_registered_module_is_rejected() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = tx(&env.alice, 5, 0, 50_000, vec![]);
    assert_eq!(err(&env, &mut s, &t), LedgerError::UnknownKind(5));
    // and outside the range it does not even build
    let bad = brdu_ledger::Tx::new_signed(&env.alice, CHAIN, 23, 0, 0, vec![]).unwrap();
    assert_eq!(
        env.exec.apply_tx(&mut s, &bad),
        Err(LedgerError::UnknownKind(23))
    );
}

#[test]
fn destination_address_does_not_need_to_exist_and_is_born_on_the_first_credit() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let stranger = Address(Hash32([0xab; 32]));
    okm!(env, s, &transfer(&env, &s, &env.alice, &stranger, 1));
    assert_eq!(s.balance(&stranger), 1);
}
