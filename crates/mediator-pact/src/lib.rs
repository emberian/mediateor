//! `mediator-pact` — FORWARD CONSTITUTIONS (certified pacts).
//!
//! "Don't mediate the breakup; prove the relationship resolves every breakup it
//! named."
//!
//! A [`Pact`] is a two-party agreement that, **before any dispute**, ships a
//! machine-checkable, anyone-re-verifiable certificate that over its own
//! **declared question-space** it is:
//!
//!   * **(A) COMPLETE** — the disjunction of clause-guards is *valid*: every
//!     foreseeable world fires some clause. (`coverage` obligation.)
//!   * **(B) NON-CONTRADICTORY** — no world fires two clauses demanding
//!     conflicting outcomes. (`consistent_i_j` obligation per clause pair.)
//!
//! …with the human value-predicates left **UNINTERPRETED** — FREE symbols
//! (abstracted as the guard `definition`s' parameters and universally quantified
//! in every obligation, exactly as the hand-validated `isabelle/Pact.thy` does
//! it), still handed back `Unknown` at dispute time. That universal quantifier
//! is exactly what lets the certificate be issued before the fight, while the
//! moral question stays the humans'.
//!
//! # The validated seam
//!
//! The emitted `.thy` has *exactly* the shape of the hand-written, real-Isabelle
//! GREEN `isabelle/Pact.thy`, and closes the same way:
//!
//!   * guards/outcomes are `definition`s over FREE boolean crux predicates +
//!     integer (cents/days) arithmetic;
//!   * COVERAGE goal `∀ (free preds…). (g_1 ∨ … ∨ g_n)`, proof
//!     `by (auto simp: <all guard defs>)`;
//!   * CONSISTENCY goal per pair `(i<j)`:
//!     `¬ (g_i ∧ g_j ∧ o_i r ∧ ¬ o_j r)`, proof
//!     `by (auto simp: <the two guard defs + two outcome defs>)`.
//!
//! `auto` closes these when guards mix a free bool with linear int arithmetic.
//! We keep that fragment: no nonlinear arithmetic, no deep equivalences.
//!
//! # The honesty discipline (this project's bar)
//!
//! The certificate is COMPLETE / CONSISTENT **relative to the DECLARED predicate
//! space**. It provably CANNOT detect a *missing-but-foreseeable predicate*
//! nobody named — only a missing *combination* of named worlds. A green
//! certificate therefore never implies completeness-in-the-world. When coverage
//! fails, the open subgoal IS the diagnosis: the uncovered world is named in
//! plain terms. No overclaim, no hidden `sorry` in any emitted proof.

use mediator_audit::{build as build_record, generate_keypair, MediationRecord};
use mediator_prover::IsabelleProver;
use mediator_types::{Formula, Obligation, Prover, Receipt, Sig, Verdict};
use serde::{Deserialize, Serialize};

mod codegen;

pub use codegen::{
    consistency_obligation_name, coverage_obligation_name, guard_def_name, outcome_def_name,
    pact_codegen, pact_obligations, PACT_THEORY_NAME,
};

// ───────────────────────────── the pact ─────────────────────────────────

/// One clause of a pact: a `name`, a boolean `guard` (the world it fires in),
/// and an `outcome` (which reduces to a distinct integer-cents award, so a clash
/// of clauses is a clash of *numbers*).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clause {
    /// A short identifier, used to name the guard/outcome `definition`s and the
    /// `consistent_i_j` obligations. Sanitized into an Isabelle id at codegen.
    pub name: String,
    /// The guard: when does this clause fire? A [`Formula`] over the declared
    /// FREE predicates + integer arithmetic (e.g. `notice_days ≥ 30 ∧ ¬stain`).
    pub guard: Formula,
    /// The outcome: what this clause awards. Reduces to a distinct integer-cents
    /// award per clause (typically `refund = <cents>`), so a contradiction
    /// between two clauses is a contradiction between two numbers.
    pub outcome: Formula,
}

/// A FORWARD CONSTITUTION: a two-party agreement certified, before any dispute,
/// to be complete and non-contradictory over its own declared question-space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pact {
    pub title: String,
    /// The two parties' display names (e.g. `["Robin", "Sam"]`).
    pub parties: Vec<String>,
    /// The declared future-crux symbols. Kept FREE / uninterpreted in the `.thy`
    /// (abstracted as guard parameters + universally quantified in the goals);
    /// still handed back `Unknown` at dispute time. The value-predicates
    /// (`Bool`-sorted, nullary) are the contested questions; integer measurables
    /// (`Int`-sorted, like `notice_days`) are the agreed dials.
    pub predicates: Vec<Sig>,
    /// The clauses. Their guards must *cover* the declared world (coverage) and
    /// never *clash* (consistency) for the pact to certify.
    pub clauses: Vec<Clause>,
}

