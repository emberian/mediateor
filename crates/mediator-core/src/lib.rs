//! `mediator-core` — the kernel brain.
//!
//! Owns the *reduction*: turns a `Dispute` into Isabelle/HOL theory source +
//! named proof obligations (it generates the `.thy`), calls a `dyn Prover` to
//! gate them, interprets the verdicts, runs fair division via a
//! `dyn FairDivider`, and assembles the `Analysis` plus the hash-chained
//! receipt ledger. Generic over the trait seams — wired to concrete impls by
//! the demo.
//!
//! Motto: *model proposes, prover disposes.* The kernel only ever asserts what
//! the host certifies. The crux — the genuinely human predicate — is isolated
//! and handed back, never decided here.

pub mod codegen;
pub mod render;
pub mod receipts;

use mediator_types::{
    Analysis, Conflict, Dispute, FairDivider, Formula, PartyId, Prover, Receipt, Term, Verdict,
};
use serde_json::json;
use std::collections::HashMap;

use codegen::{build_obligations, build_preamble};
use receipts::ReceiptChain;
use render::{formula_to_english, money};

pub use codegen::standalone_theory;

/// Load a `Dispute` from a JSON scenario file.
pub fn load_dispute(path: &str) -> anyhow::Result<Dispute> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading dispute {path}: {e}"))?;
    let dispute: Dispute = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parsing dispute {path}: {e}"))?;
    Ok(dispute)
}

