//! M-of-N approvals over a document (issuers, root authorities, organs). Each approval is
//! `public key ‖ signature` (96 bytes); a key counts once, only if it belongs to the authorized set and its
//! signature verifies. Malformed, repeated or outside approvals do not count.

use brdu_crypto::{Hash32, PublicKey};

use crate::coin::{APPROVAL_LEN, Approval, MAX_APPROVALS};
use crate::error::LedgerError;

/// Distinct keys of `keys` whose signature over `doc` is valid.
#[must_use]
pub fn signers(doc: &Hash32, approvals: &[Approval], keys: &[PublicKey]) -> Vec<PublicKey> {
    let mut seen: Vec<PublicKey> = Vec::new();
    for a in approvals {
        if keys.contains(&a.issuer)
            && !seen.contains(&a.issuer)
            && a.issuer.verify(doc.as_bytes(), &a.signature).is_ok()
        {
            seen.push(a.issuer);
        }
    }
    seen
}

/// At least `threshold` valid approvals from `keys`?
#[must_use]
pub fn enough(doc: &Hash32, approvals: &[Approval], keys: &[PublicKey], threshold: usize) -> bool {
    threshold >= 1 && signers(doc, approvals, keys).len() >= threshold
}

/// Like [`enough`], as the ledger error with the counts (for modules).
pub fn require(
    doc: &Hash32,
    approvals: &[Approval],
    keys: &[PublicKey],
    threshold: usize,
) -> Result<(), LedgerError> {
    let valid = signers(doc, approvals, keys).len();
    if threshold >= 1 && valid >= threshold {
        Ok(())
    } else {
        Err(LedgerError::NotEnoughApprovals {
            valid,
            needed: threshold,
        })
    }
}

/// Reads a list of approvals from a payload decoder (count + one 96-byte blob each).
pub fn read_list(d: &mut brdu_crypto::encoding::Decoder<'_>) -> Result<Vec<Approval>, LedgerError> {
    let n = d.count(MAX_APPROVALS).map_err(|_| LedgerError::Malformed)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let raw = d.bytes(APPROVAL_LEN).map_err(|_| LedgerError::Malformed)?;
        out.push(Approval::from_bytes(raw).ok_or(LedgerError::Malformed)?);
    }
    Ok(out)
}

/// Writes a list of approvals (count + one 96-byte blob each).
pub fn write_list(e: &mut brdu_crypto::encoding::Encoder, approvals: &[Approval]) {
    e.count(approvals.len());
    for a in approvals {
        e.bytes(&a.to_bytes());
    }
}
