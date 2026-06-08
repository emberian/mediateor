//! The UNKNOWN-GATE as a *type invariant*, enforced at AUTHORING time.
//!
//! The whole project's load-bearing promise is that the machine never decides
//! the value question — it hands the crux back `Unknown`. At dispute time that
//! is a runtime verdict (the prover answers `Unknown`). [`validate`] lifts the
//! same firewall to AUTHORING time, before a single line of `.thy` is emitted:
//!
//! > **Every clause guard MUST branch on at least one DECLARED free crux
//! > predicate.**
//!
//! A "crux predicate" is a declared, `Bool`-sorted, *nullary* symbol — the
//! contested future question, kept uninterpreted (`stain_is_damage`,
//! `departure_for_cause`, …). The integer dials (`notice_days`, `cliff_months`)
//! are *agreed measurables*: their truth the prover can compute on its own.
//!
//! So a guard that mentions only integer dials + literals (e.g. `notice_days <
//! 30`) is a guard the prover could **decide entirely by itself** — there is no
//! uninterpreted question in it. That is not a guard at all: it is a *value-call
//! smuggled in disguised as a guard*. If we let such a clause through, the
//! machine would be silently deciding, at codegen time, a question it claims to
//! hand back — and the coverage/consistency certificate would be *about* a
//! pact whose guards the prover decides unilaterally.
//!
//! [`validate`] REJECTS such a pact up front, in plain language, so "the machine
//! never decides the value question" becomes a property of the pact's *type*
//! (it cannot be constructed-and-certified otherwise), not merely a runtime
//! verdict we observe after the fact.
//!
//! # The honest boundary (no overclaim)
//!
//! This is a *syntactic* firewall: it checks that a free crux symbol literally
//! occurs in the guard. It deliberately does NOT try to prove the guard is
//! *semantically* undecidable (that the crux genuinely changes the guard's
//! value) — that would itself be a prover question, and a conservative
//! syntactic check is the honest, re-readable bar. A guard like
//! `stain_is_damage ∨ ¬stain_is_damage` references a crux yet is a tautology;
//! we do not reject it here (it is degenerate, not a smuggled value-call, and
//! the coverage/consistency gate will treat it on its merits). What we catch is
//! the load-bearing failure: a guard with *no dependence whatsoever* on any
//! uninterpreted question.

use crate::Pact;
use mediator_types::{Formula, Sig, Sort, Term};
use std::collections::BTreeSet;

/// Why a pact was rejected at authoring time, before any Isabelle ran. Each
/// variant renders to a plain-language reason a non-logician can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// The pact declares no clauses — it covers no world and cannot certify.
    NoClauses,
    /// The pact declares not a single free crux predicate (a `Bool`-sorted
    /// nullary symbol). Then there is no value question to hand back, and every
    /// guard is decidable by the prover — the whole construction is pointless.
    NoCruxDeclared {
        /// The integer dials that *were* declared (if any), so the author sees
        /// what they have vs. what's missing.
        declared_dials: Vec<String>,
    },
    /// A clause guard does not branch on ANY declared free crux predicate — its
    /// truth is something the prover could decide entirely on its own. This is a
    /// value-call disguised as a guard; the firewall refuses it.
    DecidableGuard {
        /// The offending clause's index and name.
        clause_index: usize,
        clause_name: String,
        /// The declared free crux predicates the guard *could* have used.
        available_cruxes: Vec<String>,
        /// The (non-crux) symbols the guard actually referenced — typically the
        /// integer dials — so the diagnosis is concrete.
        guard_mentions: Vec<String>,
    },
    /// A clause guard references a symbol that was never declared in the pact's
    /// predicate list. The pact is malformed: the `.thy` would not even bind it.
    UndeclaredSymbol {
        clause_index: usize,
        clause_name: String,
        symbol: String,
    },
}

impl ValidationError {
    /// A plain-language explanation a pact author (not a logician) can act on.
    pub fn plain(&self) -> String {
        match self {
            ValidationError::NoClauses => {
                "this pact declares no clauses, so it resolves no world at all. Add the \
                 clauses that say what happens in each foreseeable situation."
                    .to_string()
            }
            ValidationError::NoCruxDeclared { declared_dials } => {
                let mut s = String::from(
                    "this pact declares no contested question to hand back: there is no free \
                     crux predicate (a Bool-typed, uninterpreted symbol like `stain_is_damage` \
                     or `departure_for_cause`). Without one, every clause guard is something the \
                     prover would simply DECIDE — and the whole point of a forward constitution \
                     is that the machine refuses the value question.",
                );
                if !declared_dials.is_empty() {
                    s.push_str(&format!(
                        " You declared the integer dial(s) [{}], but a dial is an agreed \
                         measurable, not a contested question. Name the human question the \
                         clauses turn on, and leave it uninterpreted.",
                        declared_dials.join(", ")
                    ));
                }
                s
            }
            ValidationError::DecidableGuard {
                clause_index,
                clause_name,
                available_cruxes,
                guard_mentions,
            } => {
                let mentions = if guard_mentions.is_empty() {
                    "only constants".to_string()
                } else {
                    format!("only [{}]", guard_mentions.join(", "))
                };
                format!(
                    "clause #{clause_index} `{clause_name}` has a guard the prover could decide \
                     entirely on its own: it branches on {mentions}, none of which is a contested \
                     question. That is a value-call disguised as a guard. A clause guard MUST \
                     depend on at least one declared free crux predicate [{}] — the uninterpreted \
                     question the machine hands back — or the machine would be deciding, at \
                     authoring time, the very thing it promises to leave to you.",
                    available_cruxes.join(", ")
                )
            }
            ValidationError::UndeclaredSymbol {
                clause_index,
                clause_name,
                symbol,
            } => format!(
                "clause #{clause_index} `{clause_name}` references `{symbol}`, which the pact \
                 never declared in its predicate list. Declare it (as a free crux Bool, or an \
                 integer dial) before using it in a guard."
            ),
        }
    }
}

