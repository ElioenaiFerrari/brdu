use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use brdu_bft::Vote;
use brdu_chain::{Chain, ChainError, Genesis};
use brdu_crypto::{Hash32, PublicKey, SecretKey};
use brdu_ledger::Tx;
use thiserror::Error;

use crate::core::{Core, Input};
use crate::wire::Limits;

/// Errors while starting the node.
#[derive(Debug, Error)]
pub enum NodeError {
    /// I/O.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// Chain.
    #[error("chain: {0}")]
    Chain(#[from] ChainError),
    /// Network.
    #[error("net: {0}")]
    Net(#[from] brdu_p2p::NetError),
    /// Configuration.
    #[error("config: {0}")]
    Config(String),
}

/// Consensus timeouts: `base + delta * round`.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// Wait for the proposal.
    pub propose: Duration,
    /// Wait after more than 2/3 of prevotes.
    pub prevote: Duration,
    /// Wait after more than 2/3 of precommits.
    pub precommit: Duration,
    /// Increase per round.
    pub delta: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            propose: Duration::from_millis(3000),
            prevote: Duration::from_millis(1000),
            precommit: Duration::from_millis(1000),
            delta: Duration::from_millis(500),
        }
    }
}

/// Node configuration.
pub struct Config {
    /// The network genesis.
    pub genesis: Genesis,
    /// This validator's consensus key.
    pub key: SecretKey,
    /// Data directory (block log, journal, Noise key).
    pub data_dir: PathBuf,
    /// Where to listen for the other validators (port 0 = any).
    pub listen: SocketAddr,
    /// HTTP API (None = no API).
    pub api: Option<SocketAddr>,
    /// Addresses of the other validators (`host:port`).
    pub peers: Vec<String>,
    /// Timeouts.
    pub timeouts: Timeouts,
    /// Minimum spacing between blocks (batches transactions).
    pub min_block_interval: Duration,
    /// Produce empty blocks? (default: no; with no transactions the network sleeps.)
    pub empty_blocks: bool,
    /// Mempool limit in transactions.
    pub max_pool_txs: usize,
    /// Mempool limit in bytes.
    pub max_pool_bytes: usize,
}

