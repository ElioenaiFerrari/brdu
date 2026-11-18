#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Test environment: 3 issuers (threshold 2), test accounts and 4 validators.

use brdu_crypto::{Address, SecretKey};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Block, Executor, LedgerError, Rules, State, Tx, kind};

pub const CHAIN: &str = "brdu-test";

pub fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

pub fn addr(k: &SecretKey) -> Address {
    Address::from_public_key(&k.public())
}

pub struct Env {
    pub exec: Executor,
    pub issuers: Vec<SecretKey>,
    pub alice: SecretKey,
    pub bob: SecretKey,
    pub carol: SecretKey,
    pub relayer: SecretKey,
    pub validators: Vec<SecretKey>,
}

impl Env {
    pub fn new() -> Self {
        Self::with_rules(|_| {})
    }

    pub fn with_rules(tune: impl FnOnce(&mut Rules)) -> Self {
        let issuers: Vec<SecretKey> = (10..13).map(key).collect();
        let pubs: Vec<_> = issuers.iter().map(SecretKey::public).collect();
        let mut rules = Rules::new(CHAIN, &pubs, 2).unwrap();
        tune(&mut rules);
        Self {
            exec: Executor::new(rules),
            issuers,
            alice: key(1),
            bob: key(2),
            carol: key(3),
            relayer: key(9),
            validators: (20..24).map(key).collect(),
        }
    }

    pub fn rules(&self) -> &Rules {
        self.exec.rules()
    }

    /// The validators in address order (as they go in the header).
    pub fn signers(&self) -> Vec<Address> {
        let mut v: Vec<Address> = self.validators.iter().map(addr).collect();
        v.sort();
        v
    }

    pub fn proposer(&self) -> Address {
        self.signers()[0]
    }

    /// Approvals from `n` issuers (the first ones) over `doc`.
    pub fn approvals(&self, doc: &brdu_crypto::Hash32, n: usize) -> Vec<Approval> {
        self.issuers
            .iter()
            .take(n)
            .map(|k| Approval::new(doc, k))
            .collect()
    }

    pub fn mint_payload(&self, to: &Address, amount: u64, reference: &str, n: usize) -> Vec<u8> {
        let doc = coin::issuance_doc(false, CHAIN, to, amount, reference);
        coin::issuance_payload(to, amount, reference, &self.approvals(&doc, n))
    }

    pub fn burn_payload(&self, from: &Address, amount: u64, reference: &str, n: usize) -> Vec<u8> {
        let doc = coin::issuance_doc(true, CHAIN, from, amount, reference);
        coin::issuance_payload(from, amount, reference, &self.approvals(&doc, n))
    }

    pub fn attest_payload(&self, seq: u64, reserve: u64, reference: &str, n: usize) -> Vec<u8> {
        let doc = coin::attestation_doc(CHAIN, seq, reserve, reference);
        coin::attestation_payload(seq, reserve, reference, &self.approvals(&doc, n))
    }
}

pub fn tx(key: &SecretKey, kind: u8, nonce: u64, fee: u64, payload: Vec<u8>) -> Tx {
    Tx::new_signed(key, CHAIN, kind, nonce, fee, payload).unwrap()
}

/// A transfer with the minimum fee and the right nonce for the state.
pub fn transfer(env: &Env, state: &State, from: &SecretKey, to: &Address, amount: u64) -> Tx {
    let nonce = state.nonce(&addr(from));
    let mut t = tx(
        from,
        kind::TRANSFER,
        nonce,
        0,
        coin::transfer_payload(to, amount),
    );
    let min = env.exec.min_fee(&t).unwrap();
    t = tx(
        from,
        kind::TRANSFER,
        nonce,
        min,
        coin::transfer_payload(to, amount),
    );
    t
}

pub fn mint(env: &Env, state: &State, to: &Address, amount: u64, reference: &str) -> Tx {
    let nonce = state.nonce(&addr(&env.relayer));
    tx(
        &env.relayer,
        kind::MINT,
        nonce,
        0,
        env.mint_payload(to, amount, reference, 2),
    )
}

pub fn burn(env: &Env, state: &State, from: &SecretKey, amount: u64, reference: &str) -> Tx {
    let nonce = state.nonce(&addr(from));
    tx(
        from,
        kind::BURN,
        nonce,
        0,
        env.burn_payload(&addr(from), amount, reference, 2),
    )
}

pub fn attest(env: &Env, state: &State, seq: u64, reserve: u64, reference: &str) -> Tx {
    let nonce = state.nonce(&addr(&env.relayer));
    tx(
        &env.relayer,
        kind::ATTESTATION,
        nonce,
        0,
        env.attest_payload(seq, reserve, reference, 2),
    )
}

/// Applies a transaction to the state and requires success.
pub fn ok(env: &Env, state: &mut State, t: &Tx) {
    env.exec.apply_tx(state, t).unwrap();
}

pub fn err(env: &Env, state: &mut State, t: &Tx) -> LedgerError {
    env.exec.apply_tx(state, t).unwrap_err()
}

/// Builds and applies a block with `txs`.
pub fn block_of(env: &Env, state: &mut State, time: u64, txs: Vec<Tx>) -> Block {
    let b = env
        .exec
        .build_block(state, env.proposer(), env.signers(), time, txs)
        .unwrap();
    env.exec.apply_block(state, &b).unwrap();
    b
}

/// Sum of all balances.
pub fn total_balances(state: &State) -> u128 {
    state.accounts().map(|(_, a)| u128::from(a.balance)).sum()
}

/// `okm!(env, state, tx_expression)`: builds the transaction (reading the state) and only then applies it (writing).
#[macro_export]
macro_rules! okm {
    ($env:expr, $s:expr, $t:expr) => {{
        let __t = $t;
        $crate::common::ok(&$env, &mut $s, &__t)
    }};
}

/// Like `okm!`, but expects the error and returns it.
#[macro_export]
macro_rules! errm {
    ($env:expr, $s:expr, $t:expr) => {{
        let __t = $t;
        $crate::common::err(&$env, &mut $s, &__t)
    }};
}
