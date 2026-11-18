#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[macro_use]
mod common;

use brdu_crypto::Hash32;
use brdu_ledger::{Block, LedgerError, State};
use common::*;
use proptest::prelude::*;

fn funded(env: &Env) -> State {
    let mut s = State::new();
    let t = mint(env, &s, &addr(&env.alice), 100_000_000, "dep-1");
    block_of(env, &mut s, 1_000, vec![t]);
    s
}

#[test]
fn block_is_valid_as_a_whole_and_the_root_checks() {
    let env = Env::new();
    let mut s = funded(&env);
    assert_eq!((s.height(), s.time()), (1, 1_000));
    let t1 = transfer(&env, &s, &env.alice, &addr(&env.bob), 1_000_000);
    let mut probe = s.clone();
    env.exec.apply_tx(&mut probe, &t1).unwrap();
    let t2 = transfer(&env, &probe, &env.alice, &addr(&env.carol), 2_000_000);
    let b = block_of(&env, &mut s, 1_005, vec![t1, t2]);
    assert_eq!(s.height(), 2);
    assert_eq!(s.last_hash(), Some(b.hash()));
    assert_eq!(s.balance(&addr(&env.bob)), 1_000_000);
    assert_eq!(s.balance(&addr(&env.carol)), 2_000_000);
}

#[test]
fn fee_is_split_among_validators_and_the_remainder_goes_to_the_proposer() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 1_000_000);
    let fee = t.fee;
    let b = block_of(&env, &mut s, 1_010, vec![t]);
    let (share, rest) = (fee / 4, fee % 4);
    for signer in &b.signers {
        let extra = if *signer == b.proposer { rest } else { 0 };
        assert_eq!(s.balance(signer), share + extra);
    }
    // no unit is lost: the sum of balances is exactly what was issued
    assert_eq!(total_balances(&s), u128::from(s.supply()));
}

#[test]
fn an_invalid_transaction_brings_down_the_block_and_the_state_does_not_change() {
    let env = Env::new();
    let mut s = funded(&env);
    let good = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    // the second repeats the nonce of the first: invalid
    let bad = transfer(&env, &s, &env.alice, &addr(&env.bob), 6);
    let before = s.clone();
    let root_before = s.root();
    let built = env.exec.build_block(
        &mut s,
        env.proposer(),
        env.signers(),
        1_020,
        vec![good.clone(), bad.clone()],
    );
    assert!(
        matches!(built, Err(LedgerError::InvalidTx { .. })),
        "{built:?}"
    );
    assert_eq!(s, before);
    // and a block built by hand with the root of a good block, but with the bad transaction, does not get in either
    let mut ok_block = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_020, vec![good])
        .unwrap();
    ok_block.txs.push(bad);
    assert!(matches!(
        env.exec.apply_block(&mut s, &ok_block),
        Err(LedgerError::InvalidTx { .. })
    ));
    assert_eq!(s, before);
    assert_eq!(s.root(), root_before);
}

#[test]
fn wrong_root_wrong_linkage_time_and_limits_are_rejected_without_changing_the_state() {
    let env = Env::with_rules(|r| r.max_block_txs = 2);
    let mut s = funded(&env);
    let before = s.clone();
    let good = {
        let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
        env.exec
            .build_block(&mut s, env.proposer(), env.signers(), 1_100, vec![t])
            .unwrap()
    };

    let mut b = good.clone();
    b.state_root = Hash32([1; 32]);
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::StateRootMismatch)
    );

    let mut b = good.clone();
    b.prev_hash = Some(Hash32([2; 32]));
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::BadLinkage)
    );

    let mut b = good.clone();
    b.height = 9;
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::BadLinkage)
    );

    let mut b = good.clone();
    b.time = 999; // the state is at 1,000
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::TimeWentBack)
    );

    let mut b = good.clone();
    b.txs = vec![good.txs[0].clone(); 3];
    assert!(
        env.exec.apply_block(&mut s, &b).is_err(),
        "3 transactions with limit 2 (and repeated)"
    );
    assert_eq!(s, before, "none of that touched the state");
    // the good block still gets in
    env.exec.apply_block(&mut s, &good).unwrap();
}