impl Pact {
    /// Load a pact from a JSON file.
    pub fn load(path: &str) -> anyhow::Result<Pact> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("could not read pact {path}: {e}"))?;
        let pact: Pact = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("could not parse pact {path}: {e}"))?;
        Ok(pact)
    }
}

// ─────────────────────────── the certificate ────────────────────────────

/// The verdict of one named obligation, folded plainly so a skeptic can read it
/// without touching Isabelle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObligationOutcome {
    /// The obligation name (`coverage`, `consistent_0_1`, …).
    pub name: String,
    /// The host's verdict, keyed entirely on Isabelle's own output via the
    /// trusted gate — never on anything we decide.
    pub verdict: Verdict,
    /// A plain-terms description of what this obligation checks.
    pub describes: String,
}

impl ObligationOutcome {
    /// Whether the host discharged this obligation.
    pub fn proved(&self) -> bool {
        matches!(self.verdict, Verdict::Proved)
    }
}

/// Whether a pact certified, and if not, *why not* in plain terms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Status {
    /// Coverage Proved AND every consistency Proved.
    Certified,
    /// Some obligation was not Proved. Carries the gap, surfaced plainly.
    Refused { gap: Gap },
}

/// The diagnosis when a pact is REFUSED: which obligation failed, and what that
/// means in human terms. The open subgoal IS the diagnosis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    /// The first failing obligation's name.
    pub obligation: String,
    /// The host's verdict on it (`Unknown` ⇒ the goal genuinely does not hold;
    /// `Error` ⇒ a malformed obligation, surfaced honestly).
    pub verdict: Verdict,
    /// What failed, in plain terms — e.g. "the pact does not cover every declared
    /// world: a world fires no clause." For coverage failures we also name the
    /// uncovered world as best we can from the declared predicates.
    pub plain: String,
}

/// The product of [`certify_pact`]: per-obligation verdicts, an overall
/// [`Status`], and a signed, re-verifiable [`MediationRecord`] folding the
/// coverage + consistency verdicts.
///
/// The honesty caveat travels *with* the certificate, in [`Self::scope_note`]:
/// CERTIFIED means complete & consistent **relative to the declared predicate
/// space**, never completeness-in-the-world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PactCertificate {
    pub title: String,
    pub parties: Vec<String>,
    /// The declared predicate symbols, echoed so a reader sees exactly which
    /// question-space the certificate is *relative to*.
    pub declared_predicates: Vec<String>,
    /// Every obligation's outcome, in emission order (coverage first, then each
    /// `consistent_i_j`).
    pub obligations: Vec<ObligationOutcome>,
    pub status: Status,
    /// The signed, hash-chained, anyone-re-verifiable record. Folds every
    /// obligation verdict (as a receipt) plus the certification event.
    pub record: MediationRecord,
    /// The honesty caveat carried with the certificate (see crate docs).
    pub scope_note: String,
}

impl PactCertificate {
    /// Whether the pact certified.
    pub fn certified(&self) -> bool {
        matches!(self.status, Status::Certified)
    }

    /// The gap, if refused.
    pub fn gap(&self) -> Option<&Gap> {
        match &self.status {
            Status::Refused { gap } => Some(gap),
            Status::Certified => None,
        }
    }

    /// A plain-text rendering of the certificate (what the `pact` bin prints).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Forward constitution — {}\n", self.title));
        out.push_str(&format!("  parties : {}\n", self.parties.join(" + ")));
        out.push_str(&format!(
            "  declared question-space : {}\n",
            if self.declared_predicates.is_empty() {
                "(none)".to_string()
            } else {
                self.declared_predicates.join(", ")
            }
        ));
        out.push('\n');

        for ob in &self.obligations {
            let mark = match ob.verdict {
                Verdict::Proved => "PROVED ",
                Verdict::Unknown => "OPEN   ",
                Verdict::Refuted => "REFUTED",
                Verdict::Error(_) => "ERROR  ",
            };
            out.push_str(&format!("  [{mark}] {:<18} {}\n", ob.name, ob.describes));
            if let Verdict::Error(e) = &ob.verdict {
                out.push_str(&format!("           ↳ {e}\n"));
            }
        }
        out.push('\n');

