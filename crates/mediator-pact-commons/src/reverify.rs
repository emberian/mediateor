//! **RE-VERIFY WITHOUT TRUST** — a skeptic reconstructs every claim of the
//! commons from the signed artifacts alone, with no AI and no faith in the
//! commons object.
//!
//! The commons holds no authority. Its only outputs — the template grouping and
//! every controversy tally — are pure functions of (the pact JSONs) + (their
//! signed certificates). A skeptic who distrusts the commons entirely runs
//! [`reverify_corpus`] over the same `(pact, cert, label)` triples and gets:
//!
//!   1. **the gate, re-run**: each cert is `Certified`, each cert genuinely
//!      describes its pact, each pact's crux is verified-free by re-running the
//!      authoring-time firewall offline, and each signed
//!      [`mediator_audit::MediationRecord`] re-verifies (ed25519 over the hash
//!      chain). This is exactly [`crate::admission_gate`], re-executed.
//!   2. **the key, recomputed**: the [`crate::template_key`] of every pact, so the
//!      grouping the commons reported can be re-derived rather than trusted.
//!   3. **the tally, re-checked**: for any decision point, the
//!      [`crate::controversy::tally`] recomputed from the re-verified pacts must
//!      equal what the commons published.
//!
//! The single most important property — the one a test pins — is **(1) fails on a
//! tampered cited record**: flip one byte of an embedded record's signed payload
//! and re-verification rejects that pact, so the commons cannot launder a forged
//! precedent. Trust comes from re-verification, not from the commons' say-so.

use crate::{admission_gate, CommonsError, CorpusPact};
use crate::template::template_key;
use mediator_pact::{Pact, PactCertificate};

/// Why a candidate triple failed re-verification (a superset framing of
/// [`CommonsError`], naming the failing source).
#[derive(Debug)]
pub enum AdmitError {
    /// The `i`-th triple failed the admission gate (cert not certified, cert
    /// mismatched the pact, crux not free, or record did not verify).
    Gate {
        index: usize,
        label: String,
        error: CommonsError,
    },
}

impl std::fmt::Display for AdmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdmitError::Gate { index, label, error } => {
                write!(f, "triple #{index} ({label}) failed re-verification: {error}")
            }
        }
    }
}

impl std::error::Error for AdmitError {}

/// The product of a from-scratch re-verification of a corpus: the verified
/// triples (as a fresh [`crate::Commons`] a skeptic now trusts because *they*
/// checked it) and the recomputed template grouping. If any triple fails, the
/// whole thing errors — a commons with a single unverifiable member is not to be
/// trusted.
#[derive(Debug)]
pub struct ReverifyOutcome {
    /// Per-triple recomputed template key, in input order. A skeptic compares
    /// these against the grouping the commons published.
    pub recomputed_keys: Vec<String>,
    /// The number of triples that passed (equals the input length on `Ok`).
    pub verified: usize,
}

/// Re-verify a whole corpus from `(pact, cert, label)` triples, with no trust in
/// any commons object: re-run the admission gate on each (cert CERTIFIED, cert
/// matches pact, crux verified-free offline, record re-verifies) and recompute
/// each template key. Errors at the first triple that fails — most importantly, a
/// tampered cited record (its `verify` fails) is rejected here.
///
/// This is the function a skeptic calls. It deliberately takes the raw triples
/// (not a `Commons`), so it can be run by someone who never saw the commons and
/// only has the published artifacts.
pub fn reverify_corpus(
    triples: &[(Pact, PactCertificate, String)],
) -> Result<ReverifyOutcome, AdmitError> {
    let mut recomputed_keys = Vec::with_capacity(triples.len());
    for (i, (pact, cert, label)) in triples.iter().enumerate() {
        admission_gate(pact, cert).map_err(|error| AdmitError::Gate {
            index: i,
            label: label.clone(),
            error,
        })?;
        recomputed_keys.push(template_key(pact));
    }
    Ok(ReverifyOutcome {
        verified: triples.len(),
        recomputed_keys,
    })
}

/// Re-verify a single corpus pact the way a skeptic would: re-run the gate and
/// recompute its key. Returns the recomputed template key on success. Used by the
/// commons' own `reverify`, and handy for re-checking one cited precedent.
pub fn reverify_one(cp: &CorpusPact) -> Result<String, CommonsError> {
    admission_gate(&cp.pact, &cp.cert)?;
    Ok(template_key(&cp.pact))
}
