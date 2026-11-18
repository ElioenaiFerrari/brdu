//! Deterministic state: accounts, supply, used references, reserve attestation and module storage.
//!
//! - **State root:** Merkle tree over the entries **in order**, so the root does not depend on the order in which
//!   the state was built. An account with zero balance and zero nonce does not exist (canonical form).
//! - **Atomicity:** changes inside [`State::atomically`] go to an undo journal; if something fails, everything
//!   rolls back, without copying the whole state.

use std::collections::{BTreeMap, BTreeSet};

use brdu_crypto::{Address, Hash32, merkle, tagged, tags};

use crate::error::LedgerError;

/// Balance and nonce of an account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Account {
    /// Balance in minimum units.
    pub balance: u64,
    /// Next expected nonce.
    pub nonce: u64,
}

/// Last reserve attestation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attestation {
    /// Sequence number (1, 2, 3…).
    pub seq: u64,
    /// Total reais in the vault, in minimum units (1 BRDU = R$ 1).
    pub reserve: u64,
    /// Reference (for example, the bank document).
    pub reference: String,
}

/// Coverage: the attested reserve against what circulates. **Information only**: the network blocks nothing (ADR-0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// There has never been an attestation.
    None,
    /// Reserve greater than or equal to supply; that many units are left over.
    Covered(u64),
    /// Reserve smaller than supply; that many units are missing.
    Under(u64),
}

#[derive(Debug, Clone)]
enum Undo {
    Account(Address, Option<Account>),
    Minted(u64),
    Burned(u64),
    Ref(u8, String),
    Attestation(Option<Attestation>),
    Meta {
        height: u64,
        time: u64,
        last_hash: Option<Hash32>,
    },
    Ext(Vec<u8>, Option<Vec<u8>>),
}

/// The ledger state.
#[derive(Debug, Clone, Default)]
pub struct State {
    accounts: BTreeMap<Address, Account>,
    minted: u64,
    burned: u64,
    refs: BTreeSet<(u8, String)>,
    attestation: Option<Attestation>,
    height: u64,
    time: u64,
    last_hash: Option<Hash32>,
    ext: BTreeMap<Vec<u8>, Vec<u8>>,
    undo: Vec<Undo>,
    depth: usize,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.accounts == other.accounts
            && self.minted == other.minted
            && self.burned == other.burned
            && self.refs == other.refs
            && self.attestation == other.attestation
            && self.height == other.height
            && self.time == other.time
            && self.last_hash == other.last_hash
            && self.ext == other.ext
    }
}
impl Eq for State {}

impl State {
    /// The genesis state: empty, height 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // --- reads ---------------------------------------------------------------------------------

    /// The account (zeroed if it does not exist).
    #[must_use]
    pub fn account(&self, address: &Address) -> Account {
        self.accounts.get(address).copied().unwrap_or_default()
    }

    /// Balance.
    #[must_use]
    pub fn balance(&self, address: &Address) -> u64 {
        self.account(address).balance
    }

    /// Next nonce.
    #[must_use]
    pub fn nonce(&self, address: &Address) -> u64 {
        self.account(address).nonce
    }

    /// How many accounts exist.
    #[must_use]
    pub fn accounts_len(&self) -> usize {
        self.accounts.len()
    }

    /// The accounts, in address order.
    pub fn accounts(&self) -> impl Iterator<Item = (&Address, &Account)> {
        self.accounts.iter()
    }

    /// Total created (mint).
    #[must_use]
    pub const fn minted(&self) -> u64 {
        self.minted
    }

    /// Total destroyed (burn).
    #[must_use]
    pub const fn burned(&self) -> u64 {
        self.burned
    }

    /// Supply in circulation: `minted − burned`.
    #[must_use]
    pub const fn supply(&self) -> u64 {
        self.minted.saturating_sub(self.burned)
    }

    /// Last reserve attestation.
    #[must_use]
    pub const fn attestation(&self) -> Option<&Attestation> {
        self.attestation.as_ref()
    }

    /// Reserve coverage (information only).
    #[must_use]
    pub fn coverage(&self) -> Coverage {
        match &self.attestation {
            None => Coverage::None,
            Some(a) if a.reserve >= self.supply() => Coverage::Covered(a.reserve - self.supply()),
            Some(a) => Coverage::Under(self.supply() - a.reserve),
        }
    }

