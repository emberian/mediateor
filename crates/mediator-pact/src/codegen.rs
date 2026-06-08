//! The reduction: a [`Pact`] → Isabelle preamble + named proof obligations.
//!
//! Mirrors `mediator-core::codegen` in structure and discipline, and emits a
//! `.thy` whose obligations have EXACTLY the shape of the hand-written,
//! real-Isabelle-GREEN `isabelle/Pact.thy`:
//!
//!   * the declared predicates are kept FREE / uninterpreted by abstracting each
//!     guard `definition` over them as parameters and universally quantifying
//!     over them in every goal — exactly as `isabelle/Pact.thy` does (NOT as
//!     global `consts`, which would clash with the definition binders);
//!   * each clause's guard and outcome become `definition`s over those free
//!     predicates + integer (cents/days) arithmetic;
//!   * the COVERAGE obligation is `∀ (free preds…). (g_1 ∨ … ∨ g_n)`, closed
//!     `by (auto simp: <all guard defs>)`;
//!   * each CONSISTENCY obligation `(i<j)` is
//!     `¬ (g_i ∧ g_j ∧ o_i r ∧ ¬ o_j r)`, closed
//!     `by (auto simp: <the two guard defs + two outcome defs>)`.
//!
//! The preamble runs from `theory Mediator_Pact_Gen imports Main begin` through
//! every `definition` — with **no trailing `end`**. The prover appends each
//! obligation (and a closing `end`) in isolation. This module owns the
//! reduction; the prover only runs the text.

use crate::{Clause, Pact};
use mediator_types::{Obligation, Sort};

// Reuse mediator-core's *approach* to rendering guards/outcomes. We re-implement
// the small renderer here (mediator-pact must not depend on mediator-core) but
// keep it byte-for-byte compatible with `mediator-core::render::formula_to_isabelle`
// for the fragment a pact uses (atoms, ¬, ∧, ∨, =, ≤, <, int literals).
mod render;
use render::{formula_to_isabelle, sort_to_isabelle};

/// The generated theory's name. Matches the `Mediator_Pact_Gen` preamble the
/// spec asks for; distinct from the hand-written `Pact` theory so they never
/// collide in a shared build database.
pub const PACT_THEORY_NAME: &str = "Mediator_Pact_Gen";

/// The single coverage obligation's name.
pub fn coverage_obligation_name() -> String {
    "coverage".to_string()
}

/// The consistency obligation name for clause pair `(i, j)`.
pub fn consistency_obligation_name(i: usize, j: usize) -> String {
    format!("consistent_{i}_{j}")
}

/// The guard `definition` name for the clause at index `idx`.
pub fn guard_def_name(idx: usize, clause: &Clause) -> String {
    format!("g_{idx}_{}", sanitize(&clause.name))
}

/// The outcome `definition` name for the clause at index `idx`.
pub fn outcome_def_name(idx: usize, clause: &Clause) -> String {
    format!("o_{idx}_{}", sanitize(&clause.name))
}

/// Make a human gloss safe to embed inside an Isabelle `\<open>…\<close>` text
/// cartouche: the only thing that could break the comment is a literal closing
/// cartouche, so we defang any `\<close>`/`\<open>` and backslashes.
fn sanitize_text(s: &str) -> String {
    s.replace("\\<close>", "<close>")
        .replace("\\<open>", "<open>")
        .replace('\\', "/")
}

/// Render a function type `arg_sorts => ret` (nullary ⇒ just the return type).
fn fun_type(arg_sorts: &[Sort], ret: &Sort) -> String {
    if arg_sorts.is_empty() {
        sort_to_isabelle(ret)
    } else {
        let mut parts: Vec<String> = arg_sorts.iter().map(sort_to_isabelle).collect();
        parts.push(sort_to_isabelle(ret));
        parts.join(" => ")
    }
}

