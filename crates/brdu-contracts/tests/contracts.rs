#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Contracts: the three templates end to end, actors, time guards, validation and conservation of money.

use brdu_contracts::*;
use brdu_crypto::{Address, SecretKey};
use brdu_ledger::{Executor, LedgerError, Rules, State, Tx};

const CHAIN: &str = "brdu-contract-test";

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}
fn addr(k: &SecretKey) -> Address {
    Address::from_public_key(&k.public())
}

struct Env {
    exec: Executor,
    validators: Vec<SecretKey>,
    a: SecretKey,
    b: SecretKey,
    c: SecretKey,
    clock: std::cell::Cell<u64>,
}

fn env() -> (Env, State) {
    let rules = Rules::new(CHAIN, &[key(10).public()], 1).unwrap();
    let e = Env {
        exec: Executor::new(rules).with_module(Box::new(ContractsModule)),
        validators: (20..24).map(key).collect(),
        a: key(1),
        b: key(2),
        c: key(3),
        clock: std::cell::Cell::new(0),
    };
    let mut s = State::new();
    for k in [&e.a, &e.b, &e.c] {
        s.credit(&addr(k), 100_000_000).unwrap();
    }
    (e, s)
}

impl Env {
    fn run(
        &self,
        s: &mut State,
        time: u64,
        from: &SecretKey,
        k: u8,
        payload: Vec<u8>,
    ) -> Result<(), LedgerError> {
        let time = time.max(self.clock.get());
        let nonce = s.nonce(&addr(from));
        let probe = Tx::new_signed(from, CHAIN, k, nonce, 0, payload.clone()).unwrap();
        let fee = self.exec.min_fee(&probe).unwrap();
        let tx = Tx::new_signed(from, CHAIN, k, nonce, fee, payload).unwrap();
        let mut signers: Vec<Address> = self.validators.iter().map(addr).collect();
        signers.sort();
        let un = |e: LedgerError| match e {
            LedgerError::InvalidTx { reason, .. } => *reason,
            o => o,
        };
        let block = self
            .exec
            .build_block(s, signers[0], signers, time, vec![tx])
            .map_err(un)?;
        self.exec.apply_block(s, &block).map_err(un)?;
        self.clock.set(time);
        Ok(())
    }
    fn create(&self, s: &mut State, tpl: &str, id: &str, terms: &[u64]) -> Result<(), LedgerError> {
        self.run(
            s,
            1_000,
            &self.a,
            kind::CREATE,
            create_payload(tpl, id, &addr(&self.b), terms),
        )
    }
    fn act(
        &self,
        s: &mut State,
        time: u64,
        who: &SecretKey,
        id: &str,
        action: &str,
    ) -> Result<(), LedgerError> {
        self.run(s, time, who, kind::ACT, act_payload(id, action))
    }
}

fn reason(r: Result<(), LedgerError>) -> String {
    match r {
        Err(LedgerError::Module(m)) => m.to_string(),
        Err(o) => format!("{o:?}"),
        Ok(()) => "ok".into(),
    }
}

fn bal(s: &State, k: &SecretKey) -> u64 {
    s.balance(&addr(k))
}

fn total(e: &Env, s: &State) -> u64 {
    bal(s, &e.a) + bal(s, &e.b) + bal(s, &e.c) + escrow_total(s)
}

const FEE_SLACK: u64 = 1_000_000;

#[test]
fn sale_confirm_pays_the_seller_and_refund_expired_gives_the_buyer_back() {
    let (e, mut s) = env();
    let before = total(&e, &s);
    e.create(&mut s, "compra-venda", "cv-1", &[5_000_000, 3_600])
        .unwrap();
    assert_eq!(
        reason(e.act(&mut s, 1_001, &e.c, "cv-1", "accept")),
        "not_party"
    );
    e.act(&mut s, 2_000, &e.b, "cv-1", "accept").unwrap();
    assert_eq!(escrow_total(&s), 5_000_000);
    assert_eq!(get_contract(&s, "cv-1").unwrap().vars["deadline"], 5_600);
    assert_eq!(
        reason(e.act(&mut s, 2_100, &e.c, "cv-1", "refund_expired")),
        "too_early"
    );
    assert_eq!(
        reason(e.act(&mut s, 2_100, &e.b, "cv-1", "cancel")),
        "bad_state"
    );
    e.act(&mut s, 2_200, &e.b, "cv-1", "confirm").unwrap();
    let c = get_contract(&s, "cv-1").unwrap();
    assert_eq!(
        (c.state.as_str(), c.escrow, escrow_total(&s)),
        ("done", 0, 0)
    );
    assert!(
        before - total(&e, &s) < FEE_SLACK,
        "only fees left the accounts"
    );

    // a second sale that the deadline refunds, by a stranger
    e.create(&mut s, "compra-venda", "cv-2", &[5_000_000, 3_600])
        .unwrap();
    e.act(&mut s, 3_000, &e.b, "cv-2", "accept").unwrap();
    let funded = bal(&s, &e.b);
    e.act(&mut s, 3_000 + 3_600, &e.c, "cv-2", "refund_expired")
        .unwrap();
    assert_eq!(bal(&s, &e.b), funded + 5_000_000);
    assert_eq!(get_contract(&s, "cv-2").unwrap().state, "refunded");
}

