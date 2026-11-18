#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;

use std::collections::HashMap;

use brdu_crypto::{Address, Hash32};
use brdu_ledger::coin::Approval;
use brdu_ledger::{Block, Coverage, LedgerError, State, Tx, kind};
use common::*;
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Op {
    Mint {
        to: usize,
        amount: u64,
        r: u8,
        approvals: usize,
    },
    Burn {
        from: usize,
        amount: u64,
        r: u8,
    },
    Transfer {
        from: usize,
        to: usize,
        amount: u64,
    },
    Attest {
        reserve: u64,
        skip: bool,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0usize..4, 0u64..50_000_000, 0u8..6, 0usize..4).prop_map(|(to, amount, r, approvals)| {
            Op::Mint {
                to,
                amount,
                r,
                approvals,
            }
        }),
        (0usize..4, 0u64..60_000_000, 0u8..6).prop_map(|(from, amount, r)| Op::Burn {
            from,
            amount,
            r
        }),
        (0usize..4, 0usize..4, 0u64..60_000_000).prop_map(|(from, to, amount)| Op::Transfer {
            from,
            to,
            amount
        }),
        (0u64..200_000_000, any::<bool>()).prop_map(|(reserve, skip)| Op::Attest { reserve, skip }),
    ]
}

fn actors(env: &Env) -> Vec<brdu_crypto::SecretKey> {
    vec![
        env.alice.clone_for_test(),
        env.bob.clone_for_test(),
        env.carol.clone_for_test(),
        key(4),
    ]
}

trait CloneForTest {
    fn clone_for_test(&self) -> Self;
}
impl CloneForTest for brdu_crypto::SecretKey {
    fn clone_for_test(&self) -> Self {
        brdu_crypto::SecretKey::from_bytes(*self.expose_seed())
    }
}

fn build(env: &Env, s: &State, who: &[brdu_crypto::SecretKey], op: &Op) -> Tx {
    match *op {
        Op::Mint {
            to,
            amount,
            r,
            approvals,
        } => {
            let nonce = s.nonce(&addr(&env.relayer));
            tx(
                &env.relayer,
                kind::MINT,
                nonce,
                0,
                env.mint_payload(&addr(&who[to]), amount, &format!("m{r}"), approvals),
            )
        }
        Op::Burn { from, amount, r } => {
            let nonce = s.nonce(&addr(&who[from]));
            tx(
                &who[from],
                kind::BURN,
                nonce,
                0,
                env.burn_payload(&addr(&who[from]), amount, &format!("b{r}"), 2),
            )
        }
        Op::Transfer { from, to, amount } => transfer(env, s, &who[from], &addr(&who[to]), amount),
        Op::Attest { reserve, skip } => {
            let seq = s.attestation().map_or(1, |a| a.seq + 1) + u64::from(skip);
            attest(env, s, seq, reserve, "banco")
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(48), ..ProptestConfig::default() })]

    /// Para qualquer sequência de operações (válidas e inválidas), depois de cada bloco: a soma dos saldos é a
    /// oferta, a raiz do estado é a do cabeçalho, as recusadas não mudam nada, e REPETIR todos os blocos do zero
    /// dá exatamente o mesmo estado (determinismo).
    #[test]
    fn invariantes_do_ledger_em_sequencias_aleatorias(ops in proptest::collection::vec(op(), 1..40)) {
        let env = Env::new();
        let who = actors(&env);
        let mut state = State::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut sent: HashMap<Address, u64> = HashMap::new();

        for (i, op) in ops.iter().enumerate() {
            let t = build(&env, &state, &who, op);
            let mut probe = state.clone();
            match env.exec.apply_tx(&mut probe, &t) {
                Ok(()) => {
                    let b = env.exec.build_block(&mut state, env.proposer(), env.signers(), 1_000 + i as u64, vec![t.clone()]).unwrap();
                    env.exec.apply_block(&mut state, &b).unwrap();
                    prop_assert_eq!(state.root(), b.state_root);
                    *sent.entry(t.sender_address()).or_default() += 1;
                    blocks.push(b);
                }
                Err(e) => {
                    let before = state.clone();
                    let again = env.exec.apply_tx(&mut state, &t).unwrap_err();
                    prop_assert_eq!(&again, &e);
                    prop_assert_eq!(&state, &before, "recusada mudou o estado: {:?}", e);
                }
            }
            // oferta: toda unidade emitida e não queimada está em alguma conta (a taxa é repartida, não destruída)
            prop_assert_eq!(total_balances(&state), u128::from(state.supply()));
            prop_assert_eq!(state.supply(), state.minted() - state.burned());
            // cobertura coerente com a oferta
            match (state.attestation(), state.coverage()) {
                (None, c) => prop_assert_eq!(c, Coverage::None),
                (Some(a), Coverage::Covered(m)) => prop_assert_eq!(a.reserve - state.supply(), m),
                (Some(a), Coverage::Under(f)) => prop_assert_eq!(state.supply() - a.reserve, f),
                (Some(_), Coverage::None) => prop_assert!(false),
            }
        }
        // nonce = quantas transações a conta enviou
        for (a, n) in &sent {
            prop_assert_eq!(state.nonce(a), *n);
        }
        // determinismo: o mesmo histórico dá o mesmo estado
        let mut replay = State::new();
        for b in &blocks {
            env.exec.apply_block(&mut replay, b).unwrap();
        }
        prop_assert_eq!(replay.root(), state.root());
        prop_assert_eq!(&replay, &state);
    }
}

