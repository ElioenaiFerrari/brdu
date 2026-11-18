#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brdu_bft::{Certificate, Vote, VoteType};
use brdu_chain::{Chain, ChainError, DecidedBlock, Genesis, RecordLog};
use brdu_crypto::{Address, SecretKey};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Block, LedgerError, Tx, kind};
use proptest::prelude::*;

const CHAIN: &str = "brdu-chain-test";

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

struct Net {
    vals: Vec<SecretKey>,
    issuers: Vec<SecretKey>,
    alice: SecretKey,
    bob: SecretKey,
    relayer: SecretKey,
}

impl Net {
    fn new() -> Self {
        Self {
            vals: (20..24).map(key).collect(),
            issuers: (10..13).map(key).collect(),
            alice: key(1),
            bob: key(2),
            relayer: key(9),
        }
    }

    fn genesis(&self) -> Genesis {
        Genesis::new(
            CHAIN,
            self.vals.iter().map(SecretKey::public).collect(),
            self.issuers.iter().map(SecretKey::public).collect(),
            2,
        )
    }

    fn chain(&self) -> Chain {
        Chain::new(self.genesis()).unwrap()
    }

    fn mint(&self, chain: &Chain, to: &SecretKey, amount: u64, reference: &str) -> Tx {
        let addr = Address::from_public_key(&to.public());
        let doc = coin::issuance_doc(false, CHAIN, &addr, amount, reference);
        let ap: Vec<Approval> = self
            .issuers
            .iter()
            .take(2)
            .map(|k| Approval::new(&doc, k))
            .collect();
        let nonce = chain
            .state()
            .nonce(&Address::from_public_key(&self.relayer.public()));
        Tx::new_signed(
            &self.relayer,
            CHAIN,
            kind::MINT,
            nonce,
            0,
            coin::issuance_payload(&addr, amount, reference, &ap),
        )
        .unwrap()
    }

    fn transfer(&self, chain: &Chain, from: &SecretKey, to: &SecretKey, amount: u64) -> Tx {
        let a = Address::from_public_key(&from.public());
        let payload = coin::transfer_payload(&Address::from_public_key(&to.public()), amount);
        let nonce = chain.state().nonce(&a);
        let probe = Tx::new_signed(from, CHAIN, kind::TRANSFER, nonce, 0, payload.clone()).unwrap();
        let fee = chain.executor().min_fee(&probe).unwrap();
        Tx::new_signed(from, CHAIN, kind::TRANSFER, nonce, fee, payload).unwrap()
    }

    /// The round proposer's block for `round`, certified by `signers` validators.
    fn decide(
        &self,
        chain: &mut Chain,
        round: u64,
        time: u64,
        txs: Vec<Tx>,
        signers: usize,
    ) -> DecidedBlock {
        let proposer = chain.proposer(round).unwrap();
        let block = chain
            .build(Address::from_public_key(&proposer), time, txs)
            .unwrap();
        self.certify(block, round, signers)
    }

    fn certify(&self, block: Block, round: u64, signers: usize) -> DecidedBlock {
        let h = block.hash();
        let votes = self
            .vals
            .iter()
            .take(signers)
            .map(|k| Vote::sign(CHAIN, VoteType::Precommit, block.height, round, Some(h), k))
            .collect();
        DecidedBlock {
            cert: Certificate {
                height: block.height,
                round,
                block_hash: h,
                votes,
            },
            block,
        }
    }
}

#[test]
fn genesis_round_trips_hash_changes_with_any_field_and_invalid_rules_are_rejected() {
    let n = Net::new();
    let g = n.genesis();
    assert_eq!(Genesis::decode(&g.encode()).unwrap(), g);
    let mut variants = vec![];
    let mut v = g.clone();
    v.chain_id = "other".into();
    variants.push(v);
    let mut v = g.clone();
    v.validators.reverse();
    variants.push(v);
    let mut v = g.clone();
    v.issuer_threshold = 3;
    variants.push(v);
    let mut v = g.clone();
    v.per_byte = 2;
    variants.push(v);
    for v in variants {
        assert_ne!(v.hash(), g.hash());
    }
    let mut dup = g.clone();
    dup.validators[1] = dup.validators[0];
    assert!(matches!(Chain::new(dup), Err(ChainError::Genesis(_))));
    let mut empty = g.clone();
    empty.validators.clear();
    assert!(Chain::new(empty).is_err());
    for t in [0, 4] {
        let mut bad = g.clone();
        bad.issuer_threshold = t;
        assert!(Chain::new(bad).is_err(), "threshold {t}");
    }
    assert!(Genesis::decode(&g.encode()[..10]).is_err());
}

