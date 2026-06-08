//! `mediator-core` — the kernel brain. STUB to be implemented by the swarm.
//!
//! Owns the *reduction*: turns a `Dispute` into Isabelle theory source + named
//! proof obligations (it generates the `.thy`, like WMT owns IR→SMT), calls a
//! `dyn Prover`, interprets the verdicts, runs fair division via a
//! `dyn FairDivider`, and assembles the `Analysis` plus the hash-chained
//! receipt ledger. Generic over the trait seams — wired to concrete impls by
//! the demo.

use mediator_types::{Analysis, Dispute, FairDivider, Prover, Receipt};

/// Analyze a dispute end to end. The single entry point the demo calls.
pub fn analyze(
    _dispute: &Dispute,
    _prover: &dyn Prover,
    _fair: &dyn FairDivider,
) -> (Analysis, Vec<Receipt>) {
    todo!("swarm: codegen .thy, gate via prover, build Analysis + receipts")
}
