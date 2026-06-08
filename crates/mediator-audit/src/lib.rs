//! `mediator-audit` — a signed, hash-chained, **verifiable** audit record of a
//! mediation. Every certified fact (from prover receipts) and every binding
//! mediation step is recorded in a tamper-evident hash chain, then signed with
//! an ed25519 key so any verifier can confirm the record wasn't altered after
//! the fact.
//!
//! ## Chain construction
//!
//! Each [`Entry`] covers:
//!
//! ```text
//! hash = hex(sha256(prev_hash || kind || canonical_json(detail) || verdict_str))
//! ```
//!
//! The genesis entry's `prev_hash` is 64 hex zeros.
//! The [`MediationRecord`]'s `signature` is the ed25519 signature of the
//! **final entry's hash bytes** (hex-decoded), signed with the holder's
//! signing key; the matching verifying key is embedded as `public_key` (hex).
//!
//! ## Usage
//!
//! ```rust
//! use mediator_audit::{build, verify, generate_keypair, summary};
//! use ed25519_dalek::SigningKey;
//!
//! let signing_key = generate_keypair();
//! let rec = build(&[], &[], &signing_key);
//! assert!(verify(&rec).is_ok());
//! println!("{}", summary(&rec));
//! ```

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use mediator_types::{Receipt, Verdict};

// ───────────────────────────── public types ─────────────────────────────────

/// One link in the tamper-evident audit chain.
///
/// `hash = hex(sha256(prev_hash || kind || canonical_json(detail) || verdict))`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// 0-based sequence number in the chain.
    pub seq: u64,
    /// Hex-encoded sha256 hash of the previous entry (64 zeros for genesis).
    pub prev_hash: String,
    /// Hex-encoded sha256 hash of this entry's content.
    pub hash: String,
    /// A short operation/event kind label, e.g. `"verify_ledger"` or
    /// `"mediation_event"`.
    pub kind: String,
    /// Arbitrary structured detail — whatever the source produced.
    pub detail: serde_json::Value,
    /// Optional prover verdict for receipt-sourced entries.
    pub verdict: Option<String>,
}

/// The complete, signed audit record of one mediation session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediationRecord {
    /// The full ordered chain.
    pub entries: Vec<Entry>,
    /// Hex-encoded ed25519 verifying (public) key.
    pub public_key: String,
    /// Hex-encoded ed25519 signature over the **final entry's hash bytes**.
    ///
    /// If the chain is empty the signature is over the 64-byte genesis
    /// prev_hash sentinel (all-zeros ASCII string, hex-decoded).
    pub signature: String,
}

// ───────────────────────────── error type ───────────────────────────────────

/// A verification failure with precise provenance.
#[derive(Debug, PartialEq)]
pub enum AuditError {
    /// Entry at `seq` has an incorrect hash.
    /// `expected` is what we recomputed; `stored` is what was in the record.
    HashMismatch {
        seq: u64,
        expected: String,
        stored: String,
    },
    /// Entry at `seq` has a `prev_hash` that does not match the hash of the
    /// previous entry (or the genesis sentinel).
    ChainBreak {
        seq: u64,
        expected_prev: String,
        stored_prev: String,
    },
    /// The ed25519 signature over the root hash is invalid.
    SignatureInvalid(String),
    /// The embedded public key bytes are not a valid ed25519 verifying key.
    PublicKeyInvalid(String),
    /// Hex decode failed (corrupted record).
    HexDecode(String),
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditError::HashMismatch { seq, expected, stored } => write!(
                f,
                "entry {seq}: hash mismatch (expected {expected}, stored {stored})"
            ),
            AuditError::ChainBreak { seq, expected_prev, stored_prev } => write!(
                f,
                "entry {seq}: chain break (expected prev_hash {expected_prev}, got {stored_prev})"
            ),
            AuditError::SignatureInvalid(msg) => write!(f, "signature invalid: {msg}"),
            AuditError::PublicKeyInvalid(msg) => write!(f, "public key invalid: {msg}"),
            AuditError::HexDecode(msg) => write!(f, "hex decode error: {msg}"),
        }
    }
}