#[test]
fn block_with_3_of_4_certificate_is_accepted_and_the_replica_reaches_the_same_state() {
    let n = Net::new();
    let mut leader = n.chain();
    let mut replica = n.chain();
    let t = n.mint(&leader, &n.alice, 50_000_000, "dep-1");
    let d1 = n.decide(&mut leader, 0, 100, vec![t], 3);
    leader.apply(d1.clone()).unwrap();
    let t2 = n.transfer(&leader, &n.alice, &n.bob, 1_000_000);
    let d2 = n.decide(&mut leader, 1, 105, vec![t2], 4);
    leader.apply(d2.clone()).unwrap();
    // the replica only receives the finalized blocks
    replica.apply(d1).unwrap();
    replica.apply(d2).unwrap();
    assert_eq!(replica.height(), 2);
    assert_eq!(replica.state().root(), leader.state().root());
    assert_eq!(
        replica
            .state()
            .balance(&Address::from_public_key(&n.bob.public())),
        1_000_000
    );
    assert_eq!(replica.blocks_after(1).len(), 1);
    assert_eq!(replica.blocks_after(9).len(), 0);
}

#[test]
fn weak_certificate_for_another_block_or_height_is_rejected_and_the_chain_does_not_change() {
    let n = Net::new();
    let mut c = n.chain();
    let t = n.mint(&c, &n.alice, 10, "d");
    let good = n.decide(&mut c, 0, 100, vec![t], 3);
    let root = c.state().root();

    let mut weak = good.clone();
    weak.cert.votes.truncate(2);
    assert_eq!(
        c.apply(weak),
        Err(ChainError::Certificate(brdu_bft::CertError::NoQuorum))
    );

    let mut other = good.clone();
    other.cert.block_hash.0[0] ^= 1;
    assert!(c.apply(other).is_err());

    let mut height = good.clone();
    height.cert.height = 9;
    assert!(matches!(c.apply(height), Err(ChainError::Block("height"))));

    // votes from a foreign network
    let stranger: Vec<Vote> = (60..64)
        .map(|i| {
            Vote::sign(
                CHAIN,
                VoteType::Precommit,
                1,
                0,
                Some(good.block.hash()),
                &key(i),
            )
        })
        .collect();
    let mut fake = good.clone();
    fake.cert.votes = stranger;
    assert!(matches!(c.apply(fake), Err(ChainError::Certificate(_))));

    assert_eq!((c.height(), c.state().root()), (0, root));
    c.apply(good).unwrap();
}

#[test]
fn wrong_proposer_or_wrong_signers_are_rejected() {
    let n = Net::new();
    let mut c = n.chain();
    let t = n.mint(&c, &n.alice, 10, "d");
    // the proposer of round 0 is validators[1 % 4]; that of round 2 is validators[3]
    let block = c
        .build(
            Address::from_public_key(&c.proposer(2).unwrap()),
            100,
            vec![t.clone()],
        )
        .unwrap();
    let wrong_round = n.certify(block, 0, 3);
    assert!(matches!(
        c.apply(wrong_round),
        Err(ChainError::Block("not the proposer of the round"))
    ));

    // the right proposer, but with a tampered signer set (it would take someone's share of the fee)
    let proposer = Address::from_public_key(&c.proposer(0).unwrap());
    let mut sig = c.signers().to_vec();
    sig.pop();
    let mut b = c.build(proposer, 100, vec![t]).unwrap();
    b.signers = sig;
    assert!(matches!(
        c.apply(n.certify(b, 0, 3)),
        Err(ChainError::Block("signers are not the validator set"))
    ));
    assert_eq!(c.height(), 0);
}

#[test]
fn block_with_bad_transaction_or_wrong_root_is_rejected_even_with_valid_certificate() {
    let n = Net::new();
    let mut c = n.chain();
    let t = n.mint(&c, &n.alice, 1_000, "d");
    let mut d = n.decide(&mut c, 0, 100, vec![t], 4);
    // tampers with the state root and recertifies (3 of 4 sign a block that lies)
    d.block.state_root.0[0] ^= 1;
    let lying = n.certify(d.block, 0, 4);
    assert_eq!(
        c.apply(lying),
        Err(ChainError::Ledger(LedgerError::StateRootMismatch))
    );
    assert_eq!(c.height(), 0);
}

#[test]
fn replaying_an_already_applied_block_is_rejected() {
    let n = Net::new();
    let mut c = n.chain();
    let t = n.mint(&c, &n.alice, 10, "d");
    let d = n.decide(&mut c, 0, 100, vec![t], 3);
    c.apply(d.clone()).unwrap();
    assert!(matches!(c.apply(d), Err(ChainError::Block("height"))));
}

