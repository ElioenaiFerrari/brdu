#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Asset module: classes, registration, trade with escrow and veto window, liens, conservation.

use brdu_assets::*;
use brdu_crypto::{Address, PublicKey, SecretKey};
use brdu_ledger::coin::Approval;
use brdu_ledger::{Executor, LedgerError, Rules, State, Tx};

const CHAIN: &str = "brdu-asset-test";
const WINDOW: u64 = 3_600;

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}
fn addr(k: &SecretKey) -> Address {
    Address::from_public_key(&k.public())
}
fn pubs(k: &[SecretKey]) -> Vec<PublicKey> {
    k.iter().map(SecretKey::public).collect()
}
fn ap(doc: &brdu_crypto::Hash32, keys: &[SecretKey], n: usize) -> Vec<Approval> {
    keys.iter().take(n).map(|k| Approval::new(doc, k)).collect()
}

struct Env {
    exec: Executor,
    authorities: Vec<SecretKey>,
    organ: Vec<SecretKey>,
    validators: Vec<SecretKey>,
    clock: std::cell::Cell<u64>,
    seller: SecretKey,
    buyer: SecretKey,
}

fn env() -> (Env, State) {
    let authorities: Vec<SecretKey> = (30..33).map(key).collect();
    let mut rules = Rules::new(CHAIN, &[key(10).public()], 1).unwrap();
    rules.authorities = pubs(&authorities);
    rules.authority_threshold = 2;
    let e = Env {
        exec: Executor::new(rules).with_module(Box::new(AssetsModule)),
        authorities,
        organ: (40..43).map(key).collect(),
        validators: (20..24).map(key).collect(),
        clock: std::cell::Cell::new(0),
        seller: key(1),
        buyer: key(2),
    };
    let mut s = State::new();
    s.credit(&addr(&e.seller), 50_000_000).unwrap();
    s.credit(&addr(&e.buyer), 50_000_000).unwrap();
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
        // block time never goes back: helpers with fixed times are lifted to the last block time
        let time = time.max(self.clock.get());
        let nonce = s.nonce(&addr(from));
        let probe = Tx::new_signed(from, CHAIN, k, nonce, 0, payload.clone()).unwrap();
        let fee = self.exec.min_fee(&probe).unwrap();
        let tx = Tx::new_signed(from, CHAIN, k, nonce, fee, payload).unwrap();
        let mut signers: Vec<Address> = self.validators.iter().map(addr).collect();
        signers.sort();
        let block = self
            .exec
            .build_block(s, signers[0], signers, time, vec![tx])
            .map_err(unwrap)?;
        self.exec
            .apply_block(s, &block)
            .map(|_| ())
            .map_err(unwrap)?;
        self.clock.set(time);
        Ok(())
    }
    fn class(&self, s: &mut State, window: u64, n: usize) -> Result<(), LedgerError> {
        let spec = ClassSpec {
            id: "imovel".into(),
            organ: pubs(&self.organ),
            threshold: 2,
            veto_window: window,
        };
        let d = class_doc(CHAIN, &spec.id, &spec.organ, 2, window);
        self.run(
            s,
            1_000,
            &self.seller,
            kind::CLASS,
            class_payload(&spec, &ap(&d, &self.authorities, n)),
        )
    }
    fn register(&self, s: &mut State, id: &str) {
        let owner = addr(&self.seller);
        let d = register_doc(CHAIN, "imovel", id, &owner, b"deed");
        self.run(
            s,
            1_001,
            &self.seller,
            kind::REGISTER,
            register_payload("imovel", id, &owner, b"deed", &ap(&d, &self.organ, 2)),
        )
        .unwrap();
    }
    fn offer(&self, s: &mut State, id: &str, price: u64) -> Result<(), LedgerError> {
        self.run(
            s,
            1_002,
            &self.seller,
            kind::OFFER,
            offer_payload(id, &addr(&self.buyer), price),
        )
    }
    fn accept(&self, s: &mut State, time: u64, id: &str) -> Result<(), LedgerError> {
        self.run(s, time, &self.buyer, kind::ACCEPT, asset_payload(id))
    }
    fn veto(&self, s: &mut State, time: u64, id: &str, n: usize) -> Result<(), LedgerError> {
        let (Some(a), Some(d)) = (get_asset(s, id), get_deal(s, id)) else {
            return Err(LedgerError::Module("no_deal"));
        };
        let doc = veto_doc(CHAIN, id, a.rev, &d.buyer, d.price, d.deadline);
        self.run(
            s,
            time,
            &self.seller,
            kind::VETO,
            veto_payload(id, &ap(&doc, &self.organ, n)),
        )
    }
    fn finalize(&self, s: &mut State, time: u64, id: &str) -> Result<(), LedgerError> {
        self.run(s, time, &self.buyer, kind::FINALIZE, asset_payload(id))
    }
    fn lien(&self, s: &mut State, id: &str, lien: &str, n: usize) -> Result<(), LedgerError> {
        let a = get_asset(s, id).unwrap();
        let d = lien_add_doc(CHAIN, id, a.rev, lien, "court-1");
        self.run(
            s,
            1_003,
            &self.seller,
            kind::LIEN_ADD,
            lien_add_payload(id, lien, "court-1", &ap(&d, &self.organ, n)),
        )
    }
}

