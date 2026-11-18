#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Validator rotation (ledger): change by more than 2/3, one pending, limits, taking effect at the right block.

use brdu_crypto::{Address, PublicKey, SecretKey};
use brdu_ledger::coin::Approval;
use brdu_ledger::gov::{self, Action, GovModule};
use brdu_ledger::{Executor, LedgerError, Rules, State, Tx, kind};
use proptest::prelude::*;

const CHAIN: &str = "brdu-gov";

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

struct Net {
    exec: Executor,
    vals: Vec<SecretKey>,
    relayer: SecretKey,
}

impl Net {
    fn new(n: usize, min: usize, max: usize) -> (Self, State) {
        let vals: Vec<SecretKey> = (20..20 + n as u8).map(key).collect();
        let issuers = vec![key(10).public()];
        let rules = Rules::new(CHAIN, &issuers, 1).unwrap();
        let exec = Executor::new(rules).with_module(Box::new(GovModule));
        let mut state = State::new();
        gov::init(
            &mut state,
            &vals.iter().map(SecretKey::public).collect::<Vec<_>>(),
            min,
            max,
        );
        (
            Self {
                exec,
                vals,
                relayer: key(9),
            },
            state,
        )
    }

    fn signers(&self, s: &State) -> Vec<Address> {
        let mut v: Vec<Address> = gov::validators(s)
            .iter()
            .map(Address::from_public_key)
            .collect();
        v.sort();
        v
    }

    /// Change transaction with `n_approvals` approvals from the first validators (by key, from the current set).
    fn change(
        &self,
        s: &State,
        action: Action,
        who: &PublicKey,
        effective: u64,
        approvers: &[&SecretKey],
    ) -> Tx {
        let doc = gov::change_doc(CHAIN, action, who, effective, gov::epoch(s));
        let ap: Vec<Approval> = approvers.iter().map(|k| Approval::new(&doc, k)).collect();
        let nonce = s.nonce(&Address::from_public_key(&self.relayer.public()));
        Tx::new_signed(
            &self.relayer,
            CHAIN,
            kind::VALIDATOR_CHANGE,
            nonce,
            0,
            gov::change_payload(action, who, effective, &ap),
        )
        .unwrap()
    }

    fn first(&self, n: usize) -> Vec<&SecretKey> {
        self.vals.iter().take(n).collect()
    }

    /// Applies a block with `txs` (the proposer is the first of the set).
    fn block(&self, s: &mut State, txs: Vec<Tx>) -> Result<(), LedgerError> {
        let signers = self.signers(s);
        let b = self
            .exec
            .build_block(s, signers[0], signers, 1_000 + s.height(), txs)?;
        self.exec.apply_block(s, &b).map(|_| ())
    }
}

fn newcomer() -> PublicKey {
    key(50).public()
}

#[test]
fn change_with_more_than_2_3_stays_pending_and_only_takes_effect_at_the_right_block() {
    let (net, mut s) = Net::new(4, 3, 7);
    let tx = net.change(&s, Action::Add, &newcomer(), 4, &net.first(3));
    net.block(&mut s, vec![tx]).unwrap(); // block 1
    let p = gov::pending(&s).unwrap();
    assert_eq!(
        (p.action, p.key, p.effective_height),
        (Action::Add, newcomer(), 4)
    );
    assert_eq!((gov::validators(&s).len(), gov::epoch(&s)), (4, 0));
    net.block(&mut s, vec![]).unwrap(); // block 2
    net.block(&mut s, vec![]).unwrap(); // block 3: end of block effective-1
    let set = gov::validators(&s);
    assert_eq!((set.len(), gov::epoch(&s)), (5, 1));
    assert_eq!(
        set.last(),
        Some(&newcomer()),
        "joins at the end of the rotation"
    );
    assert!(gov::pending(&s).is_none());
}

#[test]
fn removing_preserves_the_order_of_the_others() {
    let (net, mut s) = Net::new(5, 3, 7);
    let out = net.vals[1].public();
    let tx = net.change(&s, Action::Remove, &out, 3, &net.first(4));
    net.block(&mut s, vec![tx]).unwrap();
    net.block(&mut s, vec![]).unwrap();
    let want: Vec<PublicKey> = net
        .vals
        .iter()
        .map(SecretKey::public)
        .filter(|k| *k != out)
        .collect();
    assert_eq!(gov::validators(&s), want);
}

