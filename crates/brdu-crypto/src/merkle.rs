//! Árvore de Merkle binária (estilo RFC 6962), com prefixo de domínio diferente para folhas e nós internos.
//! Isso fecha o ataque de segunda pré-imagem (uma folha nunca passa por nó) e a ambiguidade de duplicar a última
//! folha: a árvore de `n` folhas divide em `[0, k)` e `[k, n)`, com `k` a maior potência de 2 menor que `n`.

use crate::hash::{Hash32, tagged, tags};

/// Raiz de uma lista de folhas (já em hash). A lista vazia tem uma raiz própria e fixa.
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

/// A maior potência de 2 estritamente menor que `n` (para `n >= 2`).
fn split_point(n: usize) -> usize {
    let mut k = 1usize;
    while k.saturating_mul(2) < n {
        k = k.saturating_mul(2);
    }
    k
}
