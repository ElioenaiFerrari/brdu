//! **Dev-only** simulators. Equivocation: with the key of a **stopped** TEST validator, open Noise connections
//! (authenticated as it) to the other validators and send **two conflicting prevotes** at the same height and
//! round. Every honest node must detect the equivocation and expose the evidence at `/evidence`.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use brdu_bft::{Vote, VoteType};
use brdu_chain::Genesis;
use brdu_crypto::{Hash32, PublicKey, SecretKey, tagged};
use brdu_node::Wire;
use brdu_p2p::{NetIdentity, PeerPolicy, connect};

struct AcceptValidators(Vec<PublicKey>);

impl PeerPolicy for AcceptValidators {
    fn accepts(&self, k: &PublicKey) -> bool {
        self.0.contains(k)
    }
}

/// Sends the two conflicting votes to every target (P2P `host:port`) at height `height`. Returns how many targets
/// it delivered to.
pub fn equivocate(
    genesis: &Genesis,
    key: &SecretKey,
    targets: &[String],
    height: u64,
) -> Result<usize, String> {
    let id = NetIdentity::generate(
        SecretKey::from_bytes(*key.expose_seed()),
        &genesis.chain_id,
        genesis.hash(),
    )
    .map_err(|e| e.to_string())?;
    let policy = AcceptValidators(genesis.validators.clone());
    let mut salt = [0u8; 8];
    getrandom::fill(&mut salt).map_err(|e| e.to_string())?;
    let (a, b): (Hash32, Hash32) = (
        tagged("brdu/sim/a", &[&salt]),
        tagged("brdu/sim/b", &[&salt]),
    );
    let votes = [
        Vote::sign(
            &genesis.chain_id,
            VoteType::Prevote,
            height,
            0,
            Some(a),
            key,
        ),
        Vote::sign(
            &genesis.chain_id,
            VoteType::Prevote,
            height,
            0,
            Some(b),
            key,
        ),
    ];
    // 1) open ALL connections; 2) send both votes to everyone at once (the honest network can decide the height in
    // a few milliseconds, and a vote for a stale height is ignored)
    let mut sessions = Vec::new();
    for t in targets {
        let Some(addr) = t.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
            continue;
        };
        let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
            continue;
        };
        if let Ok(session) = connect(stream, &id, &policy, None) {
            sessions.push(session);
        }
    }
    let mut delivered = 0;
    for session in &mut sessions {
        let mut ok = true;
        for v in &votes {
            let (k, body) = Wire::Vote(v.clone()).encode();
            ok &= session.send(k, &body, Duration::from_secs(3)).is_ok();
        }
        delivered += usize::from(ok);
    }
    // give the targets time to read before closing
    std::thread::sleep(Duration::from_millis(500));
    Ok(delivered)
}
