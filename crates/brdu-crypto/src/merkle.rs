//! Binary Merkle tree (RFC 6962 style), with a different domain prefix for leaves and internal nodes.
//! This closes the second-preimage attack (a leaf never passes as a node) and the ambiguity of duplicating the last
//! leaf: a tree of `n` leaves splits into `[0, k)` and `[k, n)`, with `k` the largest power of 2 less than `n`.

use crate::hash::{Hash32, tagged, tags};

/// Root of a list of leaves (already hashed). The empty list has its own fixed root.
#[must_use]
pub fn root(leaves: &[Hash32]) -> Hash32 {
    match leaves {
        [] => tagged(tags::MERKLE_EMPTY, &[]),
        _ => subtree(leaves),
    }
}

fn subtree(leaves: &[Hash32]) -> Hash32 {
    match leaves {
        [] => tagged(tags::MERKLE_EMPTY, &[]),
        [one] => tagged(tags::MERKLE_LEAF, &[one.as_bytes()]),
        _ => {
            let (left, right) = leaves.split_at(split_point(leaves.len()));
            tagged(
                tags::MERKLE_NODE,
                &[subtree(left).as_bytes(), subtree(right).as_bytes()],
            )
        }
    }
}

/// The largest power of 2 strictly less than `n` (for `n >= 2`).
fn split_point(n: usize) -> usize {
    let mut k = 1usize;
    while k.saturating_mul(2) < n {
        k = k.saturating_mul(2);
    }
    k
}