#[test]
fn a_raiz_nao_depende_da_ordem_em_que_o_estado_foi_montado() {
    let addrs: Vec<Address> = (1..=6u8).map(|n| Address(Hash32([n; 32]))).collect();
    let mut a = State::new();
    let mut b = State::new();
    for (i, x) in addrs.iter().enumerate() {
        a.credit(x, 100 + i as u64).unwrap();
    }
    for (i, x) in addrs.iter().enumerate().rev() {
        b.credit(x, 100 + i as u64).unwrap();
    }
    assert_eq!(a.root(), b.root());
    assert_eq!(a, b);
    b.credit(&addrs[0], 1).unwrap();
    assert_ne!(a.root(), b.root());
}

#[test]
fn conta_zerada_nao_existe_e_a_raiz_volta_ao_vazio() {
    let x = Address(Hash32([7; 32]));
    let empty = State::new().root();
    let mut s = State::new();
    s.credit(&x, 5).unwrap();
    assert_ne!(s.root(), empty);
    s.debit(&x, 5).unwrap();
    assert_eq!(
        s.accounts_len(),
        0,
        "forma canônica: saldo e nonce zero removem a conta"
    );
    assert_eq!(s.root(), empty);
}

#[test]
fn atomically_desfaz_tudo_inclusive_aninhado_e_speculate_sempre_desfaz() {
    let x = Address(Hash32([1; 32]));
    let y = Address(Hash32([2; 32]));
    let mut s = State::new();
    s.credit(&x, 100).unwrap();
    let before = s.clone();
    let root = s.root();

    let r: Result<(), LedgerError> = s.atomically(|st| {
        st.debit(&x, 30)?;
        st.credit(&y, 30)?;
        // um aninhado que falha: só o interno desfaz
        let inner: Result<(), LedgerError> = st.atomically(|st2| {
            st2.credit(&y, 1)?;
            Err(LedgerError::Overflow)
        });
        assert_eq!(inner, Err(LedgerError::Overflow));
        assert_eq!(st.balance(&y), 30, "o interno desfez o dele e nada mais");
        // e agora o externo também falha: tudo volta
        Err(LedgerError::ZeroAmount)
    });
    assert_eq!(r, Err(LedgerError::ZeroAmount));
    assert_eq!(s, before);
    assert_eq!(s.root(), root);

    let seen = s.speculate(|st| {
        st.credit(&y, 999).unwrap();
        st.balance(&y)
    });
    assert_eq!(seen, 999);
    assert_eq!(s, before, "speculate nunca deixa rastro");
}

#[test]
fn estado_so_credita_ate_o_limite_de_64_bits() {
    let x = Address(Hash32([1; 32]));
    let mut s = State::new();
    s.credit(&x, u64::MAX).unwrap();
    assert_eq!(s.credit(&x, 1), Err(LedgerError::Overflow));
    assert_eq!(
        s.debit(&Address(Hash32([9; 32])), 1),
        Err(LedgerError::InsufficientFunds)
    );
}

#[test]
fn aprovacao_serializa_em_96_bytes_e_volta_igual() {
    let doc = Hash32([5; 32]);
    let a = Approval::new(&doc, &key(10));
    let bytes = a.to_bytes();
    assert_eq!(bytes.len(), 96);
    assert_eq!(Approval::from_bytes(&bytes), Some(a));
    assert_eq!(Approval::from_bytes(&bytes[..95]), None);
    assert!(a.issuer.verify(doc.as_bytes(), &a.signature).is_ok());
}