/// The free arguments every guard `definition` abstracts over, in declaration
/// order, as `(name::type)` binders. The guard is a predicate over the WHOLE
/// declared space so the coverage disjunction quantifies over the same binders.
///
/// e.g. `(stain_is_damage::bool) (notice_days::int)`.
fn guard_binders(pact: &Pact) -> String {
    pact.predicates
        .iter()
        .map(|sig| format!("({}::{})", sig.name, fun_type(&sig.arg_sorts, &sig.ret)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The guard `definition`'s Isabelle type: `<arg types> => bool`.
fn guard_def_type(pact: &Pact) -> String {
    let mut parts: Vec<String> = pact
        .predicates
        .iter()
        .map(|sig| fun_type(&sig.arg_sorts, &sig.ret))
        .collect();
    parts.push("bool".to_string());
    parts.join(" => ")
}

/// A guard *applied* to the declared predicate symbols by name (the form used in
/// the coverage disjunction and the consistency conjunctions):
/// `g_0_clean stain_is_damage notice_days`.
fn guard_applied(pact: &Pact, idx: usize, clause: &Clause) -> String {
    let name = guard_def_name(idx, clause);
    let args: Vec<&str> = pact.predicates.iter().map(|s| s.name.as_str()).collect();
    if args.is_empty() {
        name
    } else {
        format!("{name} {}", args.join(" "))
    }
}

/// The outcome `definition`'s single bound variable (the award being checked).
/// Outcomes reduce to a distinct integer-cents award per clause, so the bound
/// variable is an `int` (named `award` to read clearly).
const OUTCOME_VAR: &str = "award";

/// The outcome applied to the bound award variable: `o_0_clean award`.
fn outcome_applied(idx: usize, clause: &Clause) -> String {
    format!("{} {OUTCOME_VAR}", outcome_def_name(idx, clause))
}

/// Build the shared Isabelle preamble for a pact.
///
/// Layout (NO trailing `end`). The declared predicates are kept free by being
/// abstracted as the guard definitions' typed parameters (and later quantified
/// in the goals), never declared as global `consts`:
/// ```text
/// theory Mediator_Pact_Gen
///   imports Main
/// begin
///
/// text \<open>… declared question-space: stain_is_damage :: bool, notice_days :: int …\<close>
///
/// definition g_0_clean :: "bool => int => bool" where
///   "g_0_clean (stain_is_damage::bool) (notice_days::int) \<equiv> <guard formula>"
/// ...
/// definition o_0_clean :: "int => bool" where
///   "o_0_clean award \<equiv> <outcome formula>"
/// ...
/// ```
pub fn pact_preamble(pact: &Pact) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "theory {PACT_THEORY_NAME}\n  imports Main\nbegin\n\n"
    ));

    // The declared question-space, named in a comment. The value-predicates are
    // kept FREE / uninterpreted exactly the way the hand-validated `Pact.thy`
    // does it: each guard `definition` ABSTRACTS over them as parameters, and
    // every coverage / consistency goal UNIVERSALLY QUANTIFIES over them. That
    // universal quantifier is what lets the certificate be issued before any
    // dispute, while the moral question stays the humans'. (We deliberately do
    // NOT also declare them as global `consts`: a name cannot be both a global
    // const and a definition's bound parameter — Isabelle rejects that — and the
    // validated seam needs them quantifiable, which a fixed const is not.)
    out.push_str(
        "text \\<open>Forward constitution (certified pact), machine-generated.\n\n  \
         Declared question-space (FREE — left uninterpreted, handed back Unknown at\n  \
         dispute time):\n",
    );
    for sig in &pact.predicates {
        let ty = fun_type(&sig.arg_sorts, &sig.ret);
        out.push_str(&format!("    \\<^item> {} :: {ty}", sig.name));
        if !sig.gloss.is_empty() {
            out.push_str(&format!("  — {}", sanitize_text(&sig.gloss)));
        }
        out.push('\n');
    }
    out.push_str(
        "\n  Each guard ABSTRACTS over these; every goal QUANTIFIES over them, so the\n  \
         certificate is issued before any dispute while the value-predicates stay free.\\<close>\n\n",
    );

    let binders = guard_binders(pact);
    let g_ty = guard_def_type(pact);

    // ── guard definitions (over the free preds + int arithmetic) ──
    out.push_str("text \\<open>Clause guards: when each clause fires.\\<close>\n");
    for (idx, clause) in pact.clauses.iter().enumerate() {
        let name = guard_def_name(idx, clause);
        let body = formula_to_isabelle(&clause.guard);
        // `definition g_i :: "<ty>" where "g_i <binders> \<equiv> <guard>"`
        let lhs = if binders.is_empty() {
            name.clone()
        } else {
            format!("{name} {binders}")
        };
        out.push_str(&format!(
            "definition {name} :: \"{g_ty}\" where\n  \"{lhs} \\<equiv> {body}\"\n"
        ));
    }
    out.push('\n');

    // ── outcome definitions (each reduces to a distinct int-cents award) ──
    out.push_str(
        "text \\<open>Clause outcomes: a distinct integer-cents award per clause, so a clash\n  \
         of clauses is a clash of numbers.\\<close>\n",
    );
    for (idx, clause) in pact.clauses.iter().enumerate() {
        let name = outcome_def_name(idx, clause);
        let body = formula_to_isabelle(&clause.outcome);
        out.push_str(&format!(
            "definition {name} :: \"int => bool\" where\n  \"{name} {OUTCOME_VAR} \\<equiv> {body}\"\n"
        ));
    }

    out
}

