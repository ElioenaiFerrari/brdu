//! Motor BFT de **uma altura**. Mais de 2/3 de prevotes no mesmo bloco **travam** o validador nele (só destrava
//! com prova de uma rodada mais nova); mais de 2/3 de precommits tornam o bloco **final**. Prazo sem progresso
//! passa para a rodada seguinte, com outro proponente: dois blocos diferentes nunca ficam finais na mesma altura
//! com até 1/3 de bizantinos.

use std::collections::{BTreeMap, BTreeSet};

use brdu_crypto::{Hash32, PublicKey, SecretKey};

use crate::types::{Certificate, Message, Proposal, Vote, VoteType, quorum};

/// Passo da rodada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// Esperando a proposta.
    Propose,
    /// Prevote enviado.
    Prevote,
    /// Precommit enviado.
    Precommit,
}

/// O que entra no motor.
#[derive(Debug, Clone)]
pub enum Event {
    /// Proposta recebida (já decodificada; a assinatura é conferida aqui).
    Proposal(Proposal),
    /// Voto recebido.
    Vote(Vote),
    /// Resposta a `NeedBlock`: o bloco (bytes opacos).
    Block(Vec<u8>),
    /// Prazo vencido.
    Timeout(Step, u64),
}

/// O que sai do motor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Enviar aos demais validadores.
    Broadcast(Message),
    /// Quem roda dispara `Event::Timeout(passo, rodada)` depois do prazo.
    ScheduleTimeout(Step, u64),
    /// Sou o proponente: entregue um bloco com `Event::Block`.
    NeedBlock(u64),
    /// **Bloco final**, com o certificado de commit.
    Decide(Vec<u8>, Certificate),
    /// Um validador votou em dois blocos na mesma rodada (equivocação).
    Evidence(Vote, Vote),
}

/// Função de hash de bloco (bytes → hash).
pub type HashFn = Box<dyn Fn(&[u8]) -> Hash32 + Send>;
/// Validação de bloco (a do ledger): recebe a **rodada** da proposta, porque o proponente certo depende dela.
pub type ValidFn = Box<dyn Fn(u64, &[u8]) -> bool + Send>;

/// Configuração do motor.
pub struct EngineConfig {
    /// Rede.
    pub chain_id: String,
    /// Altura.
    pub height: u64,
    /// Validadores, em ordem (a mesma em todos os nós).
    pub validators: Vec<PublicKey>,
    /// Chave deste validador.
    pub key: SecretKey,
    /// Bloco (bytes) → hash.
    pub hash_fn: HashFn,
    /// Bloco (bytes) → válido? (a validação do ledger).
    pub valid_fn: ValidFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Flag {
    PrevoteTimeout(u64),
    PrecommitTimeout(u64),
    Lock(u64),
}

/// O motor de uma altura.
pub struct Engine {
    cfg: EngineConfig,
    me: PublicKey,
    round: u64,
    step: Step,
    locked: Option<(Hash32, u64)>,
    valid: Option<(Hash32, u64)>,
    proposals: BTreeMap<u64, Proposal>,
    blocks: BTreeMap<Hash32, Vec<u8>>,
    votes: BTreeMap<(VoteType, u64), BTreeMap<PublicKey, Vote>>,
    fired: BTreeSet<Flag>,
    decision: Option<(Vec<u8>, Certificate)>,
    out: Vec<Action>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Engine<h={} r={} {:?}, key: [REDACTED]>",
            self.cfg.height, self.round, self.step
        )
    }
}

impl Engine {
    /// Cria o motor.
    #[must_use]
    pub fn new(cfg: EngineConfig) -> Self {
        let me = cfg.key.public();
        Self {
            cfg,
            me,
            round: 0,
            step: Step::Propose,
            locked: None,
            valid: None,
            proposals: BTreeMap::new(),
            blocks: BTreeMap::new(),
            votes: BTreeMap::new(),
            fired: BTreeSet::new(),
            decision: None,
            out: Vec::new(),
        }
    }

    /// Começa a altura (rodada 0).
    pub fn start(&mut self) -> Vec<Action> {
        self.start_round(0);
        self.settle();
        self.flush()
    }

