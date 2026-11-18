#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[macro_use]
mod common;

use brdu_crypto::{Address, Hash32};
use brdu_ledger::coin::{self, Approval};
use brdu_ledger::{Coverage, LedgerError, State, kind};
use common::*;

fn funded(env: &Env, amount: u64) -> State {
    let mut s = State::new();
    let t = mint(env, &s, &addr(&env.alice), amount, "dep-1");
    ok(env, &mut s, &t);
    s
}

// ---------------------------------------------------------------- mint

#[test]
fn mint_com_2_de_3_emissores_cria_saldo_e_oferta() {
    let env = Env::new();
    let s = funded(&env, 5_000_000);
    assert_eq!(s.balance(&addr(&env.alice)), 5_000_000);
    assert_eq!(
        (s.minted(), s.burned(), s.supply()),
        (5_000_000, 0, 5_000_000)
    );
}

#[test]
fn mint_com_menos_de_m_aprovacoes_e_recusado() {
    let env = Env::new();
    let mut s = State::new();
    for n in [0, 1] {
        let t = tx(
            &env.relayer,
            kind::MINT,
            0,
            0,
            env.mint_payload(&addr(&env.alice), 10, "r", n),
        );
        assert_eq!(
            err(&env, &mut s, &t),
            LedgerError::NotEnoughApprovals {
                valid: n,
                needed: 2
            }
        );
    }
    assert_eq!(s.supply(), 0);
    assert_eq!(
        s.nonce(&addr(&env.relayer)),
        0,
        "recusada não consome nonce"
    );
}

#[test]
fn o_mesmo_emissor_duas_vezes_conta_uma_so() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    let doc = coin::issuance_doc(false, CHAIN, &to, 10, "r");
    let one = Approval::new(&doc, &env.issuers[0]);
    let t = tx(
        &env.relayer,
        kind::MINT,
        0,
        0,
        coin::issuance_payload(&to, 10, "r", &[one, one, one]),
    );
    assert_eq!(
        err(&env, &mut s, &t),
        LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        }
    );
}

#[test]
fn aprovacao_de_quem_nao_e_emissor_e_ignorada() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    let doc = coin::issuance_doc(false, CHAIN, &to, 10, "r");
    let outsiders: Vec<_> = (50..53).map(|n| Approval::new(&doc, &key(n))).collect();
    let t = tx(
        &env.relayer,
        kind::MINT,
        0,
        0,
        coin::issuance_payload(&to, 10, "r", &outsiders),
    );
    assert_eq!(
        err(&env, &mut s, &t),
        LedgerError::NotEnoughApprovals {
            valid: 0,
            needed: 2
        }
    );
}

#[test]
fn aprovacao_so_vale_para_o_documento_exato() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    let approved = |doc: &Hash32| env.approvals(doc, 2);
    let variants = [
        coin::issuance_doc(false, CHAIN, &to, 11, "r"), // outro valor
        coin::issuance_doc(false, CHAIN, &to, 10, "outra"), // outra referência
        coin::issuance_doc(false, "outra-rede", &to, 10, "r"), // outra rede
        coin::issuance_doc(false, CHAIN, &addr(&env.bob), 10, "r"), // outra conta
        coin::issuance_doc(true, CHAIN, &to, 10, "r"),  // documento de BURN usado num mint
    ];
    for doc in variants {
        let t = tx(
            &env.relayer,
            kind::MINT,
            0,
            0,
            coin::issuance_payload(&to, 10, "r", &approved(&doc)),
        );
        assert!(
            matches!(
                err(&env, &mut s, &t),
                LedgerError::NotEnoughApprovals { valid: 0, .. }
            ),
            "{doc}"
        );
    }
}

#[test]
fn referencia_de_deposito_vale_uma_vez() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    let t = mint(&env, &s, &addr(&env.bob), 7, "dep-1"); // mesma referência, outro valor e outra conta
    assert_eq!(err(&env, &mut s, &t), LedgerError::RefUsed);
    okm!(env, s, &mint(&env, &s, &addr(&env.bob), 7, "dep-2"));
}