fn unwrap(e: LedgerError) -> LedgerError {
    match e {
        LedgerError::InvalidTx { reason, .. } => *reason,
        other => other,
    }
}

fn reason(r: Result<(), LedgerError>) -> String {
    match r {
        Err(LedgerError::Module(m)) => m.to_string(),
        Err(LedgerError::NotEnoughApprovals { .. }) => "not_enough_approvals".into(),
        Err(other) => format!("{other:?}"),
        Ok(()) => "ok".into(),
    }
}

fn total(e: &Env, s: &State) -> u64 {
    let _ = e;
    [addr(&key(1)), addr(&key(2))]
        .iter()
        .map(|a| s.balance(a))
        .sum::<u64>()
        + escrow_total(s)
}

#[test]
fn class_needs_the_authorities_and_a_window_in_range() {
    let (e, mut s) = env();
    assert_eq!(reason(e.class(&mut s, WINDOW, 1)), "not_enough_approvals");
    assert_eq!(reason(e.class(&mut s, 1, 2)), "bad_class");
    e.class(&mut s, WINDOW, 2).unwrap();
    assert_eq!(get_class(&s, "imovel").unwrap().veto_window, WINDOW);
    assert_eq!(reason(e.class(&mut s, WINDOW, 2)), "class_exists");
}

#[test]
fn registration_needs_the_organ_and_a_free_id() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    let owner = addr(&e.seller);
    let d = register_doc(CHAIN, "imovel", "casa-1", &owner, b"deed");
    let p = register_payload("imovel", "casa-1", &owner, b"deed", &ap(&d, &e.organ, 1));
    assert_eq!(
        reason(e.run(&mut s, 1_001, &e.seller, kind::REGISTER, p)),
        "not_enough_approvals"
    );
    // an approval for other metadata is worthless
    let p = register_payload("imovel", "casa-1", &owner, b"OTHER", &ap(&d, &e.organ, 2));
    assert_eq!(
        reason(e.run(&mut s, 1_001, &e.seller, kind::REGISTER, p)),
        "not_enough_approvals"
    );
    e.register(&mut s, "casa-1");
    assert_eq!(get_asset(&s, "casa-1").unwrap().owner, owner);
    assert_eq!(assets_of(&s, &owner).len(), 1);
    let p = register_payload("imovel", "casa-1", &owner, b"deed", &ap(&d, &e.organ, 2));
    assert_eq!(
        reason(e.run(&mut s, 1_002, &e.seller, kind::REGISTER, p)),
        "asset_exists"
    );
}

#[test]
fn trade_finalizes_after_the_window_and_moves_money_and_ownership() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    let before = total(&e, &s);
    e.offer(&mut s, "casa-1", 10_000_000).unwrap();
    // only the buyer accepts
    assert_eq!(
        reason(e.run(
            &mut s,
            1_010,
            &e.seller,
            kind::ACCEPT,
            asset_payload("casa-1")
        )),
        "not_buyer"
    );
    e.accept(&mut s, 2_000, "casa-1").unwrap();
    assert_eq!(escrow_total(&s), 10_000_000);
    let deal = get_deal(&s, "casa-1").unwrap();
    assert_eq!(
        (deal.status, deal.deadline),
        (DealStatus::Accepted, 2_000 + WINDOW)
    );
    // too early
    assert_eq!(
        reason(e.finalize(&mut s, 2_000 + WINDOW - 1, "casa-1")),
        "window_open"
    );
    // exactly at the deadline it can finalize
    e.finalize(&mut s, 2_000 + WINDOW, "casa-1").unwrap();
    assert_eq!(get_asset(&s, "casa-1").unwrap().owner, addr(&e.buyer));
    assert_eq!(escrow_total(&s), 0);
    assert!(get_deal(&s, "casa-1").is_none());
    // fees are paid out of balances: nothing is created or lost
    assert!(total(&e, &s) <= before);
    assert!(
        before - total(&e, &s) < 1_000_000,
        "only fees left the two accounts"
    );
}

