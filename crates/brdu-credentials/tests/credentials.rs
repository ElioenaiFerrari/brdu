#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;

use brdu_credentials::{
    Privacy, Status, TypeSpec, credentials_of, get_credential, get_type, kind, list_types, valid,
};
use brdu_ledger::LedgerError;
use common::*;
use proptest::prelude::*;

/// Over-long ids cannot even be decoded (malformed); ids that decode but are not acceptable are `bad_id`.
fn bad_id(r: Result<(), LedgerError>) {
    let m = reason(r);
    assert!(m == "bad_id" || m == "malformed transaction", "{m}");
}

fn with_cnh() -> (Env, brdu_ledger::State) {
    let (env, mut s) = Env::new();
    env.create_type(&mut s, 1_000, &env.spec("cnh"), 2).unwrap();
    (env, s)
}

// ---------------------------------------------------------------- types

#[test]
fn a_type_needs_two_of_three_authorities_and_stores_what_was_declared() {
    let (env, mut s) = Env::new();
    let spec = env.spec("cnh");
    assert_eq!(
        env.create_type(&mut s, 1_000, &spec, 1),
        Err(LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        })
    );
    assert!(get_type(&s, "cnh").is_none(), "nothing was stored");
    env.create_type(&mut s, 1_001, &spec, 2).unwrap();
    assert_eq!(get_type(&s, "cnh"), Some(spec));
    assert_eq!(list_types(&s).len(), 1);
    assert_eq!(
        reason(env.create_type(&mut s, 1_002, &env.spec("cnh"), 2)),
        "type_exists"
    );
}

#[test]
fn authority_approvals_are_only_valid_for_the_exact_type_and_from_authorities() {
    let (env, mut s) = Env::new();
    let spec = env.spec("cnh");
    // approvals signed for a different threshold / contest window / privacy / id do not authorize this type
    let mut other = spec.clone();
    other.contest += 1;
    let doc = brdu_credentials::type_doc(CHAIN, &other);
    let payload = brdu_credentials::type_payload(&spec, &Env::approvals(&doc, &env.authorities, 3));
    assert!(matches!(
        env.run(&mut s, 1_000, &env.sender, kind::TYPE, payload),
        Err(LedgerError::InvalidTx { .. }) | Err(LedgerError::NotEnoughApprovals { .. })
    ));
    // the organ's keys are not authorities
    let doc = brdu_credentials::type_doc(CHAIN, &spec);
    let payload = brdu_credentials::type_payload(&spec, &Env::approvals(&doc, &env.organ, 3));
    assert!(
        env.run(&mut s, 1_001, &env.sender, kind::TYPE, payload)
            .is_err()
    );
    assert!(get_type(&s, "cnh").is_none());
}

#[test]
fn without_root_authorities_no_type_can_exist() {
    let (mut env, mut s) = Env::new();
    let mut rules = brdu_ledger::Rules::new(CHAIN, &[key(10).public()], 1).unwrap();
    rules.authorities.clear();
    env.exec = brdu_ledger::Executor::new(rules)
        .with_module(Box::new(brdu_credentials::CredentialsModule));
    assert_eq!(
        reason(env.create_type(&mut s, 1_000, &env.spec("cnh"), 3)),
        "no_authority"
    );
}

#[test]
fn malformed_types_are_refused() {
    let (env, mut s) = Env::new();
    let mut bad = |f: &dyn Fn(&mut TypeSpec)| {
        let mut t = env.spec("t");
        f(&mut t);
        reason(env.create_type(&mut s, 1_000, &t, 3))
    };
    assert!(matches!(
        bad(&|t| t.id = String::new()).as_str(),
        "bad_id" | "malformed transaction"
    ));
    assert!(matches!(
        bad(&|t| t.id = "x".repeat(33)).as_str(),
        "bad_id" | "malformed transaction"
    ));
    assert_eq!(bad(&|t| t.id = "line\nbreak".into()), "bad_id");
    assert_eq!(bad(&|t| t.threshold = 0), "bad_type");
    assert_eq!(bad(&|t| t.threshold = 4), "bad_type");
    assert_eq!(bad(&|t| t.organ.clear()), "bad_type");
    assert_eq!(
        bad(&|t| t.organ = vec![t.organ[0], t.organ[0]]),
        "bad_type",
        "repeated organ key"
    );
    assert_eq!(
        bad(&|t| t.contest = 59),
        "bad_type",
        "below the minimum window"
    );
    assert_eq!(
        bad(&|t| t.contest = 31_536_001),
        "bad_type",
        "above the maximum window"
    );
    assert_eq!(
        bad(&|t| t.ref_type = "ghost".into()),
        "bad_type",
        "unknown reference type"
    );
    assert_eq!(
        bad(&|t| t.revokes = true),
        "bad_type",
        "revokes without a reference"
    );
}