        match &self.status {
            Status::Certified => {
                out.push_str("CERTIFIED.\n");
                out.push_str("  Coverage proved and every clause pair proved consistent —\n");
                out.push_str("  over the DECLARED question-space, every foreseeable world fires\n");
                out.push_str(
                    "  exactly a non-conflicting set of clauses. The value-predicates stay\n",
                );
                out.push_str("  uninterpreted (free), handed back Unknown at dispute time.\n");
            }
            Status::Refused { gap } => {
                out.push_str("REFUSED.\n");
                out.push_str(&format!(
                    "  gap: {} ({})\n",
                    gap.obligation,
                    verdict_word(&gap.verdict)
                ));
                out.push_str(&format!("  {}\n", gap.plain));
            }
        }

        out.push('\n');
        out.push_str(&self.scope_note);
        out.push('\n');
        out.push_str(&mediator_audit::summary(&self.record));
        out.push('\n');
        out
    }
}

/// The honesty caveat carried with *every* certificate.
pub const SCOPE_NOTE: &str = "\
Scope (honesty): this certificate is COMPLETE and CONSISTENT *relative to the\n\
DECLARED predicate space* only. It provably CANNOT detect a missing-but-\n\
foreseeable predicate nobody named — only a missing COMBINATION of the worlds\n\
that were named. A green certificate does NOT imply completeness-in-the-world.";

// ─────────────────────────── certification ──────────────────────────────

/// Certify a pact: run every obligation through the trusted gate, collect
/// per-obligation Proved/Unknown/Error, and fold into a signed
/// [`MediationRecord`].
///
/// A pact is **CERTIFIED** iff coverage Proved AND every consistency Proved;
/// otherwise **REFUSED**, and the certificate carries WHICH obligation failed
/// (the gap), surfaced in plain terms.
///
/// The verdicts are keyed entirely on the [`Prover`]'s output — never on
/// anything decided here. Use a [`IsabelleProver`] to run the REAL gate.
pub fn certify_pact(pact: &Pact, prover: &dyn Prover) -> PactCertificate {
    let signing_key = generate_keypair();
    certify_pact_with_key(pact, prover, &signing_key)
}

/// As [`certify_pact`], but with a caller-supplied signing key (so a holder can
/// sign the certificate with a stable identity, and tests can pin determinism).
pub fn certify_pact_with_key(
    pact: &Pact,
    prover: &dyn Prover,
    signing_key: &ed25519_dalek::SigningKey,
) -> PactCertificate {
    let preamble = pact_codegen_preamble(pact);
    let obligations = pact_obligations(pact);

    // Run the WHOLE obligation set through the trusted gate at once.
    let verdicts = prover.check(&preamble, &obligations);

    // Re-walk in emission order, attaching plain descriptions.
    let mut outcomes: Vec<ObligationOutcome> = Vec::with_capacity(obligations.len());
    for ob in &obligations {
        let verdict = verdicts
            .get(&ob.name)
            .cloned()
            .unwrap_or_else(|| Verdict::Error("prover returned no verdict".into()));
        outcomes.push(ObligationOutcome {
            name: ob.name.clone(),
            verdict,
            describes: describe_obligation(pact, &ob.name),
        });
    }

    // Decide the status purely from the prover's verdicts. The gap is the FIRST
    // obligation that did not close — coverage takes priority (a coverage failure
    // is the load-bearing "a world fires no clause" diagnosis), then consistency.
    let status = decide_status(pact, &outcomes);

    // Fold every obligation verdict into the signed, re-verifiable record: one
    // receipt per obligation, then a final certification event.
    let record = fold_record(pact, &outcomes, &status, signing_key);

    PactCertificate {
        title: pact.title.clone(),
        parties: pact.parties.clone(),
        declared_predicates: pact.predicates.iter().map(|s| s.name.clone()).collect(),
        obligations: outcomes,
        status,
        record,
        scope_note: SCOPE_NOTE.to_string(),
    }
}

/// Convenience: certify through a freshly located [`IsabelleProver`] (the real
/// gate). Equivalent to `certify_pact(pact, &IsabelleProver::locate())`.
pub fn certify_pact_isabelle(pact: &Pact) -> PactCertificate {
    let prover = IsabelleProver::locate();
    certify_pact(pact, &prover)
}

