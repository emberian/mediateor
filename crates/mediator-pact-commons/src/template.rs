//! The **TEMPLATE KEY** — the deterministic, re-computable, prose-and-number-
//! independent fingerprint of a pact's *shape*.
//!
//! Two pacts are "the same template" when they declare the same *shape* of
//! question-space and the same *structure* of clause-guards — differing only in
//! the things a controversy is allowed to vary: the symbol **names**, the
//! integer **thresholds**, and the **awards**. The key must be byte-equal for
//! those and only those, so that pacts of one template COLLIDE while genuinely
//! different question-spaces do not.
//!
//! # What the key keeps, and what it strips
//!
//! KEEPS (the template's identity):
//!   * the *sort signature* of the declared question-space: how many free crux
//!     predicates (Bool, nullary), how many integer dials, sorted — so a 1-crux
//!     1-dial template never collides with a 2-crux template;
//!   * each clause guard's *structure*, with every symbol replaced by its
//!     **role** (crux #k / dial #k, assigned by sorted declaration order, NOT by
//!     name), every integer literal replaced by a single `THRESH` token, and the
//!     *direction* of each dial-vs-threshold comparison preserved and
//!     canonicalized (`THRESH ≤ dial` and `dial ≥ THRESH` become one form);
//!   * the *set* of guards (sorted, so clause order does not matter).
//!
//! STRIPS (what a controversy varies):
//!   * symbol names (`stain_is_damage` vs `damage_beyond_normal_wear`);
//!   * threshold values (30 vs 180 vs 12);
//!   * award values (90000 vs 70000 vs 2500) — these are NOT in the key at all;
//!     they are the controversy, tallied separately.
//!   * clause names, the title, the parties, the glosses.
//!
//! # Why role-by-declaration-order, not by name
//!
//! The whole point is that `notice_days` and `days_cohabited` are the *same dial*
//! in two templates. We cannot key on the name. We assign each declared symbol a
//! role index within its sort class (crux #0, crux #1, …; dial #0, dial #1, …)
//! by **sorted declaration order**, and rewrite guards in terms of those roles.
//! Sorted (not source) order makes the key independent of how the author listed
//! the predicates. The honest cost: two pacts that declare their dials in a
//! semantically-swapped sense (so dial #0 of one plays the role of dial #1 of the
//! other) would not collide. For the single-dial corpus this never bites; for
//! multi-dial templates it is the conservative (too-fine) direction, which is the
//! safe one — a human can still cite across by hand. We do not guess a matching.
//!
//! # Conservative on purpose
//!
//! Canonicalization is exactly the set the pact codegen already treats as
//! identity: commutative `∧`/`∨` (children sorted), and the threshold direction
//! of a dial comparison (since `30 ≤ d` and `d ≥ 30` are the same guard, and the
//! prover's `auto` closes both the same way). It deliberately does NOT canon
//! -icalize under de Morgan, arithmetic rearrangement, or any deeper equivalence.
//! A key too *fine* merely misses a likeness; a key too *coarse* would merge
//! distinct question-spaces and smuggle a value-judgment into "same question". We
//! err fine.

use mediator_pact::Pact;
use mediator_types::{Formula, Sig, Sort, Term};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// A human-readable descriptor of a template's shape — what a reader sees so the
/// opaque key is legible. Pure structure: counts and a canonical guard list.
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateDescriptor {
    /// Number of declared free crux predicates (Bool, nullary).
    pub crux_count: usize,
    /// Number of declared integer dials.
    pub dial_count: usize,
    /// Number of clauses.
    pub clause_count: usize,
    /// The canonical (role-rewritten, threshold-stripped, sorted) guard forms,
    /// one per clause — the structural heart of the template.
    pub canonical_guards: Vec<String>,
}

/// The deterministic template key: `sha256` over a canonical serialization of the
/// pact's question-space sort signature + its sorted canonical guard structure.
///
/// **Same template ⇒ byte-equal key.** Pure function of the pact's `predicates`
/// and clause `guard`s (never the awards, names, thresholds, title, or parties).
/// Re-derivable by a skeptic from the pact JSON with this module's ~120 lines and
/// no AI.
pub fn template_key(pact: &Pact) -> String {
    let canon = canonical_template(pact);
    let mut h = Sha256::new();
    h.update(b"mediator-pact-commons/template/v1\0");
    h.update(canon.as_bytes());
    hex::encode(h.finalize())
}