/// Analyze a dispute end to end. The single entry point the demo calls.
///
/// Steps (each emits a receipt):
///   1. build_preamble   — generate the shared Isabelle theory text
///   2. verify_ledger     — gate both refund worlds
///   3. refute_overclaim  — gate the over-claim refutation
///   4. isolate_crux      — gate the crux iff + the (undecidable) crux predicate
///   5. fair_division     — certified-fair settlement options
pub fn analyze(
    dispute: &Dispute,
    prover: &dyn Prover,
    fair: &dyn FairDivider,
) -> (Analysis, Vec<Receipt>) {
    let mut chain = ReceiptChain::new();
    let mut analysis = Analysis::default();

    // ── 1. codegen ──────────────────────────────────────────────────────
    let preamble = build_preamble(dispute);
    let obligations = build_obligations(dispute);
    chain.append(
        "build_preamble",
        json!({
            "theory": "Mediator_Probe",
            "n_axioms": dispute.stipulated.len(),
            "n_obligations": obligations.len(),
            "obligations": obligations.iter().map(|o| &o.name).collect::<Vec<_>>(),
        }),
        None,
    );

    // ── gate everything in one isolated prover pass ─────────────────────
    let verdicts: HashMap<String, Verdict> = prover.check(&preamble, &obligations);
    let v = |name: &str| verdicts.get(name).cloned().unwrap_or(Verdict::Unknown);

    // ── shared core: the bigger-than-the-fight common ground ────────────
    // Stipulated facts, rendered deterministically in plain English.
    for fact in &dispute.stipulated {
        analysis
            .shared_core
            .push(format!("You both stipulate: {}", formula_to_english(fact)));
    }
    // Undisputed ledger items are shared ground too.
    for item in &dispute.ledger.items {
        if !item.disputed {
            analysis.shared_core.push(format!(
                "You both agree the {} ({}) is not in dispute.",
                item.label.to_lowercase(),
                money(item.amount_cents)
            ));
        }
    }
    analysis.shared_core.push(format!(
        "The amount at stake is {}.",
        money(dispute.ledger.deposit_cents)
    ));

    // ── 2. verify_ledger ────────────────────────────────────────────────
    let damage_v = v("refund_damage_world");
    let wear_v = v("refund_wear_world");
    let refund_damage = world_refund(dispute, true);
    let refund_wear = world_refund(dispute, false);

    chain.append(
        "verify_ledger",
        json!({
            "refund_if_damage_cents": refund_damage,
            "refund_if_wear_cents": refund_wear,
            "damage_world": fmt_verdict(&damage_v),
            "wear_world": fmt_verdict(&wear_v),
        }),
        Some(combine(&damage_v, &wear_v)),
    );

    if proved(&damage_v) && proved(&wear_v) {
        analysis.ledger_findings.push(match crux_gloss(dispute) {
            Some(g) => format!(
                "If {g}: the certified amount is {}. If not: {}.",
                money(refund_damage),
                money(refund_wear)
            ),
            None => format!(
                "The certified amount is {} or {}, depending on the one open question.",
                money(refund_damage),
                money(refund_wear)
            ),
        });
        // The rest hinges on the crux; the headline figure is the
        // crux-does-not-hold world. Honest: it is the *contingent* figure.
        analysis.ledger_refund_cents = Some(refund_wear);
    } else {
        // Do not overclaim a number the host could not certify.
        analysis
            .ledger_findings
            .push("The refund ledger could not be certified by the host.".to_string());
        analysis.ledger_refund_cents = None;
    }

    // ── 3. refute_overclaim ─────────────────────────────────────────────
    let overclaim_v = v("over_claim_refuted");
    let itemized: i64 = dispute.ledger.items.iter().map(|i| i.amount_cents).sum();
    let claimed = claimed_total_value(dispute);
    chain.append(
        "refute_overclaim",
        json!({
            "claimed_total_cents": claimed,
            "itemized_total_cents": itemized,
            "verdict": fmt_verdict(&overclaim_v),
        }),
        Some(overclaim_v.clone()),
    );

    if proved(&overclaim_v) {
        if let Some(c) = claimed {
            analysis.ledger_findings.push(format!(
                "The stated figure of {} is not what the itemization supports — \
                 the items total {}.",
                money(c),
                money(itemized)
            ));
            // Framed kindly in `dissolved`: a number to correct, not a lie.
            analysis.dissolved.push(format!(
                "The {}-vs-{} gap is a number to correct, not a deception: \
                 the items simply add up to {}.",
                money(c),
                money(itemized),
                money(itemized)
            ));
        }
    }

    // ── 4. isolate_crux ─────────────────────────────────────────────────
    let crux_iff_v = v("crux_iff");
    let crux_damage_v = v("crux_is_damage");
    let crux_wear_v = v("crux_is_wear");
    chain.append(
        "isolate_crux",
        json!({
            "crux_iff": fmt_verdict(&crux_iff_v),
            "crux_is_damage": fmt_verdict(&crux_damage_v),
            "crux_is_wear": fmt_verdict(&crux_wear_v),
        }),
        Some(crux_iff_v.clone()),
    );

    // The kernel only declares a crux when it has *proved the reduction* (the
    // iff) AND honestly *cannot decide* the predicate either way.
    if proved(&crux_iff_v) && undecided(&crux_damage_v) && undecided(&crux_wear_v) {
        analysis.crux = Some(match crux_gloss(dispute) {
            Some(g) => format!(
                "The whole dispute reduces to one question: whether {g}. That is the single \
                 fact the kernel cannot — and will not — decide for you. Everything else above \
                 follows from your answer to it."
            ),
            None => "The whole dispute reduces to a single contested point, which the kernel \
                     cannot — and will not — decide for you. Everything else above follows \
                     from your answer to it."
                .to_string(),
        });
    } else if proved(&crux_damage_v) || proved(&crux_wear_v) {
        // The host actually settled it; report that instead of a false "Unknown".
        analysis.crux = Some(
            "The host was able to settle the contested question from the stipulated facts; it \
             is not, in this dispute, the open crux."
                .to_string(),
        );
    }

    // ── genuine conflicts: the irreducible knot ─────────────────────────
    if let Some(conflict) = crux_conflict(dispute) {
        analysis.genuine_conflicts.push(conflict);
    }

    // ── 5. fair_division ────────────────────────────────────────────────
    let party_ids: Vec<PartyId> = dispute.parties.iter().map(|p| p.id.clone()).collect();
    let settlements = fair.divide(&dispute.contested_items, &dispute.valuations, &party_ids);
    chain.append(
        "fair_division",
        json!({
            "n_settlements": settlements.len(),
            "labels": settlements.iter().map(|s| &s.label).collect::<Vec<_>>(),
            "envy_free": settlements.iter().all(|s| s.envy_free),
        }),
        None,
    );
    analysis.settlements = settlements;

    (analysis, chain.into_vec())
}

// ───────────────────────────── helpers ──────────────────────────────────

fn proved(v: &Verdict) -> bool {
    matches!(v, Verdict::Proved)
}

/// Undecided = the honest "we could not settle it" verdict (Unknown), which for
/// the crux predicate is the *informative* answer.
fn undecided(v: &Verdict) -> bool {
    matches!(v, Verdict::Unknown)
}

fn fmt_verdict(v: &Verdict) -> String {
    match v {
        Verdict::Proved => "Proved".to_string(),
        Verdict::Refuted => "Refuted".to_string(),
        Verdict::Unknown => "Unknown".to_string(),
        Verdict::Error(e) => format!("Error: {e}"),
    }
}

