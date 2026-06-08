//! `mediator-prover` — the Isabelle/HOL backend. THE TRUSTED GATE.
//!
//! STUB to be implemented by the swarm. Drives the `isabelle` binary to check
//! generated theory source and reports which named lemmas were discharged.
//! Knows nothing about disputes — it runs `.thy` text and parses the result.

use mediator_types::{Obligation, Prover, Verdict};
use std::collections::HashMap;

/// Verifier backed by the local Isabelle install.
pub struct IsabelleProver {
    /// Absolute path to the `isabelle` executable.
    pub isabelle_bin: String,
}

impl IsabelleProver {
    pub fn new(isabelle_bin: impl Into<String>) -> Self {
        Self { isabelle_bin: isabelle_bin.into() }
    }
}

impl Prover for IsabelleProver {
    fn check(&self, _preamble: &str, _obligations: &[Obligation]) -> HashMap<String, Verdict> {
        todo!("swarm: per-obligation isolated isabelle runs; parse Proved/Unknown/Error")
    }
}