#[test]
fn veto_before_the_deadline_returns_the_escrow_and_keeps_the_owner() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    e.offer(&mut s, "casa-1", 10_000_000).unwrap();
    e.accept(&mut s, 2_000, "casa-1").unwrap();
    let after_accept = s.balance(&addr(&e.buyer));
    assert_eq!(
        reason(e.veto(&mut s, 2_100, "casa-1", 1)),
        "not_enough_approvals"
    );
    // too late
    assert_eq!(
        reason(e.veto(&mut s, 2_000 + WINDOW, "casa-1", 2)),
        "window_closed"
    );
    e.veto(&mut s, 2_100, "casa-1", 2).unwrap();
    assert_eq!(s.balance(&addr(&e.buyer)), after_accept + 10_000_000);
    assert_eq!(escrow_total(&s), 0);
    assert_eq!(get_asset(&s, "casa-1").unwrap().owner, addr(&e.seller));
    assert_eq!(get_asset(&s, "casa-1").unwrap().rev, 1);
}

#[test]
fn offers_have_rules_and_can_be_cancelled_only_before_the_accept() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    // not the owner
    assert_eq!(
        reason(e.run(
            &mut s,
            1_002,
            &e.buyer,
            kind::OFFER,
            offer_payload("casa-1", &addr(&e.seller), 1)
        )),
        "not_owner"
    );
    // self deal
    assert_eq!(
        reason(e.run(
            &mut s,
            1_002,
            &e.seller,
            kind::OFFER,
            offer_payload("casa-1", &addr(&e.seller), 1)
        )),
        "self_deal"
    );
    e.offer(&mut s, "casa-1", 5).unwrap();
    assert_eq!(reason(e.offer(&mut s, "casa-1", 6)), "deal_active");
    e.run(
        &mut s,
        1_005,
        &e.seller,
        kind::CANCEL,
        asset_payload("casa-1"),
    )
    .unwrap();
    assert!(get_deal(&s, "casa-1").is_none());
    e.offer(&mut s, "casa-1", 5).unwrap();
    e.accept(&mut s, 2_000, "casa-1").unwrap();
    assert_eq!(
        reason(e.run(
            &mut s,
            2_001,
            &e.seller,
            kind::CANCEL,
            asset_payload("casa-1")
        )),
        "bad_status"
    );
}

#[test]
fn accepting_without_money_fails_and_leaves_no_escrow() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    e.offer(&mut s, "casa-1", 900_000_000).unwrap();
    assert!(e.accept(&mut s, 2_000, "casa-1").is_err());
    assert_eq!(escrow_total(&s), 0);
    assert_eq!(get_deal(&s, "casa-1").unwrap().status, DealStatus::Offered);
}

#[test]
fn liens_block_offers_and_finalization_and_approvals_are_single_use() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    assert_eq!(
        reason(e.lien(&mut s, "casa-1", "lien-1", 1)),
        "not_enough_approvals"
    );
    // keep an approval for the CURRENT revision to try replaying it later
    let a0 = get_asset(&s, "casa-1").unwrap();
    let d0 = lien_add_doc(CHAIN, "casa-1", a0.rev, "lien-1", "court-1");
    let replay = lien_add_payload("casa-1", "lien-1", "court-1", &ap(&d0, &e.organ, 2));
    e.lien(&mut s, "casa-1", "lien-1", 2).unwrap();
    assert_eq!(get_asset(&s, "casa-1").unwrap().rev, 1);
    assert_eq!(reason(e.offer(&mut s, "casa-1", 5)), "has_lien");
    // the same signed approval is useless now (revision moved)
    assert_eq!(
        reason(e.run(&mut s, 1_004, &e.seller, kind::LIEN_ADD, replay)),
        "lien_exists"
    );
    // remove it, with the stored reference in the document
    let a = get_asset(&s, "casa-1").unwrap();
    let d = lien_remove_doc(CHAIN, "casa-1", a.rev, "lien-1", "court-1");
    assert_eq!(
        reason(e.run(
            &mut s,
            1_005,
            &e.seller,
            kind::LIEN_REMOVE,
            lien_remove_payload("casa-1", "nope", &ap(&d, &e.organ, 2))
        )),
        "no_lien"
    );
    e.run(
        &mut s,
        1_005,
        &e.seller,
        kind::LIEN_REMOVE,
        lien_remove_payload("casa-1", "lien-1", &ap(&d, &e.organ, 2)),
    )
    .unwrap();
    assert!(get_asset(&s, "casa-1").unwrap().liens.is_empty());
    e.offer(&mut s, "casa-1", 5).unwrap();
}

