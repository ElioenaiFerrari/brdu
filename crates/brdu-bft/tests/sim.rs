#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Deterministic simulator: message queue delivered in a seed-drawn order, timeouts fired only
//! when nothing else moves. Behaviors: honest, crashed, mute, partition, equivocator.

use std::collections::BTreeSet;

use brdu_bft::{Action, Certificate, Engine, EngineConfig, Event, Message, Step, Vote, VoteType};
use brdu_crypto::{Hash32, PublicKey, SecretKey, tagged};
use proptest::prelude::*;

const CHAIN: &str = "brdu-sim";
const HEIGHT: u64 = 7;

fn hash(b: &[u8]) -> Hash32 {
    tagged("test/block", &[b])
}

fn keys(n: usize) -> Vec<SecretKey> {
    (0..n)
        .map(|i| SecretKey::from_bytes([i as u8 + 1; 32]))
        .collect()
}

fn engine(ks: &[SecretKey], i: usize, valid: fn(u64, &[u8]) -> bool) -> Engine {
    let validators: Vec<PublicKey> = ks.iter().map(SecretKey::public).collect();
    Engine::new(EngineConfig {
        chain_id: CHAIN.into(),
        height: HEIGHT,
        validators,
        key: SecretKey::from_bytes(*ks[i].expose_seed()),
        hash_fn: Box::new(hash),
        valid_fn: Box::new(valid),
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Honest,
    Crashed,
    /// Receives, but never transmits anything (mute proposer, mute voter).
    Mute,
}

struct Sim {
    ks: Vec<SecretKey>,
    nodes: Vec<Engine>,
    modes: Vec<Mode>,
    queue: Vec<(usize, Event)>,
    timers: Vec<(usize, Step, u64)>,
    /// `group[i]`: nodes in different groups do not talk to each other (partition).
    group: Vec<u8>,
    seed: u64,
    decided: Vec<Option<(Vec<u8>, Certificate)>>,
    evidence: usize,
}

impl Sim {
    fn new(n: usize, modes: Vec<Mode>, seed: u64) -> Self {
        let ks = keys(n);
        let nodes = (0..n).map(|i| engine(&ks, i, |_, _| true)).collect();
        Self {
            ks,
            nodes,
            modes,
            queue: vec![],
            timers: vec![],
            group: vec![0; n],
            seed,
            decided: vec![None; n],
            evidence: 0,
        }
    }

    fn rnd(&mut self) -> u64 {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.seed >> 33
    }

    fn act(&mut self, from: usize, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Broadcast(m) => {
                    if self.modes[from] == Mode::Mute {
                        continue;
                    }
                    for to in 0..self.nodes.len() {
                        if to != from && self.group[to] == self.group[from] {
                            let ev = match &m {
                                Message::Proposal(p) => Event::Proposal(p.clone()),
                                Message::Vote(v) => Event::Vote(v.clone()),
                            };
                            self.queue.push((to, ev));
                        }
                    }
                }
                Action::ScheduleTimeout(s, r) => self.timers.push((from, s, r)),
                Action::NeedBlock(r) => {
                    let b = format!("block-{HEIGHT}-{r}-{from}").into_bytes();
                    self.queue.push((from, Event::Block(b)));
                }
                Action::Decide(b, c) => self.decided[from] = Some((b, c)),
                Action::Evidence(..) => self.evidence += 1,
            }
        }
    }

    fn run(&mut self, max_steps: usize) {
        for i in 0..self.nodes.len() {
            if self.modes[i] != Mode::Crashed {
                let a = self.nodes[i].start();
                self.act(i, a);
            }
        }
        for _ in 0..max_steps {
            if self.live_all_decided() {
                return;
            }
            if self.queue.is_empty() {
                // nothing moves: fire the oldest timeout of a live node
                let Some(pos) = self
                    .timers
                    .iter()
                    .position(|(n, ..)| self.modes[*n] != Mode::Crashed)
                else {
                    return;
                };
                let (n, s, r) = self.timers.remove(pos);
                let a = self.nodes[n].handle(Event::Timeout(s, r));
                self.act(n, a);
                continue;
            }
            let i = (self.rnd() as usize) % self.queue.len();
            let (to, ev) = self.queue.swap_remove(i);
            if self.modes[to] == Mode::Crashed {
                continue;
            }
            let a = self.nodes[to].handle(ev);
            self.act(to, a);
        }
    }

    fn live_all_decided(&self) -> bool {
        (0..self.nodes.len())
            .filter(|i| self.modes[*i] != Mode::Crashed)
            .all(|i| self.decided[i].is_some())
    }

    fn validators(&self) -> Vec<PublicKey> {
        self.ks.iter().map(SecretKey::public).collect()
    }

    /// Safety: everyone who decided, decided the same block, and every certificate checks out.
    fn assert_safety(&self) {
        let vals = self.validators();
        let blocks: BTreeSet<&Vec<u8>> = self.decided.iter().flatten().map(|(b, _)| b).collect();
        assert!(blocks.len() <= 1, "two final blocks: {blocks:?}");
        for (b, c) in self.decided.iter().flatten() {
            c.verify(CHAIN, &vals).unwrap();
            assert_eq!(c.block_hash, hash(b));
            assert_eq!(c.height, HEIGHT);
        }
    }
}

