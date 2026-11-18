#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[macro_use]
mod common;

use brdu_crypto::Hash32;
use brdu_ledger::{Block, LedgerError, State};
use common::*;
use proptest::prelude::*;

fn funded(env: &Env) -> State {
    let mut s = State::new();
    let t = mint(env, &s, &addr(&env.alice), 100_000_000, "dep-1");
    block_of(env, &mut s, 1_000, vec![t]);
    s
}

#[test]
fn bloco_vale_por_inteiro_e_a_raiz_confere() {
    let env = Env::new();
    let mut s = funded(&env);
    assert_eq!((s.height(), s.time()), (1, 1_000));
    let t1 = transfer(&env, &s, &env.alice, &addr(&env.bob), 1_000_000);
    let mut probe = s.clone();
    env.exec.apply_tx(&mut probe, &t1).unwrap();
    let t2 = transfer(&env, &probe, &env.alice, &addr(&env.carol), 2_000_000);
    let b = block_of(&env, &mut s, 1_005, vec![t1, t2]);
    assert_eq!(s.height(), 2);
    assert_eq!(s.last_hash(), Some(b.hash()));
    assert_eq!(s.balance(&addr(&env.bob)), 1_000_000);
    assert_eq!(s.balance(&addr(&env.carol)), 2_000_000);
}

#[test]
fn a_taxa_e_repartida_entre_os_validadores_e_o_resto_vai_ao_proponente() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 1_000_000);
    let fee = t.fee;
    let b = block_of(&env, &mut s, 1_010, vec![t]);
    let (share, rest) = (fee / 4, fee % 4);
    for signer in &b.signers {
        let extra = if *signer == b.proposer { rest } else { 0 };
        assert_eq!(s.balance(signer), share + extra);
    }
    // nenhuma unidade se perde: a soma dos saldos é exatamente o que foi emitido
    assert_eq!(total_balances(&s), u128::from(s.supply()));
}

#[test]
fn uma_transacao_invalida_derruba_o_bloco_e_o_estado_nao_muda_nada() {
    let env = Env::new();
    let mut s = funded(&env);
    let good = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    // a segunda repete o nonce da primeira: inválida
    let bad = transfer(&env, &s, &env.alice, &addr(&env.bob), 6);
    let before = s.clone();
    let root_before = s.root();
    let built = env.exec.build_block(
        &mut s,
        env.proposer(),
        env.signers(),
        1_020,
        vec![good.clone(), bad.clone()],
    );
    assert!(
        matches!(built, Err(LedgerError::InvalidTx { .. })),
        "{built:?}"
    );
    assert_eq!(s, before);
    // e um bloco montado à mão com a raiz de um bloco bom, mas com a transação ruim, também não entra
    let mut ok_block = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_020, vec![good])
        .unwrap();
    ok_block.txs.push(bad);
    assert!(matches!(
        env.exec.apply_block(&mut s, &ok_block),
        Err(LedgerError::InvalidTx { .. })
    ));
    assert_eq!(s, before);
    assert_eq!(s.root(), root_before);
}

#[test]
fn raiz_errada_ligacao_errada_hora_e_limites_sao_recusados_sem_alterar_o_estado() {
    let env = Env::with_rules(|r| r.max_block_txs = 2);
    let mut s = funded(&env);
    let before = s.clone();
    let good = {
        let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
        env.exec
            .build_block(&mut s, env.proposer(), env.signers(), 1_100, vec![t])
            .unwrap()
    };

    let mut b = good.clone();
    b.state_root = Hash32([1; 32]);
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::StateRootMismatch)
    );

    let mut b = good.clone();
    b.prev_hash = Some(Hash32([2; 32]));
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::BadLinkage)
    );

    let mut b = good.clone();
    b.height = 9;
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::BadLinkage)
    );

    let mut b = good.clone();
    b.time = 999; // o estado está em 1.000
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::TimeWentBack)
    );

    let mut b = good.clone();
    b.txs = vec![good.txs[0].clone(); 3];
    assert!(
        env.exec.apply_block(&mut s, &b).is_err(),
        "3 transações com limite 2 (e repetidas)"
    );
    assert_eq!(s, before, "nada disso mexeu no estado");
    // o bloco bom ainda entra
    env.exec.apply_block(&mut s, &good).unwrap();
}

#[test]
fn transacao_repetida_no_bloco_e_recusada() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    let built = env.exec.build_block(
        &mut s,
        env.proposer(),
        env.signers(),
        1_050,
        vec![t.clone(), t.clone()],
    );
    assert_eq!(built, Err(LedgerError::DuplicateTx(t.id())));
}

