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

/// Erros ao subir o nó.
#[derive(Debug, Error)]
pub enum NodeError {
    /// E/S.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// Cadeia.
    #[error("chain: {0}")]
    Chain(#[from] ChainError),
    /// Rede.
    #[error("net: {0}")]
    Net(#[from] brdu_p2p::NetError),
    /// Configuração.
    #[error("config: {0}")]
    Config(String),
}

/// Prazos do consenso: `base + delta * rodada`.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// Espera pela proposta.
    pub propose: Duration,
    /// Espera depois de mais de 2/3 de prevotes.
    pub prevote: Duration,
    /// Espera depois de mais de 2/3 de precommits.
    pub precommit: Duration,
    /// Acréscimo por rodada.
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

/// Configuração do nó.
pub struct Config {
    /// A gênese da rede.
    pub genesis: Genesis,
    /// Chave de consenso deste validador.
    pub key: SecretKey,
    /// Pasta de dados (log de blocos, diário, chave do Noise).
    pub data_dir: PathBuf,
    /// Onde escutar os outros validadores (porta 0 = qualquer).
    pub listen: SocketAddr,
    /// API HTTP (None = sem API).
    pub api: Option<SocketAddr>,
    /// Endereços dos outros validadores (`host:porta`).
    pub peers: Vec<String>,
    /// Prazos.
    pub timeouts: Timeouts,
    /// Espaço mínimo entre blocos (agrupa transações).
    pub min_block_interval: Duration,
    /// Produzir blocos vazios? (padrão: não; sem transação a rede dorme.)
    pub empty_blocks: bool,
    /// Limite do mempool em transações.
    pub max_pool_txs: usize,
    /// Limite do mempool em bytes.
    pub max_pool_bytes: usize,
}

impl Config {
    /// Configuração com os padrões.
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

/// Contadores para o laboratório de carga.
#[derive(Default)]
pub struct Stats {
    /// Transações aceitas no mempool.
    pub txs_accepted: AtomicU64,
    /// Recusadas (qualquer motivo).
    pub txs_rejected: AtomicU64,
    /// Duplicatas de fofoca recebidas.
    pub txs_duplicate: AtomicU64,
    /// Transações retransmitidas.
    pub txs_relayed: AtomicU64,
    /// Blocos aplicados.
    pub blocks: AtomicU64,
    /// Rodadas além da primeira (troca de turno).
    pub extra_rounds: AtomicU64,
    /// Mensagens recebidas.
    pub msgs_in: AtomicU64,
}

/// O que as outras threads leem.
pub struct Info {
    /// Chaves dos pares conectados.
    pub peers: Vec<PublicKey>,
    /// Transações no mempool.
    pub pool_len: usize,
    /// Bytes no mempool.
    pub pool_bytes: usize,
    /// Evidências de equivocação vistas.
    pub evidence: Vec<(Vote, Vote)>,
    /// id da transação → altura do bloco.
    pub tx_index: HashMap<Hash32, u64>,
    /// O consenso está rodando (ou esperando transação)?
    pub engine_running: bool,
    /// Últimos erros de rede (aperto de mão recusado, discagem falhou): diagnóstico.
    pub net_errors: Vec<String>,
}

/// O que o núcleo compartilha com as demais threads.
pub struct Shared {
    /// A cadeia.
    pub chain: Mutex<Chain>,
    /// Estado de leitura.
    pub info: Mutex<Info>,
    /// Contadores.
    pub stats: Stats,
    /// Pedido de parada.
    pub stop: AtomicBool,
    /// Limites de mensagem.
    pub limits: Limits,
    /// Nossa chave pública.
    pub me: PublicKey,
    /// Quem pode abrir conexão conosco: o conjunto de validadores em vigor mais a chave que entra numa troca
    /// pendente (para o novo nó sincronizar antes de valer). O núcleo atualiza a cada bloco.
    pub allowed: Mutex<std::collections::HashSet<PublicKey>>,
}

/// Um nó em execução.
pub struct Node {
    pub(crate) shared: Arc<Shared>,
    pub(crate) core_tx: Sender<Input>,
    threads: Vec<JoinHandle<()>>,
    listen_addr: SocketAddr,
    api_addr: Option<SocketAddr>,
}

impl Node {
    /// Sobe o nó: carrega o log, escuta, disca aos pares e liga a API.
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

    /// Endereço em que o nó escuta os pares.
    #[must_use]
    pub const fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// Endereço da API HTTP.
    #[must_use]
    pub const fn api_addr(&self) -> Option<SocketAddr> {
        self.api_addr
    }

    /// Compartilhado (leituras).
    #[must_use]
    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// Altura da cadeia.
    #[must_use]
    pub fn height(&self) -> u64 {
        self.shared.chain.lock().map_or(0, |c| c.height())
    }

    /// Raiz de estado atual.
    #[must_use]
    pub fn state_root(&self) -> Hash32 {
        self.shared
            .chain
            .lock()
            .map_or(Hash32::ZERO, |c| c.state().root())
    }

    /// Hash do último bloco.
    #[must_use]
    pub fn tip(&self) -> Option<Hash32> {
        self.shared
            .chain
            .lock()
            .ok()
            .and_then(|c| c.state().last_hash())
    }

    /// Quantos pares conectados.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.shared.info.lock().map_or(0, |i| i.peers.len())
    }

    /// Evidências de equivocação vistas.
    #[must_use]
    pub fn evidence_count(&self) -> usize {
        self.shared.info.lock().map_or(0, |i| i.evidence.len())
    }

    /// Envia uma transação ao mempool (o mesmo caminho da API).
    pub fn submit(&self, tx: &Tx) -> Result<Hash32, String> {
        let raw = tx.encode().map_err(|e| e.to_string())?;
        crate::api::submit_raw(&self.shared, &self.core_tx, &raw)
    }

    /// Para o nó e espera as threads.
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
