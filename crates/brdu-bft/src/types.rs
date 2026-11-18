//! Votes, proposals and the commit certificate (BRDU ADR-0003, "permissioned BFT").
//!
//! Every signature covers the `chain_id`, the height, the round and the content: nothing is valid on another network, height, round
//! or type. The block travels as **opaque bytes**: consensus only knows its hash.

use std::collections::BTreeSet;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{DecodeError, Hash32, PublicKey, SecretKey, Signature, tagged};
use thiserror::Error;

/// Domain prefix of the document a vote signs.
pub const VOTE_TAG: &str = "brdu/vote";
/// Domain prefix of the document a proposal signs.
pub const PROPOSAL_TAG: &str = "brdu/proposal";
/// More validators than this is abuse (and bounds the size of a certificate).
pub const MAX_VALIDATORS: usize = 256;

/// Prevote or precommit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VoteType {
    /// First voting phase of the round.
    Prevote,
    /// Second phase: precommits for a block from more than 2/3 make it final.
    Precommit,
}

impl VoteType {
    const fn byte(self) -> u8 {
        match self {
            Self::Prevote => 1,
            Self::Precommit => 2,
        }
    }

    const fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Prevote),
            2 => Some(Self::Precommit),
            _ => None,
        }
    }
}

/// Quorum: **more than 2/3** of `n`.
#[must_use]
pub const fn quorum(count: usize, n: usize) -> bool {
    count * 3 > 2 * n
}

/// Signed vote. `block_hash = None` is the **nil vote** ("I did not see a valid block in time").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    /// Type.
    pub vote_type: VoteType,
    /// Height.
    pub height: u64,
    /// Round.
    pub round: u64,
    /// The block voted for, or `None` (nil).
    pub block_hash: Option<Hash32>,
    /// Who voted.
    pub voter: PublicKey,
    /// The signature.
    pub signature: Signature,
}

fn vote_doc(chain_id: &str, t: VoteType, height: u64, round: u64, hash: Option<&Hash32>) -> Hash32 {
    let mut e = Encoder::new();
    e.string(chain_id)
        .u8(t.byte())
        .u64(height)
        .u64(round)
        .bytes(hash.map_or(&[][..], |h| h.as_bytes()));
    tagged(VOTE_TAG, &[&e.finish().unwrap_or_default()])
}

impl Vote {
    /// Signs a vote.
    #[must_use]
    pub fn sign(
        chain_id: &str,
        t: VoteType,
        height: u64,
        round: u64,
        hash: Option<Hash32>,
        key: &SecretKey,
    ) -> Self {
        let doc = vote_doc(chain_id, t, height, round, hash.as_ref());
        Self {
            vote_type: t,
            height,
            round,
            block_hash: hash,
            voter: key.public(),
            signature: key.sign(doc.as_bytes()),
        }
    }

    /// Does the signature check out?
    #[must_use]
    pub fn valid(&self, chain_id: &str) -> bool {
        let doc = vote_doc(
            chain_id,
            self.vote_type,
            self.height,
            self.round,
            self.block_hash.as_ref(),
        );
        self.voter.verify(doc.as_bytes(), &self.signature).is_ok()
    }

    /// Deterministic encoding.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.u8(self.vote_type.byte())
            .u64(self.height)
            .u64(self.round)
            .bytes(self.block_hash.as_ref().map_or(&[][..], |h| h.as_bytes()))
            .fixed(self.voter.as_bytes())
            .fixed(self.signature.as_bytes());
        e.finish().unwrap_or_default()
    }

    /// Reads a vote. Rejects garbage, trailing bytes and a hash that is not 0 or 32 bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut d = Decoder::new(bytes);
        let vote_type = VoteType::from_byte(d.u8()?).ok_or(DecodeError::Truncated)?;
        let height = d.u64()?;
        let round = d.u64()?;
        let block_hash = match d.bytes(32)? {
            [] => None,
            h => Some(Hash32(h.try_into().map_err(|_| DecodeError::Truncated)?)),
        };
        let voter = PublicKey(d.fixed::<32>()?);
        let signature = Signature(d.fixed::<64>()?);
        d.finish()?;
        Ok(Self {
            vote_type,
            height,
            round,
            block_hash,
            voter,
            signature,
        })
    }
}

/// Block proposal from the round's proposer. `valid_round` is the round in which the proposer saw more than 2/3 of
/// prevotes for this same block (`-1` if it is a new block): it serves to unlock validators stuck on another block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Altura.
    pub height: u64,
    /// Rodada.
    pub round: u64,
    /// Round of the proposer's last "valid block", or -1.
    pub valid_round: i64,
    /// Block hash.
    pub block_hash: Hash32,
    /// The block (opaque to consensus).
    pub block: Vec<u8>,
    /// Who proposed.
    pub proposer: PublicKey,
    /// The signature (covers the block hash).
    pub signature: Signature,
}

fn proposal_doc(
    chain_id: &str,
    height: u64,
    round: u64,
    valid_round: i64,
    hash: &Hash32,
) -> Hash32 {
    let mut e = Encoder::new();
    // valid_round + 1, so that -1 fits in an unsigned integer
    e.string(chain_id)
        .u64(height)
        .u64(round)
        .u64(shift(valid_round))
        .fixed(hash.as_bytes());
    tagged(PROPOSAL_TAG, &[&e.finish().unwrap_or_default()])
}

fn shift(valid_round: i64) -> u64 {
    u64::try_from(valid_round.saturating_add(1)).unwrap_or(0)
}

