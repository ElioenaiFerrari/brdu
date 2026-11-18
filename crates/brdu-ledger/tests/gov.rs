#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Rotação de validadores (ledger): troca por mais de 2/3, uma pendente, limites, efetivação no bloco certo.

use brdu_crypto::{Address, PublicKey, SecretKey};
use brdu_ledger::coin::Approval;
use brdu_ledger::gov::{self, Action, GovModule};
use brdu_ledger::{Executor, LedgerError, Rules, State, Tx, kind};
use proptest::prelude::*;

const CHAIN: &str = "brdu-gov";

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes([n; 32])
}

struct Net {
    exec: Executor,
    vals: Vec<SecretKey>,
    relayer: SecretKey,
}

impl Net {
    fn new(n: usize, min: usize, max: usize) -> (Self, State) {
        let vals: Vec<SecretKey> = (20..20 + n as u8).map(key).collect();
        let issuers = vec![key(10).public()];
        let rules = Rules::new(CHAIN, &issuers, 1).unwrap();
        let exec = Executor::new(rules).with_module(Box::new(GovModule));
        let mut state = State::new();
        gov::init(
            &mut state,
            &vals.iter().map(SecretKey::public).collect::<Vec<_>>(),
            min,
            max,
        );
        (
            Self {
                exec,
                vals,
                relayer: key(9),
            },
            state,
        )
    }

    fn signers(&self, s: &State) -> Vec<Address> {
        let mut v: Vec<Address> = gov::validators(s)
            .iter()
            .map(Address::from_public_key)
            .collect();
        v.sort();
        v
    }

    /// Transação de troca com `n_approvals` aprovações dos primeiros validadores (por chave, do conjunto atual).
    fn change(
        &self,
        s: &State,
        action: Action,
        who: &PublicKey,
        effective: u64,
        approvers: &[&SecretKey],
    ) -> Tx {
        let doc = gov::change_doc(CHAIN, action, who, effective, gov::epoch(s));
        let ap: Vec<Approval> = approvers.iter().map(|k| Approval::new(&doc, k)).collect();
        let nonce = s.nonce(&Address::from_public_key(&self.relayer.public()));
        Tx::new_signed(
            &self.relayer,
            CHAIN,
            kind::VALIDATOR_CHANGE,
            nonce,
            0,
            gov::change_payload(action, who, effective, &ap),
        )
        .unwrap()
    }

    fn first(&self, n: usize) -> Vec<&SecretKey> {
        self.vals.iter().take(n).collect()
    }

    /// Aplica um bloco com `txs` (o proponente é o primeiro do conjunto).
    fn block(&self, s: &mut State, txs: Vec<Tx>) -> Result<(), LedgerError> {
        let signers = self.signers(s);
        let b = self
            .exec
            .build_block(s, signers[0], signers, 1_000 + s.height(), txs)?;
        self.exec.apply_block(s, &b).map(|_| ())
    }
}

fn newcomer() -> PublicKey {
    key(50).public()
}

#[test]
fn troca_com_mais_de_2_3_fica_pendente_e_so_vale_no_bloco_certo() {
    let (net, mut s) = Net::new(4, 3, 7);
    let tx = net.change(&s, Action::Add, &newcomer(), 4, &net.first(3));
    net.block(&mut s, vec![tx]).unwrap(); // bloco 1
    let p = gov::pending(&s).unwrap();
    assert_eq!(
        (p.action, p.key, p.effective_height),
        (Action::Add, newcomer(), 4)
    );
    assert_eq!((gov::validators(&s).len(), gov::epoch(&s)), (4, 0));
    net.block(&mut s, vec![]).unwrap(); // bloco 2
    net.block(&mut s, vec![]).unwrap(); // bloco 3: fim do bloco efetiva-1
    let set = gov::validators(&s);
    assert_eq!((set.len(), gov::epoch(&s)), (5, 1));
    assert_eq!(set.last(), Some(&newcomer()), "entra no fim do rodízio");
    assert!(gov::pending(&s).is_none());
}

#[test]
fn remover_preserva_a_ordem_dos_demais() {
    let (net, mut s) = Net::new(5, 3, 7);
    let out = net.vals[1].public();
    let tx = net.change(&s, Action::Remove, &out, 3, &net.first(4));
    net.block(&mut s, vec![tx]).unwrap();
    net.block(&mut s, vec![]).unwrap();
    let want: Vec<PublicKey> = net
        .vals
        .iter()
        .map(SecretKey::public)
        .filter(|k| *k != out)
        .collect();
    assert_eq!(gov::validators(&s), want);
}

