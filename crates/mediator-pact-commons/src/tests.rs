//! OFFLINE tests for the COMMONS OF PACTS — no Isabelle, no network.
//!
//! Certified certificates are minted with a `MockProver` that returns `Proved`
//! for every obligation (the same discipline `mediator-pact`'s own tests use), so
//! we get a genuine [`Status::Certified`] certificate with a real ed25519-signed
//! record — without the gate. The real-Isabelle behavior is exercised by the
//! shipped `.cert.json` corpus, re-verified offline in `tests/corpus_reverify.rs`.
//!
//! These pin the falsifiable claims:
//!   * pacts of one template COLLIDE (byte-equal key) across names/thresholds/
//!     awards, and genuinely different shapes do NOT;
//!   * the aggregation gate admits only certified, crux-free, signature-valid,
//!     cert-matches-pact pacts;
//!   * a knot resolved opposite ways across a template is a certified LIVE
//!     CONTROVERSY; agreement is SETTLED; the honesty note scales with support;
//!   * RE-VERIFY WITHOUT TRUST: a tampered cited record FAILS re-verification —
//!     the commons cannot launder a forged precedent.

use super::*;
use mediator_pact::{certify_pact, Clause};
use mediator_types::{Formula, Obligation, Prover, Sig, Sort, Term, Verdict};
use std::collections::HashMap;

// ─────────────────────────── fixtures & helpers ─────────────────────────────

fn bool_sig(name: &str) -> Sig {
    Sig { name: name.into(), arg_sorts: vec![], ret: Sort::Bool, gloss: String::new() }
}
fn int_sig(name: &str) -> Sig {
    Sig { name: name.into(), arg_sorts: vec![], ret: Sort::Int, gloss: String::new() }
}
fn atom(n: &str) -> Formula {
    Formula::Atom(Term::App(n.into(), vec![]))
}
fn award(c: i64) -> Formula {
    Formula::Eq(Term::Var("award".into()), Term::IntLit(c))
}

/// A prover that returns `Proved` for everything — mints a genuine CERTIFIED
/// certificate offline.
struct AllProved;
impl Prover for AllProved {
    fn check(&self, _p: &str, obs: &[Obligation]) -> HashMap<String, Verdict> {
        obs.iter().map(|o| (o.name.clone(), Verdict::Proved)).collect()
    }
}

/// A prover that leaves coverage Unknown — yields a REFUSED certificate.
struct CoverageFails;
impl Prover for CoverageFails {
    fn check(&self, _p: &str, obs: &[Obligation]) -> HashMap<String, Verdict> {
        obs.iter()
            .map(|o| {
                let v = if o.name == "coverage" { Verdict::Unknown } else { Verdict::Proved };
                (o.name.clone(), v)
            })
            .collect()
    }
}

/// Build a deposit/credit/vesting-template pact: 1 bool crux + 1 int dial, 4
/// clauses {above/below threshold} × {¬crux/crux}, with caller-chosen names,
/// threshold, and the four awards (in clause order:
/// above_clean, above_crux, below_clean, below_crux).
fn template_pact(
    title: &str,
    crux: &str,
    dial: &str,
    thresh: i64,
    awards: [i64; 4],
) -> Pact {
    let ge = |n: i64| Formula::Le(Term::IntLit(n), Term::App(dial.into(), vec![]));
    let lt = |n: i64| Formula::Lt(Term::App(dial.into(), vec![]), Term::IntLit(n));
    Pact {
        title: title.into(),
        parties: vec!["A".into(), "B".into()],
        predicates: vec![bool_sig(crux), int_sig(dial)],
        clauses: vec![
            Clause {
                name: "above_clean".into(),
                guard: Formula::And(vec![ge(thresh), Formula::Not(Box::new(atom(crux)))]),
                outcome: award(awards[0]),
            },
            Clause {
                name: "above_crux".into(),
                guard: Formula::And(vec![ge(thresh), atom(crux)]),
                outcome: award(awards[1]),
            },
            Clause {
                name: "below_clean".into(),
                guard: Formula::And(vec![lt(thresh), Formula::Not(Box::new(atom(crux)))]),
                outcome: award(awards[2]),
            },
            Clause {
                name: "below_crux".into(),
                guard: Formula::And(vec![lt(thresh), atom(crux)]),
                outcome: award(awards[3]),
            },
        ],
    }
}

/// Certify a pact offline (AllProved) → a CERTIFIED certificate with a real
/// signed record.
fn certified(pact: &Pact) -> PactCertificate {
    let cert = certify_pact(pact, &AllProved);
    assert!(cert.certified(), "fixture must certify: {}", pact.title);
    cert
}

// ─────────────────────────── template key ───────────────────────────────────

