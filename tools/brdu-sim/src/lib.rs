//! Simuladores **só de dev**. A equivocação: com a chave de um validador de TESTE **parado**, abre conexões Noise
//! (autenticadas como ele) com os outros validadores e manda **dois prevotes conflitantes** na mesma altura e
//! rodada. Cada nó honesto deve detectar a equivocação e expor a evidência em `/evidence`.

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

/// Manda os dois votos conflitantes a cada alvo (`host:porta` P2P) na altura `height`. Devolve em quantos alvos
/// entregou.
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
    // 1) abre TODAS as conexões; 2) manda os dois votos a todos de uma vez (a rede honesta pode decidir a altura em
    // poucos milissegundos, e voto de altura velha é ignorado)
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
    // dá tempo de os alvos lerem antes de fechar
    std::thread::sleep(Duration::from_millis(500));
    Ok(delivered)
}
