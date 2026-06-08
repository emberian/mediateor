//! OFFLINE unit tests — no Isabelle. These exercise the codegen *shape* and the
//! pure status/record folding (driven by a mock prover), so the whole suite runs
//! in milliseconds. The real-Isabelle behavior is gated in
//! `tests/isabelle_integration.rs`, mirroring how mediator-core gates its
//! real-Isabelle test.

use super::*;
use mediator_types::{Sort, Term};
use std::collections::HashMap;

// ── tiny formula helpers ──────────────────────────────────────────────────

fn bool_sig(name: &str) -> Sig {
    Sig {
        name: name.into(),
        arg_sorts: vec![],
        ret: Sort::Bool,
        gloss: String::new(),
    }
}
fn int_sig(name: &str) -> Sig {
    Sig {
        name: name.into(),
        arg_sorts: vec![],
        ret: Sort::Int,
        gloss: String::new(),
    }
}
fn atom(n: &str) -> Formula {
    Formula::Atom(Term::App(n.into(), vec![]))
}
fn nd_ge(n: i64) -> Formula {
    Formula::Le(Term::IntLit(n), Term::App("notice_days".into(), vec![]))
}
fn nd_lt(n: i64) -> Formula {
    Formula::Lt(Term::App("notice_days".into(), vec![]), Term::IntLit(n))
}
fn award(cents: i64) -> Formula {
    Formula::Eq(Term::Var("award".into()), Term::IntLit(cents))
}

/// The complete roommate move-out pact (mirrors Pact.thy / the seed JSON).
fn complete_pact() -> Pact {
    Pact {
        title: "Roommate move-out".into(),
        parties: vec!["Robin".into(), "Sam".into()],
        predicates: vec![bool_sig("stain_is_damage"), int_sig("notice_days")],
        clauses: vec![
            Clause {
                name: "clean".into(),
                guard: Formula::And(vec![
                    nd_ge(30),
                    Formula::Not(Box::new(atom("stain_is_damage"))),
                ]),
                outcome: award(120000),
            },
            Clause {
                name: "damaged".into(),
                guard: Formula::And(vec![nd_ge(30), atom("stain_is_damage")]),
                outcome: award(90000),
            },
            Clause {
                name: "short".into(),
                guard: nd_lt(30),
                outcome: award(100000),
            },
        ],
    }
}

/// The broken pact: the short-notice clause removed → coverage must fail.
fn broken_pact() -> Pact {
    let mut p = complete_pact();
    p.clauses.retain(|c| c.name != "short");
    p
}

// ── a mock prover: lets us drive status/record folding OFFLINE ─────────────

/// A prover that returns a fixed verdict per obligation name. Unlisted
/// obligations default to `Proved`, so a "complete & consistent" run is the
/// empty map.
struct MockProver {
    verdicts: HashMap<String, Verdict>,
    default: Verdict,
}
impl MockProver {
    fn all_proved() -> Self {
        Self {
            verdicts: HashMap::new(),
            default: Verdict::Proved,
        }
    }
    fn with(mut self, name: &str, v: Verdict) -> Self {
        self.verdicts.insert(name.into(), v);
        self
    }
}
impl Prover for MockProver {
    fn check(&self, _preamble: &str, obligations: &[Obligation]) -> HashMap<String, Verdict> {
        obligations
            .iter()
            .map(|o| {
                let v = self
                    .verdicts
                    .get(&o.name)
                    .cloned()
                    .unwrap_or(self.default.clone());
                (o.name.clone(), v)
            })
            .collect()
    }
}

// ── codegen shape (offline) ────────────────────────────────────────────────

#[test]
fn obligations_are_coverage_then_pairs() {
    let obs = pact_obligations(&complete_pact());
    assert_eq!(obs[0].name, "coverage");
    // 3 clauses => 3 consistency pairs.
    let names: Vec<&str> = obs.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "coverage",
            "consistent_0_1",
            "consistent_0_2",
            "consistent_1_2"
        ]
    );
}

#[test]
fn codegen_matches_pact_thy_fragment() {
    let thy = pact_codegen(&complete_pact());
    // The value-predicate stays uninterpreted — free via abstraction over the
    // guard parameters + universal quantification in the goals (Pact.thy's
    // discipline), NOT a global `consts` (which would clash with the binders).
    assert!(thy.contains("stain_is_damage :: bool"));
    assert!(thy.contains("notice_days :: int"));
    // guard def over free bool + int arithmetic, exactly Pact.thy's g_clean.
    // The definition's LHS binds typed args; the body mixes the free bool with
    // linear int arithmetic — the fragment `auto` closes.
    assert!(thy.contains(
        "g_0_clean (stain_is_damage::bool) (notice_days::int) \\<equiv> ((30 \\<le> notice_days) \\<and> (\\<not> stain_is_damage))"
    ));
    // outcome reduces to a distinct integer-cents award.
    assert!(thy.contains("o_0_clean award \\<equiv> (award = 120000)"));
    assert!(thy.contains("o_1_damaged award \\<equiv> (award = 90000)"));
    assert!(thy.contains("o_2_short award \\<equiv> (award = 100000)"));
    // coverage: universally quantified disjunction, auto over all guard defs.
    assert!(thy.contains("theorem coverage:"));
    assert!(thy.contains("\\<forall>(stain_is_damage::bool) (notice_days::int)"));
    assert!(thy.contains("by (auto simp: g_0_clean_def g_1_damaged_def g_2_short_def)"));
    // consistency: the ¬(g_i ∧ g_j ∧ o_i ∧ ¬o_j) shape, auto over the 4 defs.
    assert!(thy.contains(
        "\\<not> (g_0_clean stain_is_damage notice_days \\<and> g_1_damaged stain_is_damage notice_days \\<and> o_0_clean award \\<and> \\<not> o_1_damaged award)"
    ));
    assert!(
        thy.contains("by (auto simp: g_0_clean_def g_1_damaged_def o_0_clean_def o_1_damaged_def)")
    );
    // no hidden sorry anywhere.
    assert!(
        !thy.contains("sorry"),
        "no sorry may appear in an emitted proof"
    );
}

