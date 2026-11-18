//! O núcleo: dono do consenso, do mempool, do diário e da cadeia. Tudo o que chega (rede, API, prazos) entra por
//! `Input`; tudo o que sai vai por canais das conexões. Sem E/S bloqueante aqui, exceto o disco.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use brdu_bft::{Action, Certificate, Engine, EngineConfig, Event, Message, Step};
use brdu_chain::{DecidedBlock, RecordLog};
use brdu_crypto::{Address, Hash32, PublicKey, SecretKey, tagged};
use brdu_ledger::{Block, Tx};

use crate::mempool::{Mempool, PoolError};
use crate::node::{Config, NodeError, Shared, Timeouts};
use crate::wire::{MAX_BLOCKS_PER_REPLY, MAX_TXS_PER_MSG, Wire};

/// O que entra no núcleo.
pub enum Input {
    /// Mensagem de um par.
    Net(PublicKey, Wire),
    /// Uma conexão estabeleceu (e passou na prova de identidade).
    PeerUp {
        key: PublicKey,
        outbound: bool,
        tx: SyncSender<Arc<(u32, Vec<u8>)>>,
        id: u64,
    },
    /// Uma conexão caiu.
    PeerDown { key: PublicKey, id: u64 },
    /// Transação já conferida (assinatura e forma), vinda da API ou de um par.
    Tx {
        tx: Tx,
        from: Option<PublicKey>,
        reply: Option<Sender<Result<Hash32, String>>>,
    },
    /// Parar.
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Timer {
    Step(Step, u64, u64),
    Start(u64),
    Heartbeat,
}

struct Peer {
    tx: SyncSender<Arc<(u32, Vec<u8>)>>,
    outbound: bool,
    id: u64,
}

const SEEN_MAX: usize = 262_144;
const EARLY_MAX: usize = 4096;
const CLOCK_SKEW_SECS: u64 = 15;
const JOURNAL_PROPOSAL: u8 = 1;
const JOURNAL_VOTE: u8 = 2;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// O núcleo do nó.
pub struct Core {
    shared: Arc<Shared>,
    rx: Receiver<Input>,
    key: SecretKey,
    me: PublicKey,
    me_addr: Address,
    timeouts: Timeouts,
    min_interval: Duration,
    empty_blocks: bool,
    engine: Option<Engine>,
    height: u64,
    starting: bool,
    early: Vec<Message>,
    journal: RecordLog,
    journal_msgs: Vec<Message>,
    blocklog: RecordLog,
    pool: Mempool,
    peers: HashMap<PublicKey, Peer>,
    peer_heights: HashMap<PublicKey, u64>,
    sync_asked: HashMap<PublicKey, (u64, Instant)>,
    seen: HashSet<Hash32>,
    seen_order: VecDeque<Hash32>,
    timers: BinaryHeap<Reverse<(Instant, Timer)>>,
    last_block_at: Instant,
    chain_id: String,
    in_set: bool,
}

impl Core {
    /// Abre o disco, refaz a cadeia a partir do log de blocos e prepara o diário.
    pub fn new(
        cfg: Config,
        shared: Arc<Shared>,
        rx: Receiver<Input>,
        _tx: Sender<Input>,
    ) -> Result<Self, NodeError> {
        let (blocklog, records) = RecordLog::open(&cfg.data_dir.join("blocks.log"))?;
        let limits = shared.limits;
        {
            let mut chain = shared
                .chain
                .lock()
                .map_err(|_| NodeError::Config("poisoned".into()))?;
            let mut info = shared
                .info
                .lock()
                .map_err(|_| NodeError::Config("poisoned".into()))?;
            for raw in records {
                let d = DecidedBlock::decode(&raw, limits.max_block_txs, limits.max_block_bytes)?;
                let height = d.block.height;
                for tx in &d.block.txs {
                    info.tx_index.insert(tx.id(), height);
                }
                chain.apply(d)?;
            }
        }
        let (journal, jrecords) = RecordLog::open(&cfg.data_dir.join("journal.log"))?;
        let journal_msgs = decode_journal(&jrecords, limits.max_block_bytes);
        let height = shared.chain.lock().map_or(0, |c| c.height());
        Ok(Self {
            me: cfg.key.public(),
            me_addr: Address::from_public_key(&cfg.key.public()),
            key: cfg.key,
            timeouts: cfg.timeouts,
            min_interval: cfg.min_block_interval,
            empty_blocks: cfg.empty_blocks,
            engine: None,
            height,
            starting: false,
            early: Vec::new(),
            journal,
            journal_msgs,
            blocklog,
            pool: Mempool::new(cfg.max_pool_txs, cfg.max_pool_bytes),
            peers: HashMap::new(),
            peer_heights: HashMap::new(),
            sync_asked: HashMap::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            timers: BinaryHeap::new(),
            last_block_at: Instant::now(),
            chain_id: cfg.genesis.chain_id.clone(),
            in_set: true,
            shared,
            rx,
        })
    }

