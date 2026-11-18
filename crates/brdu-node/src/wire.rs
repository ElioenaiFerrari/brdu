//! Mensagens entre validadores. Cada tipo tem um limite de tamanho conferido **antes** de ler o corpo.

use brdu_bft::{Proposal, Vote};
use brdu_chain::DecidedBlock;
use brdu_crypto::Hash32;
use brdu_crypto::encoding::{Decoder, Encoder};
use brdu_ledger::Tx;

/// Tipos de mensagem.
pub mod kind {
    /// Proposta de bloco.
    pub const PROPOSAL: u32 = 1;
    /// Voto.
    pub const VOTE: u32 = 2;
    /// Lote de transações (fofoca).
    pub const TXS: u32 = 3;
    /// Altura e ponta do remetente.
    pub const STATUS: u32 = 4;
    /// Pedido de blocos.
    pub const GET_BLOCKS: u32 = 5;
    /// Blocos finais.
    pub const BLOCKS: u32 = 6;
}

/// Quantos blocos finais cabem numa resposta.
pub const MAX_BLOCKS_PER_REPLY: usize = 32;
/// Quantas transações cabem num lote de fofoca.
pub const MAX_TXS_PER_MSG: usize = 500;

/// Uma mensagem já decodificada.
#[derive(Debug, Clone)]
pub enum Wire {
    /// Proposta.
    Proposal(Proposal),
    /// Voto.
    Vote(Vote),
    /// Transações.
    Txs(Vec<Tx>),
    /// Altura e hash do último bloco.
    Status { height: u64, tip: Option<Hash32> },
    /// "Mande-me os blocos a partir de `from`."
    GetBlocks { from: u64, max: u32 },
    /// Blocos finais.
    Blocks(Vec<DecidedBlock>),
}

/// Limites de tamanho de corpo por tipo.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Bytes de um bloco.
    pub max_block_bytes: usize,
    /// Transações de um bloco.
    pub max_block_txs: usize,
}

impl Limits {
    /// Maior corpo que o tipo `k` pode ter.
    #[must_use]
    pub const fn max_for(&self, k: u32) -> usize {
        match k {
            kind::PROPOSAL => self.max_block_bytes + 4096,
            kind::VOTE => 512,
            kind::TXS => 1 << 20,
            kind::STATUS => 64,
            kind::GET_BLOCKS => 32,
            kind::BLOCKS => MAX_BLOCKS_PER_REPLY * (self.max_block_bytes + 70_000) + 64,
            _ => 0,
        }
    }
}

impl Wire {
    /// `(tipo, corpo)`.
    #[must_use]
    pub fn encode(&self) -> (u32, Vec<u8>) {
        match self {
            Self::Proposal(p) => (kind::PROPOSAL, p.encode()),
            Self::Vote(v) => (kind::VOTE, v.encode()),
            Self::Txs(txs) => {
                let mut e = Encoder::new();
                e.count(txs.len());
                for t in txs {
                    e.bytes(&t.encode().unwrap_or_default());
                }
                (kind::TXS, e.finish().unwrap_or_default())
            }
            Self::Status { height, tip } => {
                let mut e = Encoder::new();
                e.u64(*height)
                    .bytes(tip.as_ref().map_or(&[][..], |h| h.as_bytes()));
                (kind::STATUS, e.finish().unwrap_or_default())
            }
            Self::GetBlocks { from, max } => {
                let mut e = Encoder::new();
                e.u64(*from).u32(*max);
                (kind::GET_BLOCKS, e.finish().unwrap_or_default())
            }
            Self::Blocks(bs) => {
                let mut e = Encoder::new();
                e.count(bs.len());
                for b in bs {
                    e.bytes(&b.encode().unwrap_or_default());
                }
                (kind::BLOCKS, e.finish().unwrap_or_default())
            }
        }
    }

    /// Lê uma mensagem; `None` se malformada.
    #[must_use]
    pub fn decode(k: u32, body: &[u8], lim: &Limits) -> Option<Self> {
        match k {
            kind::PROPOSAL => Proposal::decode(body, lim.max_block_bytes)
                .ok()
                .map(Self::Proposal),
            kind::VOTE => Vote::decode(body).ok().map(Self::Vote),
            kind::TXS => {
                let mut d = Decoder::new(body);
                let n = d.count(MAX_TXS_PER_MSG).ok()?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(Tx::decode(d.bytes(70_000).ok()?).ok()?);
                }
                d.finish().ok()?;
                Some(Self::Txs(out))
            }
            kind::STATUS => {
                let mut d = Decoder::new(body);
                let height = d.u64().ok()?;
                let tip = match d.bytes(32).ok()? {
                    [] => None,
                    h => Some(Hash32(h.try_into().ok()?)),
                };
                d.finish().ok()?;
                Some(Self::Status { height, tip })
            }
            kind::GET_BLOCKS => {
                let mut d = Decoder::new(body);
                let from = d.u64().ok()?;
                let max = d.u32().ok()?;
                d.finish().ok()?;
                Some(Self::GetBlocks { from, max })
            }
            kind::BLOCKS => {
                let mut d = Decoder::new(body);
                let n = d.count(MAX_BLOCKS_PER_REPLY).ok()?;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(
                        DecidedBlock::decode(
                            d.bytes(lim.max_block_bytes + 70_000).ok()?,
                            lim.max_block_txs,
                            lim.max_block_bytes,
                        )
                        .ok()?,
                    );
                }
                d.finish().ok()?;
                Some(Self::Blocks(out))
            }
            _ => None,
        }
    }
}