#[test]
fn insufficient_repeated_foreign_or_other_document_approvals_are_rejected() {
    let (net, mut s) = Net::new(4, 3, 7);
    let before = s.clone();
    // 2 of 4 does not exceed 2/3
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(2));
    assert!(
        net.block(&mut s, vec![t])
            .unwrap_err()
            .to_string()
            .contains("insufficient")
    );
    // the same validator 3 times counts once
    let v0 = &net.vals[0];
    let t = net.change(&s, Action::Add, &newcomer(), 4, &[v0, v0, v0]);
    assert!(net.block(&mut s, vec![t]).is_err());
    // whoever is not a validator is ignored
    let strangers = [key(70), key(71), key(72), key(73)];
    let t = net.change(
        &s,
        Action::Add,
        &newcomer(),
        4,
        &strangers.iter().collect::<Vec<_>>(),
    );
    assert!(net.block(&mut s, vec![t]).is_err());
    // approvals for ANOTHER key, another height, another action or another network do not count for this payload
    let ap_for = |action, who: &PublicKey, eff, chain: &str, epoch| -> Vec<Approval> {
        let doc = brdu_ledger::gov::change_doc(chain, action, who, eff, epoch);
        net.first(3)
            .iter()
            .map(|k| Approval::new(&doc, k))
            .collect()
    };
    for ap in [
        ap_for(Action::Add, &key(51).public(), 4, CHAIN, 0),
        ap_for(Action::Add, &newcomer(), 5, CHAIN, 0),
        ap_for(Action::Remove, &newcomer(), 4, CHAIN, 0),
        ap_for(Action::Add, &newcomer(), 4, "other-network", 0),
        ap_for(Action::Add, &newcomer(), 4, CHAIN, 1),
    ] {
        let nonce = s.nonce(&Address::from_public_key(&net.relayer.public()));
        let t = Tx::new_signed(
            &net.relayer,
            CHAIN,
            kind::VALIDATOR_CHANGE,
            nonce,
            0,
            gov::change_payload(Action::Add, &newcomer(), 4, &ap),
        )
        .unwrap();
        assert!(net.block(&mut s, vec![t]).is_err());
    }
    assert_eq!(s, before, "none of that touched the state");
}

#[test]
fn past_effective_height_single_pending_duplicate_and_size_limits() {
    let (net, mut s) = Net::new(4, 4, 5);
    // effective must be at least height + 2 (here the block under construction is 1: effective >= 3)
    for eff in [0, 1, 2] {
        let t = net.change(&s, Action::Add, &newcomer(), eff, &net.first(3));
        assert!(net.block(&mut s, vec![t]).is_err(), "effective {eff}");
    }
    // already a validator / not a validator
    let t = net.change(&s, Action::Add, &net.vals[0].public(), 5, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err());
    let t = net.change(&s, Action::Remove, &newcomer(), 5, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err());
    // minimum 4: removing one of 4 would leave the limit
    let t = net.change(&s, Action::Remove, &net.vals[0].public(), 5, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err());
    // one pending change at a time
    let t = net.change(&s, Action::Add, &newcomer(), 6, &net.first(3));
    net.block(&mut s, vec![t]).unwrap();
    let t = net.change(&s, Action::Add, &key(51).public(), 8, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err(), "second pending");
    // first one taken effect (5 validators = maximum), a new entry exceeds the maximum
    while gov::epoch(&s) == 0 {
        net.block(&mut s, vec![]).unwrap();
    }
    assert_eq!(gov::validators(&s).len(), 5);
    let t = net.change(
        &s,
        Action::Add,
        &key(51).public(),
        s.height() + 3,
        &net.first(4),
    );
    assert!(net.block(&mut s, vec![t]).is_err(), "maximum 5");
}