#[test]
fn broken_pact_drops_short_clause_in_codegen() {
    let thy = pact_codegen(&broken_pact());
    assert!(!thy.contains("g_2_short"), "short clause must be gone");
    // coverage disjunction now has only two guards.
    assert!(thy.contains("by (auto simp: g_0_clean_def g_1_damaged_def)"));
    // only one consistency pair remains: (0,1).
    let obs = pact_obligations(&broken_pact());
    let names: Vec<&str> = obs.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, vec!["coverage", "consistent_0_1"]);
}

// ── status / record folding (offline, via mock prover) ─────────────────────

#[test]
fn all_proved_certifies_and_signs() {
    let cert = certify_pact(&complete_pact(), &MockProver::all_proved());
    assert!(cert.certified(), "expected CERTIFIED");
    assert!(cert.gap().is_none());
    // every obligation present and Proved
    assert_eq!(cert.obligations.len(), 4);
    assert!(cert.obligations.iter().all(|o| o.proved()));
    // the record is signed and re-verifiable.
    assert!(
        mediator_audit::verify(&cert.record).is_ok(),
        "signed record must verify"
    );
    // it carries one receipt per obligation + the certification event.
    assert_eq!(cert.record.entries.len(), 4 + 1);
    assert_eq!(cert.record.entries.last().unwrap().kind, "pact_certified");
    // scope note travels with it.
    assert!(cert.scope_note.contains("DECLARED predicate space"));
    let rendered = cert.render();
    assert!(rendered.contains("CERTIFIED."));
    assert!(rendered.contains("completeness-in-the-world"));
}

#[test]
fn coverage_failure_refuses_with_named_gap() {
    // The broken pact: coverage comes back Unknown (the disjunction is not valid).
    let prover = MockProver::all_proved().with("coverage", Verdict::Unknown);
    let cert = certify_pact(&broken_pact(), &prover);
    assert!(!cert.certified(), "expected REFUSED");
    let gap = cert.gap().expect("a gap");
    assert_eq!(gap.obligation, "coverage");
    assert_eq!(gap.verdict, Verdict::Unknown);
    // The uncovered world is named in the humans' own declared terms.
    assert!(gap.plain.contains("does NOT cover every declared world"));
    assert!(
        gap.plain.contains("notice_days"),
        "names the integer dial in play"
    );
    // honesty: the refusal says the gap is a missing COMBINATION, not a missing predicate.
    assert!(gap.plain.contains("missing COMBINATION"));
    // the refusal is recorded in the signed chain too.
    assert!(mediator_audit::verify(&cert.record).is_ok());
    assert_eq!(cert.record.entries.last().unwrap().kind, "pact_refused");
    let rendered = cert.render();
    assert!(rendered.contains("REFUSED."));
    assert!(rendered.contains("coverage"));
}

#[test]
fn consistency_failure_names_the_two_clashing_clauses() {
    // Coverage proves, but clauses 0 and 1 clash.
    let prover = MockProver::all_proved().with("consistent_0_1", Verdict::Unknown);
    let cert = certify_pact(&complete_pact(), &prover);
    assert!(!cert.certified());
    let gap = cert.gap().unwrap();
    assert_eq!(gap.obligation, "consistent_0_1");
    assert!(gap.plain.contains("clean"));
    assert!(gap.plain.contains("damaged"));
    assert!(gap.plain.contains("CLASH"));
}

#[test]
fn coverage_takes_priority_over_consistency_in_gap() {
    // Both fail; coverage must be the reported headline gap.
    let prover = MockProver::all_proved()
        .with("coverage", Verdict::Unknown)
        .with("consistent_0_1", Verdict::Unknown);
    let cert = certify_pact(&complete_pact(), &prover);
    assert_eq!(cert.gap().unwrap().obligation, "coverage");
}

#[test]
fn prover_error_surfaces_honestly_as_gap() {
    let prover =
        MockProver::all_proved().with("coverage", Verdict::Error("inner syntax error".into()));
    let cert = certify_pact(&complete_pact(), &prover);
    let gap = cert.gap().unwrap();
    assert!(matches!(gap.verdict, Verdict::Error(_)));
    assert!(gap.plain.contains("malformed") || gap.plain.contains("type-check"));
}

#[test]
fn record_round_trips_through_json() {
    let cert = certify_pact(&complete_pact(), &MockProver::all_proved());
    let json = serde_json::to_string(&cert.record).unwrap();
    let rec2: mediator_audit::MediationRecord = serde_json::from_str(&json).unwrap();
    assert!(mediator_audit::verify(&rec2).is_ok());
}

#[test]
fn pact_json_round_trips() {
    let p = complete_pact();
    let json = serde_json::to_string(&p).unwrap();
    let p2: Pact = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}