impl Proposal {
    /// Signs a proposal.
    #[must_use]
    pub fn sign(
        chain_id: &str,
        height: u64,
        round: u64,
        valid_round: i64,
        block: Vec<u8>,
        hash: Hash32,
        key: &SecretKey,
    ) -> Self {
        let doc = proposal_doc(chain_id, height, round, valid_round, &hash);
        Self {
            height,
            round,
            valid_round,
            block_hash: hash,
            block,
            proposer: key.public(),
            signature: key.sign(doc.as_bytes()),
        }
    }

    /// Does the signature check out? (The caller also checks that the hash matches the block.)
    #[must_use]
    pub fn valid(&self, chain_id: &str) -> bool {
        let doc = proposal_doc(
            chain_id,
            self.height,
            self.round,
            self.valid_round,
            &self.block_hash,
        );
        self.proposer
            .verify(doc.as_bytes(), &self.signature)
            .is_ok()
    }

    /// Deterministic encoding.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.u64(self.height)
            .u64(self.round)
            .u64(shift(self.valid_round))
            .fixed(self.block_hash.as_bytes())
            .bytes(&self.block)
            .fixed(self.proposer.as_bytes())
            .fixed(self.signature.as_bytes());
        e.finish().unwrap_or_default()
    }

    /// Reads a proposal; rejects a block larger than `max_block`.
    pub fn decode(bytes: &[u8], max_block: usize) -> Result<Self, DecodeError> {
        let mut d = Decoder::new(bytes);
        let height = d.u64()?;
        let round = d.u64()?;
        let shifted = i64::try_from(d.u64()?).map_err(|_| DecodeError::TooLong)?;
        let block_hash = Hash32(d.fixed::<32>()?);
        let block = d.bytes(max_block)?.to_vec();
        let proposer = PublicKey(d.fixed::<32>()?);
        let signature = Signature(d.fixed::<64>()?);
        d.finish()?;
        Ok(Self {
            height,
            round,
            valid_round: shifted - 1,
            block_hash,
            block,
            proposer,
            signature,
        })
    }
}

/// A consensus message that travels over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Proposal.
    Proposal(Proposal),
    /// Vote.
    Vote(Vote),
}

/// Why a certificate is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CertError {
    /// Some vote is not a precommit of the certificate's height and round.
    #[error("votes from another height, round or type")]
    MismatchedVotes,
    /// Some vote is for another block (or nil).
    #[error("vote for another block")]
    DifferentBlock,
    /// The same validator twice.
    #[error("duplicate validator")]
    DuplicateVoter,
    /// Whoever voted is not a validator.
    #[error("unknown voter")]
    UnknownVoter,
    /// Invalid signature.
    #[error("invalid signature")]
    BadSignature,
    /// Does not exceed 2/3.
    #[error("no quorum")]
    NoQuorum,
}

/// Commit certificate: the precommits of **more than 2/3** of the validators for the same block. Whoever did not take part in
/// consensus (a syncing node, the wallet, the explorer) checks it alone and comes to **trust the block without
/// trusting whoever delivered it**. It also says **who signed**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    /// Altura.
    pub height: u64,
    /// Rodada.
    pub round: u64,
    /// The decided block.
    pub block_hash: Hash32,
    /// The precommits.
    pub votes: Vec<Vote>,
}

impl Certificate {
    /// Checks against the validator set in force: precommits of the same height and round, for the same block, from
    /// distinct, known validators, with a valid signature, and more than 2/3 of them.
    pub fn verify(&self, chain_id: &str, validators: &[PublicKey]) -> Result<(), CertError> {
        if !self.votes.iter().all(|v| {
            v.vote_type == VoteType::Precommit && v.height == self.height && v.round == self.round
        }) {
            return Err(CertError::MismatchedVotes);
        }
        if !self
            .votes
            .iter()
            .all(|v| v.block_hash == Some(self.block_hash))
        {
            return Err(CertError::DifferentBlock);
        }
        let voters: BTreeSet<&PublicKey> = self.votes.iter().map(|v| &v.voter).collect();
        if voters.len() != self.votes.len() {
            return Err(CertError::DuplicateVoter);
        }
        if !voters.iter().all(|v| validators.contains(v)) {
            return Err(CertError::UnknownVoter);
        }
        if !self.votes.iter().all(|v| v.valid(chain_id)) {
            return Err(CertError::BadSignature);
        }
        if quorum(voters.len(), validators.len()) {
            Ok(())
        } else {
            Err(CertError::NoQuorum)
        }
    }

    /// Who signed.
    #[must_use]
    pub fn signers(&self) -> Vec<PublicKey> {
        self.votes.iter().map(|v| v.voter).collect()
    }

    /// Deterministic encoding.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.u64(self.height)
            .u64(self.round)
            .fixed(self.block_hash.as_bytes())
            .count(self.votes.len());
        for v in &self.votes {
            e.bytes(&v.encode());
        }
        e.finish().unwrap_or_default()
    }

    /// Reads a certificate (at most `MAX_VALIDATORS` votes).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut d = Decoder::new(bytes);
        let height = d.u64()?;
        let round = d.u64()?;
        let block_hash = Hash32(d.fixed::<32>()?);
        let n = d.count(MAX_VALIDATORS)?;
        let mut votes = Vec::with_capacity(n);
        for _ in 0..n {
            votes.push(Vote::decode(d.bytes(256)?)?);
        }
        d.finish()?;
        Ok(Self {
            height,
            round,
            block_hash,
            votes,
        })
    }
}