    /// Trata um evento.
    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        if self.decision.is_some() {
            return Vec::new();
        }
        match event {
            Event::Proposal(p) => self.ingest_proposal(p),
            Event::Vote(v) => self.ingest_vote(v),
            Event::Block(b) => self.on_block(b),
            Event::Timeout(step, r) => self.on_timeout(step, r),
        }
        self.settle();
        self.flush()
    }

    /// Decisão tomada, se houver.
    #[must_use]
    pub fn decision(&self) -> Option<&(Vec<u8>, Certificate)> {
        self.decision.as_ref()
    }

    /// Rodada atual.
    #[must_use]
    pub const fn round(&self) -> u64 {
        self.round
    }

    /// Passo atual.
    #[must_use]
    pub const fn step(&self) -> Step {
        self.step
    }

    /// Tudo o que o motor sabe da altura (propostas e votos de todos) das últimas `rounds` rodadas, para
    /// **reenviar** a quem (re)conectou: são mensagens assinadas, reentregar é inofensivo.
    #[must_use]
    pub fn known_messages(&self, rounds: u64) -> Vec<Message> {
        let min = self.round.saturating_sub(rounds.saturating_sub(1));
        let mut out: Vec<Message> = self
            .proposals
            .iter()
            .filter(|(r, _)| **r >= min)
            .map(|(_, p)| Message::Proposal(p.clone()))
            .collect();
        for ((_, r), m) in &self.votes {
            if *r >= min {
                out.extend(m.values().cloned().map(Message::Vote));
            }
        }
        out
    }

    /// Retoma a altura depois de uma queda, a partir do **diário** do que **este validador assinou** (mensagens
    /// de outros são ignoradas). O motor não assina nada que contradiga o diário.
    pub fn restore(&mut self, journal: &[Message]) -> Vec<Action> {
        for m in journal {
            match m {
                Message::Proposal(p) if p.proposer == self.me && p.height == self.cfg.height => {
                    self.blocks.insert(p.block_hash, p.block.clone());
                    self.proposals.insert(p.round, p.clone());
                }
                Message::Vote(v) if v.voter == self.me && v.height == self.cfg.height => {
                    self.votes
                        .entry((v.vote_type, v.round))
                        .or_default()
                        .insert(v.voter, v.clone());
                }
                _ => {}
            }
        }
        let mine: Vec<&Vote> = self
            .votes
            .values()
            .filter_map(|m| m.get(&self.me))
            .collect();
        let max_vote = mine.iter().map(|v| v.round).max().unwrap_or(0);
        let max_prop = self
            .proposals
            .values()
            .filter(|p| p.proposer == self.me)
            .map(|p| p.round)
            .max()
            .unwrap_or(0);
        self.round = max_vote.max(max_prop);
        self.locked = mine
            .iter()
            .filter(|v| v.vote_type == VoteType::Precommit)
            .filter_map(|v| v.block_hash.map(|h| (h, v.round)))
            .max_by_key(|(_, r)| *r);
        self.valid = self.locked.filter(|(h, _)| self.blocks.contains_key(h));
        let voted = |t: VoteType| {
            mine.iter()
                .any(|v| v.vote_type == t && v.round == self.round)
        };
        self.step = if voted(VoteType::Precommit) {
            Step::Precommit
        } else if voted(VoteType::Prevote) {
            Step::Prevote
        } else {
            Step::Propose
        };
        match self.step {
            Step::Propose => self.start_round(self.round),
            s => self.out.push(Action::ScheduleTimeout(s, self.round)),
        }
        self.settle();
        self.flush()
    }

    // ------------------------------------------------------------------ interno

    fn flush(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.out)
    }

    fn n(&self) -> usize {
        self.cfg.validators.len()
    }

    fn proposer(&self, round: u64) -> Option<PublicKey> {
        let n = u64::try_from(self.n()).ok().filter(|n| *n > 0)?;
        let i = usize::try_from(self.cfg.height.wrapping_add(round) % n).ok()?;
        self.cfg.validators.get(i).copied()
    }

    fn start_round(&mut self, round: u64) {
        self.round = round;
        self.step = Step::Propose;
        if self.proposer(round) == Some(self.me) && !self.own_proposal(round) {
            match self
                .valid
                .and_then(|(h, vr)| self.blocks.get(&h).cloned().map(|b| (h, b, vr)))
            {
                Some((h, b, vr)) => self.propose(b, h, i64::try_from(vr).unwrap_or(-1)),
                None => self.out.push(Action::NeedBlock(round)),
            }
        }
        // o prazo também vale para o proponente: se o bloco nunca vier, a rodada segue
        self.out.push(Action::ScheduleTimeout(Step::Propose, round));
    }

    fn own_proposal(&self, round: u64) -> bool {
        self.proposals
            .get(&round)
            .is_some_and(|p| p.proposer == self.me)
    }

    fn propose(&mut self, block: Vec<u8>, hash: Hash32, valid_round: i64) {
        let p = Proposal::sign(
            &self.cfg.chain_id,
            self.cfg.height,
            self.round,
            valid_round,
            block.clone(),
            hash,
            &self.cfg.key,
        );
        self.blocks.insert(hash, block);
        self.proposals.insert(self.round, p.clone());
        self.out.push(Action::Broadcast(Message::Proposal(p)));
    }

    fn cast(&mut self, t: VoteType, hash: Option<Hash32>) {
        let v = Vote::sign(
            &self.cfg.chain_id,
            t,
            self.cfg.height,
            self.round,
            hash,
            &self.cfg.key,
        );
        self.votes
            .entry((t, self.round))
            .or_default()
            .insert(self.me, v.clone());
        self.out.push(Action::Broadcast(Message::Vote(v)));
    }

    fn on_block(&mut self, block: Vec<u8>) {
        if self.step == Step::Propose
            && self.proposer(self.round) == Some(self.me)
            && !self.own_proposal(self.round)
        {
            let hash = (self.cfg.hash_fn)(&block);
            self.propose(block, hash, -1);
        }
    }

    fn on_timeout(&mut self, step: Step, r: u64) {
        match step {
            Step::Propose if self.round == r && self.step == Step::Propose => {
                self.step = Step::Prevote;
                self.cast(VoteType::Prevote, None);
            }
            Step::Prevote if self.round == r && self.step == Step::Prevote => {
                self.step = Step::Precommit;
                self.cast(VoteType::Precommit, None);
            }
            Step::Precommit if self.round == r => self.start_round(r + 1),
            _ => {}
        }
    }

    fn ingest_proposal(&mut self, p: Proposal) {
        let ok = p.height == self.cfg.height
            && !self.proposals.contains_key(&p.round)
            && self.proposer(p.round) == Some(p.proposer)
            && p.valid(&self.cfg.chain_id)
            && (self.cfg.hash_fn)(&p.block) == p.block_hash;
        if ok {
            self.blocks.insert(p.block_hash, p.block.clone());
            self.proposals.insert(p.round, p);
        }
    }

    fn ingest_vote(&mut self, v: Vote) {
        if v.height != self.cfg.height
            || !self.cfg.validators.contains(&v.voter)
            || !v.valid(&self.cfg.chain_id)
        {
            return;
        }
        let slot = self.votes.entry((v.vote_type, v.round)).or_default();
        match slot.get(&v.voter) {
            None => {
                slot.insert(v.voter, v);
            }
            Some(old) if old.block_hash != v.block_hash => {
                let ev = Action::Evidence(old.clone(), v);
                self.out.push(ev);
            }
            Some(_) => {}
        }
    }

    fn settle(&mut self) {
        while self.decision.is_none() && self.rule() {}
    }

    fn rule(&mut self) -> bool {
        self.commit_rule()
            || self.skip_round_rule()
            || self.propose_rules()
            || self.prevote_timeout_rule()
            || self.lock_rule()
            || self.nil_prevotes_rule()
            || self.precommit_timeout_rule()
    }

    fn count(&self, t: VoteType, round: u64) -> usize {
        self.votes.get(&(t, round)).map_or(0, BTreeMap::len)
    }

    fn count_for(&self, t: VoteType, round: u64, hash: Option<Hash32>) -> usize {
        self.votes
            .get(&(t, round))
            .map_or(0, |m| m.values().filter(|v| v.block_hash == hash).count())
    }

    fn once(&mut self, flag: Flag) -> bool {
        self.fired.insert(flag)
    }

    fn commit_rule(&mut self) -> bool {
        let found = self.proposals.iter().find_map(|(r, p)| {
            (quorum(
                self.count_for(VoteType::Precommit, *r, Some(p.block_hash)),
                self.n(),
            ) && (self.cfg.valid_fn)(p.round, &p.block))
            .then(|| (*r, p.block_hash, p.block.clone()))
        });
        let Some((r, hash, block)) = found else {
            return false;
        };
        let votes: Vec<Vote> = self
            .votes
            .get(&(VoteType::Precommit, r))
            .map(|m| {
                m.values()
                    .filter(|v| v.block_hash == Some(hash))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let cert = Certificate {
            height: self.cfg.height,
            round: r,
            block_hash: hash,
            votes,
        };
        self.decision = Some((block.clone(), cert.clone()));
        self.out.push(Action::Decide(block, cert));
        true
    }

    /// Mais de 1/3 dos validadores já numa rodada à frente: ir para lá (não esperar o prazo).
    fn skip_round_rule(&mut self) -> bool {
        let mut by_round: BTreeMap<u64, BTreeSet<PublicKey>> = BTreeMap::new();
        for ((_, r), m) in &self.votes {
            if *r > self.round {
                by_round.entry(*r).or_default().extend(m.keys().copied());
            }
        }
        let n = self.n();
        let target = by_round
            .iter()
            .find(|(_, s)| s.len() * 3 > n)
            .map(|(r, _)| *r);
        match target {
            Some(r) => {
                self.start_round(r);
                true
            }
            None => false,
        }
    }

    fn propose_rules(&mut self) -> bool {
        if self.step != Step::Propose {
            return false;
        }
        let Some(p) = self.proposals.get(&self.round).cloned() else {
            return false;
        };
        let ok = (self.cfg.valid_fn)(p.round, &p.block);
        let vote_for = ok
            && if p.valid_round == -1 {
                self.locked_ok(&p.block_hash, -1)
            } else {
                let vr = u64::try_from(p.valid_round).unwrap_or(u64::MAX);
                p.valid_round < i64::try_from(self.round).unwrap_or(i64::MAX)
                    && quorum(
                        self.count_for(VoteType::Prevote, vr, Some(p.block_hash)),
                        self.n(),
                    )
                    && self.locked_ok(&p.block_hash, p.valid_round)
            };
        self.step = Step::Prevote;
        self.cast(VoteType::Prevote, vote_for.then_some(p.block_hash));
        true
    }

    fn locked_ok(&self, hash: &Hash32, valid_round: i64) -> bool {
        match self.locked {
            None => true,
            Some((h, _)) if h == *hash => true,
            Some((_, lr)) => i64::try_from(lr).unwrap_or(i64::MAX) <= valid_round,
        }
    }

    fn prevote_timeout_rule(&mut self) -> bool {
        let r = self.round;
        if self.step == Step::Prevote
            && quorum(self.count(VoteType::Prevote, r), self.n())
            && self.once(Flag::PrevoteTimeout(r))
        {
            self.out.push(Action::ScheduleTimeout(Step::Prevote, r));
            return true;
        }
        false
    }

    fn lock_rule(&mut self) -> bool {
        let r = self.round;
        if !matches!(self.step, Step::Prevote | Step::Precommit) {
            return false;
        }
        let Some(p) = self.proposals.get(&r).cloned() else {
            return false;
        };
        if !(self.cfg.valid_fn)(p.round, &p.block)
            || !quorum(
                self.count_for(VoteType::Prevote, r, Some(p.block_hash)),
                self.n(),
            )
            || !self.once(Flag::Lock(r))
        {
            return false;
        }
        self.valid = Some((p.block_hash, r));
        if self.step == Step::Prevote {
            self.locked = Some((p.block_hash, r));
            self.step = Step::Precommit;
            self.cast(VoteType::Precommit, Some(p.block_hash));
        }
        true
    }

    fn nil_prevotes_rule(&mut self) -> bool {
        if self.step == Step::Prevote
            && quorum(
                self.count_for(VoteType::Prevote, self.round, None),
                self.n(),
            )
        {
            self.step = Step::Precommit;
            self.cast(VoteType::Precommit, None);
            return true;
        }
        false
    }

    fn precommit_timeout_rule(&mut self) -> bool {
        let r = self.round;
        if quorum(self.count(VoteType::Precommit, r), self.n())
            && self.once(Flag::PrecommitTimeout(r))
        {
            self.out.push(Action::ScheduleTimeout(Step::Precommit, r));
            return true;
        }
        false
    }
}