#[test]
fn decided_block_round_trips_and_garbage_does_not_panic() {
    let n = Net::new();
    let mut c = n.chain();
    let t = n.mint(&c, &n.alice, 10, "d");
    let d = n.decide(&mut c, 0, 100, vec![t], 3);
    let bytes = d.encode().unwrap();
    assert_eq!(DecidedBlock::decode(&bytes, 1_000, 1 << 20).unwrap(), d);
    assert!(DecidedBlock::decode(&bytes[..bytes.len() - 1], 1_000, 1 << 20).is_err());
    assert!(
        DecidedBlock::decode(&bytes, 1_000, 10).is_err(),
        "byte limit"
    );
}

proptest! {
    #[test]
    fn garbage_never_panics(data in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = DecidedBlock::decode(&data, 1_000, 1 << 20);
        let _ = Genesis::decode(&data);
    }
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("brdu-chain-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&d);
    d
}

#[test]
fn log_stores_reopens_and_truncates_an_incomplete_or_corrupted_tail() {
    let p = tmp("log");
    {
        let (mut log, old) = RecordLog::open(&p).unwrap();
        assert!(old.is_empty());
        for r in [&b"one"[..], b"two", b""] {
            log.append(r, true).unwrap();
        }
    }
    let (mut log, old) = RecordLog::open(&p).unwrap();
    assert_eq!(old, vec![b"one".to_vec(), b"two".to_vec(), vec![]]);
    log.append(b"four", false).unwrap();
    drop(log);
    // crash in the middle of a write: cut the last byte
    let len = std::fs::metadata(&p).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(len - 1)
        .unwrap();
    let (mut log, old) = RecordLog::open(&p).unwrap();
    assert_eq!(
        old.len(),
        3,
        "the incomplete record disappears, the earlier ones stay"
    );
    // and the log stays usable: the next record does not end up behind garbage
    log.append(b"five", false).unwrap();
    drop(log);
    let (_, old) = RecordLog::open(&p).unwrap();
    assert_eq!(old.last().unwrap(), b"five");
    // corruption in the middle: everything from it onward is discarded
    let mut raw = std::fs::read(&p).unwrap();
    raw[9] ^= 0xff;
    std::fs::write(&p, &raw).unwrap();
    let (_, old) = RecordLog::open(&p).unwrap();
    assert!(old.len() < 4);
    std::fs::remove_file(&p).unwrap();
}

#[test]
fn log_clear_empties() {
    let p = tmp("clear");
    let (mut log, _) = RecordLog::open(&p).unwrap();
    log.append(b"x", true).unwrap();
    log.clear().unwrap();
    log.append(b"y", true).unwrap();
    drop(log);
    let (_, old) = RecordLog::open(&p).unwrap();
    assert_eq!(old, vec![b"y".to_vec()]);
    RecordLog::remove(&p).unwrap();
}

// ---------------------------------------------------------------- validator rotation

fn certify_with(block: Block, round: u64, keys: &[&SecretKey]) -> DecidedBlock {
    let h = block.hash();
    let votes = keys
        .iter()
        .map(|k| Vote::sign(CHAIN, VoteType::Precommit, block.height, round, Some(h), k))
        .collect();
    DecidedBlock {
        cert: Certificate {
            height: block.height,
            round,
            block_hash: h,
            votes,
        },
        block,
    }
}

fn change_tx(
    net: &Net,
    chain: &Chain,
    action: brdu_ledger::gov::Action,
    who: &SecretKey,
    effective: u64,
    approvers: &[&SecretKey],
) -> Tx {
    let doc = brdu_ledger::gov::change_doc(CHAIN, action, &who.public(), effective, chain.epoch());
    let ap: Vec<Approval> = approvers.iter().map(|k| Approval::new(&doc, k)).collect();
    let nonce = chain
        .state()
        .nonce(&Address::from_public_key(&net.relayer.public()));
    Tx::new_signed(
        &net.relayer,
        CHAIN,
        kind::VALIDATOR_CHANGE,
        nonce,
        0,
        brdu_ledger::gov::change_payload(action, &who.public(), effective, &ap),
    )
    .unwrap()
}

/// Next block, certified by `keys` in round `round`.
fn next(chain: &mut Chain, round: u64, txs: Vec<Tx>, keys: &[&SecretKey]) -> DecidedBlock {
    let proposer = Address::from_public_key(&chain.proposer(round).unwrap());
    let time = 100 + chain.height();
    let block = chain.build(proposer, time, txs).unwrap();
    certify_with(block, round, keys)
}

#[test]
fn genesis_rejects_inconsistent_validator_limits() {
    let n = Net::new();
    for (min, max) in [(0, 21), (5, 21), (1, 33), (1, 3)] {
        let mut g = n.genesis();
        g.min_validators = min;
        g.max_validators = max;
        assert!(Chain::new(g).is_err(), "min {min} max {max}");
    }
    let mut g = n.genesis();
    g.min_validators = 3;
    assert!(Chain::new(g).is_ok());
}

#[test]
fn after_the_change_only_the_new_set_counts_and_the_old_one_no_longer_certifies() {
    let n = Net::new();
    let newcomer = key(30);
    let mut c = n.chain();
    let old: Vec<&SecretKey> = n.vals.iter().collect();
    // block 1: "the 5th joins" change effective at height 4, approved by 3 of 4
    let tx = change_tx(
        &n,
        &c,
        brdu_ledger::gov::Action::Add,
        &newcomer,
        4,
        &old[..3],
    );
    let b1 = next(&mut c, 0, vec![tx], &old[..3]);
    c.apply(b1).unwrap();
    assert_eq!((c.epoch(), c.validators().len()), (0, 4));
    assert!(c.pending_change().is_some());
    for _ in 0..2 {
        let b = next(&mut c, 0, vec![], &old[..3]);
        c.apply(b).unwrap();
    }
    // blocks 2 and 3 were still from the old set; now the epoch is 1 with 5 validators
    assert_eq!((c.epoch(), c.validators().len(), c.height()), (1, 5, 3));
    let five: Vec<&SecretKey> = n.vals.iter().chain(std::iter::once(&newcomer)).collect();

    // block 4 with 3 votes (2/3 of 4, but not of 5): rejected
    let weak = next(&mut c, 0, vec![], &five[..3]);
    assert_eq!(
        c.apply(weak),
        Err(ChainError::Certificate(brdu_bft::CertError::NoQuorum))
    );
    // with 4 of 5 (includes the newcomer) it is accepted; the proposer is already computed over the 5
    let good = next(&mut c, 0, vec![], &five[..4]);
    assert_eq!(good.block.signers.len(), 5, "the fee is now split among 5");
    c.apply(good).unwrap();
    // the newcomer can vote alone with 3 others (it would also pass without him: 4 of 5)
    let b5 = next(&mut c, 1, vec![], &[five[1], five[2], five[3], five[4]]);
    c.apply(b5).unwrap();
}

#[test]
fn block_with_signers_from_the_old_set_is_rejected_after_the_change() {
    let n = Net::new();
    let newcomer = key(30);
    let mut c = n.chain();
    let old: Vec<&SecretKey> = n.vals.iter().collect();
    let tx = change_tx(
        &n,
        &c,
        brdu_ledger::gov::Action::Add,
        &newcomer,
        3,
        &old[..3],
    );
    let b1 = next(&mut c, 0, vec![tx], &old[..3]);
    c.apply(b1).unwrap();
    let b2 = next(&mut c, 0, vec![], &old[..3]);
    c.apply(b2).unwrap();
    assert_eq!(c.validators().len(), 5);
    // a block built with the old signer set (4) is rejected, even with a 4-of-5 certificate
    let proposer = Address::from_public_key(&c.proposer(0).unwrap());
    let mut b = c.build(proposer, 500, vec![]).unwrap();
    b.signers.pop();
    let five: Vec<&SecretKey> = n.vals.iter().chain(std::iter::once(&newcomer)).collect();
    assert!(matches!(
        c.apply(certify_with(b, 0, &five[..4])),
        Err(ChainError::Block("signers are not the validator set"))
    ));
}

#[test]
fn removed_validator_no_longer_counts_and_the_replica_rebuilds_the_same_set() {
    let n = Net::new();
    let mut leader = n.chain();
    let mut replica = n.chain();
    let all: Vec<&SecretKey> = n.vals.iter().collect();
    let out = &n.vals[3];
    // removes the 4th (4 -> 3 validators; the test genesis minimum is 1)
    let tx = change_tx(
        &n,
        &leader,
        brdu_ledger::gov::Action::Remove,
        out,
        3,
        &all[..3],
    );
    let mut blocks = vec![next(&mut leader, 0, vec![tx], &all[..3])];
    leader.apply(blocks[0].clone()).unwrap();
    let b2 = next(&mut leader, 0, vec![], &all[..3]);
    leader.apply(b2.clone()).unwrap();
    blocks.push(b2);
    assert_eq!(leader.validators().len(), 3);
    // with 3 validators, 3 votes are needed (2 of 3 does not exceed 2/3), and the removed one does not count
    let with_removed = next(&mut leader, 0, vec![], &[all[0], all[1], out]);
    assert!(
        leader.apply(with_removed).is_err(),
        "the removed one is no longer a validator"
    );
    let ok = next(&mut leader, 0, vec![], &all[..3]);
    leader.apply(ok.clone()).unwrap();
    blocks.push(ok);
    for b in blocks {
        replica.apply(b).unwrap();
    }
    assert_eq!(replica.validators(), leader.validators());
    assert_eq!(replica.state().root(), leader.state().root());
    assert_eq!(replica.epoch(), 1);
}