#[test]
fn aprovacoes_insuficientes_repetidas_estranhas_ou_de_outro_documento_sao_recusadas() {
    let (net, mut s) = Net::new(4, 3, 7);
    let before = s.clone();
    // 2 de 4 não passa de 2/3
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(2));
    assert!(
        net.block(&mut s, vec![t])
            .unwrap_err()
            .to_string()
            .contains("insuficientes")
    );
    // o mesmo validador 3 vezes conta uma
    let v0 = &net.vals[0];
    let t = net.change(&s, Action::Add, &newcomer(), 4, &[v0, v0, v0]);
    assert!(net.block(&mut s, vec![t]).is_err());
    // quem não é validador é ignorado
    let strangers = [key(70), key(71), key(72), key(73)];
    let t = net.change(
        &s,
        Action::Add,
        &newcomer(),
        4,
        &strangers.iter().collect::<Vec<_>>(),
    );
    assert!(net.block(&mut s, vec![t]).is_err());
    // aprovações para OUTRA chave, outra altura, outra ação ou outra rede não valem para esta carga
    let ap_for = |action, who: &PublicKey, eff, chain: &str, epoch| -> Vec<Approval> {
        let doc = brdu_ledger::gov::change_doc(chain, action, who, eff, epoch);
        net.first(3)
            .iter()
            .map(|k| Approval::new(&doc, k))
            .collect()
    };
    for ap in [
        ap_for(Action::Add, &key(51).public(), 4, CHAIN, 0),
        ap_for(Action::Add, &newcomer(), 5, CHAIN, 0),
        ap_for(Action::Remove, &newcomer(), 4, CHAIN, 0),
        ap_for(Action::Add, &newcomer(), 4, "outra-rede", 0),
        ap_for(Action::Add, &newcomer(), 4, CHAIN, 1),
    ] {
        let nonce = s.nonce(&Address::from_public_key(&net.relayer.public()));
        let t = Tx::new_signed(
            &net.relayer,
            CHAIN,
            kind::VALIDATOR_CHANGE,
            nonce,
            0,
            gov::change_payload(Action::Add, &newcomer(), 4, &ap),
        )
        .unwrap();
        assert!(net.block(&mut s, vec![t]).is_err());
    }
    assert_eq!(s, before, "nada disso mexeu no estado");
}

#[test]
fn altura_efetiva_passada_so_uma_pendente_duplicata_e_limites_de_tamanho() {
    let (net, mut s) = Net::new(4, 4, 5);
    // efetiva tem de ser pelo menos altura + 2 (aqui o bloco em construção é o 1: efetiva >= 3)
    for eff in [0, 1, 2] {
        let t = net.change(&s, Action::Add, &newcomer(), eff, &net.first(3));
        assert!(net.block(&mut s, vec![t]).is_err(), "efetiva {eff}");
    }
    // já é validador / não é validador
    let t = net.change(&s, Action::Add, &net.vals[0].public(), 5, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err());
    let t = net.change(&s, Action::Remove, &newcomer(), 5, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err());
    // mínimo 4: remover um de 4 sairia do limite
    let t = net.change(&s, Action::Remove, &net.vals[0].public(), 5, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err());
    // uma troca pendente de cada vez
    let t = net.change(&s, Action::Add, &newcomer(), 6, &net.first(3));
    net.block(&mut s, vec![t]).unwrap();
    let t = net.change(&s, Action::Add, &key(51).public(), 8, &net.first(3));
    assert!(net.block(&mut s, vec![t]).is_err(), "segunda pendente");
    // efetivada a primeira (5 validadores = máximo), uma nova entrada estoura o máximo
    while gov::epoch(&s) == 0 {
        net.block(&mut s, vec![]).unwrap();
    }
    assert_eq!(gov::validators(&s).len(), 5);
    let t = net.change(
        &s,
        Action::Add,
        &key(51).public(),
        s.height() + 3,
        &net.first(4),
    );
    assert!(net.block(&mut s, vec![t]).is_err(), "máximo 5");
}

