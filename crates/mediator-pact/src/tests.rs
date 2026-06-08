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

/// The complete roommate move-out pact (mirrors the seed JSON). Every clause
/// guard branches on the free crux `stain_is_damage` — the authoring-time
/// firewall ([`crate::validate`]) requires it, so the short-notice case is split
/// into crux-aware `short_clean` / `short_damaged` (both award the same, since
/// short notice is dispositive there). Four clauses, pairwise-exclusive guards.
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
                name: "short_clean".into(),
                guard: Formula::And(vec![
                    nd_lt(30),
                    Formula::Not(Box::new(atom("stain_is_damage"))),
                ]),
                outcome: award(100000),
            },
            Clause {
                name: "short_damaged".into(),
                guard: Formula::And(vec![nd_lt(30), atom("stain_is_damage")]),
                outcome: award(100000),
            },
        ],
    }
}

/// The broken pact: both short-notice clauses removed → coverage must fail
/// (a short-notice world fires nothing).
fn broken_pact() -> Pact {
    let mut p = complete_pact();
    p.clauses
        .retain(|c| c.name != "short_clean" && c.name != "short_damaged");
    p
}

/// A pact that is COMPLETE but INCONSISTENT: `late_overlap` fires on any
/// notice≥30 and overlaps `damaged` (which also fires on notice≥30 ∧ stain),
/// but they demand different awards (100000 vs 90000). Every guard still
/// branches on the crux (firewall-clean), so the only failure is the clash —
/// the third trichotomy leg. Coverage holds: clean ∨ damaged ∨ late_overlap
/// covers notice≥30, and short_clean ∨ short_damaged covers notice<30.
fn inconsistent_pact() -> Pact {
    Pact {
        title: "Overlapping move-out (inconsistent)".into(),
        parties: vec!["Robin".into(), "Sam".into()],
        predicates: vec![bool_sig("stain_is_damage"), int_sig("notice_days")],
        clauses: vec![
            // 0: clean
            Clause {
                name: "clean".into(),
                guard: Formula::And(vec![
                    nd_ge(30),
                    Formula::Not(Box::new(atom("stain_is_damage"))),
                ]),
                outcome: award(120000),
            },
            // 1: damaged
            Clause {
                name: "damaged".into(),
                guard: Formula::And(vec![nd_ge(30), atom("stain_is_damage")]),
                outcome: award(90000),
            },
            // 2: late_overlap — fires on ALL notice≥30 (overlaps both above),
            //    but is crux-aware (the disjunction keeps a crux dependence).
            Clause {
                name: "late_overlap".into(),
                guard: Formula::And(vec![
                    nd_ge(30),
                    Formula::Or(vec![
                        atom("stain_is_damage"),
                        Formula::Not(Box::new(atom("stain_is_damage"))),
                    ]),
                ]),
                outcome: award(100000),
            },
            // 3,4: short cases (cover notice<30)
            Clause {
                name: "short_clean".into(),
                guard: Formula::And(vec![
                    nd_lt(30),
                    Formula::Not(Box::new(atom("stain_is_damage"))),
                ]),
                outcome: award(100000),
            },
            Clause {
                name: "short_damaged".into(),
                guard: Formula::And(vec![nd_lt(30), atom("stain_is_damage")]),
                outcome: award(100000),
            },
        ],
    }
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
    // 4 clauses => 6 consistency pairs (0,1) (0,2) (0,3) (1,2) (1,3) (2,3).
    let names: Vec<&str> = obs.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "coverage",
            "consistent_0_1",
            "consistent_0_2",
            "consistent_0_3",
            "consistent_1_2",
            "consistent_1_3",
            "consistent_2_3",
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
    // outcome reduces to an integer-cents award per clause.
    assert!(thy.contains("o_0_clean award \\<equiv> (award = 120000)"));
    assert!(thy.contains("o_1_damaged award \\<equiv> (award = 90000)"));
    assert!(thy.contains("o_2_short_clean award \\<equiv> (award = 100000)"));
    assert!(thy.contains("o_3_short_damaged award \\<equiv> (award = 100000)"));
    // coverage: universally quantified disjunction, auto over all guard defs.
    assert!(thy.contains("theorem coverage:"));
    assert!(thy.contains("\\<forall>(stain_is_damage::bool) (notice_days::int)"));
    assert!(thy.contains(
        "by (auto simp: g_0_clean_def g_1_damaged_def g_2_short_clean_def g_3_short_damaged_def)"
    ));
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
fn broken_pact_drops_short_clauses_in_codegen() {
    let thy = pact_codegen(&broken_pact());
    assert!(!thy.contains("short_clean"), "short clauses must be gone");
    assert!(!thy.contains("short_damaged"), "short clauses must be gone");
    // coverage disjunction now has only the two notice≥30 guards.
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
    assert!(cert.witness().is_none());
    // every obligation present and Proved: coverage + 6 consistency pairs.
    assert_eq!(cert.obligations.len(), 7);
    assert!(cert.obligations.iter().all(|o| o.proved()));
    // the record is signed and re-verifiable.
    assert!(
        mediator_audit::verify(&cert.record).is_ok(),
        "signed record must verify"
    );
    // it carries one receipt per obligation + the certification event.
    assert_eq!(cert.record.entries.len(), 7 + 1);
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
fn consistency_failure_without_findable_witness_falls_back_to_refused() {
    // The mock claims `consistent_0_1` is Unknown, but in the REAL complete_pact
    // clauses 0 (clean) and 1 (damaged) are pairwise-exclusive, so the bounded
    // witness search finds NO concrete clash. Honesty: rather than fabricate a
    // witness, we fall back to REFUSED with the consistency gap named.
    let prover = MockProver::all_proved().with("consistent_0_1", Verdict::Unknown);
    let cert = certify_pact(&complete_pact(), &prover);
    assert!(!cert.certified());
    assert!(
        cert.witness().is_none(),
        "no concrete witness exists for exclusive guards"
    );
    let gap = cert.gap().unwrap();
    assert_eq!(gap.obligation, "consistent_0_1");
    assert!(gap.plain.contains("clean"));
    assert!(gap.plain.contains("damaged"));
    assert!(gap.plain.contains("CLASH"));
}

/// A genuinely overlapping pact (two clauses fire in the same world demanding
/// different awards): the gate reports the consistency pair Unknown, and the
/// crate EXHIBITS the concrete witness — the third trichotomy leg, all offline.
#[test]
fn inconsistent_pact_exhibits_concrete_witness() {
    let pact = inconsistent_pact();
    // The clashing pair is (1, 2): `damaged` (notice≥30 ∧ stain → 90000) and
    // `late_overlap` (notice≥30 → 100000) both fire when notice≥30 ∧ stain.
    let prover = MockProver::all_proved().with("consistent_1_2", Verdict::Unknown);
    let cert = certify_pact(&pact, &prover);

    assert!(!cert.certified(), "an overlapping pact must not certify");
    assert!(cert.gap().is_none(), "this is INCONSISTENT, not a coverage gap");
    let w = cert.witness().expect("a concrete witness world");
    assert_eq!(w.clause_i_name, "damaged");
    assert_eq!(w.clause_j_name, "late_overlap");
    assert_eq!(w.bool_assignment.get("stain_is_damage"), Some(&true));
    assert!(*w.int_assignment.get("notice_days").unwrap() >= 30);
    assert_eq!(w.award_i_cents, 90000);
    assert_eq!(w.award_j_cents, 100000);

    // The witness re-checks independently (no Isabelle needed).
    assert!(
        crate::recheck_witness(&pact, w),
        "the exhibited witness must re-verify"
    );

    // It is recorded in the signed, verifiable chain as `pact_inconsistent`,
    // carrying the witness.
    assert!(mediator_audit::verify(&cert.record).is_ok());
    let last = cert.record.entries.last().unwrap();
    assert_eq!(last.kind, "pact_inconsistent");
    assert_eq!(last.detail["non_contradictory"], serde_json::json!(false));
    assert!(last.detail["witness_plain"]
        .as_str()
        .unwrap()
        .contains("90000 vs 100000"));

    // The rendering surfaces it in the spec's plain shape.
    let rendered = cert.render();
    assert!(rendered.contains("INCONSISTENT (witnessed)."));
    assert!(rendered.contains("stain_is_damage is true"));
    assert!(rendered.contains("90000 vs 100000"));
}

/// A pact that is rejected at AUTHORING time (the firewall fires before any
/// gate): a clause guard branches only on the integer dial — a value-call in
/// disguise. The certificate is `RejectedAtAuthoring`, with no obligations run,
/// and a signed record that says so.
#[test]
fn decidable_guard_is_rejected_at_authoring_before_gate() {
    // `complete_pact` with one extra clause whose guard is a pure dial.
    let mut pact = complete_pact();
    pact.clauses.push(Clause {
        name: "pure_dial".into(),
        guard: nd_lt(7), // notice_days < 7 — no crux, decidable shape
        outcome: award(50000),
    });

    // A prover that PANICS if ever called — proving the firewall short-circuits
    // before the gate.
    struct NeverProver;
    impl Prover for NeverProver {
        fn check(&self, _p: &str, _o: &[Obligation]) -> HashMap<String, Verdict> {
            panic!("the gate must never run for a pact rejected at authoring");
        }
    }

    let cert = certify_pact(&pact, &NeverProver);
    assert!(!cert.certified());
    let reasons = cert
        .rejected_reasons()
        .expect("authoring-time rejection reasons");
    assert!(
        reasons.iter().any(|r| r.contains("pure_dial")
            && r.contains("value-call disguised as a guard")),
        "reasons: {reasons:?}"
    );
    // No obligations were generated; the signed record still verifies.
    assert!(cert.obligations.is_empty());
    assert!(mediator_audit::verify(&cert.record).is_ok());
    assert_eq!(
        cert.record.entries.last().unwrap().kind,
        "pact_rejected_at_authoring"
    );
    let rendered = cert.render();
    assert!(rendered.contains("REJECTED AT AUTHORING"));
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