#[test]
fn mint_zero_referencia_vazia_ou_grande_e_estouro_sao_recusados() {
    let env = Env::new();
    let mut s = State::new();
    let to = addr(&env.alice);
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 0, "z")),
        LedgerError::ZeroAmount
    );
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 5, "")),
        LedgerError::BadRef
    );
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 5, &"x".repeat(65))),
        LedgerError::Malformed
    );
    okm!(env, s, &mint(&env, &s, &to, u64::MAX - 1, "grande"));
    let before = s.clone();
    assert_eq!(
        errm!(env, s, &mint(&env, &s, &to, 2, "estouro")),
        LedgerError::Overflow
    );
    assert_eq!(s, before, "o estouro desfaz tudo (inclusive o nonce)");
}

// ---------------------------------------------------------------- burn

#[test]
fn burn_so_do_dono_e_com_aprovacao() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    // bob tenta queimar o saldo da alice: o remetente tem de ser o titular
    let t = tx(
        &env.bob,
        kind::BURN,
        0,
        0,
        env.burn_payload(&addr(&env.alice), 10, "b", 2),
    );
    assert_eq!(err(&env, &mut s, &t), LedgerError::NotOwner);
    // sem aprovações suficientes
    let t = tx(
        &env.alice,
        kind::BURN,
        0,
        0,
        env.burn_payload(&addr(&env.alice), 10, "b", 1),
    );
    assert!(matches!(
        err(&env, &mut s, &t),
        LedgerError::NotEnoughApprovals {
            valid: 1,
            needed: 2
        }
    ));
    // certo
    okm!(env, s, &burn(&env, &s, &env.alice, 400, "b"));
    assert_eq!(
        (s.balance(&addr(&env.alice)), s.burned(), s.supply()),
        (600, 400, 600)
    );
}

#[test]
fn burn_maior_que_o_saldo_ou_zero_e_recusado_e_referencias_de_mint_e_burn_sao_separadas() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    assert_eq!(
        errm!(env, s, &burn(&env, &s, &env.alice, 1_001, "b")),
        LedgerError::InsufficientFunds
    );
    assert_eq!(
        errm!(env, s, &burn(&env, &s, &env.alice, 0, "b")),
        LedgerError::ZeroAmount
    );
    // "dep-1" já foi usada como referência de MINT; como referência de BURN é outra coisa
    okm!(env, s, &burn(&env, &s, &env.alice, 1, "dep-1"));
    assert_eq!(
        errm!(env, s, &burn(&env, &s, &env.alice, 1, "dep-1")),
        LedgerError::RefUsed
    );
}

// ---------------------------------------------------------------- atestado e cobertura

#[test]
fn atestado_exige_sequencia_e_aprovacoes_e_a_cobertura_e_so_informacao() {
    let env = Env::new();
    let mut s = funded(&env, 1_000);
    assert_eq!(s.coverage(), Coverage::None);
    assert_eq!(
        errm!(env, s, &attest(&env, &s, 2, 1_000, "banco")),
        LedgerError::BadSequence {
            expected: 1,
            got: 2
        }
    );
    let nonce = s.nonce(&addr(&env.relayer));
    let weak = tx(
        &env.relayer,
        kind::ATTESTATION,
        nonce,
        0,
        env.attest_payload(1, 1_000, "banco", 1),
    );
    assert!(matches!(
        err(&env, &mut s, &weak),
        LedgerError::NotEnoughApprovals { .. }
    ));
    okm!(env, s, &attest(&env, &s, 1, 1_000, "banco"));
    assert_eq!(
        s.attestation().map(|a| (a.seq, a.reserve)),
        Some((1, 1_000))
    );
    assert_eq!(s.coverage(), Coverage::Covered(0), "reserva = oferta");
    // repetir a mesma sequência (replay do atestado) é recusado
    assert_eq!(
        errm!(env, s, &attest(&env, &s, 1, 9_999, "banco")),
        LedgerError::BadSequence {
            expected: 2,
            got: 1
        }
    );
    // a cobertura acompanha a oferta, sem bloquear nada
    okm!(env, s, &mint(&env, &s, &addr(&env.bob), 500, "dep-2"));
    assert_eq!(s.coverage(), Coverage::Under(500));
    okm!(env, s, &attest(&env, &s, 2, 2_000, "banco"));
    assert_eq!(s.coverage(), Coverage::Covered(500));
}

// ---------------------------------------------------------------- transferência, taxa e nonce

#[test]
fn transferencia_move_saldo_cobra_taxa_e_sobe_o_nonce() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 2_500_000);
    let fee = t.fee;
    assert_eq!(
        fee,
        1_000 + t.size() as u64,
        "taxa mínima = base + 1 por byte"
    );
    ok(&env, &mut s, &t);
    assert_eq!(s.balance(&addr(&env.bob)), 2_500_000);
    assert_eq!(s.balance(&addr(&env.alice)), 10_000_000 - 2_500_000 - fee);
    assert_eq!(s.nonce(&addr(&env.alice)), 1);
}