    /// Height of the last applied block (0 at genesis).
    #[must_use]
    pub const fn height(&self) -> u64 {
        self.height
    }

    /// Time (Unix seconds) of the last block.
    #[must_use]
    pub const fn time(&self) -> u64 {
        self.time
    }

    /// Hash of the last applied block.
    #[must_use]
    pub const fn last_hash(&self) -> Option<Hash32> {
        self.last_hash
    }

    /// Has the reference (`kind`, text) already been used?
    #[must_use]
    pub fn ref_used(&self, kind: u8, reference: &str) -> bool {
        self.refs.contains(&(kind, reference.to_string()))
    }

    /// Read from a module's storage.
    #[must_use]
    pub fn ext_get(&self, key: &[u8]) -> Option<&[u8]> {
        self.ext.get(key).map(Vec::as_slice)
    }

    /// Entries of module storage whose key starts with `prefix`.
    pub fn ext_scan<'a>(
        &'a self,
        prefix: &'a [u8],
    ) -> impl Iterator<Item = (&'a [u8], &'a [u8])> + 'a {
        self.ext
            .range(prefix.to_vec()..)
            .take_while(move |(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
    }

    // --- atomicity -----------------------------------------------------------------------------

    /// Runs `f`; if it returns an error, **undoes everything** `f` changed. Can be nested.
    pub fn atomically<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, LedgerError>,
    ) -> Result<T, LedgerError> {
        let mark = self.undo.len();
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        match result {
            Ok(v) => {
                if self.depth == 0 {
                    self.undo.clear();
                }
                Ok(v)
            }
            Err(e) => {
                self.rollback_to(mark);
                if self.depth == 0 {
                    self.undo.clear();
                }
                Err(e)
            }
        }
    }

    /// Runs `f` and **always** undoes it (to compute a block's root without applying it).
    pub fn speculate<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let mark = self.undo.len();
        self.depth += 1;
        let out = f(self);
        self.depth -= 1;
        self.rollback_to(mark);
        if self.depth == 0 {
            self.undo.clear();
        }
        out
    }

    fn rollback_to(&mut self, mark: usize) {
        while self.undo.len() > mark {
            let Some(u) = self.undo.pop() else { break };
            match u {
                Undo::Account(addr, Some(acc)) => {
                    self.accounts.insert(addr, acc);
                }
                Undo::Account(addr, None) => {
                    self.accounts.remove(&addr);
                }
                Undo::Minted(v) => self.minted = v,
                Undo::Burned(v) => self.burned = v,
                Undo::Ref(k, r) => {
                    self.refs.remove(&(k, r));
                }
                Undo::Attestation(a) => self.attestation = a,
                Undo::Meta {
                    height,
                    time,
                    last_hash,
                } => {
                    self.height = height;
                    self.time = time;
                    self.last_hash = last_hash;
                }
                Undo::Ext(key, Some(v)) => {
                    self.ext.insert(key, v);
                }
                Undo::Ext(key, None) => {
                    self.ext.remove(&key);
                }
            }
        }
    }

    fn record(&mut self, u: Undo) {
        if self.depth > 0 {
            self.undo.push(u);
        }
    }

    // --- writes (for the executor and the modules) ---------------------------------------------

    /// Writes the account; zero balance and zero nonce remove the entry (canonical form).
    pub fn set_account(&mut self, address: Address, account: Account) {
        let old = self.accounts.get(&address).copied();
        self.record(Undo::Account(address, old));
        if account == Account::default() {
            self.accounts.remove(&address);
        } else {
            self.accounts.insert(address, account);
        }
    }

    /// Adds `amount` to the balance.
    pub fn credit(&mut self, address: &Address, amount: u64) -> Result<(), LedgerError> {
        let mut acc = self.account(address);
        acc.balance = acc
            .balance
            .checked_add(amount)
            .ok_or(LedgerError::Overflow)?;
        self.set_account(*address, acc);
        Ok(())
    }

    /// Subtracts `amount` from the balance.
    pub fn debit(&mut self, address: &Address, amount: u64) -> Result<(), LedgerError> {
        let mut acc = self.account(address);
        acc.balance = acc
            .balance
            .checked_sub(amount)
            .ok_or(LedgerError::InsufficientFunds)?;
        self.set_account(*address, acc);
        Ok(())
    }

    pub(crate) fn add_minted(&mut self, amount: u64) -> Result<(), LedgerError> {
        let new = self
            .minted
            .checked_add(amount)
            .ok_or(LedgerError::Overflow)?;
        self.record(Undo::Minted(self.minted));
        self.minted = new;
        Ok(())
    }

    pub(crate) fn add_burned(&mut self, amount: u64) -> Result<(), LedgerError> {
        let new = self
            .burned
            .checked_add(amount)
            .ok_or(LedgerError::Overflow)?;
        self.record(Undo::Burned(self.burned));
        self.burned = new;
        Ok(())
    }

    pub(crate) fn use_ref(&mut self, kind: u8, reference: &str) {
        if self.refs.insert((kind, reference.to_string())) {
            self.record(Undo::Ref(kind, reference.to_string()));
        }
    }

    pub(crate) fn set_attestation(&mut self, attestation: Attestation) {
        let old = self.attestation.take();
        self.record(Undo::Attestation(old));
        self.attestation = Some(attestation);
    }

    pub(crate) fn set_position(&mut self, height: u64, time: u64) {
        self.record(Undo::Meta {
            height: self.height,
            time: self.time,
            last_hash: self.last_hash,
        });
        self.height = height;
        self.time = time;
    }

    pub(crate) fn set_last_hash(&mut self, hash: Hash32) {
        self.record(Undo::Meta {
            height: self.height,
            time: self.time,
            last_hash: self.last_hash,
        });
        self.last_hash = Some(hash);
    }

    /// Writes to a module's storage.
    pub fn ext_put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let old = self.ext.get(&key).cloned();
        self.record(Undo::Ext(key.clone(), old));
        self.ext.insert(key, value);
    }

    /// Removes from a module's storage.
    pub fn ext_remove(&mut self, key: &[u8]) {
        if let Some(old) = self.ext.remove(key) {
            self.record(Undo::Ext(key.to_vec(), Some(old)));
        }
    }

    // --- root ----------------------------------------------------------------------------------

    /// The state root (Merkle). Does not depend on the order in which the state was built. Does **not** include the last block
    /// hash: the link between blocks is `prev_hash`, so the state root after the block is exactly the header's
    /// `state_root`.
    #[must_use]
    pub fn root(&self) -> Hash32 {
        let accounts: Vec<Hash32> = self
            .accounts
            .iter()
            .map(|(a, acc)| {
                tagged(
                    tags::STATE_ACCOUNT,
                    &[
                        a.hash().as_bytes(),
                        &acc.balance.to_be_bytes(),
                        &acc.nonce.to_be_bytes(),
                    ],
                )
            })
            .collect();
        let refs: Vec<Hash32> = self
            .refs
            .iter()
            .map(|(k, r)| tagged(tags::STATE_REF, &[&[*k], r.as_bytes()]))
            .collect();
        let ext: Vec<Hash32> = self
            .ext
            .iter()
            .map(|(k, v)| tagged(tags::STATE_EXT, &[&len_be(k.len()), k, &len_be(v.len()), v]))
            .collect();
        let (flag, seq, reserve, reference): (u8, u64, u64, &[u8]) =
            self.attestation.as_ref().map_or((0, 0, 0, &[]), |a| {
                (1, a.seq, a.reserve, a.reference.as_bytes())
            });
        let meta = tagged(
            tags::STATE_META,
            &[
                &self.minted.to_be_bytes(),
                &self.burned.to_be_bytes(),
                &self.height.to_be_bytes(),
                &self.time.to_be_bytes(),
                &[flag],
                &seq.to_be_bytes(),
                &reserve.to_be_bytes(),
                &len_be(reference.len()),
                reference,
            ],
        );
        tagged(
            tags::STATE,
            &[
                merkle::root(&accounts).as_bytes(),
                merkle::root(&refs).as_bytes(),
                meta.as_bytes(),
                merkle::root(&ext).as_bytes(),
            ],
        )
    }
}

fn len_be(n: usize) -> [u8; 4] {
    u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes()
}
