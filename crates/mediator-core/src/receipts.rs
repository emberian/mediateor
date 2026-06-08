//! Append-only, hash-chained receipt ledger. Each analysis step emits one
//! `Receipt`. The chain lets a skeptic replay every kernel action and confirm
//! nothing was inserted, reordered, or quietly altered after the fact.
//!
//! `hash = sha256( prev_hash || op || serde_json(detail) )`, hex-encoded.
//! Genesis `prev_hash` is 64 zeros.

use mediator_types::{Receipt, Verdict};
use sha2::{Digest, Sha256};

pub const GENESIS_PREV: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A growing receipt chain.
pub struct ReceiptChain {
    receipts: Vec<Receipt>,
}

impl ReceiptChain {
    pub fn new() -> Self {
        Self { receipts: Vec::new() }
    }

    fn prev_hash(&self) -> String {
        self.receipts
            .last()
            .map(|r| r.hash.clone())
            .unwrap_or_else(|| GENESIS_PREV.to_string())
    }

    /// Append a receipt for `op` with structured `detail` and an optional
    /// verdict, computing the chained hash.
    pub fn append(
        &mut self,
        op: &str,
        detail: serde_json::Value,
        verdict: Option<Verdict>,
    ) -> &Receipt {
        let seq = self.receipts.len() as u64;
        let prev_hash = self.prev_hash();
        let hash = chain_hash(&prev_hash, op, &detail);
        self.receipts.push(Receipt {
            seq,
            prev_hash,
            hash,
            op: op.to_string(),
            detail,
            verdict,
        });
        self.receipts.last().unwrap()
    }

    pub fn into_vec(self) -> Vec<Receipt> {
        self.receipts
    }
}

impl Default for ReceiptChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute `sha256(prev_hash || op || serde_json(detail))` as lowercase hex.
/// `detail` is serialized canonically via `serde_json::to_vec` (stable for a
/// given `Value` shape).
pub fn chain_hash(prev_hash: &str, op: &str, detail: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(op.as_bytes());
    let bytes = serde_json::to_vec(detail).unwrap_or_default();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    hex(&digest)
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Verify a chain is well-formed: seqs are 0..n, prev links match, and every
/// hash recomputes. Returns the index of the first bad receipt, if any.
pub fn verify_chain(receipts: &[Receipt]) -> Result<(), usize> {
    let mut prev = GENESIS_PREV.to_string();
    for (i, r) in receipts.iter().enumerate() {
        if r.seq != i as u64 || r.prev_hash != prev {
            return Err(i);
        }
        if chain_hash(&r.prev_hash, &r.op, &r.detail) != r.hash {
            return Err(i);
        }
        prev = r.hash.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn genesis_links_to_zeros() {
        let mut chain = ReceiptChain::new();
        let r = chain.append("build_preamble", json!({"n": 1}), None);
        assert_eq!(r.seq, 0);
        assert_eq!(r.prev_hash, GENESIS_PREV);
        assert_eq!(r.hash.len(), 64);
    }

    #[test]
    fn chain_links_and_verifies() {
        let mut chain = ReceiptChain::new();
        chain.append("a", json!({"x": 1}), None);
        chain.append("b", json!({"y": 2}), Some(Verdict::Proved));
        chain.append("c", json!("z"), Some(Verdict::Unknown));
        let v = chain.into_vec();
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].prev_hash, v[0].hash);
        assert_eq!(v[2].prev_hash, v[1].hash);
        assert!(verify_chain(&v).is_ok());
    }

    #[test]
    fn tampering_breaks_the_chain() {
        let mut chain = ReceiptChain::new();
        chain.append("a", json!({"x": 1}), None);
        chain.append("b", json!({"y": 2}), None);
        let mut v = chain.into_vec();
        // mutate a detail without recomputing the hash
        v[0].detail = json!({"x": 999});
        assert_eq!(verify_chain(&v), Err(0));
    }

    #[test]
    fn hash_is_deterministic() {
        let h1 = chain_hash(GENESIS_PREV, "op", &json!({"a": 1, "b": 2}));
        let h2 = chain_hash(GENESIS_PREV, "op", &json!({"a": 1, "b": 2}));
        assert_eq!(h1, h2);
    }
}