#[test]
fn replay_nonce_futuro_e_rede_errada_sao_recusados() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 1);
    ok(&env, &mut s, &t);
    assert_eq!(
        err(&env, &mut s, &t),
        LedgerError::BadNonce {
            expected: 1,
            got: 0
        },
        "replay"
    );
    let future = tx(
        &env.alice,
        kind::TRANSFER,
        5,
        10_000,
        coin::transfer_payload(&addr(&env.bob), 1),
    );
    assert_eq!(
        err(&env, &mut s, &future),
        LedgerError::BadNonce {
            expected: 1,
            got: 5
        }
    );
    let other = brdu_ledger::Tx::new_signed(
        &env.alice,
        "outra-rede",
        kind::TRANSFER,
        1,
        10_000,
        coin::transfer_payload(&addr(&env.bob), 1),
    )
    .unwrap();
    assert_eq!(err(&env, &mut s, &other), LedgerError::WrongChain);
}

#[test]
fn taxa_abaixo_do_minimo_e_recusada_e_a_exata_passa() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let probe = tx(
        &env.alice,
        kind::TRANSFER,
        0,
        0,
        coin::transfer_payload(&addr(&env.bob), 1),
    );
    let min = env.exec.min_fee(&probe).unwrap();
    let low = tx(
        &env.alice,
        kind::TRANSFER,
        0,
        min - 1,
        coin::transfer_payload(&addr(&env.bob), 1),
    );
    assert_eq!(
        err(&env, &mut s, &low),
        LedgerError::FeeTooLow { min, got: min - 1 }
    );
    ok(
        &env,
        &mut s,
        &tx(
            &env.alice,
            kind::TRANSFER,
            0,
            min,
            coin::transfer_payload(&addr(&env.bob), 1),
        ),
    );
}

#[test]
fn saldo_insuficiente_para_valor_mais_taxa_desfaz_tudo() {
    let env = Env::new();
    let mut s = funded(&env, 1_000_000);
    let before = s.clone();
    // o saldo cobre o valor, mas não valor + taxa
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 1_000_000);
    assert_eq!(err(&env, &mut s, &t), LedgerError::InsufficientFunds);
    assert_eq!(s, before, "a taxa e o nonce também voltam");
    let t = transfer(&env, &s, &env.alice, &addr(&env.bob), 0);
    assert_eq!(err(&env, &mut s, &t), LedgerError::ZeroAmount);
    assert_eq!(s, before);
}

#[test]
fn transferir_para_si_mesmo_so_gasta_a_taxa() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = transfer(&env, &s, &env.alice, &addr(&env.alice), 3_000_000);
    ok(&env, &mut s, &t);
    assert_eq!(s.balance(&addr(&env.alice)), 10_000_000 - t.fee);
}

#[test]
fn carga_com_tamanho_errado_ou_sobra_e_malformada() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let good = coin::transfer_payload(&addr(&env.bob), 5);
    for bad in [
        good[..39].to_vec(),
        [good.clone(), vec![0]].concat(),
        vec![],
    ] {
        let t = tx(&env.alice, kind::TRANSFER, 0, 50_000, bad);
        assert_eq!(err(&env, &mut s, &t), LedgerError::Malformed);
    }
}

#[test]
fn tipo_de_modulo_sem_modulo_registrado_e_recusado() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let t = tx(&env.alice, 5, 0, 50_000, vec![]);
    assert_eq!(err(&env, &mut s, &t), LedgerError::UnknownKind(5));
    // e fora do intervalo nem monta
    let bad = brdu_ledger::Tx::new_signed(&env.alice, CHAIN, 23, 0, 0, vec![]).unwrap();
    assert_eq!(
        env.exec.apply_tx(&mut s, &bad),
        Err(LedgerError::UnknownKind(23))
    );
}

#[test]
fn o_endereco_de_destino_nao_precisa_existir_e_nasce_no_primeiro_credito() {
    let env = Env::new();
    let mut s = funded(&env, 10_000_000);
    let stranger = Address(Hash32([0xab; 32]));
    okm!(env, s, &transfer(&env, &s, &env.alice, &stranger, 1));
    assert_eq!(s.balance(&stranger), 1);
}