/// The preamble half of the codegen (theory header + free consts + definitions,
/// with NO trailing `end`), as the prover contract requires.
fn pact_codegen_preamble(pact: &Pact) -> String {
    codegen::pact_preamble(pact)
}

/// Decide CERTIFIED vs REFUSED from the per-obligation verdicts alone.
fn decide_status(pact: &Pact, outcomes: &[ObligationOutcome]) -> Status {
    let coverage_name = coverage_obligation_name();

    // Coverage first: if the disjunction of guards is not valid, *that* is the
    // headline gap (a declared world fires no clause).
    if let Some(cov) = outcomes.iter().find(|o| o.name == coverage_name) {
        if !cov.proved() {
            return Status::Refused {
                gap: Gap {
                    obligation: cov.name.clone(),
                    verdict: cov.verdict.clone(),
                    plain: coverage_gap_plain(pact, &cov.verdict),
                },
            };
        }
    } else {
        // No coverage obligation at all ⇒ malformed; refuse honestly.
        return Status::Refused {
            gap: Gap {
                obligation: coverage_name,
                verdict: Verdict::Error("no coverage obligation was generated".into()),
                plain: "the pact produced no coverage obligation — it is malformed.".to_string(),
            },
        };
    }

    // Then every consistency: the first clash (or undecided pair) is the gap.
    for o in outcomes {
        if o.name == coverage_obligation_name() {
            continue;
        }
        if !o.proved() {
            return Status::Refused {
                gap: Gap {
                    obligation: o.name.clone(),
                    verdict: o.verdict.clone(),
                    plain: consistency_gap_plain(pact, &o.name, &o.verdict),
                },
            };
        }
    }

    Status::Certified
}

/// Plain-terms diagnosis of a coverage failure: name the uncovered world as best
/// we can from the declared predicates. The honest half — the open subgoal IS
/// the diagnosis.
fn coverage_gap_plain(pact: &Pact, verdict: &Verdict) -> String {
    match verdict {
        Verdict::Error(e) => format!(
            "the coverage obligation did not even type-check ({e}); the pact is malformed, \
             not merely incomplete."
        ),
        _ => {
            // The world the guards leave unhandled. We cannot, in general,
            // exhibit the exact witness without a model — but we CAN tell the
            // humans, in their own declared terms, that a combination of the
            // named worlds fires no clause, and point at the dials in play.
            let bool_preds: Vec<&str> = pact
                .predicates
                .iter()
                .filter(|s| s.ret == mediator_types::Sort::Bool && s.arg_sorts.is_empty())
                .map(|s| s.name.as_str())
                .collect();
            let int_preds: Vec<&str> = pact
                .predicates
                .iter()
                .filter(|s| s.ret == mediator_types::Sort::Int)
                .map(|s| s.name.as_str())
                .collect();
            let mut plain = String::from(
                "the pact does NOT cover every declared world: there is a combination of the \
                 named worlds that fires no clause, so the agreement is silent there.",
            );
            if !int_preds.is_empty() {
                plain.push_str(&format!(
                    " Look at the integer dial(s) [{}] — a cliff value falls through the guards.",
                    int_preds.join(", ")
                ));
            }
            if !bool_preds.is_empty() {
                plain.push_str(&format!(
                    " The contested predicate(s) [{}] remain uninterpreted; the gap is in the \
                     *combination*, not in any single predicate's meaning.",
                    bool_preds.join(", ")
                ));
            }
            plain.push_str(
                " (Honesty: this gap is a missing COMBINATION of named worlds — a predicate \
                 nobody named could not have been detected at all.)",
            );
            plain
        }
    }
}

/// Plain-terms diagnosis of a consistency failure: which two clauses clash.
fn consistency_gap_plain(pact: &Pact, ob_name: &str, verdict: &Verdict) -> String {
    if let Verdict::Error(e) = verdict {
        return format!(
            "the consistency obligation {ob_name} did not type-check ({e}); the pact is malformed."
        );
    }
    // Recover the (i, j) clause indices from the obligation name.
    if let Some((i, j)) = parse_consistency_pair(ob_name) {
        let ci = pact.clauses.get(i).map(|c| c.name.as_str()).unwrap_or("?");
        let cj = pact.clauses.get(j).map(|c| c.name.as_str()).unwrap_or("?");
        format!(
            "clauses `{ci}` and `{cj}` CLASH: there is a world where both guards fire yet they \
             demand different awards. A contradiction of numbers — the pact would owe two amounts \
             at once."
        )
    } else {
        format!("consistency obligation {ob_name} did not close: two clauses can demand conflicting awards.")
    }
}