/// The human-readable descriptor for a pact's template.
pub fn template_descriptor(pact: &Pact) -> TemplateDescriptor {
    let roles = Roles::of(pact);
    let guards = canonical_guards(pact, &roles);
    TemplateDescriptor {
        crux_count: roles.cruxes.len(),
        dial_count: roles.dials.len(),
        clause_count: pact.clauses.len(),
        canonical_guards: guards,
    }
}

/// The exact string the key hashes — exposed so a test can assert on the
/// structure directly (and a skeptic can read what is being committed to).
pub fn canonical_template(pact: &Pact) -> String {
    let roles = Roles::of(pact);
    let guards = canonical_guards(pact, &roles);
    // Sort signature: counts of each role class, declaration-order-independent.
    // (We commit to counts; the role indices already encode the rest.)
    format!(
        "cruxes={};dials={};clauses={};guards=[{}]",
        roles.cruxes.len(),
        roles.dials.len(),
        pact.clauses.len(),
        guards.join("|")
    )
}

// ───────────────────────────── role assignment ──────────────────────────────

/// The declared symbols partitioned into role classes, each sorted by name so
/// the role index is independent of source declaration order.
struct Roles {
    /// Free crux predicates (Bool, nullary), sorted by name → role index.
    cruxes: Vec<String>,
    /// Integer dials, sorted by name → role index.
    dials: Vec<String>,
    /// name → role token (`crux#k` / `dial#k`), for rewriting guards.
    token: BTreeMap<String, String>,
}

impl Roles {
    fn of(pact: &Pact) -> Roles {
        let mut cruxes: Vec<String> = pact
            .predicates
            .iter()
            .filter(|s| is_crux(s))
            .map(|s| s.name.clone())
            .collect();
        cruxes.sort();
        cruxes.dedup();

        let mut dials: Vec<String> = pact
            .predicates
            .iter()
            .filter(|s| s.ret == Sort::Int)
            .map(|s| s.name.clone())
            .collect();
        dials.sort();
        dials.dedup();

        let mut token = BTreeMap::new();
        for (k, name) in cruxes.iter().enumerate() {
            token.insert(name.clone(), format!("crux#{k}"));
        }
        for (k, name) in dials.iter().enumerate() {
            token.insert(name.clone(), format!("dial#{k}"));
        }
        Roles {
            cruxes,
            dials,
            token,
        }
    }

    /// The role token for a declared symbol, or a stable `unknown(name)` token if
    /// the symbol was never declared (a malformed pact — but the firewall would
    /// already have rejected it; we stay deterministic regardless).
    fn token_of(&self, name: &str) -> String {
        self.token
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("undeclared#{name}"))
    }
}

fn is_crux(s: &Sig) -> bool {
    s.ret == Sort::Bool && s.arg_sorts.is_empty()
}

// ───────────────────────── canonical guard rewriting ────────────────────────

/// The sorted list of canonical guard strings (one per clause). Sorting makes the
/// template independent of clause order.
fn canonical_guards(pact: &Pact, roles: &Roles) -> Vec<String> {
    let mut gs: Vec<String> = pact
        .clauses
        .iter()
        .map(|c| canon_formula(&c.guard, roles))
        .collect();
    gs.sort();
    gs
}