/// Assemble the full `.thy` text: the preamble plus the coverage and consistency
/// obligations as `theorem`s with their proofs, then a closing `end`. This is
/// the human-readable, self-contained pact theory (mirrors what the prover would
/// run, with all obligations in one file).
pub fn pact_codegen(pact: &Pact) -> String {
    let mut out = pact_preamble(pact);
    out.push_str("\n\n");

    // ── (A) coverage ──
    out.push_str(
        "section \\<open>(A) Coverage — the pact handles every declared world\\<close>\n\n",
    );
    let cov = coverage_obligation(pact);
    out.push_str(&format!(
        "theorem {}:\n  \"{}\"\n  {}\n\n",
        cov.name, cov.goal, cov.proof
    ));

    // ── (B) consistency, one per clause pair (i<j) ──
    out.push_str(
        "section \\<open>(B) Consistency — no world fires two clauses with conflicting awards\\<close>\n\n",
    );
    for ob in consistency_obligations(pact) {
        out.push_str(&format!(
            "theorem {}:\n  \"{}\"\n  {}\n\n",
            ob.name, ob.goal, ob.proof
        ));
    }

    out.push_str("end\n");
    out
}

/// Every proof obligation a pact reduces to: the single coverage obligation,
/// then one `consistent_i_j` per clause pair `(i<j)`, in that order.
pub fn pact_obligations(pact: &Pact) -> Vec<Obligation> {
    let mut obs = vec![coverage_obligation(pact)];
    obs.extend(consistency_obligations(pact));
    obs
}

/// The COVERAGE obligation: `∀ (free preds…). (g_0 ∨ … ∨ g_n)`, closed by
/// `auto` with all guard defs. Mirrors `Pact.thy`'s `pact_coverage`.
pub fn coverage_obligation(pact: &Pact) -> Obligation {
    // Universally quantify over the declared free symbols (so the certificate is
    // issued *before* any are interpreted), exactly as Pact.thy does.
    let quant: String = pact
        .predicates
        .iter()
        .map(|sig| format!("({}::{})", sig.name, fun_type(&sig.arg_sorts, &sig.ret)))
        .collect::<Vec<_>>()
        .join(" ");

    let disjuncts: Vec<String> = pact
        .clauses
        .iter()
        .enumerate()
        .map(|(i, c)| guard_applied(pact, i, c))
        .collect();

    // An empty clause list cannot cover anything: emit `False` so the gate
    // honestly fails coverage (a pact with no clauses covers no world).
    let body = if disjuncts.is_empty() {
        "False".to_string()
    } else {
        disjuncts.join("\n     \\<or> ")
    };

    let goal = if quant.is_empty() {
        body
    } else {
        format!("\\<forall>{quant}.\n       {body}")
    };

    let guard_defs: Vec<String> = pact
        .clauses
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}_def", guard_def_name(i, c)))
        .collect();
    let proof = if guard_defs.is_empty() {
        "by auto".to_string()
    } else {
        format!("by (auto simp: {})", guard_defs.join(" "))
    };

    Obligation {
        name: coverage_obligation_name(),
        goal,
        proof,
    }
}

/// The CONSISTENCY obligations, one per clause pair `(i<j)`:
/// `¬ (g_i ∧ g_j ∧ o_i award ∧ ¬ o_j award)`, closed by `auto` with the two
/// guard defs + two outcome defs. Mirrors `Pact.thy`'s `pact_consistent_*`.
pub fn consistency_obligations(pact: &Pact) -> Vec<Obligation> {
    let mut obs = Vec::new();
    let n = pact.clauses.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let ci = &pact.clauses[i];
            let cj = &pact.clauses[j];
            let gi = guard_applied(pact, i, ci);
            let gj = guard_applied(pact, j, cj);
            let oi = outcome_applied(i, ci);
            let oj = outcome_applied(j, cj);

            let goal = format!("\\<not> ({gi} \\<and> {gj} \\<and> {oi} \\<and> \\<not> {oj})");
            let gi_def = format!("{}_def", guard_def_name(i, ci));
            let gj_def = format!("{}_def", guard_def_name(j, cj));
            let oi_def = format!("{}_def", outcome_def_name(i, ci));
            let oj_def = format!("{}_def", outcome_def_name(j, cj));
            let proof = format!("by (auto simp: {gi_def} {gj_def} {oi_def} {oj_def})");
            obs.push(Obligation {
                name: consistency_obligation_name(i, j),
                goal,
                proof,
            });
        }
    }
    obs
}

/// Lowercase, replace non-alphanumeric with `_`, ensure it starts with a letter.
/// Identical discipline to `mediator-core::codegen::sanitize`.
fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || !out.chars().next().unwrap().is_ascii_alphabetic() {
        out = format!("c_{out}");
    }
    out
}

#[cfg(test)]
mod codegen_tests {
    use super::*;
    use mediator_types::{Formula, Sig, Term};