/// Combine two verdicts: Proved only if both proved; Error if either errored;
/// else Unknown. Used to summarize the ledger step.
fn combine(a: &Verdict, b: &Verdict) -> Verdict {
    match (a, b) {
        (Verdict::Error(e), _) | (_, Verdict::Error(e)) => Verdict::Error(e.clone()),
        (Verdict::Proved, Verdict::Proved) => Verdict::Proved,
        _ => Verdict::Unknown,
    }
}

/// The refund in a given world. Damage world = deposit - all items; wear world
/// = deposit - undisputed items only.
fn world_refund(dispute: &Dispute, damage: bool) -> i64 {
    let deposit = dispute.ledger.deposit_cents;
    let deductions: i64 = dispute
        .ledger
        .items
        .iter()
        .filter(|i| damage || !i.disputed)
        .map(|i| i.amount_cents)
        .sum();
    deposit - deductions
}

/// The party-claimed deduction total, if some active claim pins `claimed_total`.
fn claimed_total_value(dispute: &Dispute) -> Option<i64> {
    for claim in &dispute.claims {
        if !claim.active {
            continue;
        }
        if let Formula::Eq(lhs, rhs) = &claim.formula {
            let pin = match (lhs, rhs) {
                (Term::App(n, a), Term::IntLit(v)) if a.is_empty() && n == "claimed_total" => {
                    Some(*v)
                }
                (Term::IntLit(v), Term::App(n, a)) if a.is_empty() && n == "claimed_total" => {
                    Some(*v)
                }
                _ => None,
            };
            if pin.is_some() {
                return pin;
            }
        }
    }
    None
}

/// The crux predicate name: the right-hand side of the first stipulated `Iff`
/// whose RHS is a nullary atom (e.g. `stain_is_damage`, `work_met_spec`).
fn crux_predicate(dispute: &Dispute) -> Option<String> {
    for f in &dispute.stipulated {
        if let Formula::Iff(_, rhs) = f {
            if let Formula::Atom(Term::App(name, args)) = &**rhs {
                if args.is_empty() {
                    return Some(name.clone());
                }
            }
        }
    }
    None
}

/// The plain-English gloss of the crux predicate, taken from whichever party's
/// signature declares it. This is what makes the prose dispute-agnostic.
fn crux_gloss(dispute: &Dispute) -> Option<String> {
    let name = crux_predicate(dispute)?;
    for p in &dispute.parties {
        for s in &p.signature {
            if s.name == name && !s.gloss.is_empty() {
                return Some(s.gloss.clone());
            }
        }
    }
    None
}