impl std::error::Error for AuditError {}

// ─────────────────────────── hashing helpers ────────────────────────────────

/// The genesis `prev_hash` sentinel: 64 hex zeros.
const GENESIS_PREV_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Serialise `detail` to canonical (sorted-keys) JSON.
///
/// `serde_json::to_string` already emits keys in insertion order for
/// `serde_json::Value::Object`, which is backed by `IndexMap`. To be safe we
/// sort keys explicitly so the output is deterministic regardless of origin.
fn canonical_json(value: &serde_json::Value) -> String {
    // Recurse through the value, sorting object keys at each level.
    fn sorted(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
                keys.sort_unstable();
                let ordered: serde_json::Map<String, serde_json::Value> = keys
                    .into_iter()
                    .map(|k| (k.to_owned(), sorted(&map[k])))
                    .collect();
                serde_json::Value::Object(ordered)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(sorted).collect())
            }
            other => other.clone(),
        }
    }
    // This cannot fail for a well-formed Value.
    serde_json::to_string(&sorted(value)).unwrap_or_default()
}

/// Compute the hash of one entry from its constituent parts.
///
/// ```text
/// sha256(prev_hash_str || kind || canonical_json(detail) || verdict_str)
/// ```
///
/// All fields are concatenated as raw UTF-8 bytes with no separator; the
/// deterministic lengths of the fixed-width `prev_hash` (always 64 hex chars)
/// and `kind` (any UTF-8 label) plus the self-delimiting JSON make this
/// unambiguous.
fn compute_hash(
    prev_hash: &str,
    kind: &str,
    detail: &serde_json::Value,
    verdict: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(kind.as_bytes());
    hasher.update(canonical_json(detail).as_bytes());
    hasher.update(verdict.unwrap_or("").as_bytes());
    hex::encode(hasher.finalize())
}

/// Convert a [`Verdict`] to its stable string representation for hashing.
fn verdict_to_str(v: &Verdict) -> String {
    match v {
        Verdict::Proved => "Proved".to_owned(),
        Verdict::Refuted => "Refuted".to_owned(),
        Verdict::Unknown => "Unknown".to_owned(),
        Verdict::Error(e) => format!("Error:{e}"),
    }
}

// ───────────────────────────── public API ───────────────────────────────────

/// Generate a fresh ed25519 signing keypair using the OS RNG.
pub fn generate_keypair() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