    fn bool_sig(name: &str) -> Sig {
        Sig {
            name: name.to_string(),
            arg_sorts: vec![],
            ret: Sort::Bool,
            gloss: String::new(),
        }
    }
    fn int_sig(name: &str) -> Sig {
        Sig {
            name: name.to_string(),
            arg_sorts: vec![],
            ret: Sort::Int,
            gloss: String::new(),
        }
    }
    fn atom(n: &str) -> Formula {
        Formula::Atom(Term::App(n.to_string(), vec![]))
    }

    fn sample() -> Pact {
        // Mirrors the roommate pact in Pact.thy: free bool `stain_is_damage`,
        // int `notice_days`, 3 clauses with a 30-day cliff.
        Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("stain_is_damage"), int_sig("notice_days")],
            clauses: vec![
                Clause {
                    name: "clean".into(),
                    guard: Formula::And(vec![
                        Formula::Le(Term::IntLit(30), Term::App("notice_days".into(), vec![])),
                        Formula::Not(Box::new(atom("stain_is_damage"))),
                    ]),
                    outcome: Formula::Eq(Term::Var("award".into()), Term::IntLit(120000)),
                },
                Clause {
                    name: "damaged".into(),
                    guard: Formula::And(vec![
                        Formula::Le(Term::IntLit(30), Term::App("notice_days".into(), vec![])),
                        atom("stain_is_damage"),
                    ]),
                    outcome: Formula::Eq(Term::Var("award".into()), Term::IntLit(90000)),
                },
                Clause {
                    name: "short".into(),
                    guard: Formula::Lt(Term::App("notice_days".into(), vec![]), Term::IntLit(30)),
                    outcome: Formula::Eq(Term::Var("award".into()), Term::IntLit(100000)),
                },
            ],
        }
    }

    #[test]
    fn preamble_has_free_consts_and_defs_no_end() {
        let pact = sample();
        let pre = pact_preamble(&pact);
        assert!(pre.contains("theory Mediator_Pact_Gen"));
        assert!(pre.contains("imports Main"));
        assert!(
            !pre.trim_end().ends_with("end"),
            "preamble must not close theory"
        );
        // declared question-space named (free via abstraction + quantification,
        // matching Pact.thy — NOT global consts, which Isabelle would reject as
        // a clash with the definition binders).
        assert!(pre.contains("stain_is_damage :: bool"));
        assert!(pre.contains("notice_days :: int"));
        assert!(
            !pre.contains("consts stain_is_damage"),
            "must not declare a clashing global const"
        );
        // guard def shape with typed binders
        assert!(pre.contains("definition g_0_clean :: \"bool => int => bool\""));
        assert!(pre.contains("g_0_clean (stain_is_damage::bool) (notice_days::int) \\<equiv>"));
        // outcome def
        assert!(pre.contains("definition o_0_clean :: \"int => bool\""));
        assert!(pre.contains("o_0_clean award \\<equiv> (award = 120000)"));
    }

    #[test]
    fn coverage_goal_quantifies_over_free_preds() {
        let cov = coverage_obligation(&sample());
        assert_eq!(cov.name, "coverage");
        assert!(cov
            .goal
            .contains("\\<forall>(stain_is_damage::bool) (notice_days::int)"));
        assert!(cov.goal.contains("g_0_clean stain_is_damage notice_days"));
        assert!(cov
            .goal
            .contains("\\<or> g_1_damaged stain_is_damage notice_days"));
        assert!(cov
            .goal
            .contains("\\<or> g_2_short stain_is_damage notice_days"));
        assert_eq!(
            cov.proof,
            "by (auto simp: g_0_clean_def g_1_damaged_def g_2_short_def)"
        );
    }

    #[test]
    fn consistency_pairs_have_exact_shape() {
        let obs = consistency_obligations(&sample());
        // 3 clauses => 3 pairs: (0,1),(0,2),(1,2)
        assert_eq!(obs.len(), 3);
        let c01 = &obs[0];
        assert_eq!(c01.name, "consistent_0_1");
        assert_eq!(
            c01.goal,
            "\\<not> (g_0_clean stain_is_damage notice_days \\<and> \
             g_1_damaged stain_is_damage notice_days \\<and> \
             o_0_clean award \\<and> \\<not> o_1_damaged award)"
        );
        assert_eq!(
            c01.proof,
            "by (auto simp: g_0_clean_def g_1_damaged_def o_0_clean_def o_1_damaged_def)"
        );
        assert_eq!(obs[1].name, "consistent_0_2");
        assert_eq!(obs[2].name, "consistent_1_2");
    }

    #[test]
    fn full_codegen_is_self_contained() {
        let thy = pact_codegen(&sample());
        assert!(thy.starts_with("theory Mediator_Pact_Gen"));
        assert!(thy.trim_end().ends_with("end"));
        assert!(thy.contains("theorem coverage:"));
        assert!(thy.contains("theorem consistent_0_1:"));
        assert!(thy.contains("theorem consistent_1_2:"));
    }
}