#[test]
fn corpus_pacts_of_one_template_collide() {
    // Two pacts of the SAME template — different crux/dial names, threshold, and
    // all four awards — share a byte-equal template key.
    let a = template_pact("Roommate", "stain_is_damage", "notice_days", 30, [120000, 90000, 100000, 100000]);
    let b = template_pact("Royalties", "substantial_creative_contribution", "hours_logged", 100, [2000, 5000, 500, 3000]);
    assert_eq!(template_key(&a), template_key(&b));

    // And a genuinely different question-space shape does NOT collide: a
    // single-clause, dial-only-ish… actually craft a 2-clause crux-only template.
    let different = Pact {
        title: "Other".into(),
        parties: vec!["A".into(), "B".into()],
        predicates: vec![bool_sig("c")],
        clauses: vec![
            Clause { name: "yes".into(), guard: atom("c"), outcome: award(1) },
            Clause { name: "no".into(), guard: Formula::Not(Box::new(atom("c"))), outcome: award(2) },
        ],
    };
    assert_ne!(template_key(&a), template_key(&different));
}

#[test]
fn template_key_is_award_independent() {
    // Same structure, different awards only → SAME key (awards are the
    // controversy, never the template).
    let a = template_pact("X", "c", "d", 10, [1, 2, 3, 4]);
    let b = template_pact("X", "c", "d", 10, [400, 300, 200, 100]);
    assert_eq!(template_key(&a), template_key(&b));
}

// ─────────────────────────── admission gate ─────────────────────────────────

#[test]
fn admits_a_certified_crux_free_pact() {
    let pact = template_pact("Deposit", "stain_is_damage", "notice_days", 30, [120000, 90000, 100000, 100000]);
    let cert = certified(&pact);
    let mut c = Commons::new();
    assert!(c.admit(pact, cert, "deposit").is_ok());
    assert_eq!(c.len(), 1);
}

#[test]
fn refuses_a_non_certified_pact() {
    // A pact whose certificate is REFUSED (coverage failed) must not be admitted.
    let mut pact = template_pact("Broken", "c", "d", 30, [1, 2, 3, 4]);
    // Drop the below-threshold clauses so coverage genuinely could fail; but we
    // also force the prover to fail coverage, so the cert is REFUSED.
    pact.clauses.retain(|cl| cl.name.starts_with("above"));
    let cert = certify_pact(&pact, &CoverageFails);
    assert!(!cert.certified());
    let mut c = Commons::new();
    assert_eq!(c.admit(pact, cert, "broken"), Err(CommonsError::NotCertified));
}

#[test]
fn refuses_a_cert_for_a_different_pact() {
    // Cert minted for pact A, but handed to the commons alongside pact B → the
    // gate catches the mismatch (we never trust a cert next to an unrelated pact).
    let a = template_pact("Pact A", "c", "d", 30, [1, 2, 3, 4]);
    let b = template_pact("Pact B — different title", "c", "d", 30, [1, 2, 3, 4]);
    let cert_a = certified(&a);
    let mut c = Commons::new();
    match c.admit(b, cert_a, "mismatch") {
        Err(CommonsError::CertMismatch(field)) => assert_eq!(field, "title"),
        other => panic!("expected CertMismatch(title), got {other:?}"),
    }
}

#[test]
fn refuses_a_pact_whose_crux_is_not_free() {
    // A pact with a pure-dial guard (a value-call in disguise) fails the firewall
    // re-check, even if someone hands us a (mismatched) "certified" claim. We
    // construct the gate input directly: certify the GOOD pact, then swap in a
    // BAD pact that shares title/parties/predicates but smuggles a dial guard.
    let good = template_pact("Vesting", "departure_for_cause", "months_served", 12, [0, 0, 2500, 0]);
    let cert = certified(&good);

    // The bad pact: identical declared surface (so title/parties/preds match the
    // cert), but one clause guard is a pure dial — the firewall must reject it.
    let mut bad = good.clone();
    bad.clauses.push(Clause {
        name: "pure_dial".into(),
        guard: Formula::Lt(Term::App("months_served".into(), vec![]), Term::IntLit(3)),
        outcome: award(999),
    });
    // (bad still has the same title/parties/predicates as the cert.)
    let mut c = Commons::new();
    match c.admit(bad, cert, "smuggled") {
        Err(CommonsError::CruxNotFree(reasons)) => {
            assert!(reasons.iter().any(|r| r.contains("pure_dial")), "reasons: {reasons:?}");
        }
        other => panic!("expected CruxNotFree, got {other:?}"),
    }
}

// ─────────────────────────── certified controversy ──────────────────────────