/// The set of declared FREE CRUX predicate names: `Bool`-sorted, nullary
/// symbols. These are the contested questions kept uninterpreted and handed
/// back `Unknown`. A clause guard must branch on at least one of them.
pub fn crux_predicates(pact: &Pact) -> BTreeSet<String> {
    pact.predicates
        .iter()
        .filter(|s| is_crux_sig(s))
        .map(|s| s.name.clone())
        .collect()
}

/// Whether a declared signature is a free crux predicate (Bool, nullary).
fn is_crux_sig(s: &Sig) -> bool {
    s.ret == Sort::Bool && s.arg_sorts.is_empty()
}

/// Every declared symbol name (cruxes + dials + anything else), for the
/// undeclared-symbol check. The reserved outcome variable `award` is never a
/// declared predicate; it appears only in outcomes, not guards.
fn declared_symbols(pact: &Pact) -> BTreeSet<String> {
    pact.predicates.iter().map(|s| s.name.clone()).collect()
}

/// Validate a pact BEFORE codegen/certify. Returns the list of every reason the
/// pact is rejected (empty ⇒ the pact is well-formed and may proceed to the
/// gate). We collect *all* problems rather than stopping at the first so an
/// author fixes the pact in one pass.
///
/// The load-bearing rule (the firewall): every clause guard must branch on at
/// least one declared free crux predicate. A guard with no such dependence is a
/// value-call the prover could decide on its own, refused here at authoring.
pub fn validate(pact: &Pact) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if pact.clauses.is_empty() {
        errors.push(ValidationError::NoClauses);
    }

    let cruxes = crux_predicates(pact);
    let declared = declared_symbols(pact);

    if cruxes.is_empty() && !pact.clauses.is_empty() {
        let dials: Vec<String> = pact
            .predicates
            .iter()
            .filter(|s| s.ret == Sort::Int)
            .map(|s| s.name.clone())
            .collect();
        errors.push(ValidationError::NoCruxDeclared {
            declared_dials: dials,
        });
        // With no crux declared, EVERY guard is decidable — but the headline
        // diagnosis above already says so. Don't also spam one per clause.
        return errors;
    }

    for (idx, clause) in pact.clauses.iter().enumerate() {
        let mentioned = symbols_in_formula(&clause.guard);

        // (a) every referenced symbol must be declared.
        for sym in &mentioned {
            if !declared.contains(sym) {
                errors.push(ValidationError::UndeclaredSymbol {
                    clause_index: idx,
                    clause_name: clause.name.clone(),
                    symbol: sym.clone(),
                });
            }
        }

        // (b) THE FIREWALL: the guard must branch on at least one free crux.
        let branches_on_crux = mentioned.iter().any(|m| cruxes.contains(m));
        if !branches_on_crux {
            let guard_mentions: Vec<String> = mentioned
                .iter()
                .filter(|m| declared.contains(*m))
                .cloned()
                .collect();
            errors.push(ValidationError::DecidableGuard {
                clause_index: idx,
                clause_name: clause.name.clone(),
                available_cruxes: cruxes.iter().cloned().collect(),
                guard_mentions,
            });
        }
    }

    errors
}

/// Collect every symbol *name* a formula references — the heads of all nullary
/// (and applied) term applications appearing anywhere in it. Integer literals
/// and bound `Var`s are not symbols. This is what we test crux-membership
/// against.
fn symbols_in_formula(f: &Formula) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_formula(f, &mut out);
    out
}

fn walk_formula(f: &Formula, out: &mut BTreeSet<String>) {
    match f {
        Formula::Atom(t) => walk_term(t, out),
        Formula::Eq(a, b) | Formula::Le(a, b) | Formula::Lt(a, b) => {
            walk_term(a, out);
            walk_term(b, out);
        }
        Formula::Not(p) | Formula::Obligation(p) | Formula::Permission(p) => walk_formula(p, out),
        Formula::And(ps) | Formula::Or(ps) => {
            for p in ps {
                walk_formula(p, out);
            }
        }
        Formula::Implies(a, b) | Formula::Iff(a, b) => {
            walk_formula(a, out);
            walk_formula(b, out);
        }
        Formula::Forall(_, _, body) | Formula::Exists(_, _, body) => walk_formula(body, out),
    }
}

