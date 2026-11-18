#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Hash32, merkle, sha3};
use proptest::prelude::*;

fn leaves(n: usize) -> Vec<Hash32> {
    (0..n).map(|i| sha3(&(i as u64).to_be_bytes())).collect()
}

#[test]
fn vazia_tem_raiz_propria_e_fixa() {
    assert_eq!(merkle::root(&[]), merkle::root(&[]));
    assert_ne!(merkle::root(&[]), merkle::root(&leaves(1)));
}

#[test]
fn a_folha_nunca_e_a_raiz_dela_mesma() {
    // domínios separados: um nó interno não pode passar por folha
    let one = leaves(1);
    assert_ne!(merkle::root(&one), one[0]);
}

#[test]
fn a_ordem_importa_e_cada_folha_muda_a_raiz() {
    let l = leaves(7);
    let base = merkle::root(&l);
    let mut swapped = l.clone();
    swapped.swap(0, 6);
    assert_ne!(merkle::root(&swapped), base);
    for i in 0..l.len() {
        let mut changed = l.clone();
        changed[i] = sha3(b"outra");
        assert_ne!(merkle::root(&changed), base, "folha {i}");
    }
}

#[test]
fn tamanhos_diferentes_dao_raizes_diferentes_sem_duplicar_a_ultima() {
    // a ambiguidade clássica: [a, b, c] e [a, b, c, c] NÃO podem ter a mesma raiz
    let l = leaves(3);
    let mut dup = l.clone();
    dup.push(l[2]);
    assert_ne!(merkle::root(&l), merkle::root(&dup));
    let roots: std::collections::HashSet<_> = (0..40).map(|n| merkle::root(&leaves(n))).collect();
    assert_eq!(roots.len(), 40, "tamanhos distintos colidiram");
}

proptest! {
    #[test]
    fn deterministica(n in 0usize..64) {
        prop_assert_eq!(merkle::root(&leaves(n)), merkle::root(&leaves(n)));
    }

    #[test]
    fn prefixo_diferente_dos_dados_muda_a_raiz(n in 1usize..40, k in 0usize..40) {
        let l = leaves(n);
        let cut = k % n;
        prop_assume!(cut != 0);
        prop_assert_ne!(merkle::root(&l), merkle::root(&l[..cut]));
    }
}
