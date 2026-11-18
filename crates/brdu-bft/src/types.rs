//! Votos, propostas e certificado de commit (ADR-0003 do BRDU, "BFT permissionado").
//!
//! Toda assinatura cobre o `chain_id`, a altura, a rodada e o conteúdo: nada vale em outra rede, altura, rodada
//! ou tipo. O bloco viaja como **bytes opacos**: o consenso só conhece o hash dele.

use std::collections::BTreeSet;

use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_crypto::{DecodeError, Hash32, PublicKey, SecretKey, Signature, tagged};
use thiserror::Error;

/// Prefixo de domínio do documento que um voto assina.
pub const VOTE_TAG: &str = "brdu/vote";
/// Prefixo de domínio do documento que uma proposta assina.
pub const PROPOSAL_TAG: &str = "brdu/proposal";
/// Mais validadores que isto é abuso (e limita o tamanho de um certificado).
pub const MAX_VALIDATORS: usize = 256;

/// Prevote ou precommit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VoteType {
    /// Primeira fase de votação da rodada.
    Prevote,
    /// Segunda fase: um precommit num bloco, de mais de 2/3, o torna final.
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

/// Quórum: **mais de 2/3** de `n`.
#[must_use]
pub const fn quorum(count: usize, n: usize) -> bool {
    count * 3 > 2 * n
}

/// Voto assinado. `block_hash = None` é o **voto nulo** ("não vi bloco válido a tempo").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    /// Tipo.
    pub vote_type: VoteType,
    /// Altura.
    pub height: u64,
    /// Rodada.
    pub round: u64,
    /// O bloco votado, ou `None` (nulo).
    pub block_hash: Option<Hash32>,
    /// Quem votou.
    pub voter: PublicKey,
    /// A assinatura.
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
    /// Assina um voto.
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

    /// A assinatura confere?
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

    /// Codificação determinística.
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

    /// Lê um voto. Recusa lixo, sobras e hash que não tenha 0 ou 32 bytes.
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

/// Proposta de bloco do proponente da rodada. `valid_round` é a rodada em que o proponente viu mais de 2/3 de
/// prevotes neste mesmo bloco (`-1` se é um bloco novo): serve para destravar validadores presos a outro bloco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Altura.
    pub height: u64,
    /// Rodada.
    pub round: u64,
    /// Rodada do último "bloco válido" do proponente, ou -1.
    pub valid_round: i64,
    /// Hash do bloco.
    pub block_hash: Hash32,
    /// O bloco (opaco para o consenso).
    pub block: Vec<u8>,
    /// Quem propôs.
    pub proposer: PublicKey,
    /// A assinatura (cobre o hash do bloco).
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
    // valid_round + 1, para o -1 caber num inteiro sem sinal
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
    /// Assina uma proposta.
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

    /// A assinatura confere? (Quem chama confere também que o hash bate com o bloco.)
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

    /// Codificação determinística.
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

    /// Lê uma proposta; recusa bloco maior que `max_block`.
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

/// Uma mensagem de consenso que vai pela rede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Proposta.
    Proposal(Proposal),
    /// Voto.
    Vote(Vote),
}

/// Por que um certificado não vale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CertError {
    /// Algum voto não é precommit da altura e rodada do certificado.
    #[error("votos de outra altura, rodada ou tipo")]
    MismatchedVotes,
    /// Algum voto é para outro bloco (ou nulo).
    #[error("voto para outro bloco")]
    DifferentBlock,
    /// O mesmo validador duas vezes.
    #[error("validador repetido")]
    DuplicateVoter,
    /// Quem votou não é validador.
    #[error("votante desconhecido")]
    UnknownVoter,
    /// Assinatura inválida.
    #[error("assinatura inválida")]
    BadSignature,
    /// Não passa de 2/3.
    #[error("sem quórum")]
    NoQuorum,
}

/// Certificado de commit: os precommits de **mais de 2/3** dos validadores no mesmo bloco. Quem não participou do
/// consenso (um nó que sincroniza, a carteira, o explorador) confere sozinho e passa a **confiar no bloco sem
/// confiar em quem o entregou**. Também diz **quem assinou**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    /// Altura.
    pub height: u64,
    /// Rodada.
    pub round: u64,
    /// O bloco decidido.
    pub block_hash: Hash32,
    /// Os precommits.
    pub votes: Vec<Vote>,
}

impl Certificate {
    /// Confere contra o conjunto de validadores em vigor: precommits da mesma altura e rodada, no mesmo bloco, de
    /// validadores distintos e conhecidos, com assinatura válida, e mais de 2/3 deles.
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

    /// Quem assinou.
    #[must_use]
    pub fn signers(&self) -> Vec<PublicKey> {
        self.votes.iter().map(|v| v.voter).collect()
    }

    /// Codificação determinística.
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

    /// Lê um certificado (no máximo `MAX_VALIDATORS` votos).
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