#[test]
fn four_honest_validators_decide_the_same_block_in_round_zero() {
    let mut s = Sim::new(4, vec![Mode::Honest; 4], 1);
    s.run(10_000);
    assert!(s.live_all_decided());
    s.assert_safety();
    let (_, c) = s.decided[0].as_ref().unwrap();
    assert_eq!(c.round, 0);
    assert!(c.votes.len() >= 3, "more than 2/3 of 4");
    assert_eq!(s.evidence, 0);
}

#[test]
fn one_crashed_validator_out_of_four_does_not_stop_the_network() {
    for crashed in 0..4 {
        let mut modes = vec![Mode::Honest; 4];
        modes[crashed] = Mode::Crashed;
        let mut s = Sim::new(4, modes, 10 + crashed as u64);
        s.run(20_000);
        assert!(s.live_all_decided(), "crashed = {crashed}");
        s.assert_safety();
    }
}

#[test]
fn mute_proposer_loses_the_round_and_the_next_one_decides() {
    // height 7, round 0 -> proposer = validators[7 % 4 = 3]; it receives but never transmits
    let mut modes = vec![Mode::Honest; 4];
    modes[3] = Mode::Mute;
    let mut s = Sim::new(4, modes, 5);
    s.run(50_000);
    assert!(s.live_all_decided());
    s.assert_safety();
    let (_, c) = s.decided[0].as_ref().unwrap();
    assert!(c.round >= 1, "round 0 cannot have decided");
}

#[test]
fn two_crashed_out_of_four_decide_nothing_but_never_diverge() {
    let mut s = Sim::new(
        4,
        vec![Mode::Honest, Mode::Honest, Mode::Crashed, Mode::Crashed],
        3,
    );
    s.run(5_000);
    assert!(
        s.decided.iter().all(Option::is_none),
        "without more than 2/3 there is no final block"
    );
    s.assert_safety();
}

#[test]
fn two_two_partition_stalls_and_decides_once_healed() {
    let mut s = Sim::new(4, vec![Mode::Honest; 4], 9);
    s.group = vec![0, 0, 1, 1];
    s.run(3_000);
    assert!(
        s.decided.iter().all(Option::is_none),
        "2 of 4 is not a quorum"
    );
    s.assert_safety();
    // heal: reconnect and resend what each one knows (regossip on reconnect)
    s.group = vec![0; 4];
    for i in 0..4 {
        for m in s.nodes[i].known_messages(4) {
            for to in 0..4 {
                if to != i {
                    let ev = match &m {
                        Message::Proposal(p) => Event::Proposal(p.clone()),
                        Message::Vote(v) => Event::Vote(v.clone()),
                    };
                    s.queue.push((to, ev));
                }
            }
        }
    }
    s.run(50_000);
    assert!(s.live_all_decided(), "after healing everyone decides");
    s.assert_safety();
}

#[test]
fn equivocator_votes_generate_evidence() {
    let ks = keys(4);
    let mut e = engine(&ks, 0, |_, _| true);
    let _ = e.start();
    let a = Vote::sign(
        CHAIN,
        VoteType::Prevote,
        HEIGHT,
        0,
        Some(hash(b"A")),
        &ks[1],
    );
    let b = Vote::sign(
        CHAIN,
        VoteType::Prevote,
        HEIGHT,
        0,
        Some(hash(b"B")),
        &ks[1],
    );
    assert!(
        e.handle(Event::Vote(a.clone()))
            .iter()
            .all(|x| !matches!(x, Action::Evidence(..)))
    );
    let out = e.handle(Event::Vote(b.clone()));
    assert!(out.contains(&Action::Evidence(a, b)));
}