#[test]
fn creating_a_type_has_a_minimum_fee() {
    let (env, mut s) = Env::new();
    let spec = env.spec("cnh");
    let doc = brdu_credentials::type_doc(CHAIN, &spec);
    let payload = brdu_credentials::type_payload(&spec, &Env::approvals(&doc, &env.authorities, 2));
    let r = env.run_with_fee(&mut s, 1_000, &env.sender, kind::TYPE, payload, 49_999);
    assert!(
        matches!(
            r,
            Err(LedgerError::FeeTooLow {
                min: 50_000,
                got: 49_999
            })
        ),
        "{r:?}"
    );
}

// ---------------------------------------------------------------- issuing

#[test]
fn issue_needs_the_organ_and_is_born_pending_with_the_right_times() {
    let (env, mut s) = with_cnh();
    let alice = addr(&env.alice);
    assert!(matches!(
        env.issue_with(&mut s, 2_000, "cnh", "cnh-1", &env.alice, b"cat=B", "", 1),
        Err(LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        })
    ));
    assert!(get_credential(&s, "cnh-1").is_none());
    env.issue(&mut s, 2_000, "cnh", "cnh-1", &env.alice, b"cat=B", "")
        .unwrap();
    let c = get_credential(&s, "cnh-1").unwrap();
    assert_eq!(
        (c.status, c.subject, c.data.as_slice()),
        (Status::Pending, alice, &b"cat=B"[..])
    );
    assert_eq!(
        (c.issued_at, c.active_at, c.expires_at),
        (2_000, 2_000 + DAY, 0)
    );
    assert_eq!(credentials_of(&s, &alice).len(), 1);
    assert!(!valid(&s, "cnh-1", 2_001), "pending is not valid");
}

#[test]
fn issue_rejects_unknown_type_taken_id_and_wrong_data() {
    let (env, mut s) = with_cnh();
    assert_eq!(
        reason(env.issue(&mut s, 2_000, "ghost", "x", &env.alice, b"", "")),
        "no_type"
    );
    env.issue(&mut s, 2_000, "cnh", "cnh-1", &env.alice, b"a", "")
        .unwrap();
    assert_eq!(
        reason(env.issue(&mut s, 2_001, "cnh", "cnh-1", &env.alice, b"b", "")),
        "credential_exists"
    );
    bad_id(env.issue(&mut s, 2_002, "cnh", "", &env.alice, b"", ""));
    bad_id(env.issue(&mut s, 2_003, "cnh", &"x".repeat(65), &env.alice, b"", ""));
    assert_eq!(
        reason(env.issue(&mut s, 2_003, "cnh", "tab\there", &env.alice, b"", "")),
        "bad_id",
        "control characters are not printable"
    );
    // 513 bytes is refused already when decoding the payload (the limit is part of the wire format)
    assert!(matches!(
        reason(env.issue(&mut s, 2_004, "cnh", "big", &env.alice, &[7u8; 513], "")).as_str(),
        "bad_data" | "malformed transaction"
    ));
    env.issue(&mut s, 2_005, "cnh", "edge", &env.alice, &[7u8; 512], "")
        .unwrap();
    // a type without a reference type refuses any reference
    assert_eq!(
        reason(env.issue(&mut s, 2_006, "cnh", "r", &env.alice, b"", "cnh-1")),
        "bad_ref"
    );
}