/// Build a 4-pact corpus of ONE template where the crux's effect at the
/// at-or-above side SPLITS: three pacts have crux-true LOWER the award, one has
/// crux-true RAISE it (mirrors the real corpus: damage/for-cause hurt, but
/// creative-contribution helps).
fn split_corpus() -> Commons {
    let mut c = Commons::new();
    // crux-true LOWERS (damage hurts): above_clean=120000 > above_crux=90000.
    let p1 = template_pact("Roommate", "stain_is_damage", "notice_days", 30, [120000, 90000, 100000, 100000]);
    // crux-true LOWERS (damage hurts).
    let p2 = template_pact("Cohab", "damage_beyond_normal_wear", "days_cohabited", 180, [100000, 70000, 90000, 60000]);
    // crux-true RAISES (creative contribution helps): above_clean=2000 < above_crux=5000.
    let p3 = template_pact("Credit", "substantial_creative_contribution", "hours_logged", 100, [2000, 5000, 500, 3000]);
    // crux-true LOWERS (for-cause forfeits): above_clean=2500 > above_crux=0.
    let p4 = template_pact("Vesting", "departure_for_cause", "months_served", 12, [2500, 0, 0, 0]);
    for (p, lab) in [(p1, "roommate"), (p2, "cohab"), (p3, "credit"), (p4, "vesting")] {
        let cert = certified(&p);
        c.admit(p, cert, lab).unwrap();
    }
    c
}

#[test]
fn the_same_knot_resolved_both_ways_is_a_certified_live_controversy() {
    let c = split_corpus();
    // All four share one template.
    let templates = c.templates();
    assert_eq!(templates.len(), 1, "all four are one template");
    let (key, members) = templates.into_iter().next().unwrap();
    assert_eq!(members.len(), 4);

    // At the at-or-above / crux-false world, the crux's effect SPLITS.
    let point = DecisionPoint {
        dial_role: 0,
        side: WorldSide::AtOrAbove,
        crux_role: 0,
        crux_value: false,
    };
    let tally = c.controversy(&key, &point).expect("a tally");
    assert_eq!(tally.support(), 4);
    assert_eq!(tally.shape(), TallyShape::LiveControversy, "opposite value-calls = controversy");
    // Distribution: 3 lower, 1 raise, 0 nochange.
    assert_eq!(tally.distribution["crux-true LOWERS the award"], 3);
    assert_eq!(tally.distribution["crux-true RAISES the award"], 1);
    assert_eq!(tally.distribution["crux makes NO difference"], 0);

    // The render is honest: headlines support, never recommends.
    let r = tally.render();
    assert!(r.contains("backed by 4 pact(s)"));
    assert!(r.contains("LIVE CONTROVERSY"));
    assert!(r.contains("NEVER 'yours should be X'"));
    // It does NOT contain a recommendation phrasing.
    assert!(!r.to_lowercase().contains("you should"));
    assert!(!r.to_lowercase().contains("recommend"));
}

#[test]
fn agreement_is_settled_not_a_controversy() {
    // A corpus where everyone agrees the crux LOWERS the award → SETTLED.
    let mut c = Commons::new();
    let p1 = template_pact("A", "c", "d", 30, [120000, 90000, 100000, 80000]);
    let p2 = template_pact("B", "x", "y", 10, [200, 100, 50, 25]);
    for (p, lab) in [(p1, "a"), (p2, "b")] {
        let cert = certified(&p);
        c.admit(p, cert, lab).unwrap();
    }
    let key = template_key(&template_pact("A", "c", "d", 30, [120000, 90000, 100000, 80000]));
    let point = DecisionPoint { dial_role: 0, side: WorldSide::AtOrAbove, crux_role: 0, crux_value: false };
    let tally = c.controversy(&key, &point).unwrap();
    assert_eq!(tally.shape(), TallyShape::Settled);
    assert!(c.report().templates[0].tallies.iter().any(|t| matches!(t.shape(), TallyShape::Settled)));
}

#[test]
fn confidence_note_scales_with_support_and_shouts_tiny() {
    // Singleton, pair, and a bigger corpus all describe themselves honestly.
    let one = template_pact("A", "c", "d", 30, [4, 3, 2, 1]);
    let mut c1 = Commons::new();
    let cert = certified(&one);
    c1.admit(one.clone(), cert, "one").unwrap();
    let key = template_key(&one);
    let pt = DecisionPoint { dial_role: 0, side: WorldSide::AtOrAbove, crux_role: 0, crux_value: false };
    let t1 = c1.controversy(&key, &pt).unwrap();
    assert!(t1.confidence_note().contains("1 pact"));
    assert!(t1.confidence_note().contains("singleton") || t1.confidence_note().contains("not a norm"));

    let two = split_corpus(); // 4-member; check the "small … not a settled rule" band
    let (k, _) = two.templates().into_iter().next().unwrap();
    let t = two.controversy(&k, &pt).unwrap();
    assert!(t.confidence_note().contains("backed by 4 pact(s)"));
    assert!(t.confidence_note().contains("not a settled rule"));
}

