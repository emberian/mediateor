//! The reduction: `Dispute` → Isabelle preamble + named proof obligations.
//!
//! The preamble runs from `theory Mediator_Probe imports Main begin` through
//! every `axiomatization`/`definition` — with **no trailing `end`**. The prover
//! appends each obligation (and a closing `end`) in isolation. This crate owns
//! the reduction; the prover only runs the text.

use crate::render::{formula_to_isabelle, fun_type_to_isabelle, uses_deontic};
use mediator_types::{Dispute, Formula, Obligation, Sort, Term};
use std::collections::BTreeMap;

/// Sentinel item ids in the roommate ledger. We keep the reduction general by
/// reading these off the `Dispute`, but the wear-world refund needs to know
/// which itemized deductions are *disputed* (excluded when the stain is wear).
const THEORY_NAME: &str = "Mediator_Probe";

/// Build the shared Isabelle preamble for a dispute.
///
/// Layout:
///   theory Mediator_Probe imports Main begin
///   (provisional deontic decls, if used)
///   axiomatization <all signature symbols> where
///     <stipulated facts as named axioms>
///     <party `x = N` claims as named axioms>
///   definition deposit ...
///   definition <each ledger item> ...
///   definition itemized_total = sum(items)
///   definition refund_if_damage = deposit - itemized_total
///   definition refund_if_wear   = deposit - (undisputed items only)
///   (NO trailing `end`)
pub fn build_preamble(dispute: &Dispute) -> String {
    let mut out = String::new();
    out.push_str(&format!("theory {THEORY_NAME}\n  imports Main\nbegin\n\n"));

    // Provisional shallow deontic operators, declared only if needed.
    let any_deontic = dispute
        .stipulated
        .iter()
        .chain(dispute.claims.iter().map(|c| &c.formula))
        .any(uses_deontic);
    if any_deontic {
        out.push_str("text \\<open>Provisional shallow deontic embedding (placeholder for CondNormReasHOL).\\<close>\n");
        out.push_str("consts Obl :: \"bool => bool\"\n");
        out.push_str("consts Perm :: \"bool => bool\"\n\n");
    }

    // ── axiomatization of every signature symbol across all parties ──
    // Dedupe by name; types must agree (last writer wins, but they should match).
    let mut sigs: BTreeMap<String, String> = BTreeMap::new();
    for party in &dispute.parties {
        for sig in &party.signature {
            let ty = fun_type_to_isabelle(&sig.arg_sorts, &sig.ret);
            sigs.insert(sig.name.clone(), ty);
        }
    }

    // Collect the named axioms: stipulated facts + `lhs = N`-shaped party claims.
    let mut axioms: Vec<(String, String)> = Vec::new();
    for (i, fact) in dispute.stipulated.iter().enumerate() {
        axioms.push((format!("stip_{i}"), formula_to_isabelle(fact)));
    }
    // Any party claim of the form `App(name,[]) = IntLit(N)` becomes an axiom so
    // an over-claim can be refuted against the itemization.
    for claim in &dispute.claims {
        if !claim.active {
            continue;
        }
        if let Some((sym, _)) = equality_constant_value(&claim.formula) {
            // Only axiomatize if the symbol is a declared signature constant.
            if sigs.contains_key(&sym) {
                axioms.push((
                    format!("claim_{}", sanitize(&claim.id)),
                    formula_to_isabelle(&claim.formula),
                ));
            }
        }
    }

    if !sigs.is_empty() {
        out.push_str("axiomatization\n");
        let decls: Vec<String> = sigs
            .iter()
            .map(|(n, ty)| format!("  {n} :: \"{ty}\""))
            .collect();
        out.push_str(&decls.join(" and\n"));
        if axioms.is_empty() {
            out.push('\n');
        } else {
            out.push_str("\nwhere\n");
            let ax: Vec<String> = axioms
                .iter()
                .map(|(name, prop)| format!("  {name}: \"{prop}\""))
                .collect();
            out.push_str(&ax.join(" and\n"));
            out.push('\n');
        }
        out.push('\n');
    }

    // ── the ledger definitions ──
    out.push_str(&format!(
        "definition deposit :: int where \"deposit = {}\"\n",
        dispute.ledger.deposit_cents
    ));

    // One definition per ledger item, keyed by a sanitized id.
    let mut item_defs: Vec<String> = Vec::new();
    let mut all_item_terms: Vec<String> = Vec::new();
    let mut undisputed_item_terms: Vec<String> = Vec::new();
    for item in &dispute.ledger.items {
        let def_name = item_def_name(&item.id);
        out.push_str(&format!(
            "definition {def_name} :: int where \"{def_name} = {}\"\n",
            item.amount_cents
        ));
        item_defs.push(format!("{def_name}_def"));
        all_item_terms.push(def_name.clone());
        if !item.disputed {
            undisputed_item_terms.push(def_name.clone());
        }
    }

    // itemized_total = sum of all items.
    let itemized_rhs = if all_item_terms.is_empty() {
        "0".to_string()
    } else {
        all_item_terms.join(" + ")
    };
    out.push_str(&format!(
        "definition itemized_total :: int where \"itemized_total = {itemized_rhs}\"\n"
    ));

    // refund_if_damage = deposit - itemized_total (all deductions stand).
    out.push_str(
        "definition refund_if_damage :: int where \"refund_if_damage = deposit - itemized_total\"\n",
    );

    // refund_if_wear = deposit - (undisputed items only): the disputed carpet
    // deduction falls away if the stain is ordinary wear.
    let wear_rhs = if undisputed_item_terms.is_empty() {
        "0".to_string()
    } else {
        undisputed_item_terms.join(" + ")
    };
    out.push_str(&format!(
        "definition refund_if_wear :: int where \"refund_if_wear = deposit - ({wear_rhs})\"\n"
    ));

    out
}