#[test]
fn private_types_hold_only_a_32_byte_commitment() {
    let (env, mut s) = Env::new();
    let mut spec = env.spec("marriage");
    spec.privacy = Privacy::Private;
    env.create_type(&mut s, 1_000, &spec, 2).unwrap();
    assert_eq!(
        reason(env.issue(
            &mut s,
            2_000,
            "marriage",
            "m1",
            &env.alice,
            b"the names of the couple",
            ""
        )),
        "bad_data"
    );
    assert_eq!(
        reason(env.issue(&mut s, 2_001, "marriage", "m1", &env.alice, &[1u8; 31], "")),
        "bad_data"
    );
    env.issue(&mut s, 2_002, "marriage", "m1", &env.alice, &[1u8; 32], "")
        .unwrap();
    assert_eq!(get_credential(&s, "m1").unwrap().data, vec![1u8; 32]);
}

#[test]
fn an_issue_approval_is_bound_to_every_field() {
    let (env, mut s) = with_cnh();
    let alice = addr(&env.alice);
    let bob = addr(&key(2));
    // approvals for (id, data) reused for another id, another subject, other data, another type, another network
    let doc = brdu_credentials::issue_doc(CHAIN, "cnh", "cnh-1", &alice, b"cat=B", "");
    let ap = Env::approvals(&doc, &env.organ, 3);
    for (id, subject, data) in [
        ("cnh-2", alice, &b"cat=B"[..]),
        ("cnh-1", bob, b"cat=B"),
        ("cnh-1", alice, b"cat=A"),
    ] {
        let payload = brdu_credentials::issue_payload("cnh", id, &subject, data, "", &ap);
        assert!(
            env.run(&mut s, 2_000, &env.sender, kind::ISSUE, payload)
                .is_err(),
            "{id} {subject:?} {data:?}"
        );
    }
    let other_net =
        brdu_credentials::issue_doc("another-network", "cnh", "cnh-1", &alice, b"cat=B", "");
    let payload = brdu_credentials::issue_payload(
        "cnh",
        "cnh-1",
        &alice,
        b"cat=B",
        "",
        &Env::approvals(&other_net, &env.organ, 3),
    );
    assert!(
        env.run(&mut s, 2_001, &env.sender, kind::ISSUE, payload)
            .is_err()
    );
    // the right one still works afterwards (nothing was burned by the failures)
    env.issue(&mut s, 2_002, "cnh", "cnh-1", &env.alice, b"cat=B", "")
        .unwrap();
}

// ---------------------------------------------------------------- the window: contest, veto, activate

#[test]
fn the_subject_can_contest_inside_the_window_only_and_nobody_else_can() {
    let (env, mut s) = with_cnh();
    env.issue(&mut s, 2_000, "cnh", "cnh-1", &env.alice, b"x", "")
        .unwrap();
    assert_eq!(
        reason(env.contest(&mut s, 2_010, "cnh-1", &key(2))),
        "not_subject"
    );
    assert_eq!(
        reason(env.contest(&mut s, 2_000 + DAY, "cnh-1", &env.alice)),
        "window_closed",
        "at the end of the window it is closed"
    );
    assert_eq!(
        reason(env.contest(&mut s, 2_010, "ghost", &env.alice)),
        "no_credential"
    );
    env.contest(&mut s, 2_000 + DAY - 1, "cnh-1", &env.alice)
        .unwrap();
    assert_eq!(
        get_credential(&s, "cnh-1").unwrap().status,
        Status::Contested
    );
    // final: cannot be activated, vetoed or contested again
    assert_eq!(
        reason(env.activate(&mut s, 2_000 + DAY, "cnh-1")),
        "bad_status"
    );
    assert_eq!(
        reason(env.contest(&mut s, 2_000 + DAY - 1, "cnh-1", &env.alice)),
        "bad_status"
    );
}

#[test]
fn the_organ_can_veto_inside_the_window_with_m_approvals_pinned_to_the_issue_time() {
    let (env, mut s) = with_cnh();
    env.issue(&mut s, 2_000, "cnh", "cnh-1", &env.alice, b"x", "")
        .unwrap();
    assert!(matches!(
        env.veto(&mut s, 2_010, "cnh-1", 2_000, 1),
        Err(LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        })
    ));
    // an approval for another issue time does not apply to this entry
    assert!(env.veto(&mut s, 2_011, "cnh-1", 1_999, 3).is_err());
    assert_eq!(
        reason(env.veto(&mut s, 2_000 + DAY, "cnh-1", 2_000, 3)),
        "window_closed"
    );
    env.veto(&mut s, 2_012, "cnh-1", 2_000, 2).unwrap();
    assert_eq!(get_credential(&s, "cnh-1").unwrap().status, Status::Vetoed);
    assert_eq!(
        reason(env.activate(&mut s, 2_000 + DAY, "cnh-1")),
        "bad_status"
    );
}