impl Config {
    /// Configuration with the defaults.
    #[must_use]
    pub fn new(genesis: Genesis, key: SecretKey, data_dir: PathBuf) -> Self {
        Self {
            genesis,
            key,
            data_dir,
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            api: None,
            peers: Vec::new(),
            timeouts: Timeouts::default(),
            min_block_interval: Duration::from_millis(500),
            empty_blocks: false,
            max_pool_txs: 50_000,
            max_pool_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Counters for the load lab.
#[derive(Default)]
pub struct Stats {
    /// Transactions accepted into the mempool.
    pub txs_accepted: AtomicU64,
    /// Rejected (for any reason).
    pub txs_rejected: AtomicU64,
    /// Gossip duplicates received.
    pub txs_duplicate: AtomicU64,
    /// Transactions relayed.
    pub txs_relayed: AtomicU64,
    /// Blocks applied.
    pub blocks: AtomicU64,
    /// Rounds beyond the first (turn change).
    pub extra_rounds: AtomicU64,
    /// Messages received.
    pub msgs_in: AtomicU64,
}

/// What the other threads read.
pub struct Info {
    /// Keys of the connected peers.
    pub peers: Vec<PublicKey>,
    /// Transactions in the mempool.
    pub pool_len: usize,
    /// Bytes in the mempool.
    pub pool_bytes: usize,
    /// Equivocation evidence seen.
    pub evidence: Vec<(Vote, Vote)>,
    /// Transaction id → block height.
    pub tx_index: HashMap<Hash32, u64>,
    /// Is consensus running (or waiting for a transaction)?
    pub engine_running: bool,
    /// Latest network errors (handshake refused, dial failed): diagnostics.
    pub net_errors: Vec<String>,
}

/// What the core shares with the other threads.
pub struct Shared {
    /// The chain.
    pub chain: Mutex<Chain>,
    /// Read-side state.
    pub info: Mutex<Info>,
    /// Counters.
    pub stats: Stats,
    /// Stop request.
    pub stop: AtomicBool,
    /// Message limits.
    pub limits: Limits,
    /// Our public key.
    pub me: PublicKey,
    /// Who may open a connection to us: the validator set in force plus the key joining in a pending change
    /// (so the new node can sync before it counts). The core updates it on every block.
    pub allowed: Mutex<std::collections::HashSet<PublicKey>>,
}

/// A running node.
pub struct Node {
    pub(crate) shared: Arc<Shared>,
    pub(crate) core_tx: Sender<Input>,
    threads: Vec<JoinHandle<()>>,
    listen_addr: SocketAddr,
    api_addr: Option<SocketAddr>,
}

impl Node {
    /// Starts the node: loads the log, listens, dials the peers and brings up the API.
    pub fn start(cfg: Config) -> Result<Self, NodeError> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let chain = Chain::with_executor(cfg.genesis.clone(), |rules| {
            let threads = std::thread::available_parallelism()
                .map_or(1, usize::from)
                .min(4);
            brdu_ledger::Executor::new(rules).with_verify_threads(threads)
        })?;
        let limits = Limits {
            max_block_bytes: cfg.genesis.max_block_bytes as usize,
            max_block_txs: cfg.genesis.max_block_txs as usize,
        };
        let me = cfg.key.public();
        let shared = Arc::new(Shared {
            chain: Mutex::new(chain),
            info: Mutex::new(Info {
                peers: Vec::new(),
                pool_len: 0,
                pool_bytes: 0,
                evidence: Vec::new(),
                tx_index: HashMap::new(),
                engine_running: false,
                net_errors: Vec::new(),
            }),
            stats: Stats::default(),
            stop: AtomicBool::new(false),
            limits,
            me,
            allowed: Mutex::new(cfg.genesis.validators.iter().copied().collect()),
        });
        let (core_tx, core_rx) = channel();

        let listener = TcpListener::bind(cfg.listen)?;
        listener.set_nonblocking(true)?;
        let listen_addr = listener.local_addr()?;

        let identity = Arc::new(brdu_p2p::NetIdentity::load_or_create(
            SecretKey::from_bytes(*cfg.key.expose_seed()),
            &cfg.genesis.chain_id,
            cfg.genesis.hash(),
            &cfg.data_dir.join("noise.key"),
        )?);
        let ctx = Arc::new(crate::net::NetCtx::new(
            identity,
            &cfg.genesis,
            me,
            core_tx.clone(),
            shared.clone(),
        ));

        let mut threads = Vec::new();
        threads.push(crate::net::spawn_listener(listener, ctx.clone()));
        for addr in &cfg.peers {
            threads.push(crate::net::spawn_dialer(addr.clone(), ctx.clone()));
        }

        let api_addr = match cfg.api {
            Some(addr) => {
                let (handles, bound) = crate::api::spawn(addr, shared.clone(), core_tx.clone())?;
                threads.extend(handles);
                Some(bound)
            }
            None => None,
        };

        let core = Core::new(cfg, shared.clone(), core_rx, core_tx.clone())?;
        threads.push(
            std::thread::Builder::new()
                .name("brdu-core".into())
                .spawn(move || core.run())?,
        );

        Ok(Self {
            shared,
            core_tx,
            threads,
            listen_addr,
            api_addr,
        })
    }

    /// Address on which the node listens for peers.
    #[must_use]
    pub const fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// HTTP API address.
    #[must_use]
    pub const fn api_addr(&self) -> Option<SocketAddr> {
        self.api_addr
    }

    /// Shared state (reads).
    #[must_use]
    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// Chain height.
    #[must_use]
    pub fn height(&self) -> u64 {
        self.shared.chain.lock().map_or(0, |c| c.height())
    }

    /// Current state root.
    #[must_use]
    pub fn state_root(&self) -> Hash32 {
        self.shared
            .chain
            .lock()
            .map_or(Hash32::ZERO, |c| c.state().root())
    }

    /// Hash of the last block.
    #[must_use]
    pub fn tip(&self) -> Option<Hash32> {
        self.shared
            .chain
            .lock()
            .ok()
            .and_then(|c| c.state().last_hash())
    }

    /// How many peers are connected.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.shared.info.lock().map_or(0, |i| i.peers.len())
    }

    /// Equivocation evidence seen.
    #[must_use]
    pub fn evidence_count(&self) -> usize {
        self.shared.info.lock().map_or(0, |i| i.evidence.len())
    }

    /// Submits a transaction to the mempool (the same path as the API).
    pub fn submit(&self, tx: &Tx) -> Result<Hash32, String> {
        let raw = tx.encode().map_err(|e| e.to_string())?;
        crate::api::submit_raw(&self.shared, &self.core_tx, &raw)
    }

    /// Stops the node and waits for the threads.
    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.core_tx.send(Input::Stop);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.core_tx.send(Input::Stop);
    }
}
