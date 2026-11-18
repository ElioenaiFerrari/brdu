#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Test environment: 3 root authorities (2 of 3), organs per credential type, an executor with the credentials
//! module, and helpers that run ONE transaction as ONE block at a chosen block time.

use brdu_credentials::{
    CredentialsModule, Privacy, TypeSpec, id_payload, issue_doc, issue_payload, kind, revoke_doc,
    revoke_payload, type_doc, type_payload, veto_doc, veto_payload,
};
use brdu_crypto::{Address, PublicKey, SecretKey};
use brdu_ledger::coin::Approval;
use brdu_ledger::{Executor, LedgerError, Rules, State, Tx};

pub const CHAIN: &str = "brdu-cred-test";
pub const DAY: u64 = 86_400;

pub fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

pub fn addr(k: &SecretKey) -> Address {
    Address::from_public_key(&k.public())
}

pub struct Env {
    pub exec: Executor,
    pub authorities: Vec<SecretKey>,
    /// The organ of the type "cnh": 3 keys, 2 of 3.
    pub organ: Vec<SecretKey>,
    pub sender: SecretKey,
    pub alice: SecretKey,
    pub validators: Vec<SecretKey>,
    pub time: u64,
}

impl Env {
    pub fn new() -> (Self, State) {
        let authorities: Vec<SecretKey> = (30..33).map(key).collect();
        let issuers = vec![key(10).public()];
        let mut rules = Rules::new(CHAIN, &issuers, 1).unwrap();
        rules.authorities = authorities.iter().map(SecretKey::public).collect();
        rules.authority_threshold = 2;
        let exec = Executor::new(rules).with_module(Box::new(CredentialsModule));
        let mut state = State::new();
        let sender = key(9);
        state.credit(&addr(&sender), 100_000_000).unwrap();
        let env = Self {
            exec,
            authorities,
            organ: (40..43).map(key).collect(),
            sender,
            alice: key(1),
            validators: (20..24).map(key).collect(),
            time: 1_000,
        };
        (env, state)
    }

    pub fn pubs(keys: &[SecretKey]) -> Vec<PublicKey> {
        keys.iter().map(SecretKey::public).collect()
    }

    pub fn approvals(doc: &brdu_crypto::Hash32, keys: &[SecretKey], n: usize) -> Vec<Approval> {
        keys.iter().take(n).map(|k| Approval::new(doc, k)).collect()
    }

    pub fn spec(&self, id: &str) -> TypeSpec {
        TypeSpec {
            id: id.into(),
            organ: Self::pubs(&self.organ),
            threshold: 2,
            contest: DAY,
            validity: 0,
            privacy: Privacy::Public,
            ref_type: String::new(),
            revokes: false,
        }
    }

    /// Runs one transaction as one block at `time`. `Ok` applies it; `Err` is the inner ledger error.
    pub fn run(
        &self,
        state: &mut State,
        time: u64,
        from: &SecretKey,
        tx_kind: u8,
        payload: Vec<u8>,
    ) -> Result<(), LedgerError> {
        let nonce = state.nonce(&addr(from));
        let probe = Tx::new_signed(from, CHAIN, tx_kind, nonce, 0, payload.clone()).unwrap();
        let fee = self.exec.min_fee(&probe).unwrap();
        let tx = Tx::new_signed(from, CHAIN, tx_kind, nonce, fee, payload).unwrap();
        let mut signers: Vec<Address> = self.validators.iter().map(addr).collect();
        signers.sort();
        let block = self
            .exec
            .build_block(state, signers[0], signers, time, vec![tx])
            .map_err(unwrap_invalid)?;
        self.exec
            .apply_block(state, &block)
            .map(|_| ())
            .map_err(unwrap_invalid)
    }

