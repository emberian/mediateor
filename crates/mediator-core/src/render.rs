//! Deterministic renderers: `Term`/`Formula` → Isabelle/HOL text, and
//! `Formula` → plain English. The Isabelle renderer is what makes the kernel's
//! reductions checkable; the English renderer is the trusted prose the two UX
//! views show (the LLM's prose is only ever advisory).

use mediator_types::{Formula, Sort, Term};

// ───────────────────────────── Isabelle/HOL ─────────────────────────────

/// Render a `Sort` as an Isabelle type. `Real` maps to HOL `real`;
/// uninterpreted sorts become a fresh `'a`-free named type (declared in the
/// preamble as a `typedecl`). We keep it simple: `Uninterp(n)` → `n`.
pub fn sort_to_isabelle(s: &Sort) -> String {
    match s {
        Sort::Bool => "bool".to_string(),
        Sort::Int => "int".to_string(),
        Sort::Real => "real".to_string(),
        Sort::Uninterp(name) => name.clone(),
    }
}

/// Render a function type `arg_sorts => ret` for an `axiomatization` entry.
/// Nullary => just the return type (a constant).
pub fn fun_type_to_isabelle(arg_sorts: &[Sort], ret: &Sort) -> String {
    if arg_sorts.is_empty() {
        sort_to_isabelle(ret)
    } else {
        let mut parts: Vec<String> = arg_sorts.iter().map(sort_to_isabelle).collect();
        parts.push(sort_to_isabelle(ret));
        parts.join(" => ")
    }
}

/// Render a `Term` as Isabelle text.
pub fn term_to_isabelle(t: &Term) -> String {
    match t {
        Term::Var(v) => v.clone(),
        Term::IntLit(n) => {
            // Negative integer literals need parens to bind correctly.
            if *n < 0 {
                format!("({n})")
            } else {
                n.to_string()
            }
        }
        Term::App(f, args) => {
            if args.is_empty() {
                f.clone()
            } else {
                let rendered: Vec<String> = args.iter().map(atom_term).collect();
                format!("{f} {}", rendered.join(" "))
            }
        }
    }
}

/// A term in argument position: parenthesize anything that isn't already atomic
/// so curried application stays well-formed (`f (g x) y`).
fn atom_term(t: &Term) -> String {
    match t {
        Term::Var(_) => term_to_isabelle(t),
        Term::IntLit(n) if *n >= 0 => term_to_isabelle(t),
        Term::App(_, args) if args.is_empty() => term_to_isabelle(t),
        _ => format!("({})", term_to_isabelle(t)),
    }
}

/// Render a `Formula` as an Isabelle/HOL proposition (no surrounding quotes).
///
/// Deontic operators are rendered with a *provisional* shallow embedding:
/// `Obligation p` → `Obl (p)`, `Permission p` → `Perm (p)`, with `Obl`/`Perm`
/// declared as `bool => bool` in the preamble. This is a placeholder for the
/// CondNormReasHOL dyadic-deontic embedding and is documented as such.
pub fn formula_to_isabelle(f: &Formula) -> String {
    match f {
        Formula::Atom(t) => term_to_isabelle(t),
        Formula::Eq(a, b) => format!("({} = {})", term_to_isabelle(a), term_to_isabelle(b)),
        Formula::Le(a, b) => {
            format!("({} \\<le> {})", term_to_isabelle(a), term_to_isabelle(b))
        }
        Formula::Lt(a, b) => format!("({} < {})", term_to_isabelle(a), term_to_isabelle(b)),
        Formula::Not(p) => format!("(\\<not> {})", formula_to_isabelle(p)),
        Formula::And(ps) => join_formulas(ps, "\\<and>", "True"),
        Formula::Or(ps) => join_formulas(ps, "\\<or>", "False"),
        Formula::Implies(a, b) => format!(
            "({} \\<longrightarrow> {})",
            formula_to_isabelle(a),
            formula_to_isabelle(b)
        ),
        Formula::Iff(a, b) => format!(
            "({} \\<longleftrightarrow> {})",
            formula_to_isabelle(a),
            formula_to_isabelle(b)
        ),
        Formula::Forall(v, s, body) => format!(
            "(\\<forall>{}::{}. {})",
            v,
            sort_to_isabelle(s),
            formula_to_isabelle(body)
        ),
        Formula::Exists(v, s, body) => format!(
            "(\\<exists>{}::{}. {})",
            v,
            sort_to_isabelle(s),
            formula_to_isabelle(body)
        ),
        // Provisional shallow deontic forms; see preamble declarations.
        Formula::Obligation(p) => format!("Obl ({})", formula_to_isabelle(p)),
        Formula::Permission(p) => format!("Perm ({})", formula_to_isabelle(p)),
    }
}