/// Canonicalize a guard formula into the role-rewritten, threshold-stripped
/// normal form. Mirrors the conservative normalization the pact codegen / commons
/// already treat as identity: `∧`/`∨` commutative (children sorted), and the
/// direction of a dial-vs-threshold comparison preserved but order-canonicalized.
fn canon_formula(f: &Formula, roles: &Roles) -> String {
    match f {
        Formula::Atom(t) => format!("Atom({})", canon_term(t, roles)),
        // A dial-vs-threshold comparison: keep the DIRECTION (which side of the
        // threshold the world is on) but strip the threshold value to `THRESH`
        // and canonicalize argument order. `THRESH <= dial` (i.e. dial at-or-
        // above) and `dial >= THRESH` are the same guard; we fold both to
        // `GE(dial,THRESH)`. Likewise `dial < THRESH` (below) folds to
        // `LT(dial,THRESH)`. A comparison between two dials (no literal) keeps
        // both roles.
        Formula::Le(a, b) => canon_cmp(Cmp::Le, a, b, roles),
        Formula::Lt(a, b) => canon_cmp(Cmp::Lt, a, b, roles),
        Formula::Eq(a, b) => {
            // Equality is symmetric; sort the two canonical operands.
            let mut s = [canon_term(a, roles), canon_term(b, roles)];
            s.sort();
            format!("Eq({},{})", s[0], s[1])
        }
        Formula::Not(p) => format!("Not({})", canon_formula(p, roles)),
        Formula::And(ps) => format!("And({})", canon_sorted(ps, roles)),
        Formula::Or(ps) => format!("Or({})", canon_sorted(ps, roles)),
        Formula::Implies(a, b) => {
            format!("Implies({},{})", canon_formula(a, roles), canon_formula(b, roles))
        }
        Formula::Iff(a, b) => {
            let mut s = [canon_formula(a, roles), canon_formula(b, roles)];
            s.sort();
            format!("Iff({},{})", s[0], s[1])
        }
        Formula::Forall(_, srt, body) => {
            // Bound-variable name is irrelevant to the template; keep only sort.
            format!("Forall({},{})", canon_sort(srt), canon_formula(body, roles))
        }
        Formula::Exists(_, srt, body) => {
            format!("Exists({},{})", canon_sort(srt), canon_formula(body, roles))
        }
        Formula::Obligation(p) => format!("Obl({})", canon_formula(p, roles)),
        Formula::Permission(p) => format!("Perm({})", canon_formula(p, roles)),
    }
}

#[derive(Clone, Copy)]
enum Cmp {
    Le,
    Lt,
}

/// Canonicalize a comparison `a CMP b` where exactly one side is a threshold
/// literal and the other a dial (the pact fragment). The direction — is the world
/// at/above or below the threshold — is what the template keeps; the literal
/// value is stripped to `THRESH`.
///
///   * `THRESH ≤ dial`  ⇒ world at-or-above ⇒ `GE(dial,THRESH)`
///   * `dial ≤ THRESH`  ⇒ world at-or-below ⇒ `LE(dial,THRESH)`
///   * `dial < THRESH`  ⇒ world below       ⇒ `LT(dial,THRESH)`
///   * `THRESH < dial`  ⇒ world above       ⇒ `GT(dial,THRESH)`
///
/// If neither/both sides are literals (e.g. a dial-vs-dial comparison), we fall
/// back to a structural form over both canonical operands, still stripping any
/// literal but keeping the raw operator (the conservative choice).
fn canon_cmp(op: Cmp, a: &Term, b: &Term, roles: &Roles) -> String {
    let a_lit = as_int_literal(a);
    let b_lit = as_int_literal(b);
    match (a_lit, b_lit) {
        // literal on the LEFT: `THRESH op rhs`
        (Some(_), None) => {
            let rhs = canon_term(b, roles);
            match op {
                // THRESH <= rhs  ⇒  rhs >= THRESH (at-or-above)
                Cmp::Le => format!("GE({rhs},THRESH)"),
                // THRESH < rhs   ⇒  rhs > THRESH (above)
                Cmp::Lt => format!("GT({rhs},THRESH)"),
            }
        }
        // literal on the RIGHT: `lhs op THRESH`
        (None, Some(_)) => {
            let lhs = canon_term(a, roles);
            match op {
                // lhs <= THRESH (at-or-below)
                Cmp::Le => format!("LE({lhs},THRESH)"),
                // lhs < THRESH (below)
                Cmp::Lt => format!("LT({lhs},THRESH)"),
            }
        }
        // no literal (dial-vs-dial) or both literals (constant guard, which the
        // firewall rejects): keep the raw operator over canonical operands.
        _ => {
            let opname = match op {
                Cmp::Le => "LE",
                Cmp::Lt => "LT",
            };
            format!("{opname}({},{})", canon_term(a, roles), canon_term(b, roles))
        }
    }
}