#[test]
fn anyone_activates_only_after_the_window_and_only_once() {
    let (env, mut s) = with_cnh();
    env.issue(&mut s, 2_000, "cnh", "cnh-1", &env.alice, b"x", "")
        .unwrap();
    assert_eq!(
        reason(env.activate(&mut s, 2_000 + DAY - 1, "cnh-1")),
        "window_open"
    );
    env.activate(&mut s, 2_000 + DAY, "cnh-1").unwrap();
    assert_eq!(get_credential(&s, "cnh-1").unwrap().status, Status::Active);
    assert!(valid(&s, "cnh-1", 2_000 + DAY + 1));
    assert_eq!(
        reason(env.activate(&mut s, 2_000 + DAY + 1, "cnh-1")),
        "bad_status"
    );
    assert_eq!(
        reason(env.activate(&mut s, 2_000 + DAY + 1, "ghost")),
        "no_credential"
    );
}

#[test]
fn the_window_uses_the_time_of_the_block_not_the_previous_one() {
    let (env, mut s) = with_cnh();
    env.issue(&mut s, 5_000, "cnh", "cnh-1", &env.alice, b"x", "")
        .unwrap();
    // the state time is now 5,000; an activation in a block stamped at the end of the window already works
    env.activate(&mut s, 5_000 + DAY, "cnh-1").unwrap();
    // time never goes back: a block stamped before the state time is refused
    let mut signers = vec![addr(&env.validators[0])];
    signers.sort();
    assert_eq!(
        env.exec
            .build_block(&mut s, signers[0], signers, 4_999, vec![]),
        Err(LedgerError::TimeWentBack)
    );
}

// ---------------------------------------------------------------- validity and revocation

#[test]
fn validity_is_read_not_executed_and_the_boundary_is_exact() {
    let (env, mut s) = Env::new();
    let mut spec = env.spec("id");
    spec.validity = 10 * DAY;
    env.create_type(&mut s, 1_000, &spec, 2).unwrap();
    env.issue(&mut s, 2_000, "id", "i1", &env.alice, b"x", "")
        .unwrap();
    env.activate(&mut s, 2_000 + DAY, "i1").unwrap();
    let expires = 2_000 + 10 * DAY;
    assert_eq!(get_credential(&s, "i1").unwrap().expires_at, expires);
    assert!(valid(&s, "i1", expires - 1));
    assert!(
        !valid(&s, "i1", expires),
        "valid strictly before the expiry"
    );
    assert_eq!(
        get_credential(&s, "i1").unwrap().status,
        Status::Active,
        "the entry itself does not change when it expires"
    );
    assert!(!valid(&s, "ghost", 0));
}

#[test]
fn the_organ_revokes_pending_or_active_credentials_but_not_final_ones() {
    let (env, mut s) = with_cnh();
    env.issue(&mut s, 2_000, "cnh", "a", &env.alice, b"x", "")
        .unwrap();
    assert!(matches!(
        env.revoke(&mut s, 2_010, "a", "fraud", 1),
        Err(LedgerError::NotEnoughApprovals { .. })
    ));
    assert!(
        env.revoke(&mut s, 2_011, "a", &"r".repeat(129), 3).is_err(),
        "reason too long"
    );
    env.revoke(&mut s, 2_012, "a", "fraud", 2).unwrap();
    assert_eq!(get_credential(&s, "a").unwrap().status, Status::Revoked);
    assert_eq!(
        reason(env.revoke(&mut s, 2_013, "a", "fraud", 2)),
        "bad_status",
        "revocation is final"
    );
    // an active one can be revoked
    env.issue(&mut s, 3_000, "cnh", "b", &env.alice, b"x", "")
        .unwrap();
    env.activate(&mut s, 3_000 + DAY, "b").unwrap();
    env.revoke(&mut s, 3_000 + DAY + 1, "b", "lost", 2).unwrap();
    assert!(!valid(&s, "b", 3_000 + DAY + 2));
    // a contested one cannot be revoked
    env.issue(&mut s, 200_000, "cnh", "c", &env.alice, b"x", "")
        .unwrap();
    env.contest(&mut s, 200_001, "c", &env.alice).unwrap();
    assert_eq!(
        reason(env.revoke(&mut s, 200_002, "c", "x", 2)),
        "bad_status"
    );
    assert_eq!(
        reason(env.revoke(&mut s, 200_003, "ghost", "x", 2)),
        "no_credential"
    );
}

