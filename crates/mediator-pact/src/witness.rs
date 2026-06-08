//! The third trichotomy leg: **INCONSISTENT-with-witness**.
//!
//! When a CONSISTENCY obligation fails — two clauses can both fire while
//! demanding different awards — a bare `Unknown` from the gate is not enough.
//! The honest, actionable answer is the *witness world*: a concrete assignment
//! of truth-values to the declared crux predicates and concrete integers to the
//! declared dials, plus the two conflicting award amounts, where the clash
//! actually happens. "INCONSISTENT: when `stain_is_damage` is true and
//! `notice_days = 45`, clauses `damaged` and `late` both fire and demand 90000
//! vs 100000."
//!
//! We find it by a **bounded, re-checkable Rust search** over the declared
//! booleans × a small set of "interesting" integers for each dial (the literals
//! that appear in the guards, and their immediate neighbours — the boundary
//! points where two linear-integer guards can start to overlap). For the pact
//! fragment (a free bool crux + linear int arithmetic, each outcome an
//! `award = <const>` equation) this set provably contains a clash whenever one
//! exists: a clash needs both guards true, and across a boolean assignment each
//! guard is a conjunction/disjunction of linear int (in)equalities whose
//! feasible region, if non-empty and overlapping the other's, contains one of
//! these boundary integers.
//!
//! The witness is returned as plain data and re-rendered, so a skeptic with no
//! Isabelle can re-evaluate the two guards at the named world and the two
//! outcomes at the named award and confirm the contradiction by hand.
//!
//! # Soundness and the honest boundary
//!
//! This search NEVER decides consistency — the gate does. It only ever turns a
//! gate-reported consistency failure into a *concrete, exhibited* witness. If a
//! clash exists but the bounded search misses it (e.g. a guard that compares
//! one dial to *another dial* rather than to a literal, whose crossover point
//! is not seeded by [`interesting_ints`]), the caller falls back to an honest
//! REFUSED with the consistency gap named — never to a false CERTIFIED. The
//! search can only *upgrade* the diagnosis (Refused → Inconsistent-with-
//! witness), so the trichotomy's soundness rests entirely on Isabelle's
//! verdicts. The boundary is named, not hidden: for the corpus's fragment
//! (every guard a dial-vs-literal comparison) the candidate set is complete.

use crate::Pact;
use mediator_types::{Formula, Sort, Term};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A concrete world where two clauses clash: truth-values for the declared crux
/// predicates, integer values for the declared dials, and the two conflicting
/// award amounts (in cents) each clause demands there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InconsistencyWitness {
    /// The two clashing clauses' indices.
    pub clause_i: usize,
    pub clause_j: usize,
    /// Their names, for a plain rendering.
    pub clause_i_name: String,
    pub clause_j_name: String,
    /// The crux predicate truth-values at the witness world (declared-Bool
    /// symbol → value), sorted by name for determinism.
    pub bool_assignment: BTreeMap<String, bool>,
    /// The integer dial values at the witness world (declared-Int symbol →
    /// value), sorted by name for determinism.
    pub int_assignment: BTreeMap<String, i64>,
    /// The award (cents) clause `i` demands at this world.
    pub award_i_cents: i64,
    /// The award (cents) clause `j` demands at this world — different from
    /// `award_i_cents`, which is exactly the contradiction.
    pub award_j_cents: i64,
}

impl InconsistencyWitness {
    /// A one-line, plain-language rendering of the clash, in the shape the spec
    /// asks for.
    pub fn plain(&self) -> String {
        let mut conds: Vec<String> = Vec::new();
        for (k, v) in &self.bool_assignment {
            conds.push(format!("{k} is {v}"));
        }
        for (k, v) in &self.int_assignment {
            conds.push(format!("{k} = {v}"));
        }
        let when = if conds.is_empty() {
            "in every world".to_string()
        } else {
            format!("when {}", conds.join(" and "))
        };
        format!(
            "INCONSISTENT: {when}, clauses `{}` and `{}` both fire and demand {} vs {} (cents). \
             A contradiction of numbers — the pact would owe two different amounts at once.",
            self.clause_i_name, self.clause_j_name, self.award_i_cents, self.award_j_cents
        )
    }
}