/// The set of `_def` simp lemmas for every ledger definition, used to build
/// proof methods. Returned in a stable order.
pub fn ledger_def_lemmas(dispute: &Dispute) -> Vec<String> {
    let mut defs = vec!["deposit_def".to_string()];
    for item in &dispute.ledger.items {
        defs.push(format!("{}_def", item_def_name(&item.id)));
    }
    defs.push("itemized_total_def".to_string());
    defs.push("refund_if_damage_def".to_string());
    defs.push("refund_if_wear_def".to_string());
    defs
}

/// Frame the six proof obligations for the reduction.
pub fn build_obligations(dispute: &Dispute) -> Vec<Obligation> {
    let ledger = &dispute.ledger;
    let deposit = ledger.deposit_cents;
    let itemized: i64 = ledger.items.iter().map(|i| i.amount_cents).sum();
    let undisputed: i64 = ledger
        .items
        .iter()
        .filter(|i| !i.disputed)
        .map(|i| i.amount_cents)
        .sum();
    let refund_damage = deposit - itemized;
    let refund_wear = deposit - undisputed;

    let defs = ledger_def_lemmas(dispute);
    let defs_joined = defs.join(" ");

    // Names of the axioms feeding the over-claim refutation and the crux.
    let claim_axioms = overclaim_axiom_names(dispute);
    let stip_axioms: Vec<String> = (0..dispute.stipulated.len()).map(|i| format!("stip_{i}")).collect();
    let stip_joined = stip_axioms.join(" ");

    let mut obligations = vec![
        Obligation {
            name: "refund_damage_world".to_string(),
            goal: format!("refund_if_damage = {refund_damage}"),
            proof: format!("by (simp add: {defs_joined})"),
        },
        Obligation {
            name: "refund_wear_world".to_string(),
            goal: format!("refund_if_wear = {refund_wear}"),
            proof: format!("by (simp add: {defs_joined})"),
        },
    ];

    // over_claim_refuted: claimed_total \<noteq> itemized_total. Only meaningful
    // if some party stipulated a `claimed_total = N` claim; otherwise we still
    // emit it (it will be Unknown, honestly) but feed what we have.
    {
        let mut adds = vec!["itemized_total_def".to_string()];
        for item in &dispute.ledger.items {
            adds.push(format!("{}_def", item_def_name(&item.id)));
        }
        adds.extend(claim_axioms.iter().cloned());
        obligations.push(Obligation {
            name: "over_claim_refuted".to_string(),
            goal: "claimed_total \\<noteq> itemized_total".to_string(),
            proof: format!("by (simp add: {})", adds.join(" ")),
        });
    }

    // crux_iff: the stipulated lease term, proved straight from its axiom.
    obligations.push(Obligation {
        name: "crux_iff".to_string(),
        goal: crux_iff_goal(dispute),
        proof: if stip_axioms.is_empty() {
            "by simp".to_string()
        } else {
            format!("by (simp add: {stip_joined})")
        },
    });

    // crux_is_damage / crux_is_wear: best-effort. Expected Unknown — the kernel
    // must NOT decide the human question. We give a genuine proof attempt so a
    // *failure* is the informative verdict, not a deliberate `sorry`.
    let crux_pred = crux_predicate_name(dispute);
    obligations.push(Obligation {
        name: "crux_is_damage".to_string(),
        goal: crux_pred.clone(),
        proof: best_effort_proof(&stip_axioms),
    });
    obligations.push(Obligation {
        name: "crux_is_wear".to_string(),
        goal: format!("\\<not> {crux_pred}"),
        proof: best_effort_proof(&stip_axioms),
    });

    obligations
}

/// A best-effort proof method for an undecidable goal: try the stipulated
/// axioms with `auto`. If it does not close, the prover reports `Unknown`.
fn best_effort_proof(stip_axioms: &[String]) -> String {
    if stip_axioms.is_empty() {
        "by auto".to_string()
    } else {
        format!("by (auto simp add: {})", stip_axioms.join(" "))
    }
}

/// The crux iff goal, taken from the first stipulated `Iff`, or a default.
fn crux_iff_goal(dispute: &Dispute) -> String {
    for f in &dispute.stipulated {
        if matches!(f, Formula::Iff(..)) {
            return formula_to_isabelle(f);
        }
    }
    "tenant_owes_carpet \\<longleftrightarrow> stain_is_damage".to_string()
}

