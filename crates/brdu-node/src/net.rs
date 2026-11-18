//! Networking: accepting, dialing and driving a connection. Each peer has **one** thread that owns the `Session`
//! (Noise keeps one counter per direction inside the same object): it drains the outbound queue and peeks at the
//! inbound side, never reading a frame halfway.

use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use brdu_chain::Genesis;
use brdu_crypto::PublicKey;
use brdu_p2p::{ConnLimiter, NetIdentity, PeerPolicy, Session, accept, connect};

use crate::core::Input;
use crate::node::Shared;
use crate::wire::{Limits, Wire};

const OUT_QUEUE: usize = 8192;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Only validators currently in force (plus whoever joins in a pending change), and never ourselves. The set lives
/// in `Shared`, updated by the core on every block (rotation, ADR-0015).
struct SetPolicy {
    shared: Arc<Shared>,
    me: PublicKey,
}

impl PeerPolicy for SetPolicy {
    fn accepts(&self, k: &PublicKey) -> bool {
        *k != self.me && self.shared.allowed.lock().is_ok_and(|a| a.contains(k))
    }
}

/// What the network threads share.
pub struct NetCtx {
    identity: Arc<NetIdentity>,
    policy: SetPolicy,
    core: Sender<Input>,
    shared: Arc<Shared>,
    limiter: ConnLimiter,
    next_id: AtomicU64,
    limits: Limits,
}

impl NetCtx {
    pub fn new(
        identity: Arc<NetIdentity>,
        _genesis: &Genesis,
        me: PublicKey,
        core: Sender<Input>,
        shared: Arc<Shared>,
    ) -> Self {
        let limits = shared.limits;
        Self {
            identity,
            policy: SetPolicy {
                shared: shared.clone(),
                me,
            },
            core,
            shared,
            limiter: ConnLimiter::new(64, 64, 2),
            next_id: AtomicU64::new(1),
            limits,
        }
    }

    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::SeqCst)
    }

    fn note(&self, what: &str, e: &dyn std::fmt::Display) {
        if let Ok(mut i) = self.shared.info.lock() {
            i.net_errors.push(format!("{what}: {e}"));
            if i.net_errors.len() > 8 {
                i.net_errors.remove(0);
            }
        }
    }

    fn connected(&self, key: &PublicKey) -> bool {
        self.shared.info.lock().is_ok_and(|i| i.peers.contains(key))
    }
}

fn sleep_unless_stopped(ctx: &NetCtx, total: Duration) {
    let end = Instant::now() + total;
    while Instant::now() < end && !ctx.stopped() {
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Accepts connections from validators.
pub fn spawn_listener(listener: TcpListener, ctx: Arc<NetCtx>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut workers: Vec<JoinHandle<()>> = Vec::new();
        while !ctx.stopped() {
            match listener.accept() {
                Ok((stream, addr)) => {
                    let _ = stream.set_nonblocking(false);
                    let Some(mut guard) = ctx.limiter.try_acquire(addr.ip()) else {
                        continue;
                    };
                    let ctx = ctx.clone();
                    workers.push(std::thread::spawn(move || {
                        match accept(stream, &ctx.identity, &ctx.policy) {
                            Ok(session) => {
                                if guard.bind_key(session.peer_key().as_bytes()) {
                                    run_session(session, false, &ctx);
                                }
                            }
                            Err(e) => ctx.note("accept", &e),
                        }
                        drop(guard);
                    }));
                    workers.retain(|w| !w.is_finished());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        for w in workers {
            let _ = w.join();
        }
    })
}

/// Dials an address and **dials again** when the connection drops.
pub fn spawn_dialer(addr: String, ctx: Arc<NetCtx>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut known: Option<PublicKey> = None;
        while !ctx.stopped() {
            if known.is_some_and(|k| ctx.connected(&k)) {
                sleep_unless_stopped(&ctx, Duration::from_millis(500));
                continue;
            }
            let target = addr.to_socket_addrs().ok().and_then(|mut a| a.next());
            match target {
                None => ctx.note(&addr, &"cannot resolve"),
                Some(target) => match TcpStream::connect_timeout(&target, Duration::from_secs(2)) {
                    Err(e) => ctx.note(&format!("dial {addr}"), &e),
                    Ok(stream) => match connect(stream, &ctx.identity, &ctx.policy, None) {
                        Ok(session) => {
                            known = Some(*session.peer_key());
                            run_session(session, true, &ctx);
                        }
                        Err(e) => ctx.note(&format!("handshake {addr}"), &e),
                    },
                },
            }
            sleep_unless_stopped(&ctx, Duration::from_millis(500));
        }
    })
}

/// Waits up to `wait` for data **without consuming it**.
fn readable(stream: &TcpStream, wait: Duration) -> std::io::Result<bool> {
    stream.set_read_timeout(Some(wait))?;
    let mut b = [0u8; 1];
    match stream.peek(&mut b) {
        Ok(0) => Err(std::io::ErrorKind::UnexpectedEof.into()),
        Ok(_) => Ok(true),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

fn run_session(mut session: Session, outbound: bool, ctx: &NetCtx) {
    let key = *session.peer_key();
    let id = ctx.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = sync_channel(OUT_QUEUE);
    if ctx
        .core
        .send(Input::PeerUp {
            key,
            outbound,
            tx,
            id,
        })
        .is_err()
    {
        return;
    }
    let limits = ctx.limits;
    let max_for = move |k: u32| limits.max_for(k);
    let mut last_activity = Instant::now();
    'conn: while !ctx.stopped() {
        // outbound
        loop {
            match rx.try_recv() {
                Ok(m) => {
                    if session.send(m.0, &m.1, WRITE_TIMEOUT).is_err() {
                        break 'conn;
                    }
                    last_activity = Instant::now();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'conn, // the core dropped this connection
            }
        }
        // inbound
        let poll = if last_activity.elapsed() < Duration::from_millis(200) {
            Duration::from_millis(2)
        } else {
            Duration::from_millis(20)
        };
        match readable(session.stream(), poll) {
            Ok(false) => {}
            Ok(true) => match session.recv(&max_for, READ_TIMEOUT) {
                Ok((kind, body)) => {
                    last_activity = Instant::now();
                    let Some(wire) = Wire::decode(kind, &body, &limits) else {
                        break;
                    };
                    // transaction signatures are checked HERE, in parallel across peers
                    let wire = match wire {
                        Wire::Txs(mut txs) => {
                            txs.retain(|t| t.verify().is_ok());
                            Wire::Txs(txs)
                        }
                        other => other,
                    };
                    let input = match wire {
                        Wire::Txs(txs) => {
                            let mut ok = true;
                            for tx in txs {
                                ok &= ctx
                                    .core
                                    .send(Input::Tx {
                                        tx,
                                        from: Some(key),
                                        reply: None,
                                    })
                                    .is_ok();
                            }
                            if !ok {
                                break;
                            }
                            continue;
                        }
                        other => Input::Net(key, other),
                    };
                    if ctx.core.send(input).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },
            Err(_) => break,
        }
    }
    let _ = ctx.core.send(Input::PeerDown { key, id });
}