/// Build a [`MediationRecord`] from certified receipts and mediation events.
///
/// The receipts and events are folded in order — receipts first, then events —
/// into a single hash chain, and the final entry's hash is signed.
///
/// * `receipts` — certified prover receipts (from `mediator-core`).  Each
///   receipt's `op`, `detail`, and `verdict` become one chain entry; the
///   receipt's own `hash` field is stored in `detail` for auditing but the
///   chain hash is recomputed here.
/// * `events` — raw mediation events `(kind, detail)` appended after the
///   receipts.
/// * `signing_key` — the ed25519 key that will sign the record.
pub fn build(
    receipts: &[Receipt],
    events: &[(String, serde_json::Value)],
    signing_key: &SigningKey,
) -> MediationRecord {
    let mut entries: Vec<Entry> = Vec::new();
    let mut prev_hash = GENESIS_PREV_HASH.to_owned();
    let mut seq: u64 = 0;

    // --- fold receipts -------------------------------------------------------
    for receipt in receipts {
        let verdict_str = receipt.verdict.as_ref().map(verdict_to_str);
        // Embed the receipt's own hash and op in detail so an auditor can
        // cross-reference with the original receipt ledger.
        let detail = serde_json::json!({
            "receipt_seq": receipt.seq,
            "receipt_prev_hash": receipt.prev_hash,
            "receipt_hash": receipt.hash,
            "receipt_detail": receipt.detail,
        });
        let hash =
            compute_hash(&prev_hash, &receipt.op, &detail, verdict_str.as_deref());
        entries.push(Entry {
            seq,
            prev_hash: prev_hash.clone(),
            hash: hash.clone(),
            kind: receipt.op.clone(),
            detail,
            verdict: verdict_str,
        });
        prev_hash = hash;
        seq += 1;
    }

    // --- fold events ---------------------------------------------------------
    for (kind, detail) in events {
        let hash = compute_hash(&prev_hash, kind, detail, None);
        entries.push(Entry {
            seq,
            prev_hash: prev_hash.clone(),
            hash: hash.clone(),
            kind: kind.clone(),
            detail: detail.clone(),
            verdict: None,
        });
        prev_hash = hash;
        seq += 1;
    }

    // --- sign ----------------------------------------------------------------
    // Sign the root hash bytes (hex-decoded).  For an empty chain, sign the
    // genesis sentinel so the signature is still meaningful.
    let root_hash_bytes: Vec<u8> = hex::decode(&prev_hash)
        .expect("prev_hash is always a valid hex string produced internally");
    let signature: Signature = signing_key.sign(&root_hash_bytes);

    let verifying_key = signing_key.verifying_key();

    MediationRecord {
        entries,
        public_key: hex::encode(verifying_key.to_bytes()),
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Verify the integrity of a [`MediationRecord`].
///
/// * Recomputes every entry's hash from its fields.
/// * Checks that each `prev_hash` links to the previous entry's hash.
/// * Verifies the ed25519 signature over the final (root) hash.
///
/// Returns `Ok(())` on full verification, or a [`AuditError`] that names
/// exactly which entry failed or why.
pub fn verify(rec: &MediationRecord) -> Result<(), AuditError> {
    // --- decode public key ---------------------------------------------------
    let pk_bytes = hex::decode(&rec.public_key)
        .map_err(|e| AuditError::HexDecode(format!("public_key: {e}")))?;
    let pk_arr: [u8; 32] = pk_bytes.as_slice().try_into().map_err(|_| {
        AuditError::PublicKeyInvalid(format!(
            "expected 32 bytes, got {}",
            pk_bytes.len()
        ))
    })?;
    let verifying_key = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|e| AuditError::PublicKeyInvalid(e.to_string()))?;

    // --- decode signature ----------------------------------------------------
    let sig_bytes = hex::decode(&rec.signature)
        .map_err(|e| AuditError::HexDecode(format!("signature: {e}")))?;
    let sig_arr: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| {
        AuditError::SignatureInvalid(format!(
            "expected 64 bytes, got {}",
            sig_bytes.len()
        ))
    })?;
    let signature = Signature::from_bytes(&sig_arr);

    // --- walk the chain ------------------------------------------------------
    let mut expected_prev = GENESIS_PREV_HASH.to_owned();

    for entry in &rec.entries {
        // Check prev_hash linkage.
        if entry.prev_hash != expected_prev {
            return Err(AuditError::ChainBreak {
                seq: entry.seq,
                expected_prev,
                stored_prev: entry.prev_hash.clone(),
            });
        }

        // Recompute hash.
        let recomputed = compute_hash(
            &entry.prev_hash,
            &entry.kind,
            &entry.detail,
            entry.verdict.as_deref(),
        );
        if recomputed != entry.hash {
            return Err(AuditError::HashMismatch {
                seq: entry.seq,
                expected: recomputed,
                stored: entry.hash.clone(),
            });
        }

        expected_prev = entry.hash.clone();
    }

    // --- verify signature over root hash -------------------------------------
    let root_hash_bytes = hex::decode(&expected_prev)
        .map_err(|e| AuditError::HexDecode(format!("root hash: {e}")))?;
    verifying_key
        .verify(&root_hash_bytes, &signature)
        .map_err(|e| AuditError::SignatureInvalid(e.to_string()))?;

    Ok(())
}

