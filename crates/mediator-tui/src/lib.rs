//! `mediator-tui` — the two-tier, *kind* UX. STUB for the swarm.
//!
//! One `Analysis`, two projections:
//!   * `operator_view` — the cockpit (full graph, prover verdicts, receipts);
//!   * `party_view`    — them-focused, pared down, kind: what you share, the
//!     one knot, the dissolved misunderstandings, the fair options. Never a
//!     morphism, never a verdict-as-judgment.
//!
//! Built on `ratatui`. For the first demo these may render to strings/stdout;
//! the live TUI can follow. Kindness is a property of the projection.

use mediator_types::Analysis;

/// Render the operator cockpit (full detail).
pub fn operator_view(_analysis: &Analysis) -> String {
    todo!("swarm: cockpit projection")
}

/// Render the party-facing view (kind, pared down, plain language).
pub fn party_view(_analysis: &Analysis, _for_party: &str) -> String {
    todo!("swarm: kind party projection")
}