#[test]
fn signatarios_malformados_sao_recusados() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    let signers = env.signers();
    let mut reversed = signers.clone();
    reversed.reverse();
    let mut dup = signers.clone();
    dup.push(signers[0]);
    let outsider = addr(&key(99));
    for (name, proposer, set) in [
        ("fora de ordem", signers[0], reversed),
        ("repetidos", signers[0], dup),
        ("vazio", signers[0], vec![]),
        ("proponente fora do conjunto", outsider, signers.clone()),
    ] {
        let r = env
            .exec
            .build_block(&mut s, proposer, set, 1_060, vec![t.clone()]);
        assert_eq!(r, Err(LedgerError::BadLinkage), "{name}");
    }
}

#[test]
fn assinatura_adulterada_dentro_do_bloco_e_recusada() {
    let env = Env::new();
    let mut s = funded(&env);
    let mut t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    t.signature.0[10] ^= 1;
    let r = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_070, vec![t]);
    assert_eq!(r, Err(LedgerError::BadSignature));
}

#[test]
fn montar_um_bloco_nao_altera_o_estado_e_o_mesmo_bloco_aplicado_em_outro_no_da_a_mesma_raiz() {
    let env = Env::new();
    let mut s = funded(&env);
    let mut twin = s.clone();
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 7);
    let before = s.clone();
    let b = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_080, vec![t])
        .unwrap();
    assert_eq!(s, before);
    assert_eq!(s.root(), before.root());
    env.exec.apply_block(&mut s, &b).unwrap();
    env.exec.apply_block(&mut twin, &b).unwrap();
    assert_eq!(s.root(), twin.root());
    assert_eq!(s.root(), b.state_root);
    // o mesmo bloco duas vezes: já não é o seguinte
    assert_eq!(
        env.exec.apply_block(&mut s, &b),
        Err(LedgerError::BadLinkage)
    );
}

#[test]
fn bloco_vazio_so_avanca_a_altura_e_a_hora() {
    let env = Env::new();
    let mut s = funded(&env);
    let b = block_of(&env, &mut s, 2_000, vec![]);
    assert_eq!((s.height(), s.time(), b.txs.len()), (2, 2_000, 0));
}

#[test]
fn codec_do_bloco_ida_e_volta_e_limite_de_transacoes() {
    let env = Env::new();
    let mut s = funded(&env);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 5);
    let b = env
        .exec
        .build_block(&mut s, env.proposer(), env.signers(), 1_100, vec![t])
        .unwrap();
    let bytes = b.encode().unwrap();
    assert_eq!(Block::decode(&bytes, 1_000).unwrap(), b);
    assert_eq!(
        Block::decode(&bytes, 0),
        Err(LedgerError::Malformed),
        "mais transações que o limite"
    );
    assert_eq!(
        Block::decode(&bytes[..bytes.len() - 1], 1_000),
        Err(LedgerError::Malformed)
    );
    assert_eq!(
        Block::decode(&[bytes.clone(), vec![0]].concat(), 1_000),
        Err(LedgerError::Malformed)
    );
    assert_ne!(b.hash(), Hash32::ZERO);
}

proptest! {
    #[test]
    fn bloco_com_lixo_nunca_causa_panico(data in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = Block::decode(&data, 1_000);
    }
}

#[test]
fn cadeia_de_blocos_liga_um_no_outro() {
    let env = Env::new();
    let mut s = funded(&env);
    let mut prev = s.last_hash();
    for (i, time) in [1_100u64, 1_100, 1_300].iter().enumerate() {
        let b = block_of(&env, &mut s, *time, vec![]);
        assert_eq!(b.prev_hash, prev);
        assert_eq!(b.height, 2 + i as u64);
        prev = Some(b.hash());
    }
    assert_eq!(s.last_hash(), prev);
}

#[test]
fn estado_pendente_do_mempool_nao_vira_o_estado_do_bloco() {
    // o executor aplica transações a um estado "pendente" (taxa sai da conta); o do bloco reparte a taxa
    let env = Env::new();
    let committed = funded(&env);
    let mut pending = committed.clone();
    let t = transfer(&env, &pending, &env.alice, &addr(&env.bob), 10);
    env.exec.apply_tx(&mut pending, &t).unwrap();
    assert!(
        total_balances(&pending) < total_balances(&committed),
        "no pendente a taxa some"
    );
    let mut real = committed;
    block_of(&env, &mut real, 1_500, vec![t]);
    assert_eq!(
        total_balances(&real),
        u128::from(real.supply()),
        "no bloco a taxa é repartida"
    );
}
