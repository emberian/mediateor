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
    Analysis, Claim, Conflict, Dispute, FairDivider, Formula, PartyId, Prover, Receipt, Sig, Term,
    Verdict,
};
use serde_json::json;
use std::collections::HashMap;

use codegen::{build_obligations, build_preamble, crux_bridges};
use mediator_types::Crux;
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

    // ── dissolution: the categorical layer, live ────────────────────────
    // Before any clash is handed back as a genuine crux, ask the ontology
    // whether it is really two words for the same thing. Vocabulary mismatches
    // are dissolved here (with a plain-language note) and suppressed from the
    // genuine-conflict set below. This runs offline and deterministically.
    let dissolved_clashes = dissolve_vocabulary_clashes(dispute);
    let mut suppressed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for dc in &dissolved_clashes {
        suppressed.insert(dc.predicates.0.clone());
        suppressed.insert(dc.predicates.1.clone());
        analysis.dissolved.push(dc.note.clone());
    }
    if !dissolved_clashes.is_empty() {
        chain.append(
            "dissolve_vocabulary",
            json!({
                "n_dissolved": dissolved_clashes.len(),
                "aligned_pairs": dissolved_clashes
                    .iter()
                    .map(|d| [&d.predicates.0, &d.predicates.1])
                    .collect::<Vec<_>>(),
            }),
            None,
        );
    }

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

        // Multi-crux: when disputed items name a *controlling crux*, spell out
        // what each contested question costs on its own. This makes a dispute
        // with several independent deductions legible — each line is the swing
        // a single human answer produces, certified by the two-world arithmetic.
        let n_controlled = dispute
            .ledger
            .items
            .iter()
            .filter(|i| i.disputed && i.controlling_crux.is_some())
            .count();
        if n_controlled > 1 {
            for item in &dispute.ledger.items {
                if !item.disputed {
                    continue;
                }
                if let Some(pred) = &item.controlling_crux {
                    let q = predicate_gloss(dispute, pred)
                        .unwrap_or_else(|| pred.replace('_', " "));
                    analysis.ledger_findings.push(format!(
                        "The {} ({}) stands only if {} — otherwise it falls away.",
                        item.label.to_lowercase(),
                        money(item.amount_cents),
                        q
                    ));
                }
            }
        }
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
    // A real dispute reduces to a *set* of contested questions. For each crux
    // bridge k we proved `crux_iff_<k>` (the reduction) and asked the host the
    // predicate both ways (`crux_<k>_holds` / `crux_<k>_fails`). A bridge is a
    // genuine open crux iff the iff is Proved and BOTH directions are undecided.
    let bridges = crux_bridges(dispute);

    // Legacy back-compat verdicts (alias the first crux). The seeded single-crux
    // scenarios are summarized through these exact names, as before.
    let crux_iff_v = v("crux_iff");
    let crux_damage_v = v("crux_is_damage");
    let crux_wear_v = v("crux_is_wear");

    // Per-crux verdict resolvers. Crux 0 is *aliased* by the legacy obligation
    // names (`crux_iff` / `crux_is_damage` / `crux_is_wear`): a prover that only
    // returns the legacy names (the seeded scenarios + the in-crate fixtures)
    // still drives crux 0 identically, while real multi-crux runs get every
    // bridge via its `_<k>` names. We prefer a *decisive* verdict from either
    // source (Proved/Refuted/Error beats a defaulted Unknown).
    let iff_for = |k: usize| -> Verdict {
        let per = v(&format!("crux_iff_{k}"));
        if k == 0 { decisive(per, v("crux_iff")) } else { per }
    };
    let holds_for = |k: usize| -> Verdict {
        let per = v(&format!("crux_{k}_holds"));
        if k == 0 { decisive(per, v("crux_is_damage")) } else { per }
    };
    let fails_for = |k: usize| -> Verdict {
        let per = v(&format!("crux_{k}_fails"));
        if k == 0 { decisive(per, v("crux_is_wear")) } else { per }
    };

    // Per-crux verdict detail for the receipt.
    let mut crux_detail = serde_json::Map::new();
    for (k, _b) in bridges.iter().enumerate() {
        crux_detail.insert(format!("crux_iff_{k}"), json!(fmt_verdict(&iff_for(k))));
        crux_detail.insert(format!("crux_{k}_holds"), json!(fmt_verdict(&holds_for(k))));
        crux_detail.insert(format!("crux_{k}_fails"), json!(fmt_verdict(&fails_for(k))));
    }
    // Keep the legacy keys in the receipt too, for back-compat readers.
    crux_detail.insert("crux_iff".into(), json!(fmt_verdict(&crux_iff_v)));
    crux_detail.insert("crux_is_damage".into(), json!(fmt_verdict(&crux_damage_v)));
    crux_detail.insert("crux_is_wear".into(), json!(fmt_verdict(&crux_wear_v)));
    crux_detail.insert("n_cruxes".into(), json!(bridges.len()));
    chain.append(
        "isolate_crux",
        serde_json::Value::Object(crux_detail),
        Some(crux_iff_v.clone()),
    );

    // Assemble the certified set of open cruxes.
    for (k, b) in bridges.iter().enumerate() {
        let iff_v = iff_for(k);
        let holds_v = holds_for(k);
        let fails_v = fails_for(k);
        let gloss = predicate_gloss(dispute, &b.predicate);
        if proved(&iff_v) && undecided(&holds_v) && undecided(&fails_v) {
            let question = match &gloss {
                Some(g) => format!("Whether {g}."),
                None => format!("Whether {}.", b.predicate.replace('_', " ")),
            };
            analysis.cruxes.push(Crux {
                predicate: b.predicate.clone(),
                question,
                verdict: Verdict::Unknown,
            });
            // Each genuinely-open crux is also an irreducible inter-party knot —
            // unless the ontology already dissolved it as a vocabulary gap.
            if !suppressed.contains(&b.predicate) {
                if let Some(conflict) = predicate_conflict(dispute, &b.predicate, gloss.as_deref()) {
                    analysis.genuine_conflicts.push(conflict);
                }
            }
        } else if proved(&holds_v) || proved(&fails_v) {
            // The host actually settled this one — record it as a *decided*
            // crux (Proved/Refuted), never silently as open.
            let decided = if proved(&holds_v) { Verdict::Proved } else { Verdict::Refuted };
            let question = match &gloss {
                Some(g) => format!("Whether {g} (settled by the host from the stipulated facts)."),
                None => format!("Whether {} (settled by the host).", b.predicate.replace('_', " ")),
            };
            analysis.cruxes.push(Crux {
                predicate: b.predicate.clone(),
                question,
                verdict: decided,
            });
        }
    }

    // Back-compat single `crux` prose: derived from the FIRST open crux (or the
    // legacy verdicts when there are no bridges). The seeded scenarios keep the
    // exact previous wording.
    let first_open = analysis.cruxes.iter().find(|c| matches!(c.verdict, Verdict::Unknown));
    let any_decided = analysis.cruxes.iter().any(|c| matches!(c.verdict, Verdict::Proved | Verdict::Refuted));
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
    } else if let Some(c) = first_open {
        // Multi-crux path with no legacy first crux proved (e.g. >1 bridge where
        // the legacy alias didn't line up): name the open set honestly.
        let n = analysis.cruxes.iter().filter(|c| matches!(c.verdict, Verdict::Unknown)).count();
        analysis.crux = Some(if n == 1 {
            format!(
                "The dispute reduces to one open question — {} The kernel cannot, and will \
                 not, decide it for you.",
                c.question
            )
        } else {
            format!(
                "The dispute reduces to {n} open questions, each of which the kernel cannot — \
                 and will not — decide for you. They are listed above.",
            )
        });
    } else if any_decided {
        analysis.crux = Some(
            "The host was able to settle the contested question from the stipulated facts; it \
             is not, in this dispute, the open crux."
                .to_string(),
        );
    }

    // ── genuine conflicts: the irreducible knot ─────────────────────────
    // Single-crux fallback: when there are no bridges (no stipulated iff) we
    // still surface the legacy crux conflict if one exists.
    if bridges.is_empty() {
        if let Some(conflict) = crux_conflict(dispute) {
            // Suppress if the ontology already dissolved this clash's predicate.
            let dissolved_here = conflict
                .claim_ids
                .iter()
                .filter_map(|id| dispute.claims.iter().find(|c| &c.id == id))
                .filter_map(claim_atom_polarity)
                .any(|(name, _)| suppressed.contains(name));
            if !dissolved_here {
                analysis.genuine_conflicts.push(conflict);
            }
        }
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

/// Prefer a *decisive* verdict over a defaulted `Unknown`. Used to reconcile a
/// crux's per-index obligation name with its legacy alias: whichever source the
/// prover actually answered wins; if both answered, the per-index one is
/// authoritative (it is passed as `primary`).
fn decisive(primary: Verdict, fallback: Verdict) -> Verdict {
    match (&primary, &fallback) {
        // Primary spoke decisively — trust it.
        (Verdict::Proved | Verdict::Refuted | Verdict::Error(_), _) => primary,
        // Primary is Unknown but the alias was decisive — take the alias.
        (Verdict::Unknown, Verdict::Proved | Verdict::Refuted | Verdict::Error(_)) => fallback,
        // Both Unknown.
        _ => primary,
    }
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
        if let Formula::Iff(lhs, rhs) = f {
            // Whichever side of the bridge is a bare nullary predicate is the
            // contested fact; prefer the right side for back-compat.
            for side in [rhs, lhs] {
                if let Formula::Atom(Term::App(name, args)) = &**side {
                    if args.is_empty() {
                        return Some(name.clone());
                    }
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
    predicate_gloss(dispute, &name)
}

/// The plain-English gloss for a *named* predicate, from whichever party's
/// signature declares it. Used by the multi-crux path so every contested
/// question gets its own human phrasing.
fn predicate_gloss(dispute: &Dispute, name: &str) -> Option<String> {
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
    let gloss = crux_gloss(dispute);
    predicate_conflict(dispute, &pred, gloss.as_deref())
}

/// The genuine conflict over a *named* predicate: the two active claims that
/// assert `P` and `¬P` over it. Dispute-agnostic, and now crux-agnostic —
/// each contested question gets its own inter-party knot.
fn predicate_conflict(dispute: &Dispute, pred: &str, gloss: Option<&str>) -> Option<Conflict> {
    let mut pos: Option<&mediator_types::Claim> = None;
    let mut neg: Option<&mediator_types::Claim> = None;
    for c in &dispute.claims {
        if !c.active {
            continue;
        }
        match &c.formula {
            Formula::Atom(Term::App(n, a)) if a.is_empty() && n == pred => pos = Some(c),
            Formula::Not(inner) => {
                if let Formula::Atom(Term::App(n, a)) = &**inner {
                    if a.is_empty() && n == pred {
                        neg = Some(c);
                    }
                }
            }
            _ => {}
        }
    }
    match (pos, neg) {
        (Some(p), Some(n)) => {
            let desc = match gloss {
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

// ─────────────────────── vocabulary-mismatch dissolution ───────────────────────
// The categorical layer, made live: before we hand a clash back as a *genuine*
// crux, we ask the ontology whether the two parties are really disagreeing — or
// just using two words for the same thing. A clash whose two sides are
// *different predicate names* the ontology bridges as synonyms is not a crux at
// all; it is a vocabulary gap, dissolved with a plain-language note. Same-named
// clashes (every seeded scenario) are structurally genuine and never reach here.

/// The bare nullary predicate a claim asserts, with its polarity.
/// `Some((name, true))` for `P`, `Some((name, false))` for `¬P`, else `None`.
fn claim_atom_polarity(c: &Claim) -> Option<(&str, bool)> {
    match &c.formula {
        Formula::Atom(Term::App(n, a)) if a.is_empty() => Some((n.as_str(), true)),
        Formula::Not(inner) => {
            if let Formula::Atom(Term::App(n, a)) = &**inner {
                if a.is_empty() {
                    return Some((n.as_str(), false));
                }
            }
            None
        }
        _ => None,
    }
}

/// A party's declared signature symbols.
fn party_signature<'a>(dispute: &'a Dispute, party: &str) -> &'a [Sig] {
    dispute
        .parties
        .iter()
        .find(|p| p.id == party)
        .map(|p| p.signature.as_slice())
        .unwrap_or(&[])
}

/// One dissolved vocabulary clash: the two synonym predicates and the note.
struct DissolvedClash {
    /// The predicate names that were aligned away (so the caller suppresses any
    /// genuine conflict over either of them).
    predicates: (String, String),
    note: String,
}

/// Find clashes that are *only* a vocabulary gap and dissolve them.
///
/// For every pair of active claims from *different* parties that take *opposing*
/// polarity over *different* predicate names, ask
/// `mediator_ontology::classify_clash_sigs`. If it returns `VocabularyMismatch`
/// (with an alignment), the apparent disagreement is two words for the same
/// concept — recorded as dissolved, never a genuine crux. A `Genuine` verdict (or
/// any same-named clash, which never reaches here) is left untouched.
fn dissolve_vocabulary_clashes(dispute: &Dispute) -> Vec<DissolvedClash> {
    let active: Vec<&Claim> = dispute.claims.iter().filter(|c| c.active).collect();
    let mut out: Vec<DissolvedClash> = Vec::new();
    let mut seen: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();

    for (i, ca) in active.iter().enumerate() {
        let Some((pa, pol_a)) = claim_atom_polarity(ca) else {
            continue;
        };
        for cb in active.iter().skip(i + 1) {
            // Different parties only — one person using two words for one thing
            // is not an inter-party clash to dissolve.
            if ca.party == cb.party {
                continue;
            }
            let Some((pb, pol_b)) = claim_atom_polarity(cb) else {
                continue;
            };
            // Opposing polarity over *different* names is the dissolution shape.
            // Same-named opposing claims are the genuine-crux path (handled
            // elsewhere); same-polarity pairs are not a clash at all.
            if pol_a == pol_b || pa == pb {
                continue;
            }

            // De-dup symmetric pairs (claim order shouldn't matter).
            let key = if pa <= pb {
                (pa.to_string(), pb.to_string())
            } else {
                (pb.to_string(), pa.to_string())
            };
            if seen.contains(&key) {
                continue;
            }

            let sig_a = party_signature(dispute, &ca.party);
            let sig_b = party_signature(dispute, &cb.party);
            let verdict = mediator_ontology::classify_clash_sigs(sig_a, sig_b, pa, pb);

            if verdict.is_vocabulary() {
                seen.insert(key);
                let aligned = verdict
                    .alignment
                    .as_ref()
                    .map(|al| al.merged_name.clone())
                    .unwrap_or_else(|| pa.to_string());
                // Prefer human glosses when present; fall back to the symbol name.
                let gloss_a = predicate_gloss(dispute, pa).unwrap_or_else(|| pa.replace('_', " "));
                let gloss_b = predicate_gloss(dispute, pb).unwrap_or_else(|| pb.replace('_', " "));
                let note = format!(
                    "'{}' and '{}' are the same thing — not a disagreement, a vocabulary gap. \
                     ({} ≈ {}, aligned as '{}'.)",
                    pa.replace('_', " "),
                    pb.replace('_', " "),
                    gloss_a,
                    gloss_b,
                    aligned
                );
                out.push(DissolvedClash {
                    predicates: (pa.to_string(), pb.to_string()),
                    note,
                });
            }
        }
    }
    out
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

        // additive: the single-crux scenario yields exactly one open crux, and
        // the legacy `crux` is derived from it (back-compat preserved).
        assert_eq!(a.cruxes.len(), 1);
        assert_eq!(a.cruxes[0].predicate, "stain_is_damage");
        assert_eq!(a.cruxes[0].verdict, Verdict::Unknown);
        assert!(a.cruxes[0].question.to_lowercase().contains("ordinary wear"));

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

    // ───────────────────────── multi-crux path ──────────────────────────

    fn twocrux() -> Dispute {
        load_dispute("../../scenarios/twocrux.json").unwrap()
    }

    /// Verdicts a healthy real-Isabelle run produces for the two-crux scenario:
    /// both iffs Proved, the over-claim Proved, and EVERY crux predicate left
    /// Unknown both directions. These are the per-index obligation names.
    fn twocrux_verdicts() -> HashMap<String, Verdict> {
        let mut m = HashMap::new();
        m.insert("refund_damage_world".into(), Verdict::Proved);
        m.insert("refund_wear_world".into(), Verdict::Proved);
        m.insert("over_claim_refuted".into(), Verdict::Proved);
        // legacy alias (crux 0)
        m.insert("crux_iff".into(), Verdict::Proved);
        m.insert("crux_is_damage".into(), Verdict::Unknown);
        m.insert("crux_is_wear".into(), Verdict::Unknown);
        // per-crux names
        for k in 0..2 {
            m.insert(format!("crux_iff_{k}"), Verdict::Proved);
            m.insert(format!("crux_{k}_holds"), Verdict::Unknown);
            m.insert(format!("crux_{k}_fails"), Verdict::Unknown);
        }
        m
    }

    #[test]
    fn codegen_emits_obligations_for_every_crux() {
        let d = twocrux();
        let obs = codegen::build_obligations(&d);
        let names: Vec<&str> = obs.iter().map(|o| o.name.as_str()).collect();
        // both per-crux bridges are emitted, plus the legacy alias trio
        for n in [
            "crux_iff_0", "crux_0_holds", "crux_0_fails",
            "crux_iff_1", "crux_1_holds", "crux_1_fails",
            "crux_iff", "crux_is_damage", "crux_is_wear",
        ] {
            assert!(names.contains(&n), "missing obligation {n}: {names:?}");
        }
        // crux_iff_0 / crux_iff_1 target the two distinct stipulated iffs
        let g0 = &obs.iter().find(|o| o.name == "crux_iff_0").unwrap().goal;
        let g1 = &obs.iter().find(|o| o.name == "crux_iff_1").unwrap().goal;
        assert!(g0.contains("contractor_eats_rework") && g0.contains("kitchen_work_defective"));
        assert!(g1.contains("homeowner_owes_change_order") && g1.contains("change_order_authorized"));
        // each iff is proved from its OWN stipulated axiom only
        assert!(obs.iter().find(|o| o.name == "crux_iff_0").unwrap().proof.contains("stip_0"));
        assert!(obs.iter().find(|o| o.name == "crux_iff_1").unwrap().proof.contains("stip_1"));
    }

    #[test]
    fn two_cruxes_both_certified_and_handed_back() {
        let d = twocrux();
        let prover = TestProver { verdicts: twocrux_verdicts() };
        let (a, receipts) = analyze(&d, &prover, &TestDivider);

        // Both contested questions are reported, each Unknown (handed back).
        assert_eq!(a.cruxes.len(), 2);
        let preds: Vec<&str> = a.cruxes.iter().map(|c| c.predicate.as_str()).collect();
        assert!(preds.contains(&"kitchen_work_defective"));
        assert!(preds.contains(&"change_order_authorized"));
        assert!(a.cruxes.iter().all(|c| c.verdict == Verdict::Unknown));

        // Each crux carries its own human question text from the signature gloss.
        let kitchen = a.cruxes.iter().find(|c| c.predicate == "kitchen_work_defective").unwrap();
        assert!(kitchen.question.contains("defectively"));
        let change = a.cruxes.iter().find(|c| c.predicate == "change_order_authorized").unwrap();
        assert!(change.question.contains("change order"));

        // Two distinct inter-party knots — one per crux.
        assert_eq!(a.genuine_conflicts.len(), 2);

        // Legacy single `crux` is set from the FIRST crux (back-compat).
        assert!(a.crux.as_ref().unwrap().contains("will not"));

        // Ledger findings tie each disputed deduction to its controlling crux.
        assert!(a.ledger_findings.iter().any(|f|
            f.to_lowercase().contains("cabinetry rework") && f.contains("defectively")));
        assert!(a.ledger_findings.iter().any(|f|
            f.to_lowercase().contains("change-order") && f.contains("change order")));

        // The over-claim is still refuted ($9,000 claimed vs $12,000 itemized).
        assert!(a.dissolved.iter().any(|s| s.contains("not a deception")));

        // Receipt chain intact; isolate_crux records n_cruxes = 2.
        assert!(receipts::verify_chain(&receipts).is_ok());
        let iso = receipts.iter().find(|r| r.op == "isolate_crux").unwrap();
        assert_eq!(iso.detail["n_cruxes"], serde_json::json!(2));
    }

    #[test]
    fn one_settled_crux_does_not_hide_the_other_open_one() {
        // If the host happens to settle crux 1 (e.g. the change order is proved
        // authorized) but crux 0 stays open, we must report crux 1 as DECIDED
        // (Proved) and crux 0 as the still-open question — never silently merge.
        let d = twocrux();
        let mut v = twocrux_verdicts();
        v.insert("crux_1_holds".into(), Verdict::Proved);
        let prover = TestProver { verdicts: v };
        let (a, _) = analyze(&d, &prover, &TestDivider);

        assert_eq!(a.cruxes.len(), 2);
        let open: Vec<&Crux> = a.cruxes.iter().filter(|c| c.verdict == Verdict::Unknown).collect();
        let decided: Vec<&Crux> = a.cruxes.iter().filter(|c| c.verdict == Verdict::Proved).collect();
        assert_eq!(open.len(), 1);
        assert_eq!(decided.len(), 1);
        assert_eq!(open[0].predicate, "kitchen_work_defective");
        assert_eq!(decided[0].predicate, "change_order_authorized");
        // The decided crux's question is framed as host-settled, not open.
        assert!(decided[0].question.contains("settled by the host"));
    }

    #[test]
    fn legacy_caches_and_scenarios_still_parse() {
        // A precomputed analysis cache written before the multi-crux fields must
        // still deserialize — `cruxes` defaults to empty, nothing breaks.
        let cache = std::fs::read_to_string("../../scenarios/roommate.analysis.json").unwrap();
        let payload: serde_json::Value = serde_json::from_str(&cache).unwrap();
        let a: mediator_types::Analysis =
            serde_json::from_value(payload["analysis"].clone()).unwrap();
        assert!(a.cruxes.is_empty(), "missing cruxes must default to empty");
        assert!(a.crux.is_some(), "legacy crux preserved");

        // A scenario whose ledger items omit `controlling_crux` parses, with the
        // field defaulting to None (the seeded single-crux scenarios).
        let d = roommate();
        assert!(d.ledger.items.iter().all(|i| i.controlling_crux.is_none()));

        // The new two-crux scenario carries the controlling_crux wiring.
        let tc = twocrux();
        let controlled: Vec<&str> = tc
            .ledger
            .items
            .iter()
            .filter_map(|i| i.controlling_crux.as_deref())
            .collect();
        assert_eq!(controlled.len(), 2);
        assert!(controlled.contains(&"kitchen_work_defective"));
        assert!(controlled.contains(&"change_order_authorized"));
    }

    // ─────────────────── vocabulary-mismatch dissolution ───────────────────

    use mediator_types::{Ledger, Party, Sort};

    fn bool_pred(name: &str, gloss: &str) -> Sig {
        Sig { name: name.into(), arg_sorts: vec![], ret: Sort::Bool, gloss: gloss.into() }
    }

    fn atom_claim(id: &str, party: &str, pred: &str, positive: bool) -> Claim {
        let atom = Formula::Atom(Term::App(pred.into(), vec![]));
        let formula = if positive { atom } else { Formula::Not(Box::new(atom)) };
        Claim {
            id: id.into(),
            party: party.into(),
            nl: format!("{party} on {pred}"),
            formula,
            english_render: String::new(),
            weight: 5,
            defeasible: false,
            active: true,
        }
    }

    /// A synthetic dispute whose two opposing claims use SYNONYM predicates —
    /// different names, near-identical glosses ("carpet repair" vs "stain
    /// remediation"). The ontology must recognize the clash as a vocabulary gap
    /// and the kernel must dissolve it, never emit it as a genuine conflict.
    fn synonym_dispute() -> Dispute {
        Dispute {
            title: "Carpet wording dispute".into(),
            parties: vec![
                Party {
                    id: "ada".into(),
                    display_name: "Ada".into(),
                    signature: vec![bool_pred(
                        "carpet_needs_repair",
                        "the carpet requires professional repair work to fix the stain",
                    )],
                },
                Party {
                    id: "ben".into(),
                    display_name: "Ben".into(),
                    signature: vec![bool_pred(
                        "stain_remediation_required",
                        "the carpet stain requires professional remediation work performed",
                    )],
                },
            ],
            // Ada asserts the carpet needs repair; Ben *denies* stain remediation
            // is required. Different words, opposing polarity — looks like a fight,
            // is a vocabulary gap.
            claims: vec![
                atom_claim("a1", "ada", "carpet_needs_repair", true),
                atom_claim("b1", "ben", "stain_remediation_required", false),
            ],
            stipulated: vec![],
            ledger: Ledger { deposit_cents: 100000, items: vec![] },
            contested_items: vec![],
            valuations: vec![],
        }
    }

    #[test]
    fn synonym_clash_is_dissolved_not_a_genuine_conflict() {
        let d = synonym_dispute();
        // Ledger worlds prove (empty deductions), nothing else asserted.
        let mut verdicts = HashMap::new();
        verdicts.insert("refund_damage_world".to_string(), Verdict::Proved);
        verdicts.insert("refund_wear_world".to_string(), Verdict::Proved);
        let prover = TestProver { verdicts };
        let (a, receipts) = analyze(&d, &prover, &TestDivider);

        // The clash lands in `dissolved`, naming both words and the alignment.
        assert!(
            a.dissolved.iter().any(|s| s.contains("carpet needs repair")
                && s.contains("stain remediation required")
                && s.contains("vocabulary gap")),
            "expected a vocabulary-gap note, got {:?}",
            a.dissolved
        );

        // It is NOT a genuine conflict (the whole point of dissolution).
        assert!(
            a.genuine_conflicts.is_empty(),
            "a synonym clash must not be a genuine conflict, got {:?}",
            a.genuine_conflicts
        );
        // And it is not handed back as an open crux either.
        assert!(a.cruxes.iter().all(|c| c.predicate != "carpet_needs_repair"
            && c.predicate != "stain_remediation_required"));

        // A `dissolve_vocabulary` receipt records the aligned pair, chain intact.
        assert!(receipts::verify_chain(&receipts).is_ok());
        let diss = receipts
            .iter()
            .find(|r| r.op == "dissolve_vocabulary")
            .expect("a dissolve_vocabulary receipt should be emitted");
        assert_eq!(diss.detail["n_dissolved"], serde_json::json!(1));
    }

    #[test]
    fn same_predicate_clash_stays_genuine_not_dissolved() {
        // Guard: when both parties use the SAME predicate name (the seeded shape),
        // dissolution must do nothing — it is a structural genuine disagreement.
        let mut d = synonym_dispute();
        // Make Ben speak Ada's exact predicate, opposing polarity.
        d.parties[1].signature = vec![bool_pred(
            "carpet_needs_repair",
            "the carpet requires professional repair work to fix the stain",
        )];
        d.claims[1] = atom_claim("b1", "ben", "carpet_needs_repair", false);

        let mut verdicts = HashMap::new();
        verdicts.insert("refund_damage_world".to_string(), Verdict::Proved);
        verdicts.insert("refund_wear_world".to_string(), Verdict::Proved);
        let prover = TestProver { verdicts };
        let (a, _) = analyze(&d, &prover, &TestDivider);

        // No vocabulary-gap dissolution for a same-name clash.
        assert!(!a.dissolved.iter().any(|s| s.contains("vocabulary gap")));
    }
}