#[test]
fn approval_from_the_old_epoch_does_not_count_after_the_change() {
    let (net, mut s) = Net::new(4, 3, 7);
    let old_epoch_doc = gov::change_doc(CHAIN, Action::Remove, &net.vals[3].public(), 9, 0);
    let t = net.change(&s, Action::Add, &newcomer(), 3, &net.first(3));
    net.block(&mut s, vec![t]).unwrap();
    net.block(&mut s, vec![]).unwrap();
    assert_eq!(gov::epoch(&s), 1);
    // approvals signed in epoch 0 to remove the 4th: with epoch 1 in the state, they do not count
    let ap: Vec<Approval> = net
        .first(3)
        .iter()
        .map(|k| Approval::new(&old_epoch_doc, k))
        .collect();
    let nonce = s.nonce(&Address::from_public_key(&net.relayer.public()));
    let t = Tx::new_signed(
        &net.relayer,
        CHAIN,
        kind::VALIDATOR_CHANGE,
        nonce,
        0,
        gov::change_payload(Action::Remove, &net.vals[3].public(), 9, &ap),
    )
    .unwrap();
    assert!(net.block(&mut s, vec![t]).is_err());
}

#[test]
fn with_5_validators_4_approvals_are_needed() {
    let (net, mut s) = Net::new(5, 3, 7);
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(3));
    assert!(
        net.block(&mut s, vec![t]).is_err(),
        "3 of 5 does not exceed 2/3"
    );
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(4));
    net.block(&mut s, vec![t]).unwrap();
}

#[test]
fn history_replay_reaches_the_same_set_and_the_same_root() {
    let (net, mut s) = Net::new(4, 3, 7);
    let mut blocks = Vec::new();
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(3));
    for txs in [vec![t], vec![], vec![]] {
        let signers = net.signers(&s);
        let time = 1_000 + s.height();
        let b = net
            .exec
            .build_block(&mut s, signers[0], signers, time, txs)
            .unwrap();
        net.exec.apply_block(&mut s, &b).unwrap();
        blocks.push(b);
    }
    let (net2, mut replay) = Net::new(4, 3, 7);
    for b in &blocks {
        net2.exec.apply_block(&mut replay, b).unwrap();
    }
    assert_eq!(replay.root(), s.root());
    assert_eq!(gov::validators(&replay), gov::validators(&s));
}

#[test]
fn malformed_payload_is_rejected() {
    let (net, mut s) = Net::new(4, 3, 7);
    let nonce = s.nonce(&Address::from_public_key(&net.relayer.public()));
    for bad in [vec![], vec![9; 20], vec![1; 41 + 200]] {
        let t = Tx::new_signed(&net.relayer, CHAIN, kind::VALIDATOR_CHANGE, nonce, 0, bad).unwrap();
        assert!(net.block(&mut s, vec![t]).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(32), ..ProptestConfig::default() })]

    /// Any sequence of changes (valid and invalid): the set stays in [min, max], with no repeats, and the
    /// epoch only goes up by 1 on each change that takes effect.
    #[test]
    fn the_set_never_leaves_the_limits(ops in proptest::collection::vec((any::<bool>(), 0u8..8, 2u64..5, 2usize..6), 1..30)) {
        let (net, mut s) = Net::new(4, 3, 6);
        let pool: Vec<SecretKey> = (40..48).map(key).collect();
        let mut last_epoch = 0;
        for (add, who, lead, n_ap) in ops {
            let target = if add { pool[who as usize].public() } else { net.vals[(who % 4) as usize].public() };
            let action = if add { Action::Add } else { Action::Remove };
            let current = gov::validators(&s);
            let approvers: Vec<&SecretKey> = net.vals.iter().chain(pool.iter()).filter(|k| current.contains(&k.public())).take(n_ap).collect();
            let t = net.change(&s, action, &target, s.height() + 1 + lead, &approvers);
            let _ = net.block(&mut s, vec![t]);
            net.block(&mut s, vec![]).unwrap();
            let set = gov::validators(&s);
            let (min, max) = gov::limits(&s);
            prop_assert!(set.len() >= min && set.len() <= max);
            let mut sorted = set.clone();
            sorted.sort();
            sorted.dedup();
            prop_assert_eq!(sorted.len(), set.len());
            let e = gov::epoch(&s);
            prop_assert!(e >= last_epoch && e <= last_epoch + 1);
            last_epoch = e;
        }
    }
}