/// Parse `consistent_<i>_<j>` → `(i, j)`.
fn parse_consistency_pair(name: &str) -> Option<(usize, usize)> {
    let rest = name.strip_prefix("consistent_")?;
    let (a, b) = rest.split_once('_')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// A plain description of what an obligation checks (for the certificate body).
fn describe_obligation(pact: &Pact, name: &str) -> String {
    if name == coverage_obligation_name() {
        return "COVERAGE: the disjunction of all clause-guards is valid (every declared world \
                fires some clause)."
            .to_string();
    }
    if let Some((i, j)) = parse_consistency_pair(name) {
        let ci = pact.clauses.get(i).map(|c| c.name.as_str()).unwrap_or("?");
        let cj = pact.clauses.get(j).map(|c| c.name.as_str()).unwrap_or("?");
        return format!(
            "CONSISTENCY: clauses `{ci}` and `{cj}` never both fire while demanding conflicting awards."
        );
    }
    format!("obligation {name}")
}

/// Fold the obligation verdicts into a signed, hash-chained [`MediationRecord`].
///
/// Each obligation becomes one receipt (op = the obligation name, verdict = the
/// host's verdict); the final certification event records the overall outcome
/// and — when refused — the gap. Mirrors `mediator-web::audit_record_for`:
/// `mediator_audit::build(receipts, events, key)`.
fn fold_record(
    pact: &Pact,
    outcomes: &[ObligationOutcome],
    status: &Status,
    signing_key: &ed25519_dalek::SigningKey,
) -> MediationRecord {
    let mut receipts: Vec<Receipt> = Vec::with_capacity(outcomes.len());
    let mut prev_hash = mediator_audit_genesis();
    for (seq, o) in outcomes.iter().enumerate() {
        let detail = serde_json::json!({
            "obligation": o.name,
            "describes": o.describes,
        });
        let hash = receipt_hash(&prev_hash, &o.name, &detail);
        receipts.push(Receipt {
            seq: seq as u64,
            prev_hash: prev_hash.clone(),
            hash: hash.clone(),
            op: o.name.clone(),
            detail,
            verdict: Some(o.verdict.clone()),
        });
        prev_hash = hash;
    }

    // The certification event: the headline outcome + the declared scope. When
    // refused, the gap travels in the record too.
    let mut events: Vec<(String, serde_json::Value)> = Vec::new();
    let (kind, detail) = match status {
        Status::Certified => (
            "pact_certified".to_string(),
            serde_json::json!({
                "title": pact.title,
                "parties": pact.parties,
                "declared_predicates": pact.predicates.iter().map(|s| &s.name).collect::<Vec<_>>(),
                "complete": true,
                "non_contradictory": true,
                "scope": "relative to the DECLARED predicate space only",
            }),
        ),
        Status::Refused { gap } => (
            "pact_refused".to_string(),
            serde_json::json!({
                "title": pact.title,
                "parties": pact.parties,
                "declared_predicates": pact.predicates.iter().map(|s| &s.name).collect::<Vec<_>>(),
                "gap_obligation": gap.obligation,
                "gap_verdict": gap.verdict,
                "gap_plain": gap.plain,
                "scope": "relative to the DECLARED predicate space only",
            }),
        ),
    };
    events.push((kind, detail));

    build_record(&receipts, &events, signing_key)
}

// The audit crate's hashing is private; we reproduce its receipt-chain hash so
// the receipts we hand to `build` carry consistent self-hashes (the record's own
// chain is recomputed inside `build`, so this is only for the receipts' embedded
// provenance — kept identical to `mediator-core::receipts::chain_hash`).
fn mediator_audit_genesis() -> String {
    "0000000000000000000000000000000000000000000000000000000000000000".to_string()
}

fn receipt_hash(prev_hash: &str, op: &str, detail: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(op.as_bytes());
    hasher.update(serde_json::to_string(detail).unwrap_or_default().as_bytes());
    let out = hasher.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

fn verdict_word(v: &Verdict) -> &'static str {
    match v {
        Verdict::Proved => "proved",
        Verdict::Refuted => "refuted",
        Verdict::Unknown => "open subgoal",
        Verdict::Error(_) => "error",
    }
}

/// Re-export the obligations list shape for tests/consumers that want it without
/// running the gate.
pub fn obligations_for(pact: &Pact) -> Vec<Obligation> {
    pact_obligations(pact)
}

#[cfg(test)]
mod tests;