#[test]
fn invalid_messages_are_ignored() {
    let ks = keys(4);
    let mut e = engine(&ks, 0, |_, _| true);
    let _ = e.start();
    // from someone who is not a validator
    let outsider = SecretKey::from_bytes([99; 32]);
    let v = Vote::sign(CHAIN, VoteType::Prevote, HEIGHT, 0, None, &outsider);
    // signature from another network; wrong height; tampered signature
    let other = Vote::sign("other", VoteType::Prevote, HEIGHT, 0, None, &ks[1]);
    let wrong_h = Vote::sign(CHAIN, VoteType::Prevote, HEIGHT + 1, 0, None, &ks[1]);
    let mut bad = Vote::sign(CHAIN, VoteType::Prevote, HEIGHT, 0, None, &ks[1]);
    bad.signature.0[0] ^= 1;
    for v in [v, other, wrong_h, bad] {
        assert!(e.handle(Event::Vote(v)).is_empty());
    }
}

#[test]
fn invalid_block_gets_a_nil_prevote() {
    let ks = keys(4);
    // height 7, round 0: proposer = validators[3]
    let mut e = engine(&ks, 0, |_, _| false);
    let _ = e.start();
    let block = b"bad".to_vec();
    let p = brdu_bft::Proposal::sign(CHAIN, HEIGHT, 0, -1, block.clone(), hash(&block), &ks[3]);
    let out = e.handle(Event::Proposal(p));
    let vote = out.iter().find_map(|a| match a {
        Action::Broadcast(Message::Vote(v)) => Some(v.clone()),
        _ => None,
    });
    assert_eq!(
        vote.map(|v| v.block_hash),
        Some(None),
        "false valid_fn = nil prevote"
    );
}

#[test]
fn proposal_from_someone_who_is_not_the_round_proposer_is_ignored() {
    let ks = keys(4);
    let mut e = engine(&ks, 0, |_, _| true);
    let _ = e.start();
    let block = b"x".to_vec();
    let p = brdu_bft::Proposal::sign(CHAIN, HEIGHT, 0, -1, block.clone(), hash(&block), &ks[1]);
    assert!(e.handle(Event::Proposal(p)).is_empty());
    // hash that does not match the block
    let p = brdu_bft::Proposal::sign(CHAIN, HEIGHT, 0, -1, block, hash(b"another"), &ks[3]);
    assert!(e.handle(Event::Proposal(p)).is_empty());
}

#[test]
fn restoring_from_the_journal_does_not_contradict_what_was_already_signed() {
    let ks = keys(4);
    // node 0 prevotes A (proposal from node 3), crashes and comes back with the journal
    let mut e = engine(&ks, 0, |_, _| true);
    let mut journal = Vec::new();
    let log = |acts: &[Action], j: &mut Vec<Message>| {
        for a in acts {
            if let Action::Broadcast(m) = a {
                j.push(m.clone());
            }
        }
    };
    log(&e.start(), &mut journal);
    let block = b"A".to_vec();
    let p = brdu_bft::Proposal::sign(CHAIN, HEIGHT, 0, -1, block.clone(), hash(&block), &ks[3]);
    log(&e.handle(Event::Proposal(p.clone())), &mut journal);
    let mine: Vec<&Vote> = journal
        .iter()
        .filter_map(|m| {
            if let Message::Vote(v) = m {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(mine.len(), 1);

    let mut e2 = engine(&ks, 0, |_, _| true);
    let acts = e2.restore(&journal);
    assert_eq!(
        e2.step(),
        Step::Prevote,
        "resumes after the prevote already cast"
    );
    assert_eq!(e2.round(), 0);
    assert!(acts.contains(&Action::ScheduleTimeout(Step::Prevote, 0)));
    // redelivering the same proposal does not generate a new prevote (already voted)
    let again = e2.handle(Event::Proposal(p));
    assert!(again.iter().all(
        |a| !matches!(a, Action::Broadcast(Message::Vote(v)) if v.vote_type == VoteType::Prevote)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(40), ..ProptestConfig::default() })]

    /// Safety under any delivery order and with up to f=1 crashed or mute out of 4: never two final blocks, every
    /// certificate checks out, and (with <= f failures) all live nodes decide.
    #[test]
    fn safety_and_liveness_with_up_to_f_failures(seed in any::<u64>(), faulty in 0usize..4, kind in 0u8..3) {
        let mut modes = vec![Mode::Honest; 4];
        match kind {
            1 => modes[faulty] = Mode::Crashed,
            2 => modes[faulty] = Mode::Mute,
            _ => {}
        }
        let mut s = Sim::new(4, modes, seed);
        s.run(100_000);
        s.assert_safety();
        prop_assert!(s.live_all_decided());
    }

    /// More failures than f: it may not decide, but it **never diverges**.
    #[test]
    fn never_diverges_even_with_more_than_f_failures(seed in any::<u64>(), mask in 0u8..16) {
        let modes: Vec<Mode> = (0..4).map(|i| if mask >> i & 1 == 1 { Mode::Mute } else { Mode::Honest }).collect();
        let mut s = Sim::new(4, modes, seed);
        s.run(20_000);
        s.assert_safety();
    }
}