/// Search for a concrete witness that clauses `i` and `j` clash (both guards
/// fire, but they demand different awards). Returns `None` if no clash is found
/// within the bounded search — which, for the pact fragment, means the pair is
/// genuinely consistent (the gate's `Proved` and this search agree).
///
/// The search is over every boolean assignment to the declared crux predicates
/// × every "interesting" integer (guard literals and their neighbours) for each
/// declared dial. The award demanded by a clause at a world is read off its
/// outcome (`award = <const>`); a clash is two distinct demanded awards.
pub fn find_clash(pact: &Pact, i: usize, j: usize) -> Option<InconsistencyWitness> {
    let ci = pact.clauses.get(i)?;
    let cj = pact.clauses.get(j)?;

    let bool_syms = declared_bools(pact);
    let int_syms = declared_ints(pact);
    let candidates = interesting_ints(pact);

    // Enumerate boolean assignments (2^|bool_syms|). The crux count is tiny by
    // construction (a pact has a handful of contested questions).
    let n_bool = bool_syms.len();
    let bool_combos = 1usize << n_bool;

    for mask in 0..bool_combos {
        let mut bmap: BTreeMap<String, bool> = BTreeMap::new();
        for (bit, name) in bool_syms.iter().enumerate() {
            bmap.insert(name.clone(), (mask >> bit) & 1 == 1);
        }

        // Enumerate integer assignments over the dial cross-product. With a
        // bounded candidate set per dial and a handful of dials this stays small.
        for iassign in int_assignments(&int_syms, &candidates) {
            let env = Env {
                bools: &bmap,
                ints: &iassign,
            };
            // Both guards must fire at this world.
            if eval_formula(&ci.guard, &env) != Some(true) {
                continue;
            }
            if eval_formula(&cj.guard, &env) != Some(true) {
                continue;
            }
            // Both fire. Read each clause's demanded award. A clash is two
            // distinct concrete awards.
            let ai = demanded_award(&ci.outcome);
            let aj = demanded_award(&cj.outcome);
            if let (Some(ai), Some(aj)) = (ai, aj) {
                if ai != aj {
                    return Some(InconsistencyWitness {
                        clause_i: i,
                        clause_j: j,
                        clause_i_name: ci.name.clone(),
                        clause_j_name: cj.name.clone(),
                        bool_assignment: bmap.clone(),
                        int_assignment: iassign,
                        award_i_cents: ai,
                        award_j_cents: aj,
                    });
                }
            }
        }
    }
    None
}

/// Re-verify a witness from scratch: at the named world, both guards fire AND
/// the two demanded awards differ. This is the cheap, Isabelle-free re-check a
/// skeptic (or the public box) runs to trust the `.cert.json`'s witness.
pub fn recheck_witness(pact: &Pact, w: &InconsistencyWitness) -> bool {
    let (Some(ci), Some(cj)) = (pact.clauses.get(w.clause_i), pact.clauses.get(w.clause_j)) else {
        return false;
    };
    let env = Env {
        bools: &w.bool_assignment,
        ints: &w.int_assignment,
    };
    eval_formula(&ci.guard, &env) == Some(true)
        && eval_formula(&cj.guard, &env) == Some(true)
        && demanded_award(&ci.outcome) == Some(w.award_i_cents)
        && demanded_award(&cj.outcome) == Some(w.award_j_cents)
        && w.award_i_cents != w.award_j_cents
}

// ───────────────────────────── evaluation ──────────────────────────────────

/// A concrete world: declared crux truth-values + declared dial integers.
struct Env<'a> {
    bools: &'a BTreeMap<String, bool>,
    ints: &'a BTreeMap<String, i64>,
}

/// Evaluate a guard `Formula` to a concrete `bool` under an environment.
/// Returns `None` if the formula uses something outside the pact fragment (a
/// symbol we have no value for, a non-int comparison, …) — the search then
/// skips that world rather than guessing.
fn eval_formula(f: &Formula, env: &Env) -> Option<bool> {
    match f {
        Formula::Atom(t) => eval_bool_term(t, env),
        Formula::Eq(a, b) => Some(eval_int(a, env)? == eval_int(b, env)?),
        Formula::Le(a, b) => Some(eval_int(a, env)? <= eval_int(b, env)?),
        Formula::Lt(a, b) => Some(eval_int(a, env)? < eval_int(b, env)?),
        Formula::Not(p) => Some(!eval_formula(p, env)?),
        Formula::And(ps) => {
            let mut acc = true;
            for p in ps {
                acc &= eval_formula(p, env)?;
            }
            Some(acc)
        }
        Formula::Or(ps) => {
            let mut acc = false;
            for p in ps {
                acc |= eval_formula(p, env)?;
            }
            Some(acc)
        }
        Formula::Implies(a, b) => Some(!eval_formula(a, env)? || eval_formula(b, env)?),
        Formula::Iff(a, b) => Some(eval_formula(a, env)? == eval_formula(b, env)?),
        // Quantifiers / deontic operators are outside a well-formed pact guard.
        _ => None,
    }
}