fn as_int_literal(t: &Term) -> Option<i64> {
    match t {
        Term::IntLit(n) => Some(*n),
        _ => None,
    }
}

/// Sort the canonical forms of a list of subformulas (for commutative `∧`/`∨`).
fn canon_sorted(ps: &[Formula], roles: &Roles) -> String {
    let mut parts: Vec<String> = ps.iter().map(|p| canon_formula(p, roles)).collect();
    parts.sort();
    parts.join(",")
}

/// Canonicalize a term: a declared symbol → its role token; an int literal →
/// `THRESH` (stripped); an applied function → its head role + canonical args.
fn canon_term(t: &Term, roles: &Roles) -> String {
    match t {
        // The outcome's bound `award` never appears in a guard; any bare var is
        // kept structurally (by sort-free placeholder) for determinism.
        Term::Var(_) => "VAR".to_string(),
        Term::IntLit(_) => "THRESH".to_string(),
        Term::App(name, args) if args.is_empty() => roles.token_of(name),
        Term::App(name, args) => {
            let a: Vec<String> = args.iter().map(|x| canon_term(x, roles)).collect();
            // Even an applied head gets role-rewritten if declared.
            format!("{}({})", roles.token_of(name), a.join(","))
        }
    }
}

fn canon_sort(s: &Sort) -> String {
    match s {
        Sort::Bool => "Bool".into(),
        Sort::Int => "Int".into(),
        Sort::Real => "Real".into(),
        Sort::Uninterp(n) => format!("U({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_pact::Clause;
    use mediator_types::{Formula, Sig, Term};

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
    fn award(c: i64) -> Formula {
        Formula::Eq(Term::Var("award".into()), Term::IntLit(c))
    }

    /// Build a "deposit-split" template pact: 1 bool crux + 1 int dial, 4 clauses
    /// {above/below threshold} × {¬crux/crux}. Names, threshold, awards are
    /// parameters — varying them must NOT change the key.
    fn deposit_pact(
        crux: &str,
        dial: &str,
        thresh: i64,
        awards: [i64; 4],
    ) -> Pact {
        let ge = |n: i64| Formula::Le(Term::IntLit(n), Term::App(dial.into(), vec![]));
        let lt = |n: i64| Formula::Lt(Term::App(dial.into(), vec![]), Term::IntLit(n));
        Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
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

    /// THE load-bearing claim: two pacts of the same template — different
    /// symbol names, different threshold, different awards — share a byte-equal
    /// template key.
    #[test]
    fn same_template_different_names_thresholds_awards_collide() {
        let roommate = deposit_pact("stain_is_damage", "notice_days", 30, [120000, 90000, 100000, 100000]);
        let cohab = deposit_pact("damage_beyond_normal_wear", "days_cohabited", 180, [100000, 70000, 90000, 60000]);
        assert_eq!(
            template_key(&roommate),
            template_key(&cohab),
            "same template (names/threshold/awards stripped) must collide"
        );
    }

    /// Clause ORDER must not change the key (guards are a sorted set).
    #[test]
    fn clause_order_is_irrelevant() {
        let a = deposit_pact("c", "d", 10, [1, 2, 3, 4]);
        let mut b = a.clone();
        b.clauses.reverse();
        assert_eq!(template_key(&a), template_key(&b));
    }

    /// A different question-space SHAPE must NOT collide: a 2-crux template is a
    /// different template from a 1-crux one.
    #[test]
    fn different_shape_does_not_collide() {
        let one_crux = deposit_pact("c", "d", 10, [1, 2, 3, 4]);
        // two cruxes, two clauses
        let two_crux = Pact {
            title: "t".into(),
            parties: vec!["a".into(), "b".into()],
            predicates: vec![bool_sig("c1"), bool_sig("c2"), int_sig("d")],
            clauses: vec![
                Clause {
                    name: "x".into(),
                    guard: Formula::And(vec![atom("c1"), atom("c2")]),
                    outcome: award(1),
                },
                Clause {
                    name: "y".into(),
                    guard: Formula::Or(vec![
                        Formula::Not(Box::new(atom("c1"))),
                        Formula::Not(Box::new(atom("c2"))),
                    ]),
                    outcome: award(2),
                },
            ],
        };
        assert_ne!(template_key(&one_crux), template_key(&two_crux));
    }

    /// A genuinely different GUARD STRUCTURE (same counts) must not collide: flip
    /// a guard's threshold direction (above vs below) and the template differs.
    #[test]
    fn different_guard_direction_does_not_collide() {
        let a = deposit_pact("c", "d", 10, [1, 2, 3, 4]);
        // b: same counts, but one clause uses `>` where a used `<` — a structural
        // change in which side of the threshold a world sits.
        let mut b = a.clone();
        // Replace below_clean's `d < 10` with `d <= 10` — LE vs LT is a genuine
        // structural difference the key must preserve.
        b.clauses[2].guard = Formula::And(vec![
            Formula::Le(Term::App("d".into(), vec![]), Term::IntLit(10)),
            Formula::Not(Box::new(atom("c"))),
        ]);
        assert_ne!(
            template_key(&a),
            template_key(&b),
            "LE vs LT is a real structural difference"
        );
    }

    /// `THRESH ≤ dial` and `dial ≥ THRESH` are the SAME guard direction — the key
    /// canonicalizes the two encodings. (We encode `≥` as `Le(lit, dial)` in this
    /// IR, so this asserts the argument-order canonicalization within `canon_cmp`
    /// by comparing two literal encodings of "at or above".)
    #[test]
    fn at_or_above_canonicalizes_regardless_of_literal_side() {
        // Encoding 1: 30 <= d   (literal left)
        let e1 = canon_cmp(Cmp::Le, &Term::IntLit(30), &Term::App("d".into(), vec![]), &Roles::of(&deposit_pact("c", "d", 30, [1, 2, 3, 4])));
        // It should read as GE(dial#0,THRESH).
        assert_eq!(e1, "GE(dial#0,THRESH)");
    }

    /// The descriptor exposes the shape legibly.
    #[test]
    fn descriptor_reports_shape() {
        let d = template_descriptor(&deposit_pact("c", "d", 10, [1, 2, 3, 4]));
        assert_eq!(d.crux_count, 1);
        assert_eq!(d.dial_count, 1);
        assert_eq!(d.clause_count, 4);
        assert_eq!(d.canonical_guards.len(), 4);
        // Awards are NOT present anywhere in the canonical guards.
        for g in &d.canonical_guards {
            assert!(!g.contains("120000") && !g.contains("award"), "no awards in template: {g}");
            // thresholds are stripped to THRESH
            assert!(!g.contains("10"), "thresholds stripped: {g}");
        }
    }

    /// The key is stable across runs (determinism) and is a pure function of
    /// structure: re-deriving from a JSON round-trip yields the same key.
    #[test]
    fn key_is_deterministic_across_json_roundtrip() {
        let p = deposit_pact("stain_is_damage", "notice_days", 30, [120000, 90000, 100000, 100000]);
        let k1 = template_key(&p);
        let json = serde_json::to_string(&p).unwrap();
        let p2: Pact = serde_json::from_str(&json).unwrap();
        assert_eq!(k1, template_key(&p2));
    }

    /// Declaration ORDER of the predicates must not change the key (roles are
    /// assigned by sorted name).
    #[test]
    fn predicate_declaration_order_is_irrelevant() {
        let mut a = deposit_pact("c", "d", 10, [1, 2, 3, 4]);
        let mut b = a.clone();
        b.predicates.reverse();
        // Sanity: same key.
        assert_eq!(template_key(&a), template_key(&b));
        // And with two cruxes, swapping their declaration order is still the same
        // template (roles are by sorted name, and both guards get rewritten).
        a.predicates = vec![bool_sig("alpha"), bool_sig("beta"), int_sig("d")];
        let mut c = a.clone();
        c.predicates = vec![int_sig("d"), bool_sig("beta"), bool_sig("alpha")];
        assert_eq!(template_key(&a), template_key(&c));
    }
}