#[test]
fn a_lien_added_during_the_window_stops_finalization_until_removed() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    e.offer(&mut s, "casa-1", 5_000_000).unwrap();
    e.accept(&mut s, 2_000, "casa-1").unwrap();
    e.lien(&mut s, "casa-1", "lien-1", 2).unwrap();
    assert_eq!(
        reason(e.finalize(&mut s, 2_000 + WINDOW, "casa-1")),
        "has_lien"
    );
    assert_eq!(
        escrow_total(&s),
        5_000_000,
        "the money stays safe in escrow"
    );
}

#[test]
fn a_veto_approval_is_pinned_to_buyer_price_deadline_and_revision() {
    let (e, mut s) = env();
    e.class(&mut s, WINDOW, 2).unwrap();
    e.register(&mut s, "casa-1");
    e.offer(&mut s, "casa-1", 5_000_000).unwrap();
    e.accept(&mut s, 2_000, "casa-1").unwrap();
    let a = get_asset(&s, "casa-1").unwrap();
    let d = get_deal(&s, "casa-1").unwrap();
    for (buyer, price, deadline, rev) in [
        (addr(&e.seller), d.price, d.deadline, a.rev),
        (d.buyer, d.price + 1, d.deadline, a.rev),
        (d.buyer, d.price, d.deadline + 1, a.rev),
        (d.buyer, d.price, d.deadline, a.rev + 1),
    ] {
        let doc = veto_doc(CHAIN, "casa-1", rev, &buyer, price, deadline);
        let p = veto_payload("casa-1", &ap(&doc, &e.organ, 2));
        assert_eq!(
            reason(e.run(&mut s, 2_100, &e.seller, kind::VETO, p)),
            "not_enough_approvals"
        );
    }
    // the BRDC prefix would give a different document (domain separation)
    let other = veto_doc_with_prefix(
        "brdc/asset",
        CHAIN,
        "casa-1",
        a.rev,
        &d.buyer,
        d.price,
        d.deadline,
    );
    assert_ne!(
        other,
        veto_doc(CHAIN, "casa-1", a.rev, &d.buyer, d.price, d.deadline)
    );
}

#[test]
fn money_is_conserved_through_any_sequence_of_trade_steps() {
    use proptest::prelude::*;
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 40,
        ..ProptestConfig::default()
    });
    runner
        .run(
            &proptest::collection::vec((0u8..6, 0u64..8_000), 1..14),
            |steps| {
                let (e, mut s) = env();
                e.class(&mut s, WINDOW, 2).unwrap();
                e.register(&mut s, "casa-1");
                let mut t = 2_000u64;
                let fees_cap = total(&e, &s);
                for (op, dt) in steps {
                    t += dt;
                    let _ = match op {
                        0 => e.offer(&mut s, "casa-1", 3_000_000),
                        1 => e.accept(&mut s, t, "casa-1"),
                        2 => e.veto(&mut s, t, "casa-1", 2),
                        3 => e.finalize(&mut s, t, "casa-1"),
                        4 => e.lien(&mut s, "casa-1", &format!("l{t}"), 2),
                        _ => e.run(&mut s, t, &e.seller, kind::CANCEL, asset_payload("casa-1")),
                    };
                    // escrow is exactly the price of an accepted deal, and nothing is created
                    let accepted = get_deal(&s, "casa-1")
                        .filter(|d| d.status == DealStatus::Accepted)
                        .map_or(0, |d| d.price);
                    prop_assert_eq!(escrow_total(&s), accepted);
                    prop_assert!(total(&e, &s) <= fees_cap);
                }
                Ok(())
            },
        )
        .unwrap();
}