#[test]
fn sale_seller_can_refund_and_cancel_only_before_acceptance() {
    let (e, mut s) = env();
    e.create(&mut s, "compra-venda", "cv-1", &[1_000_000, 600])
        .unwrap();
    e.act(&mut s, 1_100, &e.a, "cv-1", "cancel").unwrap();
    assert_eq!(get_contract(&s, "cv-1").unwrap().state, "cancelled");
    assert_eq!(
        reason(e.act(&mut s, 1_200, &e.b, "cv-1", "accept")),
        "bad_state"
    );
    e.create(&mut s, "compra-venda", "cv-2", &[1_000_000, 600])
        .unwrap();
    e.act(&mut s, 1_300, &e.b, "cv-2", "accept").unwrap();
    e.act(&mut s, 1_400, &e.a, "cv-2", "refund").unwrap();
    assert_eq!(escrow_total(&s), 0);
}

#[test]
fn accepting_without_funds_fails_whole() {
    let (e, mut s) = env();
    e.create(&mut s, "compra-venda", "cv-1", &[900_000_000, 600])
        .unwrap();
    assert!(e.act(&mut s, 1_100, &e.b, "cv-1", "accept").is_err());
    let c = get_contract(&s, "cv-1").unwrap();
    assert_eq!(
        (c.state.as_str(), c.escrow, escrow_total(&s)),
        ("open", 0, 0)
    );
}

#[test]
fn rent_runs_deposit_payments_and_settlement() {
    let (e, mut s) = env();
    let before = total(&e, &s);
    // rent 1 BRDU, 3 periods of 100 s, deposit 5, grace 50
    e.create(
        &mut s,
        "aluguel",
        "al-1",
        &[1_000_000, 100, 3, 5_000_000, 50],
    )
    .unwrap();
    e.act(&mut s, 2_000, &e.b, "al-1", "accept").unwrap();
    assert_eq!(
        get_contract(&s, "al-1").unwrap().vars["settle_at"],
        2_000 + 300 + 50
    );
    for _ in 0..3 {
        e.act(&mut s, 2_100, &e.b, "al-1", "pay_rent").unwrap();
    }
    assert_eq!(
        reason(e.act(&mut s, 2_200, &e.b, "al-1", "pay_rent")),
        "not_ready"
    );
    // all paid: the landlord returns the whole deposit early
    let b0 = bal(&s, &e.b);
    e.act(&mut s, 2_300, &e.a, "al-1", "settle_paid").unwrap();
    assert_eq!(bal(&s, &e.b), b0 + 5_000_000);
    assert_eq!(get_contract(&s, "al-1").unwrap().state, "closed");
    assert!(before - total(&e, &s) < FEE_SLACK);
}

#[test]
fn rent_settle_after_the_deadline_takes_the_unpaid_rent_from_the_deposit() {
    let (e, mut s) = env();
    e.create(
        &mut s,
        "aluguel",
        "al-1",
        &[1_000_000, 100, 3, 5_000_000, 50],
    )
    .unwrap();
    e.act(&mut s, 2_000, &e.b, "al-1", "accept").unwrap();
    e.act(&mut s, 2_100, &e.b, "al-1", "pay_rent").unwrap();
    assert_eq!(
        reason(e.act(&mut s, 2_349, &e.c, "al-1", "settle")),
        "too_early"
    );
    let (a0, b0) = (bal(&s, &e.a), bal(&s, &e.b));
    e.act(&mut s, 2_350, &e.c, "al-1", "settle").unwrap();
    // 2 unpaid periods = 2 BRDU to the landlord, the rest of the deposit back to the tenant
    assert_eq!(bal(&s, &e.a), a0 + 2_000_000);
    assert_eq!(bal(&s, &e.b), b0 + 3_000_000);
    assert_eq!(escrow_total(&s), 0);
}