/// The contested predicate the dispute reduces to: the right-hand side of the
/// stipulated lease iff (e.g. `stain_is_damage`).
fn crux_predicate_name(dispute: &Dispute) -> String {
    for f in &dispute.stipulated {
        if let Formula::Iff(_, rhs) = f {
            if let Formula::Atom(Term::App(name, args)) = &**rhs {
                if args.is_empty() {
                    return name.clone();
                }
            }
        }
    }
    "stain_is_damage".to_string()
}

/// Names of the claim axioms that feed the over-claim refutation.
fn overclaim_axiom_names(dispute: &Dispute) -> Vec<String> {
    let mut names = Vec::new();
    let declared: std::collections::HashSet<String> = dispute
        .parties
        .iter()
        .flat_map(|p| p.signature.iter().map(|s| s.name.clone()))
        .collect();
    for claim in &dispute.claims {
        if !claim.active {
            continue;
        }
        if let Some((sym, _)) = equality_constant_value(&claim.formula) {
            if declared.contains(&sym) {
                names.push(format!("claim_{}", sanitize(&claim.id)));
            }
        }
    }
    names
}

/// If a formula is `App(name,[]) = IntLit(n)` (a constant pinned to a value),
/// return `(name, n)`.
fn equality_constant_value(f: &Formula) -> Option<(String, i64)> {
    if let Formula::Eq(a, b) = f {
        match (a, b) {
            (Term::App(name, args), Term::IntLit(n)) if args.is_empty() => {
                Some((name.clone(), *n))
            }
            (Term::IntLit(n), Term::App(name, args)) if args.is_empty() => {
                Some((name.clone(), *n))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// A definition name for a ledger item, safe as an Isabelle identifier.
fn item_def_name(id: &str) -> String {
    let s = sanitize(id);
    // Avoid colliding with reserved names already used in the preamble.
    if matches!(
        s.as_str(),
        "deposit" | "itemized_total" | "refund_if_damage" | "refund_if_wear"
    ) {
        format!("item_{s}")
    } else {
        s
    }
}

/// Lowercase, replace non-alphanumeric with `_`, ensure it starts with a letter.
fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() || !out.chars().next().unwrap().is_ascii_alphabetic() {
        out = format!("s_{out}");
    }
    out
}

/// Convenience: assemble a *complete, standalone* theory for one obligation,
/// mirroring what the prover does (preamble + lemma + `end`). Exposed so the
/// in-crate test harness can hit real Isabelle without the prover crate.
pub fn standalone_theory(preamble: &str, ob: &Obligation) -> String {
    format!(
        "{preamble}\nlemma {}: \"{}\"\n  {}\n\nend\n",
        ob.name, ob.goal, ob.proof
    )
}

#[allow(unused)]
fn _unused_sort(_: &Sort) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_dispute;

    fn roommate() -> Dispute {
        load_dispute("../../scenarios/roommate.json").expect("load roommate scenario")
    }

    #[test]
    fn preamble_has_axioms_and_ledger() {
        let d = roommate();
        let pre = build_preamble(&d);
        assert!(pre.contains("theory Mediator_Probe"));
        assert!(pre.contains("imports Main"));
        assert!(!pre.trim_end().ends_with("end"));
        // signature symbols axiomatized
        assert!(pre.contains("stain_is_damage :: \"bool\""));
        assert!(pre.contains("claimed_total :: \"int\""));
        // stipulated iff as axiom
        assert!(pre.contains("stip_0:"));
        // over-claim axiom from Sam's s2 claim
        assert!(pre.contains("claim_s2:"));
        // ledger
        assert!(pre.contains("definition deposit :: int where \"deposit = 120000\""));
        assert!(pre.contains("itemized_total = cleaning + carpet_repair"));
        assert!(pre.contains("refund_if_damage = deposit - itemized_total"));
        // wear world drops the disputed carpet_repair, keeps cleaning
        assert!(pre.contains("refund_if_wear = deposit - (cleaning)"));
    }

    #[test]
    fn obligations_compute_right_amounts() {
        let d = roommate();
        let obs = build_obligations(&d);
        let by_name = |n: &str| obs.iter().find(|o| o.name == n).unwrap().clone();
        assert_eq!(by_name("refund_damage_world").goal, "refund_if_damage = 75000");
        assert_eq!(by_name("refund_wear_world").goal, "refund_if_wear = 105000");
        assert_eq!(
            by_name("over_claim_refuted").goal,
            "claimed_total \\<noteq> itemized_total"
        );
        assert_eq!(
            by_name("crux_iff").goal,
            "(tenant_owes_carpet \\<longleftrightarrow> stain_is_damage)"
        );
        assert_eq!(by_name("crux_is_damage").goal, "stain_is_damage");
        assert_eq!(by_name("crux_is_wear").goal, "\\<not> stain_is_damage");
    }
}