#[test]
fn repeated_transaction_in_block_is_rejected() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    let built = env.exec.build_block(
        &mut s,
        env.proposer(),
        env.signers(),
        1_050,
        vec![t.clone(), t.clone()],
    );
    assert_eq!(built, Err(LedgerError::DuplicateTx(t.id())));
}

#[test]
fn malformed_signers_are_rejected() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    let signers = env.signers();
    let mut reversed = signers.clone();
    reversed.reverse();
    let mut dup = signers.clone();
    dup.push(signers[0]);
    let outsider = addr(&key(99));
    for (name, proposer, set) in [
        ("out of order", signers[0], reversed),
        ("repeated", signers[0], dup),
        ("empty", signers[0], vec![]),
        ("proposer outside the set", outsider, signers.clone()),
    ] {
        let r = env
            .exec
            .build_block(&mut s, proposer, set, 1_060, vec![t.clone()]);
        assert_eq!(r, Err(LedgerError::BadLinkage), "{name}");
    }
}

#[test]
fn tampered_signature_inside_the_block_is_rejected() {
    let env = Env::new();
    let mut s = funded(&env);
    let mut t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    t.signature.0[10] ^= 1;
    let r = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_070, vec![t]);
    assert_eq!(r, Err(LedgerError::BadSignature));
}

#[test]
fn building_a_block_does_not_change_the_state_and_the_same_block_applied_on_another_node_gives_the_same_root()
 {
    let env = Env::new();
    let mut s = funded(&env);
    let mut twin = s.clone();
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 7);
    let before = s.clone();
    let b = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_080, vec![t])
        .unwrap();
    assert_eq!(s, before);
    assert_eq!(s.root(), before.root());
    env.exec.apply_block(&mut s, &b).unwrap();
    env.exec.apply_block(&mut twin, &b).unwrap();
    assert_eq!(s.root(), twin.root());
    assert_eq!(s.root(), b.state_root);
    // the same block twice: it is no longer the next one
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::BadLinkage)
    );
}

#[test]
fn empty_block_only_advances_the_height_and_the_time() {
    let env = Env::new();
    let mut s = funded(&env);
    let b = block_of(&env, &mut s, 2_000, vec![]);
    assert_eq!((s.height(), s.time(), b.txs.len()), (2, 2_000, 0));
}

#[test]
fn block_codec_round_trip_and_transaction_limit() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    let b = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_100, vec![t])
        .unwrap();
    let bytes = b.encode().unwrap();
    assert_eq!(Block::decode(&bytes, 1_000).unwrap(), b);
    assert_eq!(
        Block::decode(&bytes, 0),
        Err(LedgerError::Malformed),
        "more transactions than the limit"
    );
    assert_eq!(
        Block::decode(&bytes[..bytes.len() - 1], 1_000),
        Err(LedgerError::Malformed)
    );
    assert_eq!(
        Block::decode(&[bytes.clone(), vec![0]].concat(), 1_000),
        Err(LedgerError::Malformed)
    );
    assert_ne!(b.hash(), Hash32::ZERO);
}

proptest! {
    #[test]
    fn block_with_garbage_never_panics(data in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = Block::decode(&data, 1_000);
    }
}

#[test]
fn chain_of_blocks_links_one_to_the_next() {
    let env = Env::new();
    let mut s = funded(&env);
    let mut prev = s.last_hash();
    for (i, time) in [1_100u64, 1_100, 1_300].iter().enumerate() {
        let b = block_of(&env, &mut s, *time, vec![]);
        assert_eq!(b.prev_hash, prev);
        assert_eq!(b.height, 2 + i as u64);
        prev = Some(b.hash());
    }
    assert_eq!(s.last_hash(), prev);
}

#[test]
fn pending_mempool_state_does_not_become_the_block_state() {
    // the executor applies transactions to a "pending" state (the fee leaves the account); the block one splits the fee
    let env = Env::new();
    let committed = funded(&env);
    let mut pending = committed.clone();
    let t = transfer(&env, &pending, &env.alice, &addr(&env.bob), 10);
    env.exec.apply_tx(&mut pending, &t).unwrap();
    assert!(
        total_balances(&pending) < total_balances(&committed),
        "in pending the fee vanishes"
    );
    let mut real = committed;
    block_of(&env, &mut real, 1_500, vec![t]);
    assert_eq!(
        total_balances(&real),
        u128::from(real.supply()),
        "in the block the fee is split"
    );
}