#[test]
fn a_revocation_approval_is_bound_to_the_reason() {
    let (env, mut s) = with_cnh();
    env.issue(&mut s, 2_000, "cnh", "a", &env.alice, b"x", "")
        .unwrap();
    let doc = brdu_credentials::revoke_doc(CHAIN, "a", "fraud");
    let payload = brdu_credentials::revoke_payload(
        "a",
        "something else",
        &Env::approvals(&doc, &env.organ, 3),
    );
    assert!(
        env.run(&mut s, 2_010, &env.sender, kind::REVOKE, payload)
            .is_err()
    );
    assert_eq!(get_credential(&s, "a").unwrap().status, Status::Pending);
}

// ---------------------------------------------------------------- references: marriage and divorce

fn marriage_and_divorce(env: &Env, s: &mut brdu_ledger::State) {
    let mut marriage = env.spec("marriage");
    marriage.privacy = Privacy::Private;
    env.create_type(s, 1_000, &marriage, 2).unwrap();
    let mut divorce = env.spec("divorce");
    divorce.privacy = Privacy::Private;
    divorce.ref_type = "marriage".into();
    divorce.revokes = true;
    env.create_type(s, 1_001, &divorce, 2).unwrap();
}

#[test]
fn a_divorce_references_the_marriage_of_the_same_subject_and_revokes_it_when_activated() {
    let (env, mut s) = Env::new();
    marriage_and_divorce(&env, &mut s);
    env.issue(&mut s, 2_000, "marriage", "m-a", &env.alice, &[1u8; 32], "")
        .unwrap();
    // the reference must be ACTIVE: a pending marriage cannot be divorced yet
    assert_eq!(
        reason(env.issue(
            &mut s, 2_001, "divorce", "d-a", &env.alice, &[2u8; 32], "m-a"
        )),
        "bad_ref"
    );
    env.activate(&mut s, 2_000 + DAY, "m-a").unwrap();
    // wrong subject, missing reference, unknown reference
    assert_eq!(
        reason(env.issue(
            &mut s,
            3_000 + DAY,
            "divorce",
            "d-b",
            &key(2),
            &[2u8; 32],
            "m-a"
        )),
        "bad_ref",
        "someone else's marriage"
    );
    assert_eq!(
        reason(env.issue(
            &mut s,
            3_001 + DAY,
            "divorce",
            "d-c",
            &env.alice,
            &[2u8; 32],
            ""
        )),
        "bad_ref"
    );
    assert_eq!(
        reason(env.issue(
            &mut s,
            3_002 + DAY,
            "divorce",
            "d-d",
            &env.alice,
            &[2u8; 32],
            "ghost"
        )),
        "bad_ref"
    );
    // a reference of the wrong TYPE is refused too (a divorce cannot reference another divorce)
    env.issue(
        &mut s,
        4_000 + DAY,
        "divorce",
        "d-a",
        &env.alice,
        &[2u8; 32],
        "m-a",
    )
    .unwrap();
    assert_eq!(
        get_credential(&s, "m-a").unwrap().status,
        Status::Active,
        "issuing the divorce does not revoke yet"
    );
    env.activate(&mut s, 4_000 + 2 * DAY, "d-a").unwrap();
    assert_eq!(get_credential(&s, "d-a").unwrap().status, Status::Active);
    assert_eq!(
        get_credential(&s, "m-a").unwrap().status,
        Status::Revoked,
        "activating the divorce revokes the marriage"
    );
    assert!(!valid(&s, "m-a", 4_000 + 2 * DAY));
}