#[test]
fn loan_locks_the_principal_then_pays_it_out_and_is_repaid_or_defaults() {
    let (e, mut s) = env();
    e.create(&mut s, "emprestimo", "em-1", &[10_000_000, 11_000_000, 600])
        .unwrap();
    assert_eq!(
        escrow_total(&s),
        10_000_000,
        "the creditor's money is locked at creation"
    );
    e.act(&mut s, 2_000, &e.b, "em-1", "accept").unwrap();
    assert_eq!(escrow_total(&s), 0);
    let a0 = bal(&s, &e.a);
    e.act(&mut s, 2_100, &e.b, "em-1", "repay").unwrap();
    assert_eq!(bal(&s, &e.a), a0 + 11_000_000);
    assert_eq!(get_contract(&s, "em-1").unwrap().state, "repaid");

    e.create(&mut s, "emprestimo", "em-2", &[1_000_000, 1_100_000, 600])
        .unwrap();
    e.act(&mut s, 3_000, &e.b, "em-2", "accept").unwrap();
    assert_eq!(
        reason(e.act(&mut s, 3_599, &e.c, "em-2", "mark_default")),
        "too_early"
    );
    e.act(&mut s, 3_600, &e.c, "em-2", "mark_default").unwrap();
    assert_eq!(get_contract(&s, "em-2").unwrap().state, "defaulted");

    // cancel before acceptance returns the principal
    e.create(&mut s, "emprestimo", "em-3", &[2_000_000, 2_100_000, 600])
        .unwrap();
    let a1 = bal(&s, &e.a);
    e.act(&mut s, 4_000, &e.a, "em-3", "cancel").unwrap();
    assert_eq!(
        bal(&s, &e.a),
        a1 + 2_000_000 - Rules::DEFAULT_CONTRACT_FEE,
        "the principal is back, minus the step fee"
    );
    assert_eq!(escrow_total(&s), 0);
}

#[test]
fn creation_is_validated() {
    let (e, mut s) = env();
    let same = create_payload("compra-venda", "x", &addr(&e.a), &[1_000_000, 600]);
    assert_eq!(
        reason(e.run(&mut s, 1_000, &e.a, kind::CREATE, same)),
        "self_deal"
    );
    assert_eq!(reason(e.create(&mut s, "nope", "x", &[1])), "no_template");
    assert_eq!(
        reason(e.create(&mut s, "compra-venda", "x", &[1_000_000])),
        "bad_terms"
    );
    assert_eq!(
        reason(e.create(&mut s, "compra-venda", "x", &[0, 600])),
        "bad_terms"
    );
    assert_eq!(
        reason(e.create(&mut s, "compra-venda", "x", &[1, 1])),
        "bad_terms",
        "window below the minimum"
    );
    assert_eq!(
        reason(e.create(&mut s, "aluguel", "x", &[u64::MAX, 100, 2, 0, 0])),
        "bad_terms",
        "rent times periods overflows"
    );
    assert_eq!(
        reason(e.create(&mut s, "aluguel", "x", &[1, 100, 2000, 0, 0])),
        "bad_terms"
    );
    e.create(&mut s, "compra-venda", "x", &[1_000_000, 600])
        .unwrap();
    assert_eq!(
        reason(e.create(&mut s, "compra-venda", "x", &[1_000_000, 600])),
        "contract_exists"
    );
    assert_eq!(
        reason(e.act(&mut s, 1_100, &e.b, "ghost", "accept")),
        "no_contract"
    );
    assert_eq!(
        reason(e.act(&mut s, 1_100, &e.b, "x", "fly")),
        "no_transition"
    );
    assert_eq!(contracts_of(&s, &addr(&e.b)).len(), 1);
    assert_eq!(contracts_of(&s, &addr(&e.c)).len(), 0);
}

#[test]
fn money_is_conserved_through_any_sequence_of_steps() {
    use proptest::prelude::*;
    let names = [
        "accept",
        "confirm",
        "refund",
        "refund_expired",
        "cancel",
        "pay_rent",
        "settle",
        "settle_paid",
        "repay",
        "mark_default",
    ];
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 40,
        ..ProptestConfig::default()
    });
    runner
        .run(
            &(
                0u8..3,
                proptest::collection::vec((0usize..10, 0u8..3, 0u64..900), 1..16),
            ),
            |(which, steps)| {
                let (e, mut s) = env();
                let cap = total(&e, &s);
                match which {
                    0 => e
                        .create(&mut s, "compra-venda", "k", &[3_000_000, 600])
                        .unwrap(),
                    1 => e
                        .create(&mut s, "aluguel", "k", &[1_000_000, 100, 3, 4_000_000, 50])
                        .unwrap(),
                    _ => e
                        .create(&mut s, "emprestimo", "k", &[3_000_000, 3_300_000, 600])
                        .unwrap(),
                }
                let mut t = 2_000u64;
                for (n, who, dt) in steps {
                    t += dt;
                    let k = [&e.a, &e.b, &e.c][usize::from(who)];
                    let _ = e.act(&mut s, t, k, "k", names[n]);
                    let c = get_contract(&s, "k").unwrap();
                    prop_assert_eq!(
                        escrow_total(&s),
                        c.escrow,
                        "the total escrow is this contract's escrow"
                    );
                    if TERMINAL.contains(&c.state.as_str()) {
                        prop_assert_eq!(c.escrow, 0, "final states hold no escrow");
                    }
                    prop_assert!(total(&e, &s) <= cap, "nothing is created");
                }
                Ok(())
            },
        )
        .unwrap();
}