/// Produce a human-readable summary of the record.
///
/// Example:
/// ```text
/// Mediation audit record
///   Entries : 4
///   Root hash: 3a7f…
///   Public key: 9b2e…
///   Signature : 5c1d…
///
///   #0  verify_ledger        → Proved
///   #1  isolate_crux         → Unknown
///   #2  mediation_event      (no verdict)
///   #3  mediation_event      (no verdict)
/// ```
pub fn summary(rec: &MediationRecord) -> String {
    let root = rec
        .entries
        .last()
        .map(|e| e.hash.as_str())
        .unwrap_or(GENESIS_PREV_HASH);

    let pk_short = if rec.public_key.len() > 12 {
        format!("{}…", &rec.public_key[..12])
    } else {
        rec.public_key.clone()
    };
    let sig_short = if rec.signature.len() > 12 {
        format!("{}…", &rec.signature[..12])
    } else {
        rec.signature.clone()
    };
    let root_short = if root.len() > 12 {
        format!("{}…", &root[..12])
    } else {
        root.to_owned()
    };

    let mut out = format!(
        "Mediation audit record\n  Entries   : {}\n  Root hash : {}\n  Public key: {}\n  Signature : {}\n",
        rec.entries.len(),
        root_short,
        pk_short,
        sig_short,
    );

    for entry in &rec.entries {
        let verdict_str = entry
            .verdict
            .as_deref()
            .unwrap_or("(no verdict)");
        out.push_str(&format!(
            "\n  #{:<4} {:<24} → {}",
            entry.seq, entry.kind, verdict_str
        ));
    }

    out
}