fn walk_term(t: &Term, out: &mut BTreeSet<String>) {
    match t {
        // A bare variable (like the outcome's `award`) is not a declared symbol.
        Term::Var(_) => {}
        Term::IntLit(_) => {}
        Term::App(name, args) => {
            out.insert(name.clone());
            for a in args {
                walk_term(a, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Clause;
    use mediator_types::{Formula, Term};

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
    fn award(cents: i64) -> Formula {
        Formula::Eq(Term::Var("award".into()), Term::IntLit(cents))
    }

    /// The canonical roommate pact: every guard branches on `stain_is_damage`
    /// except `short`, which is `notice_days < 30` — purely a dial. That guard
    /// is decidable on its own and MUST be refused by the firewall.
    #[test]
    fn pure_dial_guard_is_rejected_as_value_call() {
        let pact = Pact {
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
                    outcome: award(120000),
                },
                Clause {
                    name: "short".into(),
                    guard: Formula::Lt(Term::App("notice_days".into(), vec![]), Term::IntLit(30)),
                    outcome: award(100000),
                },
            ],
        };
        let errs = validate(&pact);
        assert_eq!(errs.len(), 1, "exactly the short clause should be flagged");
        match &errs[0] {
            ValidationError::DecidableGuard {
                clause_index,
                clause_name,
                guard_mentions,
                available_cruxes,
            } => {
                assert_eq!(*clause_index, 1);
                assert_eq!(clause_name, "short");
                assert_eq!(guard_mentions, &vec!["notice_days".to_string()]);
                assert_eq!(available_cruxes, &vec!["stain_is_damage".to_string()]);
            }
            other => panic!("expected DecidableGuard, got {other:?}"),
        }
        // The plain reason names the firewall in human terms.
        assert!(errs[0].plain().contains("value-call disguised as a guard"));
        assert!(errs[0].plain().contains("stain_is_damage"));
    }

    /// A guard that branches on a free crux (even combined with a dial) passes.
    #[test]
    fn guard_branching_on_crux_passes() {
        let pact = Pact {
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
                    outcome: award(120000),
                },
                Clause {
                    name: "damaged".into(),
                    guard: atom("stain_is_damage"),
                    outcome: award(90000),
                },
            ],
        };
        assert!(validate(&pact).is_empty(), "every guard branches on the crux");
    }

    /// A pact with NO free crux declared (only dials) is rejected wholesale.
    #[test]
    fn no_crux_declared_is_rejected() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![int_sig("notice_days")],
            clauses: vec![Clause {
                name: "late".into(),
                guard: Formula::Lt(Term::App("notice_days".into(), vec![]), Term::IntLit(30)),
                outcome: award(100000),
            }],
        };
        let errs = validate(&pact);
        assert_eq!(errs.len(), 1);
        match &errs[0] {
            ValidationError::NoCruxDeclared { declared_dials } => {
                assert_eq!(declared_dials, &vec!["notice_days".to_string()]);
            }
            other => panic!("expected NoCruxDeclared, got {other:?}"),
        }
        assert!(errs[0].plain().contains("no contested question"));
    }

    /// A guard referencing an undeclared symbol is flagged.
    #[test]
    fn undeclared_symbol_is_flagged() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("stain_is_damage")],
            clauses: vec![Clause {
                name: "weird".into(),
                guard: Formula::And(vec![atom("stain_is_damage"), atom("never_declared")]),
                outcome: award(1),
            }],
        };
        let errs = validate(&pact);
        // It branches on a crux (passes the firewall) but mentions an undeclared
        // symbol — so exactly one error, the undeclared one.
        assert_eq!(errs.len(), 1);
        match &errs[0] {
            ValidationError::UndeclaredSymbol { symbol, .. } => {
                assert_eq!(symbol, "never_declared");
            }
            other => panic!("expected UndeclaredSymbol, got {other:?}"),
        }
    }

    /// An empty pact (no clauses) is rejected.
    #[test]
    fn empty_pact_is_rejected() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("stain_is_damage")],
            clauses: vec![],
        };
        let errs = validate(&pact);
        assert!(errs.iter().any(|e| matches!(e, ValidationError::NoClauses)));
    }

    /// A constants-only guard (no symbols at all) is a value-call too.
    #[test]
    fn constants_only_guard_is_rejected() {
        let pact = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("stain_is_damage")],
            clauses: vec![
                Clause {
                    name: "always".into(),
                    guard: Formula::Le(Term::IntLit(1), Term::IntLit(2)),
                    outcome: award(1),
                },
                Clause {
                    name: "real".into(),
                    guard: atom("stain_is_damage"),
                    outcome: award(2),
                },
            ],
        };
        let errs = validate(&pact);
        assert_eq!(errs.len(), 1);
        match &errs[0] {
            ValidationError::DecidableGuard {
                clause_name,
                guard_mentions,
                ..
            } => {
                assert_eq!(clause_name, "always");
                assert!(guard_mentions.is_empty(), "constants-only mentions nothing");
                assert!(errs[0].plain().contains("only constants"));
            }
            other => panic!("expected DecidableGuard, got {other:?}"),
        }
    }
}