    /// The fee-too-low case needs a custom fee.
    pub fn run_with_fee(
        &self,
        state: &mut State,
        time: u64,
        from: &SecretKey,
        tx_kind: u8,
        payload: Vec<u8>,
        fee: u64,
    ) -> Result<(), LedgerError> {
        let nonce = state.nonce(&addr(from));
        let tx = Tx::new_signed(from, CHAIN, tx_kind, nonce, fee, payload).unwrap();
        let mut signers: Vec<Address> = self.validators.iter().map(addr).collect();
        signers.sort();
        self.exec
            .build_block(state, signers[0], signers, time, vec![tx])
            .map(|_| ())
            .map_err(unwrap_invalid)
    }

    /// An empty block, to move time forward.
    pub fn tick(&self, state: &mut State, time: u64) {
        let mut signers: Vec<Address> = self.validators.iter().map(addr).collect();
        signers.sort();
        let block = self
            .exec
            .build_block(state, signers[0], signers, time, vec![])
            .unwrap();
        self.exec.apply_block(state, &block).unwrap();
    }

    // ---- shortcuts that build and run each operation

    pub fn create_type(
        &self,
        state: &mut State,
        time: u64,
        spec: &TypeSpec,
        authorities_signing: usize,
    ) -> Result<(), LedgerError> {
        let doc = type_doc(CHAIN, spec);
        let payload = type_payload(
            spec,
            &Self::approvals(&doc, &self.authorities, authorities_signing),
        );
        self.run(state, time, &self.sender, kind::TYPE, payload)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        &self,
        state: &mut State,
        time: u64,
        type_id: &str,
        id: &str,
        subject: &SecretKey,
        data: &[u8],
        reference: &str,
    ) -> Result<(), LedgerError> {
        self.issue_with(state, time, type_id, id, subject, data, reference, 2)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn issue_with(
        &self,
        state: &mut State,
        time: u64,
        type_id: &str,
        id: &str,
        subject: &SecretKey,
        data: &[u8],
        reference: &str,
        approving: usize,
    ) -> Result<(), LedgerError> {
        let doc = issue_doc(CHAIN, type_id, id, &addr(subject), data, reference);
        let payload = issue_payload(
            type_id,
            id,
            &addr(subject),
            data,
            reference,
            &Self::approvals(&doc, &self.organ, approving),
        );
        self.run(state, time, &self.sender, kind::ISSUE, payload)
    }

    pub fn contest(
        &self,
        state: &mut State,
        time: u64,
        id: &str,
        by: &SecretKey,
    ) -> Result<(), LedgerError> {
        self.run(state, time, by, kind::CONTEST, id_payload(id))
    }

    pub fn veto(
        &self,
        state: &mut State,
        time: u64,
        id: &str,
        issued_at: u64,
        approving: usize,
    ) -> Result<(), LedgerError> {
        let doc = veto_doc(CHAIN, id, issued_at);
        self.run(
            state,
            time,
            &self.sender,
            kind::VETO,
            veto_payload(id, &Self::approvals(&doc, &self.organ, approving)),
        )
    }

    pub fn activate(&self, state: &mut State, time: u64, id: &str) -> Result<(), LedgerError> {
        self.run(state, time, &self.sender, kind::ACTIVATE, id_payload(id))
    }

    pub fn revoke(
        &self,
        state: &mut State,
        time: u64,
        id: &str,
        reason: &str,
        approving: usize,
    ) -> Result<(), LedgerError> {
        let doc = revoke_doc(CHAIN, id, reason);
        self.run(
            state,
            time,
            &self.sender,
            kind::REVOKE,
            revoke_payload(id, reason, &Self::approvals(&doc, &self.organ, approving)),
        )
    }
}

fn unwrap_invalid(e: LedgerError) -> LedgerError {
    match e {
        LedgerError::InvalidTx { reason, .. } => *reason,
        other => other,
    }
}

/// The reason string of a module error.
pub fn reason(r: Result<(), LedgerError>) -> String {
    match r.unwrap_err() {
        LedgerError::Module(m) => m.to_string(),
        other => other.to_string(),
    }
}