/// The genuine conflict over the crux predicate: the two active claims that
/// assert `P` and `¬P` over it. Dispute-agnostic — works for any scenario.
fn crux_conflict(dispute: &Dispute) -> Option<Conflict> {
    let pred = crux_predicate(dispute)?;
    let mut pos: Option<&mediator_types::Claim> = None;
    let mut neg: Option<&mediator_types::Claim> = None;
    for c in &dispute.claims {
        if !c.active {
            continue;
        }
        match &c.formula {
            Formula::Atom(Term::App(n, a)) if a.is_empty() && *n == pred => pos = Some(c),
            Formula::Not(inner) => {
                if let Formula::Atom(Term::App(n, a)) = &**inner {
                    if a.is_empty() && *n == pred {
                        neg = Some(c);
                    }
                }
            }
            _ => {}
        }
    }
    match (pos, neg) {
        (Some(p), Some(n)) => {
            let desc = match crux_gloss(dispute) {
                Some(g) => format!(
                    "Whether {g} — a real disagreement of fact and judgment, not just \
                     different words."
                ),
                None => "The one contested point — a real disagreement of fact and judgment, \
                         not just different words."
                    .to_string(),
            };
            Some(Conflict {
                description: desc,
                parties: vec![n.party.clone(), p.party.clone()],
                claim_ids: vec![n.id.clone(), p.id.clone()],
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::Obligation;

    /// A scriptable in-crate prover for fast unit tests. Maps obligation name →
    /// verdict; defaults to Unknown so omissions are honest, not optimistic.
    struct TestProver {
        verdicts: HashMap<String, Verdict>,
    }
    impl Prover for TestProver {
        fn check(&self, _preamble: &str, obligations: &[Obligation]) -> HashMap<String, Verdict> {
            obligations
                .iter()
                .map(|o| {
                    (
                        o.name.clone(),
                        self.verdicts.get(&o.name).cloned().unwrap_or(Verdict::Unknown),
                    )
                })
                .collect()
        }
    }

    /// A trivial fair divider that produces one envy-free settlement.
    struct TestDivider;
    impl FairDivider for TestDivider {
        fn divide(
            &self,
            _items: &[mediator_types::ContestedItem],
            _valuations: &[mediator_types::Valuation],
            parties: &[PartyId],
        ) -> Vec<mediator_types::Settlement> {
            vec![mediator_types::Settlement {
                label: "test split".to_string(),
                allocations: vec![],
                splits: vec![],
                party_points: parties.iter().map(|p| (p.clone(), 50.0)).collect(),
                envy_free: true,
                equitable: true,
                pareto_optimal: true,
                explanation: "stub".to_string(),
            }]
        }
    }

    fn expected_verdicts() -> HashMap<String, Verdict> {
        let mut m = HashMap::new();
        m.insert("refund_damage_world".into(), Verdict::Proved);
        m.insert("refund_wear_world".into(), Verdict::Proved);
        m.insert("over_claim_refuted".into(), Verdict::Proved);
        m.insert("crux_iff".into(), Verdict::Proved);
        m.insert("crux_is_damage".into(), Verdict::Unknown);
        m.insert("crux_is_wear".into(), Verdict::Unknown);
        m
    }

    fn roommate() -> Dispute {
        load_dispute("../../scenarios/roommate.json").unwrap()
    }

    #[test]
    fn analyze_assembles_expected_analysis() {
        let d = roommate();
        let prover = TestProver { verdicts: expected_verdicts() };
        let (a, receipts) = analyze(&d, &prover, &TestDivider);

        // ledger: wear-world best case is the headline
        assert_eq!(a.ledger_refund_cents, Some(105000));
        assert!(a
            .ledger_findings
            .iter()
            .any(|f| f.contains("$1050.00") && f.contains("$750.00")));
        assert!(a
            .ledger_findings
            .iter()
            .any(|f| f.contains("$500.00") && f.contains("$450.00")));

        // crux isolated and handed back
        let crux = a.crux.as_ref().expect("crux should be set");
        assert!(crux.contains("will not"));
        assert!(crux.contains("ordinary wear"));

        // genuine conflict over stain_is_damage, robin & sam
        assert_eq!(a.genuine_conflicts.len(), 1);
        let c = &a.genuine_conflicts[0];
        assert!(c.parties.contains(&"robin".to_string()));
        assert!(c.parties.contains(&"sam".to_string()));
        assert!(c.claim_ids.contains(&"r1".to_string()));
        assert!(c.claim_ids.contains(&"s1".to_string()));

        // dissolved framing is kind
        assert!(a.dissolved.iter().any(|s| s.contains("not a deception")));

        // shared core mentions the cleaning agreement
        assert!(a
            .shared_core
            .iter()
            .any(|s| s.contains("professional cleaning") && s.contains("$150.00")));

        // settlements wired through
        assert_eq!(a.settlements.len(), 1);

        // receipts chain is intact and covers all five steps
        assert!(receipts::verify_chain(&receipts).is_ok());
        let ops: Vec<&str> = receipts.iter().map(|r| r.op.as_str()).collect();
        assert_eq!(
            ops,
            vec![
                "build_preamble",
                "verify_ledger",
                "refute_overclaim",
                "isolate_crux",
                "fair_division"
            ]
        );
    }

    #[test]
    fn unknown_ledger_is_not_overclaimed() {
        let d = roommate();
        // Host fails to certify the ledger; we must NOT report a refund number.
        let mut verdicts = expected_verdicts();
        verdicts.insert("refund_wear_world".into(), Verdict::Unknown);
        let prover = TestProver { verdicts };
        let (a, _) = analyze(&d, &prover, &TestDivider);
        assert_eq!(a.ledger_refund_cents, None);
        assert!(a
            .ledger_findings
            .iter()
            .any(|f| f.contains("could not be certified")));
    }

    #[test]
    fn decided_crux_is_not_reported_as_open() {
        let d = roommate();
        // If the host actually proves the predicate, the kernel must not pretend
        // it is the open crux.
        let mut verdicts = expected_verdicts();
        verdicts.insert("crux_is_damage".into(), Verdict::Proved);
        let prover = TestProver { verdicts };
        let (a, _) = analyze(&d, &prover, &TestDivider);
        let crux = a.crux.unwrap();
        assert!(crux.contains("not, in this dispute, the open crux"));
    }
}