#[test]
fn aprovacao_da_epoca_velha_nao_vale_depois_da_troca() {
    let (net, mut s) = Net::new(4, 3, 7);
    let old_epoch_doc = gov::change_doc(CHAIN, Action::Remove, &net.vals[3].public(), 9, 0);
    let t = net.change(&s, Action::Add, &newcomer(), 3, &net.first(3));
    net.block(&mut s, vec![t]).unwrap();
    net.block(&mut s, vec![]).unwrap();
    assert_eq!(gov::epoch(&s), 1);
    // aprovações assinadas na época 0 para remover o 4º: com a época 1 no estado, não valem
    let ap: Vec<Approval> = net
        .first(3)
        .iter()
        .map(|k| Approval::new(&old_epoch_doc, k))
        .collect();
    let nonce = s.nonce(&Address::from_public_key(&net.relayer.public()));
    let t = Tx::new_signed(
        &net.relayer,
        CHAIN,
        kind::VALIDATOR_CHANGE,
        nonce,
        0,
        gov::change_payload(Action::Remove, &net.vals[3].public(), 9, &ap),
    )
    .unwrap();
    assert!(net.block(&mut s, vec![t]).is_err());
}

#[test]
fn com_5_validadores_precisa_de_4_aprovacoes() {
    let (net, mut s) = Net::new(5, 3, 7);
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(3));
    assert!(
        net.block(&mut s, vec![t]).is_err(),
        "3 de 5 não passa de 2/3"
    );
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(4));
    net.block(&mut s, vec![t]).unwrap();
}

#[test]
fn replay_do_historico_chega_ao_mesmo_conjunto_e_a_mesma_raiz() {
    let (net, mut s) = Net::new(4, 3, 7);
    let mut blocks = Vec::new();
    let t = net.change(&s, Action::Add, &newcomer(), 4, &net.first(3));
    for txs in [vec![t], vec![], vec![]] {
        let signers = net.signers(&s);
        let time = 1_000 + s.height();
        let b = net
            .exec
            .build_block(&mut s, signers[0], signers, time, txs)
            .unwrap();
        net.exec.apply_block(&mut s, &b).unwrap();
        blocks.push(b);
    }
    let (net2, mut replay) = Net::new(4, 3, 7);
    for b in &blocks {
        net2.exec.apply_block(&mut replay, b).unwrap();
    }
    assert_eq!(replay.root(), s.root());
    assert_eq!(gov::validators(&replay), gov::validators(&s));
}

#[test]
fn payload_malformado_e_recusado() {
    let (net, mut s) = Net::new(4, 3, 7);
    let nonce = s.nonce(&Address::from_public_key(&net.relayer.public()));
    for bad in [vec![], vec![9; 20], vec![1; 41 + 200]] {
        let t = Tx::new_signed(&net.relayer, CHAIN, kind::VALIDATOR_CHANGE, nonce, 0, bad).unwrap();
        assert!(net.block(&mut s, vec![t]).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(32), ..ProptestConfig::default() })]

    /// Qualquer sequência de trocas (válidas e inválidas): o conjunto fica em [min, max], sem repetição, e a
    /// época só sobe de 1 em 1 a cada troca efetivada.
    #[test]
    fn o_conjunto_nunca_sai_dos_limites(ops in proptest::collection::vec((any::<bool>(), 0u8..8, 2u64..5, 2usize..6), 1..30)) {
        let (net, mut s) = Net::new(4, 3, 6);
        let pool: Vec<SecretKey> = (40..48).map(key).collect();
        let mut last_epoch = 0;
        for (add, who, lead, n_ap) in ops {
            let target = if add { pool[who as usize].public() } else { net.vals[(who % 4) as usize].public() };
            let action = if add { Action::Add } else { Action::Remove };
            let current = gov::validators(&s);
            let approvers: Vec<&SecretKey> = net.vals.iter().chain(pool.iter()).filter(|k| current.contains(&k.public())).take(n_ap).collect();
            let t = net.change(&s, action, &target, s.height() + 1 + lead, &approvers);
            let _ = net.block(&mut s, vec![t]);
            net.block(&mut s, vec![]).unwrap();
            let set = gov::validators(&s);
            let (min, max) = gov::limits(&s);
            prop_assert!(set.len() >= min && set.len() <= max);
            let mut sorted = set.clone();
            sorted.sort();
            sorted.dedup();
            prop_assert_eq!(sorted.len(), set.len());
            let e = gov::epoch(&s);
            prop_assert!(e >= last_epoch && e <= last_epoch + 1);
            last_epoch = e;
        }
    }
}