    /// O laço principal.
    pub fn run(mut self) {
        self.refresh_set();
        self.timers.push(Reverse((
            Instant::now() + Duration::from_secs(1),
            Timer::Heartbeat,
        )));
        if !self.journal_msgs.is_empty() || self.empty_blocks {
            self.schedule_start(true);
        }
        loop {
            let wait = self
                .timers
                .peek()
                .map_or(Duration::from_millis(500), |Reverse((at, _))| {
                    at.saturating_duration_since(Instant::now())
                });
            match self.rx.recv_timeout(wait) {
                Ok(Input::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(input) => self.on_input(input),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            // drena o que já está na fila antes de olhar os prazos (agrupa trabalho sob carga)
            let mut drained = 0;
            while drained < 256 {
                match self.rx.try_recv() {
                    Ok(Input::Stop) => return,
                    Ok(i) => self.on_input(i),
                    Err(_) => break,
                }
                drained += 1;
            }
            self.fire_timers();
            if self.shared.stop.load(Ordering::SeqCst) {
                break;
            }
        }
    }

    // ------------------------------------------------------------------ entradas

    fn on_input(&mut self, input: Input) {
        match input {
            Input::Net(from, wire) => {
                self.shared.stats.msgs_in.fetch_add(1, Ordering::Relaxed);
                self.on_wire(from, wire);
            }
            Input::PeerUp {
                key,
                outbound,
                tx,
                id,
            } => self.on_peer_up(key, outbound, tx, id),
            Input::PeerDown { key, id } => {
                if self.peers.get(&key).is_some_and(|p| p.id == id) {
                    self.peers.remove(&key);
                    self.peer_heights.remove(&key);
                    self.publish_peers();
                }
            }
            Input::Tx { tx, from, reply } => {
                let r = self.admit(tx, from);
                if let Some(reply) = reply {
                    let _ = reply.send(r);
                }
            }
            Input::Stop => {}
        }
    }

    fn on_peer_up(
        &mut self,
        key: PublicKey,
        outbound: bool,
        tx: SyncSender<Arc<(u32, Vec<u8>)>>,
        id: u64,
    ) {
        // duas conexões com o mesmo par: vale a iniciada pela chave **menor**
        if let Some(old) = self.peers.get(&key) {
            let initiator = |outbound: bool| if outbound { self.me } else { key };
            let responder = |outbound: bool| if outbound { key } else { self.me };
            let preferred = |o: bool| initiator(o) < responder(o);
            if preferred(old.outbound) && !preferred(outbound) {
                return; // a nova some (o `tx` cai e o laço dela encerra)
            }
        }
        self.peers.insert(key, Peer { tx, outbound, id });
        self.publish_peers();
        // ao (re)conectar: estado, mempool e o que o consenso sabe
        self.send_to(
            &key,
            &Wire::Status {
                height: self.height,
                tip: self.tip(),
            },
        );
        let txs: Vec<Tx> = self.pool.iter().cloned().collect();
        for chunk in txs.chunks(MAX_TXS_PER_MSG) {
            self.send_to(&key, &Wire::Txs(chunk.to_vec()));
        }
        if let Some(e) = &self.engine {
            for m in e.known_messages(4) {
                self.send_to(&key, &wire_of(m));
            }
        }
    }

    fn on_wire(&mut self, from: PublicKey, wire: Wire) {
        match wire {
            Wire::Proposal(p) => {
                let h = p.height;
                self.on_consensus(from, h, Message::Proposal(p));
            }
            Wire::Vote(v) => {
                let h = v.height;
                self.on_consensus(from, h, Message::Vote(v));
            }
            Wire::Txs(txs) => {
                for tx in txs {
                    let _ = self.admit(tx, Some(from));
                }
            }
            Wire::Status { height, .. } => {
                self.peer_heights.insert(from, height);
                self.maybe_sync(from);
            }
            Wire::GetBlocks { from: first, max } => {
                let take = (max as usize).clamp(1, MAX_BLOCKS_PER_REPLY);
                let blocks: Vec<DecidedBlock> = self
                    .shared
                    .chain
                    .lock()
                    .map(|c| {
                        c.blocks_after(first.saturating_sub(1))
                            .iter()
                            .take(take)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                if !blocks.is_empty() {
                    self.send_to(&from, &Wire::Blocks(blocks));
                }
            }
            Wire::Blocks(blocks) => {
                let mut applied = false;
                for d in blocks {
                    if d.block.height != self.height + 1 {
                        continue;
                    }
                    if self.apply_decided(d, false) {
                        applied = true;
                    } else {
                        break;
                    }
                }
                if applied {
                    self.after_block();
                    self.maybe_sync(from);
                }
            }
        }
    }

    fn on_consensus(&mut self, from: PublicKey, height: u64, msg: Message) {
        let next = self.height + 1;
        if height > next {
            self.peer_heights
                .entry(from)
                .and_modify(|h| *h = (*h).max(height - 1))
                .or_insert(height - 1);
            self.maybe_sync(from);
            return;
        }
        if height < next {
            return;
        }
        if !self.in_set && self.engine.is_none() {
            return; // observador: não vota
        }
        match self.engine.is_some() {
            true => self.feed(match msg {
                Message::Proposal(p) => Event::Proposal(p),
                Message::Vote(v) => Event::Vote(v),
            }),
            false => {
                if self.early.len() < EARLY_MAX {
                    self.early.push(msg);
                }
                self.schedule_start(false);
            }
        }
    }

    // ------------------------------------------------------------------ transações

    fn admit(&mut self, tx: Tx, from: Option<PublicKey>) -> Result<Hash32, String> {
        let id = tx.id();
        if self.seen.contains(&id) || self.pool.contains(&id) {
            self.shared
                .stats
                .txs_duplicate
                .fetch_add(1, Ordering::Relaxed);
            if from.is_some() {
                return Err("duplicate".into());
            }
            // reenvio pela API: pendente = ok (idempotente); já incluída = recusa clara
            let included = self
                .shared
                .info
                .lock()
                .is_ok_and(|i| i.tx_index.contains_key(&id));
            return if included {
                Err("already included".into())
            } else {
                Ok(id)
            };
        }
        let verdict = self.check_admission(&tx);
        let result = verdict.and_then(|nonce| {
            self.pool
                .add(tx.clone(), id, nonce)
                .map_err(|e: PoolError| e.to_string())
        });
        match result {
            Ok(()) => {
                self.remember(id);
                self.shared
                    .stats
                    .txs_accepted
                    .fetch_add(1, Ordering::Relaxed);
                // fofoca **sem eco**: não devolve a quem mandou
                let msg = Wire::Txs(vec![tx]).encode();
                let msg = Arc::new(msg);
                for (k, p) in &self.peers {
                    if Some(*k) != from && p.tx.try_send(msg.clone()).is_ok() {
                        self.shared
                            .stats
                            .txs_relayed
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
                self.publish_pool();
                self.schedule_start(false);
                Ok(id)
            }
            Err(e) => {
                self.shared
                    .stats
                    .txs_rejected
                    .fetch_add(1, Ordering::Relaxed);
                Err(e)
            }
        }
    }

    /// Confere o que não é assinatura (já conferida): rede, taxa mínima, saldo para a taxa. Devolve o nonce da conta.
    fn check_admission(&self, tx: &Tx) -> Result<u64, String> {
        let chain = self.shared.chain.lock().map_err(|_| "busy".to_string())?;
        if tx.chain_id != self.chain_id {
            return Err("wrong chain".into());
        }
        let min = chain.executor().min_fee(tx).map_err(|e| e.to_string())?;
        if tx.fee < min {
            return Err(format!("fee too low: min {min}, got {}", tx.fee));
        }
        let a = tx.sender_address();
        if chain.state().balance(&a) < tx.fee {
            return Err("insufficient funds for the fee".into());
        }
        Ok(chain.state().nonce(&a))
    }

    fn remember(&mut self, id: Hash32) {
        if self.seen.insert(id) {
            self.seen_order.push_back(id);
            if self.seen_order.len() > SEEN_MAX
                && let Some(old) = self.seen_order.pop_front()
            {
                self.seen.remove(&old);
            }
        }
    }

    // ------------------------------------------------------------------ consenso

    fn tip(&self) -> Option<Hash32> {
        self.shared
            .chain
            .lock()
            .ok()
            .and_then(|c| c.state().last_hash())
    }

    fn should_start(&self) -> bool {
        // observador (fora do conjunto em vigor): só sincroniza, não vota
        self.in_set
            && (self.empty_blocks
                || !self.pool.is_empty()
                || !self.early.is_empty()
                || !self.journal_msgs.is_empty())
    }

    /// Marca a hora de começar a próxima altura (respeitando o espaço mínimo entre blocos).
    fn schedule_start(&mut self, force: bool) {
        if self.engine.is_some() || self.starting || !self.in_set || !(force || self.should_start())
        {
            return;
        }
        self.starting = true;
        let at = (self.last_block_at + self.min_interval).max(Instant::now());
        self.timers
            .push(Reverse((at, Timer::Start(self.height + 1))));
    }

    fn read_journal(&self) -> Vec<Message> {
        self.journal_msgs.clone()
    }

    fn start_height(&mut self) {
        self.starting = false;
        let height = self.height + 1;
        let journal = self.read_journal();
        let lim = self.shared.limits;
        let shared = Arc::clone(&self.shared);
        let shared_validators = shared
            .chain
            .lock()
            .map(|c| c.validators())
            .unwrap_or_default();
        let cache: Mutex<Vec<((u64, Hash32), bool)>> = Mutex::new(Vec::new());
        let cfg = EngineConfig {
            chain_id: self.chain_id.clone(),
            height,
            validators: shared_validators,
            key: SecretKey::from_bytes(*self.key.expose_seed()),
            hash_fn: Box::new(|bytes| {
                Block::decode(bytes, usize::MAX >> 1)
                    .map_or_else(|_| tagged("brdu/node/bad", &[bytes]), |b| b.hash())
            }),
            valid_fn: Box::new(move |round, bytes| {
                let key = (round, tagged("brdu/node/vcache", &[bytes]));
                if let Ok(c) = cache.lock()
                    && let Some((_, v)) = c.iter().find(|(k, _)| *k == key)
                {
                    return *v;
                }
                let ok = Block::decode(bytes, lim.max_block_txs).is_ok_and(|b| {
                    b.time <= now_secs() + CLOCK_SKEW_SECS
                        && shared
                            .chain
                            .lock()
                            .is_ok_and(|mut c| c.validate(&b, round).is_ok())
                });
                if let Ok(mut c) = cache.lock() {
                    c.push((key, ok));
                    if c.len() > 8 {
                        c.remove(0);
                    }
                }
                ok
            }),
        };
        let mut engine = Engine::new(cfg);
        let actions = if journal.is_empty() {
            engine.start()
        } else {
            engine.restore(&journal)
        };
        self.set_running(true);
        self.drive(engine, actions.into());
        // o que chegou antes de começarmos
        for m in std::mem::take(&mut self.early) {
            let h = match &m {
                Message::Proposal(p) => p.height,
                Message::Vote(v) => v.height,
            };
            if h == height {
                self.feed(match m {
                    Message::Proposal(p) => Event::Proposal(p),
                    Message::Vote(v) => Event::Vote(v),
                });
            }
        }
    }

    fn feed(&mut self, ev: Event) {
        let Some(mut e) = self.engine.take() else {
            return;
        };
        let actions = e.handle(ev);
        self.drive(e, actions.into());
    }

    /// Executa as ações do motor (e as que elas provocam) até esvaziar ou até o bloco ficar final.
    fn drive(&mut self, mut e: Engine, mut queue: VecDeque<Action>) {
        while let Some(a) = queue.pop_front() {
            match a {
                Action::Broadcast(m) => self.journal_and_broadcast(&m),
                Action::ScheduleTimeout(step, round) => {
                    let at = Instant::now() + self.duration(step, round);
                    self.timers
                        .push(Reverse((at, Timer::Step(step, round, self.height + 1))));
                }
                Action::NeedBlock(round) => {
                    if round > 0 {
                        self.shared
                            .stats
                            .extra_rounds
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    if let Some(bytes) = self.build_block() {
                        queue.extend(e.handle(Event::Block(bytes)));
                    }
                }
                Action::Decide(bytes, cert) => {
                    self.finish_height(&bytes, cert);
                    return;
                }
                Action::Evidence(a, b) => {
                    if let Ok(mut i) = self.shared.info.lock()
                        && i.evidence.len() < 100
                    {
                        i.evidence.push((a, b));
                    }
                }
            }
        }
        self.engine = Some(e);
    }

    fn duration(&self, step: Step, round: u64) -> Duration {
        let base = match step {
            Step::Propose => self.timeouts.propose,
            Step::Prevote => self.timeouts.prevote,
            Step::Precommit => self.timeouts.precommit,
        };
        base + self.timeouts.delta * u32::try_from(round.min(20)).unwrap_or(20)
    }

    /// Diário **antes** de transmitir: o que saiu da máquina está no diário, e o que não foi gravado nunca saiu.
    fn journal_and_broadcast(&mut self, m: &Message) {
        let (tag, body) = match m {
            Message::Proposal(p) => (JOURNAL_PROPOSAL, p.encode()),
            Message::Vote(v) => (JOURNAL_VOTE, v.encode()),
        };
        let mut rec = vec![tag];
        rec.extend_from_slice(&body);
        if self.journal.append(&rec, true).is_err() {
            return; // sem diário não se assina nem se transmite
        }
        self.journal_msgs.push(m.clone());
        let msg = Arc::new(wire_of(m.clone()).encode());
        for p in self.peers.values() {
            let _ = p.tx.try_send(msg.clone());
        }
    }

    fn build_block(&mut self) -> Option<Vec<u8>> {
        let mut chain = self.shared.chain.lock().ok()?;
        let (txs, bad) = chain.select_txs(self.pool.iter());
        let time = now_secs().max(chain.state().time());
        let block = match chain.build(self.me_addr, time, txs) {
            Ok(b) => b,
            Err(_) => chain.build(self.me_addr, time, Vec::new()).ok()?,
        };
        drop(chain);
        for id in bad {
            self.pool.remove(&id);
        }
        block.encode().ok()
    }

    fn finish_height(&mut self, bytes: &[u8], cert: Certificate) {
        let lim = self.shared.limits;
        let Ok(block) = Block::decode(bytes, lim.max_block_txs) else {
            return;
        };
        let decided = DecidedBlock { block, cert };
        self.engine = None;
        self.set_running(false);
        if self.apply_decided(decided, true) {
            self.after_block();
        }
    }

    /// Aplica um bloco final, grava no log e limpa o diário. `own` = decidido por nós (conta rodadas).
    fn apply_decided(&mut self, d: DecidedBlock, _own: bool) -> bool {
        let Ok(raw) = d.encode() else { return false };
        let ids: Vec<Hash32> = d.block.txs.iter().map(Tx::id).collect();
        let height = d.block.height;
        // o índice de transações muda **com a cadeia travada**: quem vê a altura nova já vê o índice novo
        let ok = self.shared.chain.lock().is_ok_and(|mut c| {
            let applied = c.apply(d).is_ok();
            if applied && let Ok(mut i) = self.shared.info.lock() {
                for id in &ids {
                    i.tx_index.insert(*id, height);
                }
            }
            applied
        });
        if !ok {
            return false;
        }
        let _ = self.blocklog.append(&raw, true);
        let _ = self.journal.clear();
        self.journal_msgs.clear();
        self.height = height;
        self.engine = None;
        self.set_running(false);
        self.last_block_at = Instant::now();
        self.shared.stats.blocks.fetch_add(1, Ordering::Relaxed);
        true
    }

    fn after_block(&mut self) {
        self.refresh_set();
        let chain = Arc::clone(&self.shared);
        self.pool
            .purge(&|a| chain.chain.lock().map_or(0, |c| c.state().nonce(a)));
        self.drop_dead_heads();
        self.publish_pool();
        self.broadcast(&Wire::Status {
            height: self.height,
            tip: self.tip(),
        });
        self.starting = false;
        self.schedule_start(false);
    }

    // ------------------------------------------------------------------ prazos e sincronização

    fn fire_timers(&mut self) {
        let now = Instant::now();
        while let Some(Reverse((at, _))) = self.timers.peek() {
            if *at > now {
                break;
            }
            let Some(Reverse((_, t))) = self.timers.pop() else {
                break;
            };
            match t {
                Timer::Step(step, round, height) => {
                    if height == self.height + 1 && self.engine.is_some() {
                        self.feed(Event::Timeout(step, round));
                    }
                }
                Timer::Start(height) => {
                    if height == self.height + 1 && self.engine.is_none() {
                        self.start_height();
                    } else {
                        self.starting = false;
                    }
                }
                Timer::Heartbeat => {
                    self.broadcast(&Wire::Status {
                        height: self.height,
                        tip: self.tip(),
                    });
                    let peers: Vec<PublicKey> = self.peers.keys().copied().collect();
                    for p in peers {
                        self.maybe_sync(p);
                    }
                    self.schedule_start(false);
                    self.timers.push(Reverse((
                        Instant::now() + Duration::from_secs(1),
                        Timer::Heartbeat,
                    )));
                }
            }
        }
    }

    fn maybe_sync(&mut self, peer: PublicKey) {
        let theirs = self.peer_heights.get(&peer).copied().unwrap_or(0);
        if theirs <= self.height {
            return;
        }
        let now = Instant::now();
        if let Some((h, at)) = self.sync_asked.get(&peer)
            && *h == self.height
            && now.duration_since(*at) < Duration::from_secs(2)
        {
            return;
        }
        self.sync_asked.insert(peer, (self.height, now));
        self.send_to(
            &peer,
            &Wire::GetBlocks {
                from: self.height + 1,
                max: 32,
            },
        );
    }

    // ------------------------------------------------------------------ saída

    fn send_to(&mut self, key: &PublicKey, w: &Wire) {
        if let Some(p) = self.peers.get(key) {
            match p.tx.try_send(Arc::new(w.encode())) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                Err(TrySendError::Full(_)) => {} // par lento: perde esta mensagem; a sincronização repõe
            }
        }
    }

    fn broadcast(&self, w: &Wire) {
        let msg = Arc::new(w.encode());
        for p in self.peers.values() {
            let _ = p.tx.try_send(msg.clone());
        }
    }

    /// Depois de cada bloco: a cabeça de cada fila do mempool que **não vale mais** (um mint com referência já usada,
    /// por exemplo) sai, junto com as seguintes do remetente. Sem isso ela ficaria parada para sempre nos nós que
    /// não montaram o bloco e travaria o nonce do remetente.
    fn drop_dead_heads(&mut self) {
        let heads = {
            let Ok(chain) = self.shared.chain.lock() else {
                return;
            };
            self.pool.heads(&|a| chain.state().nonce(a))
        };
        if heads.is_empty() {
            return;
        }
        let bad: Vec<Tx> = {
            let Ok(mut chain) = self.shared.chain.lock() else {
                return;
            };
            let (_, bad_ids) = chain.select_txs(heads.iter());
            heads
                .into_iter()
                .filter(|t| bad_ids.contains(&t.id()))
                .collect()
        };
        for t in bad {
            self.pool.remove_sender_from(&t.sender_address(), t.nonce);
        }
    }

    /// Relê o conjunto de validadores em vigor (a rotação muda a cada bloco): atualiza quem pode conectar, derruba
    /// pares que saíram e decide se este nó vota (em vigor) ou só observa.
    fn refresh_set(&mut self) {
        let (set, pending) = self
            .shared
            .chain
            .lock()
            .map(|c| (c.validators(), c.pending_change()))
            .unwrap_or_default();
        let mut allowed: std::collections::HashSet<PublicKey> = set.iter().copied().collect();
        if let Some(p) = pending
            && p.action == brdu_ledger::gov::Action::Add
        {
            allowed.insert(p.key);
        }
        let gone: Vec<PublicKey> = self
            .peers
            .keys()
            .filter(|k| !allowed.contains(k))
            .copied()
            .collect();
        for k in &gone {
            self.peers.remove(k);
            self.peer_heights.remove(k);
        }
        if !gone.is_empty() {
            self.publish_peers();
        }
        if let Ok(mut a) = self.shared.allowed.lock() {
            *a = allowed;
        }
        self.in_set = set.contains(&self.me);
    }

    fn publish_peers(&self) {
        if let Ok(mut i) = self.shared.info.lock() {
            i.peers = self.peers.keys().copied().collect();
        }
    }

    fn publish_pool(&self) {
        if let Ok(mut i) = self.shared.info.lock() {
            i.pool_len = self.pool.len();
            i.pool_bytes = self.pool.bytes();
        }
    }

    fn set_running(&self, on: bool) {
        if let Ok(mut i) = self.shared.info.lock() {
            i.engine_running = on;
        }
    }
}

fn wire_of(m: Message) -> Wire {
    match m {
        Message::Proposal(p) => Wire::Proposal(p),
        Message::Vote(v) => Wire::Vote(v),
    }
}

fn decode_journal(records: &[Vec<u8>], max_block: usize) -> Vec<Message> {
    records
        .iter()
        .filter_map(|r| {
            let (tag, body) = r.split_first()?;
            match *tag {
                JOURNAL_PROPOSAL => brdu_bft::Proposal::decode(body, max_block)
                    .ok()
                    .map(Message::Proposal),
                JOURNAL_VOTE => brdu_bft::Vote::decode(body).ok().map(Message::Vote),
                _ => None,
            }
        })
        .collect()
}