fn join_formulas(ps: &[Formula], op: &str, empty: &str) -> String {
    if ps.is_empty() {
        return empty.to_string();
    }
    let rendered: Vec<String> = ps.iter().map(formula_to_isabelle).collect();
    format!("({})", rendered.join(&format!(" {op} ")))
}

/// True iff this formula uses a deontic operator anywhere, so the preamble
/// knows to declare `Obl`/`Perm`.
pub fn uses_deontic(f: &Formula) -> bool {
    match f {
        Formula::Obligation(_) | Formula::Permission(_) => true,
        Formula::Atom(_) | Formula::Eq(..) | Formula::Le(..) | Formula::Lt(..) => false,
        Formula::Not(p) => uses_deontic(p),
        Formula::And(ps) | Formula::Or(ps) => ps.iter().any(uses_deontic),
        Formula::Implies(a, b) | Formula::Iff(a, b) => uses_deontic(a) || uses_deontic(b),
        Formula::Forall(_, _, b) | Formula::Exists(_, _, b) => uses_deontic(b),
    }
}

// ─────────────────────────────── English ────────────────────────────────

/// Render a `Formula` as a plain-English sentence (deterministic, trusted).
/// Used for `shared_core` and `dissolved` so the prose the humans see does not
/// depend on the LLM. Money atoms (`*_total`, `*_cents`, equalities with an int
/// literal) are formatted as dollars when they look monetary.
pub fn formula_to_english(f: &Formula) -> String {
    let s = render_eng(f);
    // Capitalize first letter, end with a period.
    let mut c = s.chars();
    let first = c.next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default();
    let rest: String = c.collect();
    let body = format!("{first}{rest}");
    if body.ends_with('.') {
        body
    } else {
        format!("{body}.")
    }
}

fn render_eng(f: &Formula) -> String {
    match f {
        Formula::Atom(t) => predicate_phrase(t),
        Formula::Not(inner) => match &**inner {
            Formula::Atom(t) => format!("it is not the case that {}", predicate_phrase(t)),
            other => format!("it is not the case that {}", render_eng(other)),
        },
        Formula::Eq(a, b) => format!("{} equals {}", term_phrase(a), term_phrase(b)),
        Formula::Le(a, b) => format!("{} is at most {}", term_phrase(a), term_phrase(b)),
        Formula::Lt(a, b) => format!("{} is less than {}", term_phrase(a), term_phrase(b)),
        Formula::And(ps) => ps.iter().map(render_eng).collect::<Vec<_>>().join(", and "),
        Formula::Or(ps) => ps.iter().map(render_eng).collect::<Vec<_>>().join(", or "),
        Formula::Implies(a, b) => format!("if {}, then {}", render_eng(a), render_eng(b)),
        Formula::Iff(a, b) => {
            format!("{} exactly when {}", render_eng(a), render_eng(b))
        }
        Formula::Forall(v, _, body) => format!("for every {v}, {}", render_eng(body)),
        Formula::Exists(v, _, body) => format!("there is some {v} such that {}", render_eng(body)),
        Formula::Obligation(p) => format!("it is required that {}", render_eng(p)),
        Formula::Permission(p) => format!("it is permitted that {}", render_eng(p)),
    }
}

/// A predicate application as an English clause: `stain_is_damage` →
/// "the carpet stain is chargeable damage" via humanized identifier.
fn predicate_phrase(t: &Term) -> String {
    match t {
        Term::App(name, args) if args.is_empty() => humanize_predicate(name),
        _ => term_phrase(t),
    }
}

