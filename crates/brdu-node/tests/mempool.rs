#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Address, SecretKey};
use brdu_ledger::{Tx, coin, kind};
use brdu_node::{Mempool, PoolError};

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

fn tx(from: &SecretKey, nonce: u64) -> Tx {
    let to = Address::from_public_key(&key(200).public());
    Tx::new_signed(
        from,
        "m",
        kind::TRANSFER,
        nonce,
        5_000,
        coin::transfer_payload(&to, 1),
    )
    .unwrap()
}

fn add(p: &mut Mempool, t: &Tx, committed: u64) -> Result<(), PoolError> {
    p.add(t.clone(), t.id(), committed)
}

#[test]
fn duplicate_stale_nonce_and_gap() {
    let mut p = Mempool::new(100, 1 << 20);
    let a = key(1);
    let t0 = tx(&a, 0);
    add(&mut p, &t0, 0).unwrap();
    assert_eq!(add(&mut p, &t0, 0), Err(PoolError::Duplicate));
    assert!(p.contains(&t0.id()));
    // nonce already used on the chain
    assert_eq!(
        add(&mut p, &tx(&a, 3), 5),
        Err(PoolError::StaleNonce {
            expected: 5,
            got: 3
        })
    );
    // huge gap
    assert!(matches!(
        add(&mut p, &tx(&a, 500), 0),
        Err(PoolError::NonceGap { .. })
    ));
}

#[test]
fn gapped_transaction_waits_and_is_promoted_when_the_previous_one_arrives() {
    let mut p = Mempool::new(100, 1 << 20);
    let a = key(1);
    let (t0, t1, t2) = (tx(&a, 0), tx(&a, 1), tx(&a, 2));
    add(&mut p, &t2, 0).unwrap();
    add(&mut p, &t1, 0).unwrap();
    assert_eq!(
        (p.len(), p.ready_len()),
        (2, 0),
        "waiting for the nonce 0 one"
    );
    add(&mut p, &t0, 0).unwrap();
    assert_eq!(
        (p.len(), p.ready_len()),
        (3, 3),
        "all three become ready, in nonce order"
    );
    let nonces: Vec<u64> = p.iter().map(|t| t.nonce).collect();
    assert_eq!(nonces, vec![0, 1, 2]);
}

#[test]
fn limits_in_transactions_and_in_bytes() {
    let mut p = Mempool::new(3, 1 << 20);
    for i in 0..3 {
        add(&mut p, &tx(&key(i + 1), 0), 0).unwrap();
    }
    assert_eq!(add(&mut p, &tx(&key(9), 0), 0), Err(PoolError::Full));
    let size = tx(&key(1), 0).size();
    let mut p = Mempool::new(100, size * 2 + size / 2);
    add(&mut p, &tx(&key(1), 0), 0).unwrap();
    add(&mut p, &tx(&key(2), 0), 0).unwrap();
    assert_eq!(
        add(&mut p, &tx(&key(3), 0), 0),
        Err(PoolError::Full),
        "overflows in bytes before in transactions"
    );
    assert!(p.bytes() <= size * 2 + size / 2);
}

#[test]
fn remove_and_purge_after_a_block() {
    let mut p = Mempool::new(100, 1 << 20);
    let (a, b) = (key(1), key(2));
    let ts: Vec<Tx> = (0..4).map(|n| tx(&a, n)).collect();
    for t in &ts {
        add(&mut p, t, 0).unwrap();
    }
    add(&mut p, &tx(&b, 0), 0).unwrap();
    add(&mut p, &tx(&b, 2), 0).unwrap(); // with a gap
    p.remove(&ts[3].id());
    assert!(!p.contains(&ts[3].id()));
    // a block included a[0..2] and b[0]
    let (aa, bb) = (
        Address::from_public_key(&a.public()),
        Address::from_public_key(&b.public()),
    );
    p.purge(&|x| {
        if *x == aa {
            2
        } else if *x == bb {
            1
        } else {
            0
        }
    });
    let left: Vec<(u64, bool)> = p
        .iter()
        .map(|t| (t.nonce, t.sender == a.public()))
        .collect();
    assert_eq!(
        left,
        vec![(2, true)],
        "only a's nonce 2 one; b's gapped one keeps waiting"
    );
    assert_eq!(p.len(), 2);
    // b's gap closes when the nonce 1 one arrives
    add(&mut p, &tx(&b, 1), 1).unwrap();
    assert_eq!(p.ready_len(), 3, "b[1] and b[2] became ready");
    p.remove(&ts[2].id());
    let bytes_before = p.bytes();
    p.remove(&ts[2].id());
    assert_eq!(
        p.bytes(),
        bytes_before,
        "removing twice does not corrupt the count"
    );
}

#[test]
fn dedupe_in_constant_time_with_many_pending() {
    // the BRDC lesson: with thousands pending, checking for a duplicate must not cost O(n)
    let mut p = Mempool::new(50_000, 1 << 30);
    let keys: Vec<SecretKey> = (0..50u8).map(key).collect();
    let mut all = Vec::new();
    for n in 0..100u64 {
        for k in &keys {
            let t = tx(k, n);
            add(&mut p, &t, 0).unwrap();
            all.push(t);
        }
    }
    assert_eq!(p.len(), 5_000);
    let started = std::time::Instant::now();
    for t in &all {
        assert!(p.contains(&t.id()));
        assert_eq!(add(&mut p, t, 0), Err(PoolError::Duplicate));
    }
    // 5,000 duplicates (including recomputing each id): comfortable even in debug; O(n) per check would be 25 million hashes
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn transaction_removed_on_failure_gives_the_nonce_back_to_the_sender() {
    // the CLI lesson: a repeated mint (rejected while building the block) must not "burn" the nonce in the pool
    let mut p = Mempool::new(100, 1 << 20);
    let a = key(1);
    let bad = tx(&a, 0);
    add(&mut p, &bad, 0).unwrap();
    p.remove(&bad.id()); // the block rejected it: it leaves the pool, nonce 0 stays free on the chain
    let good = Tx::new_signed(
        &a,
        "m",
        kind::TRANSFER,
        0,
        6_000,
        coin::transfer_payload(&Address::from_public_key(&key(201).public()), 2),
    )
    .unwrap();
    add(&mut p, &good, 0).unwrap();
    assert_eq!(p.ready_len(), 1);
}
