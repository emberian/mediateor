//! Deterministic `Term`/`Formula` → Isabelle/HOL text, for the *fragment* a pact
//! uses: atoms (nullary predicate applications), `\<not>`, `\<and>`, `\<or>`,
//! `=`, `\<le>`, `<`, integer literals, and bare variables.
//!
//! This is the SAME rendering approach as `mediator-core::render::formula_to_isabelle`,
//! kept byte-for-byte compatible on the fragment so the emitted guards/outcomes
//! match the hand-validated `isabelle/Pact.thy` shape. We re-implement it here
//! (rather than depend on mediator-core) so mediator-pact stays leaf-level.
//!
//! Pact guards mix a FREE boolean crux predicate with linear integer (cents/days)
//! arithmetic — exactly the fragment `auto` closes. We deliberately do not render
//! anything outside it; nonlinear arithmetic and deep equivalences are out of
//! scope by construction.

use mediator_types::{Formula, Sort, Term};

/// Render a `Sort` as an Isabelle type. Identical to
/// `mediator-core::render::sort_to_isabelle`.
pub fn sort_to_isabelle(s: &Sort) -> String {
    match s {
        Sort::Bool => "bool".to_string(),
        Sort::Int => "int".to_string(),
        Sort::Real => "real".to_string(),
        Sort::Uninterp(name) => name.clone(),
    }
}

/// Render a `Term` as Isabelle text. Same shape as the core renderer: nullary
/// applications and bare vars print as their name; negative int literals get
/// parens; curried application parenthesizes non-atomic args.
pub fn term_to_isabelle(t: &Term) -> String {
    match t {
        Term::Var(v) => v.clone(),
        Term::IntLit(n) => {
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

fn atom_term(t: &Term) -> String {
    match t {
        Term::Var(_) => term_to_isabelle(t),
        Term::IntLit(n) if *n >= 0 => term_to_isabelle(t),
        Term::App(_, args) if args.is_empty() => term_to_isabelle(t),
        _ => format!("({})", term_to_isabelle(t)),
    }
}

/// Render a `Formula` as an Isabelle/HOL proposition (no surrounding quotes),
/// for the pact fragment. Matches `mediator-core::render::formula_to_isabelle`
/// on every variant a pact uses.
pub fn formula_to_isabelle(f: &Formula) -> String {
    match f {
        Formula::Atom(t) => term_to_isabelle(t),
        Formula::Eq(a, b) => format!("({} = {})", term_to_isabelle(a), term_to_isabelle(b)),
        Formula::Le(a, b) => format!("({} \\<le> {})", term_to_isabelle(a), term_to_isabelle(b)),
        Formula::Lt(a, b) => format!("({} < {})", term_to_isabelle(a), term_to_isabelle(b)),
        Formula::Not(p) => format!("(\\<not> {})", formula_to_isabelle(p)),
        Formula::And(ps) => join(ps, "\\<and>", "True"),
        Formula::Or(ps) => join(ps, "\\<or>", "False"),
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
        // A pact's guards/outcomes are not deontic; render defensively rather
        // than panic, but these should never appear in a well-formed pact.
        Formula::Obligation(p) => format!("Obl ({})", formula_to_isabelle(p)),
        Formula::Permission(p) => format!("Perm ({})", formula_to_isabelle(p)),
    }
}

fn join(ps: &[Formula], op: &str, empty: &str) -> String {
    if ps.is_empty() {
        return empty.to_string();
    }
    let rendered: Vec<String> = ps.iter().map(formula_to_isabelle).collect();
    format!("({})", rendered.join(&format!(" {op} ")))
}