fn term_phrase(t: &Term) -> String {
    match t {
        Term::Var(v) => v.clone(),
        Term::IntLit(n) => money(*n),
        Term::App(name, args) if args.is_empty() => humanize_noun(name),
        Term::App(name, args) => {
            let inner: Vec<String> = args.iter().map(term_phrase).collect();
            format!("{} of {}", humanize_noun(name), inner.join(" and "))
        }
    }
}

/// Format integer cents as dollars, e.g. 45000 → "$450.00".
pub fn money(cents: i64) -> String {
    let neg = cents < 0;
    let abs = cents.unsigned_abs();
    let dollars = abs / 100;
    let rem = abs % 100;
    let sign = if neg { "-" } else { "" };
    format!("{sign}${dollars}.{rem:02}")
}

/// Turn a snake_case predicate id into a clause: `stain_is_damage` →
/// "the carpet stain is chargeable damage". We special-case the known
/// roommate-scenario predicates for readability; everything else gets a
/// generic humanization so the renderer stays total.
fn humanize_predicate(name: &str) -> String {
    match name {
        "stain_is_damage" => "the carpet stain is chargeable damage".to_string(),
        "tenant_owes_carpet" => "the tenant must bear the carpet repair cost".to_string(),
        _ => humanize_noun(name),
    }
}

fn humanize_noun(name: &str) -> String {
    match name {
        "claimed_total" => "the verbally claimed deduction total".to_string(),
        "itemized_total" => "the itemized deduction total".to_string(),
        _ => name.replace('_', " "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::Term;

    fn atom(n: &str) -> Formula {
        Formula::Atom(Term::App(n.to_string(), vec![]))
    }

    #[test]
    fn renders_iff_isabelle() {
        let f = Formula::Iff(Box::new(atom("a")), Box::new(atom("b")));
        assert_eq!(formula_to_isabelle(&f), "(a \\<longleftrightarrow> b)");
    }

    #[test]
    fn renders_eq_with_intlit() {
        let f = Formula::Eq(Term::App("claimed_total".into(), vec![]), Term::IntLit(50000));
        assert_eq!(formula_to_isabelle(&f), "(claimed_total = 50000)");
    }

    #[test]
    fn renders_not_and_le_lt() {
        let f = Formula::Not(Box::new(atom("p")));
        assert_eq!(formula_to_isabelle(&f), "(\\<not> p)");
        let le = Formula::Le(Term::Var("x".into()), Term::IntLit(3));
        assert_eq!(formula_to_isabelle(&le), "(x \\<le> 3)");
        let lt = Formula::Lt(Term::Var("x".into()), Term::IntLit(3));
        assert_eq!(formula_to_isabelle(&lt), "(x < 3)");
    }

    #[test]
    fn renders_quantifiers() {
        let f = Formula::Forall(
            "x".into(),
            Sort::Int,
            Box::new(Formula::Le(Term::Var("x".into()), Term::Var("x".into()))),
        );
        assert_eq!(formula_to_isabelle(&f), "(\\<forall>x::int. (x \\<le> x))");
    }

    #[test]
    fn renders_deontic_provisional() {
        let f = Formula::Obligation(Box::new(atom("pay")));
        assert_eq!(formula_to_isabelle(&f), "Obl (pay)");
        assert!(uses_deontic(&f));
        assert!(!uses_deontic(&atom("pay")));
    }

    #[test]
    fn money_formats_cents() {
        assert_eq!(money(45000), "$450.00");
        assert_eq!(money(105000), "$1050.00");
        assert_eq!(money(7), "$0.07");
        assert_eq!(money(-300), "-$3.00");
    }

    #[test]
    fn english_render_is_deterministic_and_kind() {
        let f = Formula::Not(Box::new(atom("stain_is_damage")));
        assert_eq!(
            formula_to_english(&f),
            "It is not the case that the carpet stain is chargeable damage."
        );
        let iff = Formula::Iff(
            Box::new(atom("tenant_owes_carpet")),
            Box::new(atom("stain_is_damage")),
        );
        assert_eq!(
            formula_to_english(&iff),
            "The tenant must bear the carpet repair cost exactly when the carpet stain is chargeable damage."
        );
    }
}