// ─────────────────────────────── tests ──────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::{Receipt, Verdict};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn make_receipt(seq: u64, prev_hash: &str, op: &str, verdict: Option<Verdict>) -> Receipt {
        // Build a minimal receipt; the hash field is illustrative only.
        let detail = serde_json::json!({ "note": op });
        let hash = compute_hash(prev_hash, op, &detail, verdict.as_ref().map(verdict_to_str).as_deref());
        Receipt {
            seq,
            prev_hash: prev_hash.to_owned(),
            hash,
            op: op.to_owned(),
            detail,
            verdict,
        }
    }

    fn two_receipts() -> Vec<Receipt> {
        let r0 = make_receipt(0, GENESIS_PREV_HASH, "verify_ledger", Some(Verdict::Proved));
        let hash0 = r0.hash.clone();
        let r1 = make_receipt(1, &hash0, "isolate_crux", Some(Verdict::Unknown));
        vec![r0, r1]
    }

    fn two_events() -> Vec<(String, serde_json::Value)> {
        vec![
            (
                "mediation_open".to_owned(),
                serde_json::json!({ "parties": ["alex", "sam"] }),
            ),
            (
                "mediation_close".to_owned(),
                serde_json::json!({ "outcome": "settled" }),
            ),
        ]
    }

    // ── round-trip ───────────────────────────────────────────────────────────

    /// A built record verifies correctly.
    #[test]
    fn round_trip_verifies() {
        let key = generate_keypair();
        let rec = build(&two_receipts(), &two_events(), &key);
        assert!(verify(&rec).is_ok(), "expected Ok, got {:?}", verify(&rec));
    }

    /// JSON round-trip: serialise → deserialise → verify still passes.
    #[test]
    fn json_round_trip() {
        let key = generate_keypair();
        let rec = build(&two_receipts(), &two_events(), &key);
        let json = serde_json::to_string(&rec).expect("serialise");
        let rec2: MediationRecord = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(rec, rec2);
        assert!(verify(&rec2).is_ok());
    }

    // ── receipts-only record ─────────────────────────────────────────────────

    /// A record built from receipts only (no events) still verifies.
    #[test]
    fn receipts_only_verifies() {
        let key = generate_keypair();
        let rec = build(&two_receipts(), &[], &key);
        assert_eq!(rec.entries.len(), 2);
        assert!(verify(&rec).is_ok());
    }

    // ── empty record ─────────────────────────────────────────────────────────

    /// An empty record (no receipts, no events) is valid — the signature is
    /// over the genesis sentinel.
    #[test]
    fn empty_record_verifies() {
        let key = generate_keypair();
        let rec = build(&[], &[], &key);
        assert!(rec.entries.is_empty());
        assert!(verify(&rec).is_ok());
    }

    // ── tamper detection ─────────────────────────────────────────────────────

    /// Tampering with entry 0's detail causes verify() to fail AT entry 0.
    #[test]
    fn tamper_entry_0_detail_detected() {
        let key = generate_keypair();
        let mut rec = build(&two_receipts(), &two_events(), &key);

        // Mutate entry 0's detail without updating its hash.
        rec.entries[0].detail["receipt_detail"]["note"] =
            serde_json::Value::String("TAMPERED".to_owned());

        match verify(&rec) {
            Err(AuditError::HashMismatch { seq, .. }) => {
                assert_eq!(seq, 0, "mismatch should be at entry 0");
            }
            other => panic!("expected HashMismatch at 0, got {other:?}"),
        }
    }

    /// Tampering with entry 2's detail causes verify() to fail AT entry 2
    /// (not at 0 or 1).
    #[test]
    fn tamper_middle_entry_detected_at_correct_seq() {
        let key = generate_keypair();
        let mut rec = build(&two_receipts(), &two_events(), &key);

        // Entry 2 is the first event ("mediation_open").
        rec.entries[2].detail["parties"] =
            serde_json::Value::String("TAMPERED".to_owned());

        match verify(&rec) {
            Err(AuditError::HashMismatch { seq, .. }) => {
                assert_eq!(seq, 2, "mismatch should be at entry 2");
            }
            other => panic!("expected HashMismatch at 2, got {other:?}"),
        }
    }

    /// Tampering with entry 1's stored hash causes a ChainBreak at entry 2
    /// (its successor sees a wrong prev_hash).
    #[test]
    fn tamper_stored_hash_causes_chain_break() {
        let key = generate_keypair();
        let mut rec = build(&two_receipts(), &two_events(), &key);

        // Corrupt entry 1's stored hash directly.
        rec.entries[1].hash = "a".repeat(64);

        // Entry 2's prev_hash still points at the *original* hash of entry 1,
        // but now entry 1's stored hash differs — so entry 2's prev_hash check
        // fails.
        match verify(&rec) {
            Err(AuditError::ChainBreak { seq, .. }) => {
                assert_eq!(seq, 2, "chain break should surface at entry 2");
            }
            // Alternatively a HashMismatch at entry 1 is also correct if the
            // verifier catches entry 1's recomputed hash ≠ stored hash first.
            Err(AuditError::HashMismatch { seq, .. }) => {
                assert_eq!(seq, 1);
            }
            other => panic!("expected chain error, got {other:?}"),
        }
    }

    /// A corrupted signature is detected even if the chain is intact.
    #[test]
    fn corrupted_signature_detected() {
        let key = generate_keypair();
        let mut rec = build(&two_receipts(), &two_events(), &key);

        // Flip a byte in the signature hex string.
        let mut sig_bytes = hex::decode(&rec.signature).unwrap();
        sig_bytes[0] ^= 0xff;
        rec.signature = hex::encode(sig_bytes);

        match verify(&rec) {
            Err(AuditError::SignatureInvalid(_)) => {}
            other => panic!("expected SignatureInvalid, got {other:?}"),
        }
    }

    /// Signing with one key and then swapping the public_key field is caught.
    #[test]
    fn wrong_public_key_detected() {
        let key1 = generate_keypair();
        let key2 = generate_keypair();
        let mut rec = build(&two_receipts(), &[], &key1);

        // Replace the public key with a different valid key — the signature
        // will no longer verify.
        rec.public_key = hex::encode(key2.verifying_key().to_bytes());

        match verify(&rec) {
            Err(AuditError::SignatureInvalid(_)) => {}
            other => panic!("expected SignatureInvalid, got {other:?}"),
        }
    }

    // ── ordering & seq ───────────────────────────────────────────────────────

    /// Entry sequence numbers are 0-based and contiguous.
    #[test]
    fn seq_numbers_are_contiguous() {
        let key = generate_keypair();
        let rec = build(&two_receipts(), &two_events(), &key);
        for (i, entry) in rec.entries.iter().enumerate() {
            assert_eq!(entry.seq, i as u64);
        }
    }

    /// Receipts come before events in the chain.
    #[test]
    fn receipts_before_events_in_chain() {
        let key = generate_keypair();
        let rec = build(&two_receipts(), &two_events(), &key);
        assert_eq!(rec.entries[0].kind, "verify_ledger");
        assert_eq!(rec.entries[1].kind, "isolate_crux");
        assert_eq!(rec.entries[2].kind, "mediation_open");
        assert_eq!(rec.entries[3].kind, "mediation_close");
    }

    // ── canonical JSON ────────────────────────────────────────────────────────

    /// Two JSON objects with the same keys in different insertion order must
    /// produce the same canonical JSON (and therefore the same hash).
    #[test]
    fn canonical_json_is_key_sorted() {
        let a = serde_json::json!({ "z": 1, "a": 2, "m": 3 });
        let b = serde_json::json!({ "a": 2, "m": 3, "z": 1 });
        assert_eq!(canonical_json(&a), canonical_json(&b));
    }

    /// Canonical JSON of the same object must hash identically regardless of
    /// source, so two independently-built records with identical inputs
    /// produce the same hashes.
    #[test]
    fn identical_inputs_produce_identical_hashes() {
        let key1 = generate_keypair();
        let key2 = generate_keypair();
        let r = two_receipts();
        let e = two_events();
        let rec1 = build(&r, &e, &key1);
        let rec2 = build(&r, &e, &key2);
        // Same content → same hashes in every entry.
        for (a, b) in rec1.entries.iter().zip(rec2.entries.iter()) {
            assert_eq!(a.hash, b.hash);
            assert_eq!(a.prev_hash, b.prev_hash);
        }
    }

    // ── verdict hashing ──────────────────────────────────────────────────────

    /// Changing only a receipt's verdict makes the hash differ.
    #[test]
    fn verdict_is_included_in_hash() {
        let detail = serde_json::json!({ "x": 1 });
        let h_proved = compute_hash(GENESIS_PREV_HASH, "op", &detail, Some("Proved"));
        let h_refuted = compute_hash(GENESIS_PREV_HASH, "op", &detail, Some("Refuted"));
        let h_none = compute_hash(GENESIS_PREV_HASH, "op", &detail, None);
        assert_ne!(h_proved, h_refuted);
        assert_ne!(h_proved, h_none);
        assert_ne!(h_refuted, h_none);
    }

    // ── summary smoke test ───────────────────────────────────────────────────

    #[test]
    fn summary_contains_entry_count_and_kinds() {
        let key = generate_keypair();
        let rec = build(&two_receipts(), &two_events(), &key);
        let s = summary(&rec);
        assert!(s.contains("Entries   : 4"), "got: {s}");
        assert!(s.contains("verify_ledger"));
        assert!(s.contains("isolate_crux"));
        assert!(s.contains("mediation_open"));
        assert!(s.contains("mediation_close"));
    }

    #[test]
    fn summary_empty_record() {
        let key = generate_keypair();
        let rec = build(&[], &[], &key);
        let s = summary(&rec);
        assert!(s.contains("Entries   : 0"), "got: {s}");
    }
}
