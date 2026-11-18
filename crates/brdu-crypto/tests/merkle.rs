#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use brdu_crypto::{Hash32, merkle, sha3};
use proptest::prelude::*;

fn leaves(n: usize) -> Vec<Hash32> {
    (0..n).map(|i| sha3(&(i as u64).to_be_bytes())).collect()
}

#[test]
fn empty_has_its_own_fixed_root() {
    assert_eq!(merkle::root(&[]), merkle::root(&[]));
    assert_ne!(merkle::root(&[]), merkle::root(&leaves(1)));
}

#[test]
fn a_leaf_is_never_its_own_root() {
    // separated domains: an internal node cannot pass as a leaf
    let one = leaves(1);
    assert_ne!(merkle::root(&one), one[0]);
}

#[test]
fn order_matters_and_each_leaf_changes_the_root() {
    let l = leaves(7);
    let base = merkle::root(&l);
    let mut swapped = l.clone();
    swapped.swap(0, 6);
    assert_ne!(merkle::root(&swapped), base);
    for i in 0..l.len() {
        let mut changed = l.clone();
        changed[i] = sha3(b"other");
        assert_ne!(merkle::root(&changed), base, "leaf {i}");
    }
}

#[test]
fn different_sizes_give_different_roots_without_duplicating_the_last() {
    // the classic ambiguity: [a, b, c] and [a, b, c, c] must NOT have the same root
    let l = leaves(3);
    let mut dup = l.clone();
    dup.push(l[2]);
    assert_ne!(merkle::root(&l), merkle::root(&dup));
    let roots: std::collections::HashSet<_> = (0..40).map(|n| merkle::root(&leaves(n))).collect();
    assert_eq!(roots.len(), 40, "distinct sizes collided");
}

proptest! {
    #[test]
    fn deterministic(n in 0usize..64) {
        prop_assert_eq!(merkle::root(&leaves(n)), merkle::root(&leaves(n)));
    }

    #[test]
    fn different_prefix_of_the_data_changes_the_root(n in 1usize..40, k in 0usize..40) {
        let l = leaves(n);
        let cut = k % n;
        prop_assume!(cut != 0);
        prop_assert_ne!(merkle::root(&l), merkle::root(&l[..cut]));
    }
}