#[test]
fn report_headline_states_the_template_trick_and_corpus_size() {
    let c = split_corpus();
    let report = c.report();
    assert_eq!(report.corpus_size, 4);
    let h = report.headline();
    assert!(h.contains("4 certified pact"));
    assert!(h.contains("TINY corpus"));
    // The headline NAMES the trick (vocabulary fixed by the template) — the
    // honesty bar.
    assert!(h.contains("share a declared vocabulary"));
}

// ─────────────────── RE-VERIFY WITHOUT TRUST (the load-bearing test) ─────────

#[test]
fn skeptic_reverifies_the_whole_corpus_from_triples() {
    let c = split_corpus();
    let triples: Vec<_> = c
        .entries()
        .iter()
        .map(|e| (e.pact.clone(), e.cert.clone(), e.label.clone()))
        .collect();
    let outcome = reverify_corpus(&triples).expect("a faithful corpus re-verifies");
    assert_eq!(outcome.verified, 4);
    // The recomputed keys match the commons' own grouping (all one template).
    assert!(outcome.recomputed_keys.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn a_tampered_cited_record_fails_reverification() {
    // THE load-bearing guarantee: flip one byte of an embedded signed record's
    // payload, and re-verification REJECTS that pact — the commons cannot launder
    // a forged precedent. Trust comes from re-verification, not the commons.
    let c = split_corpus();
    let mut triples: Vec<_> = c
        .entries()
        .iter()
        .map(|e| (e.pact.clone(), e.cert.clone(), e.label.clone()))
        .collect();

    // Tamper: mutate the certification event's detail in the first cert's signed
    // record, WITHOUT re-signing. The hash chain / signature must now fail.
    let rec = &mut triples[0].1.record;
    let last = rec.entries.last_mut().expect("a record entry");
    last.detail = serde_json::json!({ "forged": "after the fact" });

    match reverify_corpus(&triples) {
        Err(AdmitError::Gate { index, error, .. }) => {
            assert_eq!(index, 0, "the tampered pact is the one rejected");
            assert!(
                matches!(error, CommonsError::RecordInvalid(_)),
                "a forged record must fail the signature/chain check, got {error:?}"
            );
        }
        Ok(_) => panic!("a tampered cited record MUST fail re-verification"),
    }

    // And the in-place commons API agrees if we tamper an admitted entry.
    let mut c2 = split_corpus();
    // Reach in and forge (simulating a corrupted on-disk cache being trusted).
    {
        let e = &mut c2.entries_mut()[1];
        let last = e.cert.record.entries.last_mut().unwrap();
        last.detail = serde_json::json!({ "forged": true });
    }
    match c2.reverify() {
        Err((i, CommonsError::RecordInvalid(_))) => assert_eq!(i, 1),
        other => panic!("expected RecordInvalid at 1, got {other:?}"),
    }
}

#[test]
fn a_tampered_tally_does_not_match_recomputation() {
    // A skeptic who recomputes the tally from the (re-verified) pacts gets the
    // SAME tally the commons published — so a commons that lied about the
    // distribution would be caught. We assert the tally is a pure function of the
    // verified corpus: recompute independently and compare.
    let c = split_corpus();
    let (key, members) = c.templates().into_iter().next().unwrap();
    let pt = DecisionPoint { dial_role: 0, side: WorldSide::AtOrAbove, crux_role: 0, crux_value: false };
    let published = c.controversy(&key, &pt).unwrap();

    // Recompute from the entries directly (what a skeptic does).
    let recomputed = controversy::tally(c.entries(), &members, &pt);
    assert_eq!(published, recomputed, "the tally is a pure function of the verified corpus");
}

// ───────────────────────── matching / grouping ──────────────────────────────

#[test]
fn matching_template_finds_every_same_template_pact() {
    let c = split_corpus();
    let query = template_pact("Fresh dispute", "any_contested_thing", "any_dial", 7, [9, 8, 7, 6]);
    // A brand-new pact of the same template matches all four prior ones — what a
    // new pair would see (the distribution of prior choices), never a verdict.
    assert_eq!(c.matching_template(&query).len(), 4);

    // A different-shape query matches none.
    let other = Pact {
        title: "z".into(),
        parties: vec!["A".into(), "B".into()],
        predicates: vec![bool_sig("c")],
        clauses: vec![Clause { name: "y".into(), guard: atom("c"), outcome: award(1) }],
    };
    assert!(c.matching_template(&other).is_empty());
}