/// Evaluate a Bool-sorted term (a nullary crux application) under the env.
fn eval_bool_term(t: &Term, env: &Env) -> Option<bool> {
    match t {
        Term::App(name, args) if args.is_empty() => env.bools.get(name).copied(),
        _ => None,
    }
}

/// Evaluate an Int-sorted term (a dial application or an int literal) under the
/// env. We support exactly the pact fragment; anything else yields `None`.
fn eval_int(t: &Term, env: &Env) -> Option<i64> {
    match t {
        Term::IntLit(n) => Some(*n),
        Term::App(name, args) if args.is_empty() => env.ints.get(name).copied(),
        // A bare `Var` only appears as the outcome's `award`, never in a guard's
        // integer comparison; if one shows up we can't evaluate it.
        _ => None,
    }
}

/// Read the concrete award (cents) a clause outcome demands, for the common
/// pact shape `award = <const>` (in either argument order). Returns `None` if
/// the outcome is not a simple award equation — then we can't compare numbers,
/// and `find_clash` conservatively reports no clash for that pair.
fn demanded_award(outcome: &Formula) -> Option<i64> {
    if let Formula::Eq(a, b) = outcome {
        match (a, b) {
            (Term::Var(v), Term::IntLit(n)) if v == "award" => return Some(*n),
            (Term::IntLit(n), Term::Var(v)) if v == "award" => return Some(*n),
            _ => {}
        }
    }
    None
}

// ───────────────────────── candidate enumeration ───────────────────────────