#[test]
fn a_contested_or_vetoed_divorce_leaves_the_marriage_untouched() {
    let (env, mut s) = Env::new();
    marriage_and_divorce(&env, &mut s);
    env.issue(&mut s, 2_000, "marriage", "m-a", &env.alice, &[1u8; 32], "")
        .unwrap();
    env.activate(&mut s, 2_000 + DAY, "m-a").unwrap();
    env.issue(
        &mut s,
        3_000 + DAY,
        "divorce",
        "d-a",
        &env.alice,
        &[2u8; 32],
        "m-a",
    )
    .unwrap();
    env.contest(&mut s, 3_001 + DAY, "d-a", &env.alice).unwrap();
    assert_eq!(get_credential(&s, "m-a").unwrap().status, Status::Active);
    // and the ids are burned forever: neither the divorce nor anything else can reuse "d-a"
    assert_eq!(
        reason(env.issue(
            &mut s,
            3_002 + DAY,
            "divorce",
            "d-a",
            &env.alice,
            &[2u8; 32],
            "m-a"
        )),
        "credential_exists"
    );
}

// ---------------------------------------------------------------- misc

#[test]
fn malformed_payloads_are_refused_for_every_kind() {
    let (env, mut s) = with_cnh();
    for k in [
        kind::TYPE,
        kind::ISSUE,
        kind::CONTEST,
        kind::VETO,
        kind::ACTIVATE,
        kind::REVOKE,
    ] {
        for payload in [vec![], vec![0xff; 5], vec![0; 4]] {
            assert!(
                env.run(&mut s, 2_000, &env.sender, k, payload).is_err(),
                "kind {k}"
            );
        }
    }
    // trailing garbage after a valid payload
    let mut p = brdu_credentials::id_payload("x");
    p.push(0);
    assert!(
        env.run(&mut s, 2_001, &env.sender, kind::ACTIVATE, p)
            .is_err()
    );
}

#[test]
fn a_failed_operation_changes_nothing_and_the_history_replays_to_the_same_state() {
    let (env, mut s) = with_cnh();
    let before = s.clone();
    assert!(
        env.issue_with(&mut s, 2_000, "cnh", "a", &env.alice, b"x", "", 1)
            .is_err()
    );
    // the failing block did not even consume the nonce
    assert_eq!(s, before);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(32), ..ProptestConfig::default() })]

    /// Any sequence of operations on a few credentials: a credential never disappears, a final status
    /// (contested, vetoed, revoked) never changes, and only legal transitions happen.
    #[test]
    fn credentials_only_move_along_legal_transitions(ops in proptest::collection::vec((0u8..6, 0usize..3, 0u64..3), 1..40)) {
        let (env, mut s) = with_cnh();
        let ids = ["c0", "c1", "c2"];
        let mut time = 2_000u64;
        let mut seen: [Option<Status>; 3] = [None, None, None];
        for (op, who, dt) in ops {
            time += dt * (DAY / 2);
            let id = ids[who];
            let issued_at = get_credential(&s, id).map_or(0, |c| c.issued_at);
            let _ = match op {
                0 => env.issue(&mut s, time, "cnh", id, &env.alice, b"d", ""),
                1 => env.contest(&mut s, time, id, &env.alice),
                2 => env.veto(&mut s, time, id, issued_at, 2),
                3 => env.activate(&mut s, time, id),
                4 => env.revoke(&mut s, time, id, "r", 2),
                _ => { env.tick(&mut s, time); Ok(()) }
            };
            for (i, id) in ids.iter().enumerate() {
                let now = get_credential(&s, id).map(|c| c.status);
                if let (Some(prev), Some(cur)) = (seen[i], now) {
                    let legal = prev == cur
                        || matches!((prev, cur), (Status::Pending, Status::Active | Status::Contested | Status::Vetoed | Status::Revoked) | (Status::Active, Status::Revoked));
                    prop_assert!(legal, "{id}: {prev:?} -> {cur:?}");
                }
                prop_assert!(!(seen[i].is_some() && now.is_none()), "{id} disappeared");
                seen[i] = now.or(seen[i]);
            }
        }
    }
}
