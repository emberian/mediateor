//! `mediator-types` — the shared contract for the trusted-mediator kernel.
//!
//! Pure data + trait interfaces. **No logic.** Every other crate depends on
//! this and only this for cross-crate types. Agents implementing the other
//! crates may add helper methods, but must not break these signatures.
//!
//! Design north star: a *backstage cathedral*. The host prover holds the
//! formalizable core; the LLM operates it; humans receive a kind, plain
//! rendering. Money is always integer **cents** (never floats).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type PartyId = String;
pub type ClaimId = String;
pub type ItemId = String;

// ───────────────────────── typed many-sorted IR ─────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Sort {
    Bool,
    Int,
    Real,
    Uninterp(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Term {
    Var(String),
    IntLit(i64),
    /// Function/predicate/constant application. Nullary `args` = a constant.
    App(String, Vec<Term>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Formula {
    /// A Bool-sorted term (predicate application).
    Atom(Term),
    Eq(Term, Term),
    Le(Term, Term),
    Lt(Term, Term),
    Not(Box<Formula>),
    And(Vec<Formula>),
    Or(Vec<Formula>),
    Implies(Box<Formula>, Box<Formula>),
    Iff(Box<Formula>, Box<Formula>),
    Forall(String, Sort, Box<Formula>),
    Exists(String, Sort, Box<Formula>),
    /// Shallow deontic operators (the embedded normative layer).
    Obligation(Box<Formula>),
    Permission(Box<Formula>),
}

/// An ontology/signature symbol a party brings to the table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sig {
    pub name: String,
    pub arg_sorts: Vec<Sort>,
    pub ret: Sort,
    pub gloss: String,
}

// ───────────────────────────── the dispute ──────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub party: PartyId,
    /// The human sentence, verbatim.
    pub nl: String,
    /// Its formalization (the untrusted proposal, once gated).
    pub formula: Formula,
    /// Deterministic plain-English back-render shown for confirmation.
    pub english_render: String,
    /// Epistemic entrenchment: higher = harder to give up (AGM weight).
    pub weight: i64,
    pub defeasible: bool,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LedgerItem {
    pub id: ItemId,
    pub label: String,
    pub amount_cents: i64,
    pub asserted_by: PartyId,
    pub disputed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub deposit_cents: i64,
    pub items: Vec<LedgerItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContestedItem {
    pub id: ItemId,
    pub label: String,
    pub divisible: bool,
}

/// A party's 100-point allocation across contested items (fair division).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Valuation {
    pub party: PartyId,
    pub item: ItemId,
    pub points: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Party {
    pub id: PartyId,
    pub display_name: String,
    pub signature: Vec<Sig>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dispute {
    pub title: String,
    pub parties: Vec<Party>,
    pub claims: Vec<Claim>,
    /// Facts both parties stipulate (shared ground / lease terms).
    pub stipulated: Vec<Formula>,
    pub ledger: Ledger,
    pub contested_items: Vec<ContestedItem>,
    pub valuations: Vec<Valuation>,
}

// ─────────────────────────── prover verdicts ────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Verdict {
    /// The named lemma was discharged by the host.
    Proved,
    /// Its negation was discharged (the claim is certified false).
    Refuted,
    /// The host could not decide it (reported honestly, never as consistent).
    Unknown,
    Error(String),
}

// ───────────────────────────── settlements ──────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settlement {
    pub label: String,
    /// Indivisible items awarded whole.
    pub allocations: Vec<(ItemId, PartyId)>,
    /// Divisible items, fraction to the *first* party in `parties` order.
    pub splits: Vec<(ItemId, f64)>,
    /// Each party's total points received (fairness is read off these).
    pub party_points: Vec<(PartyId, f64)>,
    pub envy_free: bool,
    pub equitable: bool,
    pub pareto_optimal: bool,
    pub explanation: String,
}

// ─────────────────── receipts: append-only, hash-chained ─────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub seq: u64,
    pub prev_hash: String,
    pub hash: String,
    /// The operation name (e.g. "verify_ledger", "isolate_crux").
    pub op: String,
    pub detail: serde_json::Value,
    pub verdict: Option<Verdict>,
}

// ──────────────── the analysis: what both UX views render ────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub description: String,
    pub parties: Vec<PartyId>,
    pub claim_ids: Vec<ClaimId>,
}

/// The product of analyzing a dispute. The operator cockpit and the party
/// view are two *projections* of this single structure.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Analysis {
    /// Plain-language facts both parties already share (bigger than the fight).
    pub shared_core: Vec<String>,
    /// The irreducible knots — genuine inter-party disagreement.
    pub genuine_conflicts: Vec<Conflict>,
    /// "Fights" that were only different words — dissolved with a receipt.
    pub dissolved: Vec<String>,
    /// The certified refund, if the ledger is decidable.
    pub ledger_refund_cents: Option<i64>,
    /// Plain findings, e.g. "claimed total $500 refuted; itemized = $450".
    pub ledger_findings: Vec<String>,
    /// The single contested predicate the whole obligation reduces to.
    pub crux: Option<String>,
    /// Certified-fair settlement options to accept, reject, or counter.
    pub settlements: Vec<Settlement>,
}

// ──────────────────────── trait interfaces (seams) ───────────────────────

/// A single proof obligation, fully expressed in Isabelle/HOL text. The core
/// generates these (it owns the reduction); the prover only runs them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Obligation {
    pub name: String,
    /// The goal as an Isabelle/HOL proposition, e.g. `"refund_due = 75000"`.
    pub goal: String,
    /// Proof method text to attempt, e.g. `"by (simp add: refund_due_def)"`.
    /// A goal that is *expected to be undecidable* (e.g. the crux) is given a
    /// best-effort method; failure to discharge is reported as `Unknown`, which
    /// is itself the informative answer.
    pub proof: String,
}

/// The **trusted gate**. Each obligation is checked *in isolation* against a
/// shared `preamble` (theory text from `theory … begin` through all
/// declarations/definitions/axiomatizations, with no trailing `end`). Knows
/// nothing about disputes — it runs Isabelle and parses the outcome.
pub trait Prover {
    fn check(&self, preamble: &str, obligations: &[Obligation]) -> HashMap<String, Verdict>;
}

/// Fair division over divisible stakes. Returns one or more certified options.
pub trait FairDivider {
    fn divide(
        &self,
        items: &[ContestedItem],
        valuations: &[Valuation],
        parties: &[PartyId],
    ) -> Vec<Settlement>;
}

/// The **untrusted operator**: proposes formalizations. Never trusted; every
/// output is gated by a `Prover` before it touches the record.
pub trait LlmOperator {
    /// Propose a formalization of `nl` reusing the given signature symbols.
    fn formalize(&self, nl: &str, sig: &[Sig]) -> Result<Formula, String>;
    /// A model's prose rendering of a formula (advisory; the deterministic
    /// renderer in `mediator-core` is the trusted one).
    fn render_english(&self, f: &Formula) -> String;
}