/// The declared free crux (Bool, nullary) symbol names, sorted.
fn declared_bools(pact: &Pact) -> Vec<String> {
    let mut v: Vec<String> = pact
        .predicates
        .iter()
        .filter(|s| s.ret == Sort::Bool && s.arg_sorts.is_empty())
        .map(|s| s.name.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The declared integer dial symbol names, sorted.
fn declared_ints(pact: &Pact) -> Vec<String> {
    let mut v: Vec<String> = pact
        .predicates
        .iter()
        .filter(|s| s.ret == Sort::Int)
        .map(|s| s.name.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The "interesting" integer values to try for each dial: every integer literal
/// that appears anywhere in any guard, plus each literal's immediate neighbours
/// (`lit-1`, `lit+1`) and `0`. These are the boundary points of the linear
/// guards; a clash between two linear-int guards, if one exists, is witnessed at
/// one of them.
fn interesting_ints(pact: &Pact) -> Vec<i64> {
    use std::collections::BTreeSet;
    let mut lits: BTreeSet<i64> = BTreeSet::new();
    lits.insert(0);
    for clause in &pact.clauses {
        collect_int_literals(&clause.guard, &mut lits);
    }
    // Expand with neighbours so a strict `<`/`>` boundary is covered on both
    // sides (e.g. a 30-day cliff: 29, 30, 31).
    let mut out: BTreeSet<i64> = BTreeSet::new();
    for &l in &lits {
        out.insert(l);
        out.insert(l.saturating_sub(1));
        out.insert(l.saturating_add(1));
    }
    out.into_iter().collect()
}

fn collect_int_literals(f: &Formula, out: &mut std::collections::BTreeSet<i64>) {
    match f {
        Formula::Atom(t) => collect_term_literals(t, out),
        Formula::Eq(a, b) | Formula::Le(a, b) | Formula::Lt(a, b) => {
            collect_term_literals(a, out);
            collect_term_literals(b, out);
        }
        Formula::Not(p) | Formula::Obligation(p) | Formula::Permission(p) => {
            collect_int_literals(p, out)
        }
        Formula::And(ps) | Formula::Or(ps) => {
            for p in ps {
                collect_int_literals(p, out);
            }
        }
        Formula::Implies(a, b) | Formula::Iff(a, b) => {
            collect_int_literals(a, out);
            collect_int_literals(b, out);
        }
        Formula::Forall(_, _, body) | Formula::Exists(_, _, body) => collect_int_literals(body, out),
    }
}

fn collect_term_literals(t: &Term, out: &mut std::collections::BTreeSet<i64>) {
    match t {
        Term::IntLit(n) => {
            out.insert(*n);
        }
        Term::App(_, args) => {
            for a in args {
                collect_term_literals(a, out);
            }
        }
        Term::Var(_) => {}
    }
}

/// Every assignment of the dial symbols to candidate integers (the
/// cross-product). For zero dials this yields exactly one empty assignment, so
/// a boolean-only pact still searches its single integer world.
fn int_assignments(syms: &[String], candidates: &[i64]) -> Vec<BTreeMap<String, i64>> {
    let mut acc: Vec<BTreeMap<String, i64>> = vec![BTreeMap::new()];
    for sym in syms {
        let mut next = Vec::with_capacity(acc.len() * candidates.len());
        for base in &acc {
            for &c in candidates {
                let mut m = base.clone();
                m.insert(sym.clone(), c);
                next.push(m);
            }
        }
        acc = next;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Clause;
    use mediator_types::Sig;

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
    fn award(cents: i64) -> Formula {
        Formula::Eq(Term::Var("award".into()), Term::IntLit(cents))
    }

    /// A pact with two overlapping clauses that demand different awards: when
    /// `stain_is_damage` is true and notice ≥ 30, BOTH `damaged` (90000) and an
    /// overlapping `late_overlap` (100000, fires on notice ≥ 30 regardless)
    /// fire — a real clash. The search must exhibit the concrete world.
    #[test]
    fn finds_concrete_clash() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("stain_is_damage"), int_sig("notice_days")],
            clauses: vec![
                // damaged: notice ≥ 30 ∧ stain → 90000
                Clause {
                    name: "damaged".into(),
                    guard: Formula::And(vec![nd_ge(30), atom("stain_is_damage")]),
                    outcome: award(90000),
                },
                // late_overlap: notice ≥ 30 → 100000  (overlaps `damaged`)
                Clause {
                    name: "late_overlap".into(),
                    guard: nd_ge(30),
                    outcome: award(100000),
                },
            ],
        };
        let w = find_clash(&pact, 0, 1).expect("a clash exists");
        assert_eq!(w.clause_i_name, "damaged");
        assert_eq!(w.clause_j_name, "late_overlap");
        assert_eq!(w.bool_assignment.get("stain_is_damage"), Some(&true));
        let nd = *w.int_assignment.get("notice_days").unwrap();
        assert!(nd >= 30, "witness must satisfy both guards: notice_days = {nd}");
        assert_eq!(w.award_i_cents, 90000);
        assert_eq!(w.award_j_cents, 100000);
        // It re-verifies independently.
        assert!(recheck_witness(&pact, &w), "witness must re-check");
        // And renders in the spec's shape.
        let p = w.plain();
        assert!(p.contains("INCONSISTENT"));
        assert!(p.contains("stain_is_damage is true"));
        assert!(p.contains("90000 vs 100000"));
    }

    /// Pairwise-exclusive guards (the real roommate pact) have NO clash; the
    /// search returns None, agreeing with the gate's `Proved`.
    #[test]
    fn no_clash_for_exclusive_guards() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
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
            ],
        };
        assert!(
            find_clash(&pact, 0, 1).is_none(),
            "exclusive guards must not clash"
        );
    }

    /// Two clauses that overlap but demand the SAME award are not a clash (no
    /// contradiction of numbers).
    #[test]
    fn same_award_is_not_a_clash() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("stain_is_damage"), int_sig("notice_days")],
            clauses: vec![
                Clause {
                    name: "a".into(),
                    guard: nd_ge(30),
                    outcome: award(90000),
                },
                Clause {
                    name: "b".into(),
                    guard: Formula::And(vec![nd_ge(30), atom("stain_is_damage")]),
                    outcome: award(90000),
                },
            ],
        };
        assert!(find_clash(&pact, 0, 1).is_none());
    }

    /// The boundary point is found: guards `notice ≥ 30` and `notice ≤ 30`
    /// overlap exactly at 30, and the search's neighbour-expansion includes it.
    #[test]
    fn boundary_overlap_is_found() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("crux"), int_sig("notice_days")],
            clauses: vec![
                Clause {
                    name: "atleast".into(),
                    guard: Formula::And(vec![nd_ge(30), atom("crux")]),
                    outcome: award(90000),
                },
                Clause {
                    name: "atmost".into(),
                    guard: Formula::And(vec![
                        Formula::Le(Term::App("notice_days".into(), vec![]), Term::IntLit(30)),
                        atom("crux"),
                    ]),
                    outcome: award(100000),
                },
            ],
        };
        let w = find_clash(&pact, 0, 1).expect("overlap at notice_days = 30");
        assert_eq!(*w.int_assignment.get("notice_days").unwrap(), 30);
    }
}
