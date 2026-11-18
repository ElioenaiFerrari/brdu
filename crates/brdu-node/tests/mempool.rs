#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Address, SecretKey};
use brdu_ledger::{Tx, coin, kind};
use brdu_node::{Mempool, PoolError};

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

fn tx(from: &SecretKey, nonce: u64) -> Tx {
    let to = Address::from_public_key(&key(200).public());
    Tx::new_signed(
        from,
        "m",
        kind::TRANSFER,
        nonce,
        5_000,
        coin::transfer_payload(&to, 1),
    )
    .unwrap()
}

fn add(p: &mut Mempool, t: &Tx, committed: u64) -> Result<(), PoolError> {
    p.add(t.clone(), t.id(), committed)
}

#[test]
fn duplicata_vencida_e_buraco() {
    let mut p = Mempool::new(100, 1 << 20);
    let a = key(1);
    let t0 = tx(&a, 0);
    add(&mut p, &t0, 0).unwrap();
    assert_eq!(add(&mut p, &t0, 0), Err(PoolError::Duplicate));
    assert!(p.contains(&t0.id()));
    // nonce já usado na cadeia
    assert_eq!(
        add(&mut p, &tx(&a, 3), 5),
        Err(PoolError::StaleNonce {
            expected: 5,
            got: 3
        })
    );
    // buraco enorme
    assert!(matches!(
        add(&mut p, &tx(&a, 500), 0),
        Err(PoolError::NonceGap { .. })
    ));
}

#[test]
fn transacao_com_buraco_espera_e_e_promovida_quando_a_anterior_chega() {
    let mut p = Mempool::new(100, 1 << 20);
    let a = key(1);
    let (t0, t1, t2) = (tx(&a, 0), tx(&a, 1), tx(&a, 2));
    add(&mut p, &t2, 0).unwrap();
    add(&mut p, &t1, 0).unwrap();
    assert_eq!((p.len(), p.ready_len()), (2, 0), "esperando a de nonce 0");
    add(&mut p, &t0, 0).unwrap();
    assert_eq!(
        (p.len(), p.ready_len()),
        (3, 3),
        "as três ficam prontas, em ordem de nonce"
    );
    let nonces: Vec<u64> = p.iter().map(|t| t.nonce).collect();
    assert_eq!(nonces, vec![0, 1, 2]);
}

#[test]
fn limites_em_transacoes_e_em_bytes() {
    let mut p = Mempool::new(3, 1 << 20);
    for i in 0..3 {
        add(&mut p, &tx(&key(i + 1), 0), 0).unwrap();
    }
    assert_eq!(add(&mut p, &tx(&key(9), 0), 0), Err(PoolError::Full));
    let size = tx(&key(1), 0).size();
    let mut p = Mempool::new(100, size * 2 + size / 2);
    add(&mut p, &tx(&key(1), 0), 0).unwrap();
    add(&mut p, &tx(&key(2), 0), 0).unwrap();
    assert_eq!(
        add(&mut p, &tx(&key(3), 0), 0),
        Err(PoolError::Full),
        "estoura em bytes antes das transações"
    );
    assert!(p.bytes() <= size * 2 + size / 2);
}

#[test]
fn remover_e_purgar_depois_do_bloco() {
    let mut p = Mempool::new(100, 1 << 20);
    let (a, b) = (key(1), key(2));
    let ts: Vec<Tx> = (0..4).map(|n| tx(&a, n)).collect();
    for t in &ts {
        add(&mut p, t, 0).unwrap();
    }
    add(&mut p, &tx(&b, 0), 0).unwrap();
    add(&mut p, &tx(&b, 2), 0).unwrap(); // com buraco
    p.remove(&ts[3].id());
    assert!(!p.contains(&ts[3].id()));
    // um bloco incluiu a[0..2] e b[0]
    let (aa, bb) = (
        Address::from_public_key(&a.public()),
        Address::from_public_key(&b.public()),
    );
    p.purge(&|x| {
        if *x == aa {
            2
        } else if *x == bb {
            1
        } else {
            0
        }
    });
    let left: Vec<(u64, bool)> = p
        .iter()
        .map(|t| (t.nonce, t.sender == a.public()))
        .collect();
    assert_eq!(
        left,
        vec![(2, true)],
        "só a de nonce 2 da conta a; a de b com buraco segue esperando"
    );
    assert_eq!(p.len(), 2);
    // a lacuna de b se fecha quando chega a de nonce 1
    add(&mut p, &tx(&b, 1), 1).unwrap();
    assert_eq!(p.ready_len(), 3, "b[1] e b[2] ficaram prontas");
    p.remove(&ts[2].id());
    let bytes_before = p.bytes();
    p.remove(&ts[2].id());
    assert_eq!(
        p.bytes(),
        bytes_before,
        "remover duas vezes não corrompe a contagem"
    );
}

#[test]
fn dedupe_em_tempo_constante_com_muitas_pendentes() {
    // a lição do BRDC: com milhares de pendentes, checar duplicata não pode custar O(n)
    let mut p = Mempool::new(50_000, 1 << 30);
    let keys: Vec<SecretKey> = (0..50u8).map(key).collect();
    let mut all = Vec::new();
    for n in 0..100u64 {
        for k in &keys {
            let t = tx(k, n);
            add(&mut p, &t, 0).unwrap();
            all.push(t);
        }
    }
    assert_eq!(p.len(), 5_000);
    let started = std::time::Instant::now();
    for t in &all {
        assert!(p.contains(&t.id()));
        assert_eq!(add(&mut p, t, 0), Err(PoolError::Duplicate));
    }
    // 5.000 duplicatas (inclui refazer o id de cada uma): folgado mesmo em depuração; O(n) por checagem seriam 25 milhões de hashes
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn transacao_que_sai_por_falha_devolve_o_nonce_ao_remetente() {
    // a lição do CLI: um mint repetido (recusado ao montar o bloco) não pode "queimar" o nonce no pool
    let mut p = Mempool::new(100, 1 << 20);
    let a = key(1);
    let bad = tx(&a, 0);
    add(&mut p, &bad, 0).unwrap();
    p.remove(&bad.id()); // o bloco a recusou: sai do pool, o nonce 0 continua livre na cadeia
    let good = Tx::new_signed(
        &a,
        "m",
        kind::TRANSFER,
        0,
        6_000,
        coin::transfer_payload(&Address::from_public_key(&key(201).public()), 2),
    )
    .unwrap();
    add(&mut p, &good, 0).unwrap();
    assert_eq!(p.ready_len(), 1);
}
