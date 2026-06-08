//! FORWARD CONSTITUTIONS — the certified-pact gallery, in the room's calm voice.
//!
//! "Don't mediate the breakup; prove the relationship resolves every breakup it
//! named." A pact is a two-party agreement that, **before any dispute**, was run
//! through the real Isabelle gate (off the box, by the anchor worker) and got a
//! signed certificate: over its own *declared question-space* it is COMPLETE
//! (every situation you named is handled by some clause) and NON-CONTRADICTORY
//! (no two rules can both apply and disagree) — while the genuinely human
//! value-question (the future crux) stays *uninterpreted*, handed back to you.
//!
//! This box has **no Isabelle**. It serves what the worker already proved: each
//! pact ships as `scenarios/pacts/<name>.json` (the agreement) beside
//! `scenarios/pacts/<name>.cert.json` (the CACHED, signed certificate). We render
//! THOSE. We never run the gate, and we never imply this box certified anything.
//!
//! The "Draft your own" panel uses [`mediator_pact::pact_codegen`] — pure text
//! emission, no prover — to reveal the machine-checkable promise a pact compiles
//! to, framed honestly as "this is what gets certified where the prover lives".
//!
//! Two routes, added beside the live room:
//!
//! - `GET /pacts` — the calm gallery of certified pact templates.
//! - `GET /pact/:id` — one pact: its open questions, its warm clauses, its
//!   verdict from the cached cert, the quiet provenance, and the draft-your-own.

use std::path::{Path, PathBuf};

use axum::{
    extract::{Path as AxPath, State},
    http::StatusCode,
    response::IntoResponse,
};
use maud::{Markup, PreEscaped, html};
use mediator_pact::{InconsistencyWitness, Pact, PactCertificate, Status};
use mediator_types::{Formula, Sig, Sort, Term};

use crate::{SharedState, brandbar, money, not_found, page};

// ─────────────────────────── loading the corpus ──────────────────────────────

/// One loaded pact: the agreement plus its CACHED, signed certificate (what the
/// anchor worker proved through the real gate; this box only renders it).
pub struct PactRecord {
    /// Stable id from the file stem (`roommate_moveout`), used in `/pact/:id`.
    pub id: String,
    pub pact: Pact,
    pub cert: PactCertificate,
}

/// Locate `scenarios/pacts/`. Reuses the same resolution as the dispute loader
/// (env `MEDIATEOR_SCENARIOS`, then crate-relative, then cwd), with `pacts/`
/// appended — so a box that already serves disputes finds the pact corpus too.
pub fn pacts_dir() -> PathBuf {
    crate::scenarios_dir().join("pacts")
}

/// Discover every `scenarios/pacts/*.json` (the agreements, excluding the
/// `.cert.json` sidecars) that has a sibling cached certificate, and load both.
///
/// A pact with no cached cert is skipped (this box can't prove it — only render
/// what was already proved); a malformed file is skipped with a warning so one
/// bad pact never takes the gallery down. Mirrors `load::discover_disputes`.
pub fn discover_pacts(dir: &Path) -> Vec<PactRecord> {
    let mut records = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        // No pact corpus on this box — fine; the gallery just shows empty state.
        return records;
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".json") && !name.ends_with(".cert.json")
        })
        .collect();
    paths.sort();

    for path in paths {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("pact")
            .to_string();
        match load_pact_record(&id, &path) {
            Ok(rec) => records.push(rec),
            Err(e) => eprintln!("⚠  skipping pact {id}: {e}"),
        }
    }
    records
}

/// Load one pact + its sibling cached certificate. The cert is REQUIRED: this box
/// renders proved results, it does not run the gate.
fn load_pact_record(id: &str, pact_path: &Path) -> anyhow::Result<PactRecord> {
    let praw = std::fs::read_to_string(pact_path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", pact_path.display()))?;
    let pact: Pact = serde_json::from_str(&praw)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", pact_path.display()))?;

    let cert_path = sibling_cert(pact_path);
    let craw = std::fs::read_to_string(&cert_path).map_err(|e| {
        anyhow::anyhow!(
            "no cached certificate beside {} ({e}); this box renders proved pacts only",
            pact_path.display()
        )
    })?;
    let cert: PactCertificate = serde_json::from_str(&craw)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", cert_path.display()))?;

    Ok(PactRecord { id: id.to_string(), pact, cert })
}

/// `…/roommate_moveout.json` → `…/roommate_moveout.cert.json`.
fn sibling_cert(pact_path: &Path) -> PathBuf {
    let stem = pact_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("pact");
    pact_path.with_file_name(format!("{stem}.cert.json"))
}

// ─────────────────── warm language: parties, questions ───────────────────────

/// "Robin (moving out)" → "Robin". (Local copy of the gallery's first-name trim,
/// so the pact pages read in the same warm voice without reaching across.)
fn first_name(display: &str) -> String {
    display
        .split([' ', '(', ','])
        .next()
        .unwrap_or(display)
        .trim()
        .to_string()
}

/// The two parties' first names joined warmly: "Robin & Sam".
fn parties_line(pact: &Pact) -> String {
    let names: Vec<String> = pact.parties.iter().map(|p| first_name(p)).collect();
    match names.as_slice() {
        [a, b] => format!("{a} & {b}"),
        _ => names.join(" & "),
    }
}

/// The declared *question-space*, in plain language: "the questions you're
/// agreeing to leave open between you". Each contested value-predicate (a free
/// Bool) becomes a warm "whether …" line from its gloss; each agreed dial (an
/// Int) becomes a "you'll measure …" line. These are exactly the symbols the
/// certificate is *relative to* — naming them is the honesty.
struct DeclaredQuestion {
    /// `true` = a contested value-question left open (the future crux); `false` =
    /// an agreed, measurable dial both sides accept up front.
    open: bool,
    text: String,
}

fn declared_questions(pact: &Pact) -> Vec<DeclaredQuestion> {
    pact.predicates
        .iter()
        .map(|sig| {
            let open = is_value_predicate(sig);
            let g = clean_gloss(sig);
            let text = if open {
                // A contested yes/no question: "Whether the stain counts as damage".
                // Use the declarative core so a "whether …" gloss isn't doubled. Do
                // NOT force-lowercase it — the gloss already cases the first word
                // right (common noun lowercase, proper noun like "Avery's" upper).
                format!("Whether {}", declarative(&g))
            } else if starts_interrogative(&g) {
                // The dial gloss is already a "how many / how much …" question —
                // just present it as one (capitalised), no doubled "measure".
                upper_first(&g)
            } else {
                format!("What we agree on for {}", lower_first(&g))
            };
            DeclaredQuestion { open, text }
        })
        .collect()
}

/// Does a phrase already open with an interrogative we can present as-is?
fn starts_interrogative(s: &str) -> bool {
    let l = s.trim_start().to_ascii_lowercase();
    l.starts_with("how many ")
        || l.starts_with("how much ")
        || l.starts_with("how long ")
        || l.starts_with("whether ")
        || l.starts_with("what ")
}

/// Strip a leading interrogative ("how many ", "how much ", …) so a dial gloss
/// becomes a noun-ish phrase that slots into "X is at least N".
fn measure_phrase(gloss: &str) -> String {
    let lower = gloss.to_ascii_lowercase();
    for pre in ["how many ", "how much ", "how long ", "the number of "] {
        if lower.starts_with(pre) {
            return gloss[pre.len()..].trim().to_string();
        }
    }
    gloss.to_string()
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A contested value-question (the kind handed back `Unknown`): a nullary,
/// Bool-sorted free symbol. Everything else (the Int dials) is an agreed measure.
fn is_value_predicate(sig: &Sig) -> bool {
    sig.ret == Sort::Bool && sig.arg_sorts.is_empty()
}

/// A gloss tidied for prose: trim, drop a trailing period, and cut any parenthetical
/// machine aside (e.g. "— the contested future crux, left uninterpreted") so the
/// human sentence reads cleanly. Falls back to a prettified symbol if no gloss.
fn clean_gloss(sig: &Sig) -> String {
    let g = sig.gloss.trim();
    if g.is_empty() {
        return prettify(&sig.name);
    }
    // Cut an em-dash aside that carries the formal annotation.
    let core = g.split(" — ").next().unwrap_or(g).trim();
    let core = core.split(" (vs.").next().unwrap_or(core).trim();
    // Drop stray markdown emphasis asterisks ("substantial *creative* work").
    core.trim_end_matches('.').replace('*', "")
}

/// `stain_is_damage` → "stain is damage" (for last-resort prose only).
fn prettify(id: &str) -> String {
    id.replace(['_', '-'], " ")
}

// ─────────────────── warm language: clauses (if-this-then-that) ───────────────

/// One clause rendered for human eyes: the world it fires in ("if …") and what it
/// awards ("then …"), both in warm language built from the declared glosses.
struct ClauseLine {
    name: String,
    when: String,
    then: String,
}

fn clause_lines(pact: &Pact) -> Vec<ClauseLine> {
    pact.clauses
        .iter()
        .map(|c| ClauseLine {
            name: prettify(&c.name),
            when: render_guard(pact, &c.guard),
            then: render_outcome(pact, &c.outcome),
        })
        .collect()
}

/// Render a guard [`Formula`] as a warm, lower-case condition phrase (no leading
/// "if"). Builds from each declared predicate's gloss; conjunctions read "…, and
/// …", disjunctions "… or …". Kept to the pact fragment (And/Or/Not/Le/Lt over a
/// free Bool + integer dials); anything outside it degrades to a plain phrase
/// rather than leaking machine syntax.
fn render_guard(pact: &Pact, f: &Formula) -> String {
    match f {
        Formula::And(parts) => join_clauses(pact, parts, ", and "),
        Formula::Or(parts) => join_clauses(pact, parts, ", or "),
        Formula::Not(inner) => negate_phrase(pact, inner),
        Formula::Atom(t) => atom_phrase(pact, t, true),
        Formula::Le(a, b) => compare_phrase(pact, a, b, Cmp::Le),
        Formula::Lt(a, b) => compare_phrase(pact, a, b, Cmp::Lt),
        // Outside the warm fragment — describe rather than dump syntax.
        _ => "a specific situation you named".to_string(),
    }
}

fn join_clauses(pact: &Pact, parts: &[Formula], sep: &str) -> String {
    let mut rendered: Vec<String> = parts
        .iter()
        .map(|p| render_guard(pact, p))
        .filter(|s| !s.trim().is_empty())
        .collect();
    // A tautological "X or not-X" dial-free disjunction (the inconsistent corpus
    // uses one to make a clause fire in every reading) reads as nothing useful;
    // fold it away so the condition stays honest about what actually constrains.
    rendered.retain(|s| s != "either way on the open question");
    match rendered.len() {
        0 => "any situation you named".to_string(),
        1 => rendered.remove(0),
        _ => rendered.join(sep),
    }
}

/// A boolean value-predicate phrase, `positive` or negated, from its gloss. The
/// gloss is a clause ("the carpet stain counts as chargeable damage"); positive
/// uses it as-is, negative prefixes a clean "it's not the case that …".
fn atom_phrase(pact: &Pact, t: &Term, positive: bool) -> String {
    let Term::App(sym, args) = t else {
        return "the situation holds".to_string();
    };
    if !args.is_empty() {
        return prettify(sym);
    }
    // A declared Bool → its gloss, as a *declarative* statement. Some glosses are
    // phrased as a question ("whether the departure was for cause"); strip the
    // leading "whether " so it reads as a clause ("the departure was for cause").
    let phrase = gloss_for(pact, sym)
        .map(|g| declarative(&g))
        .unwrap_or_else(|| prettify(sym));
    if positive {
        phrase
    } else {
        format!("it's not the case that {phrase}")
    }
}

/// Turn a possibly-interrogative value gloss into a declarative clause: drop a
/// leading "whether ". ("whether the departure was for cause" → "the departure
/// was for cause"; an already-declarative gloss is returned unchanged.)
fn declarative(gloss: &str) -> String {
    let t = gloss.trim();
    if let Some(rest) = t.strip_prefix("whether ").or_else(|| t.strip_prefix("Whether ")) {
        return rest.trim().to_string();
    }
    t.to_string()
}

/// Render `Not(inner)` warmly. Special-cases negated atoms (so we say "it is not
/// the case that …" cleanly) and the tautology guard pattern.
fn negate_phrase(pact: &Pact, inner: &Formula) -> String {
    match inner {
        Formula::Atom(t) => atom_phrase(pact, t, false),
        _ => {
            let inner_r = render_guard(pact, inner);
            format!("it is not the case that {inner_r}")
        }
    }
}

enum Cmp {
    Le,
    Lt,
}

/// Render an integer comparison between a dial and a literal as a warm phrase,
/// e.g. `Le(30, notice_days)` → "Robin gives at least 30 days' notice" via the
/// dial's gloss, or a clean generic if the gloss is absent.
fn compare_phrase(pact: &Pact, a: &Term, b: &Term, cmp: Cmp) -> String {
    // Identify which side is the declared dial and which is the literal.
    let (dial, lit, dial_on_left) = match (a, b) {
        (Term::App(sym, args), Term::IntLit(n)) if args.is_empty() => (sym.clone(), *n, true),
        (Term::IntLit(n), Term::App(sym, args)) if args.is_empty() => (sym.clone(), *n, false),
        _ => return "a measured threshold you agreed on".to_string(),
    };
    let raw = gloss_for(pact, &dial).unwrap_or_else(|| prettify(&dial));
    // The dial gloss is a "how many …" question; strip the interrogative so it
    // slots into "… is at least N".
    let measure = measure_phrase(&raw);
    // Normalise to "dial CMP lit".
    // `Le(lit, dial)` (lit on left) ⇒ dial ≥ lit ; `Le(dial, lit)` ⇒ dial ≤ lit.
    // `Lt(lit, dial)` ⇒ dial > lit ; `Lt(dial, lit)` ⇒ dial < lit.
    let phrase = match (cmp, dial_on_left) {
        (Cmp::Le, true) => format!("{measure} is at most {lit}"),
        (Cmp::Le, false) => format!("{measure} is at least {lit}"),
        (Cmp::Lt, true) => format!("{measure} is fewer than {lit}"),
        (Cmp::Lt, false) => format!("{measure} is more than {lit}"),
    };
    phrase
}

/// What the `award` outcome variable *means*. Several pacts document it on the
/// integer dial's gloss (e.g. "the outcome `award` is Devon's royalty share in
/// basis points" / "Sasha's deposit return in cents"); others (the roommate
/// deposit pacts) leave it implicit. The project's canonical award is integer
/// **cents** (a deposit/refund), so that's the default; we switch to basis points
/// only when the gloss explicitly says so. (No pact in the corpus uses any other
/// unit; if one did, cents is the safe, money-shaped reading for a deposit pact.)
enum AwardUnit {
    /// Integer cents → formatted as money. The default.
    Cents,
    /// Basis points → formatted as a percentage (bps / 100). Opt-in via the gloss.
    BasisPoints,
}

/// Resolve the award unit from the dial glosses: basis points if any gloss says
/// so (a royalty/equity share), else cents (the deposit/refund default).
fn award_unit(pact: &Pact) -> AwardUnit {
    for sig in &pact.predicates {
        let g = sig.gloss.to_ascii_lowercase();
        if g.contains("basis point") || g.contains("bps") {
            return AwardUnit::BasisPoints;
        }
    }
    AwardUnit::Cents
}

/// Render an outcome [`Formula`] (`award = <n>`) as "then …" prose, formatting the
/// figure in the unit the pact documented. Recipient-neutral: the awards in the
/// corpus go to different parties (a deposit return, a royalty share, vested
/// equity), so we say "the agreed figure is …" rather than assert a recipient the
/// outcome formula doesn't carry.
fn render_outcome(pact: &Pact, f: &Formula) -> String {
    if let Formula::Eq(_lhs, Term::IntLit(n)) = f {
        return match award_unit(pact) {
            AwardUnit::Cents => format!("the agreed amount is {}", money(*n)),
            AwardUnit::BasisPoints => format!("the agreed share is {}", bps(*n)),
        };
    }
    // Any other outcome shape: a calm generic, never machine syntax.
    "the agreed amount is settled".to_string()
}

/// Basis points → a friendly percentage: `2500` → "25%", `500` → "5%", `250` →
/// "2.5%". (Keeps one decimal only when needed.)
fn bps(n: i64) -> String {
    let whole = n / 100;
    let frac = (n % 100).abs();
    if frac == 0 {
        format!("{whole}%")
    } else if frac % 10 == 0 {
        format!("{whole}.{}%", frac / 10)
    } else {
        format!("{whole}.{:02}%", frac)
    }
}

/// The human gloss for a declared symbol (its dial/value meaning), tidied. For an
/// Int dial we want the *measure* phrase ("how many days' notice Robin gives"),
/// not a yes/no clause — so we keep the gloss's core noun phrase.
fn gloss_for(pact: &Pact, sym: &str) -> Option<String> {
    let sig = pact.predicates.iter().find(|s| s.name == sym)?;
    let g = clean_gloss(sig);
    if g.is_empty() { None } else { Some(g) }
}

// ─────────────────── warm language: the verdict (trichotomy) ──────────────────

/// The cached verdict rendered for human eyes: a calm headline, a tone, and a
/// plain-language body — CERTIFIED / INCONSISTENT / REFUSED — never a machine
/// word. Built ENTIRELY from the cached cert's [`Status`]; this box decides
/// nothing.
struct Verdict {
    /// One of "certified" | "inconsistent" | "refused" | "rejected", for styling.
    tone: &'static str,
    headline: String,
    body: String,
    /// For INCONSISTENT: the concrete "here is exactly when" line; for REFUSED:
    /// the uncovered-world line. Empty for CERTIFIED.
    detail: Option<String>,
}

fn verdict_view(rec: &PactRecord) -> Verdict {
    let pact = &rec.pact;
    let a = first_name(pact.parties.first().map(|s| s.as_str()).unwrap_or("one of you"));
    let b = first_name(pact.parties.get(1).map(|s| s.as_str()).unwrap_or("the other"));
    match &rec.cert.status {
        Status::Certified => Verdict {
            tone: "certified",
            headline: "This agreement holds.".to_string(),
            body: format!(
                "Every situation {a} and {b} named is handled by one of the clauses, and no two \
                 clauses can both apply and disagree. You settled this in advance — fairly, and \
                 once — so the one thing left to you is the human question itself, never the \
                 bookkeeping around it."
            ),
            detail: None,
        },
        Status::Inconsistent { witness } => Verdict {
            tone: "inconsistent",
            headline: "Two of your rules can collide — here is exactly when.".to_string(),
            body:
                "These clauses can both apply to the same situation and ask for different amounts. \
                 That's not a judgement call left open on purpose; it's a genuine clash you'd want \
                 to fix before signing — tighten the two rules so they can't both fire in this \
                 world."
                    .to_string(),
            detail: Some(witness_plain(pact, witness)),
        },
        Status::Refused { gap } => Verdict {
            tone: "refused",
            headline: "You left a situation unhandled — here it is.".to_string(),
            body: format!(
                "There's a situation {a} and {b} could end up in that none of the clauses speak \
                 to, so the agreement would be silent exactly when you'd need it. Adding a clause \
                 for that world closes the gap."
            ),
            detail: Some(refused_world(pact, gap)),
        },
        Status::RejectedAtAuthoring { reasons } => Verdict {
            tone: "rejected",
            headline: "This one wasn't a real agreement to certify.".to_string(),
            body: "Before any proof ran, this pact was turned away: one of its rules is something \
                   a calculator could decide on its own, with no genuinely human question left \
                   open. A forward constitution has to leave the value-question to you — that's \
                   the whole point."
                .to_string(),
            detail: reasons.first().map(|r| soften(r)),
        },
    }
}

/// Re-word the witness's one-liner into warm prose, keeping the concrete world
/// (the truth-values and dial numbers) so a reader sees the exact collision —
/// but dropping the "INCONSISTENT:"/cents machine framing, and formatting the two
/// clashing awards in the unit the pact documented.
fn witness_plain(pact: &Pact, w: &InconsistencyWitness) -> String {
    let mut conds: Vec<String> = Vec::new();
    for (k, v) in &w.bool_assignment {
        // Prefer the value-question's gloss ("the stain counts as damage") over the
        // bare symbol, so the concrete world reads in the parties' own words.
        let phrase = gloss_for(pact, k)
            .map(|g| declarative(&g))
            .unwrap_or_else(|| prettify(k));
        if *v {
            conds.push(format!("it is read as “{phrase}”"));
        } else {
            conds.push(format!("it is read as “not {phrase}”"));
        }
    }
    for (k, v) in &w.int_assignment {
        let measure = gloss_for(pact, k)
            .map(|g| measure_phrase(&g))
            .unwrap_or_else(|| prettify(k));
        conds.push(format!("{measure} is {v}"));
    }
    let when = if conds.is_empty() {
        "In any situation".to_string()
    } else {
        format!("When {}", conds.join(" and "))
    };
    let fmt = |n: i64| match award_unit(pact) {
        AwardUnit::Cents => money(n),
        AwardUnit::BasisPoints => bps(n),
    };
    format!(
        "{when}, the “{}” rule and the “{}” rule both apply — one asks for {}, the other for {}. \
         The agreement would owe two different amounts at once.",
        prettify(&w.clause_i_name),
        prettify(&w.clause_j_name),
        fmt(w.award_i_cents),
        fmt(w.award_j_cents),
    )
}

/// The concrete uncovered world for a REFUSED pact, in warm language. We search
/// the SAME bounded space the kernel's witness machinery uses (each value-question
/// truth assignment × the guard thresholds and their neighbours) for a world that
/// fires NO clause, and name it plainly. This NEVER overrides the gate's verdict —
/// the gate already said coverage fails; we only exhibit *where*, mirroring how
/// the inconsistency witness exhibits a clash. If the bounded probe can't pin an
/// exact world (a guard outside the dial-vs-literal fragment), we fall back to the
/// kernel's own softened diagnosis rather than invent one.
fn refused_world(pact: &Pact, gap: &mediator_pact::Gap) -> String {
    if let Some(world) = find_uncovered_world(pact) {
        let mut conds: Vec<String> = Vec::new();
        for (sym, v) in &world.bools {
            let phrase = gloss_for(pact, sym)
                .map(|g| declarative(&g))
                .unwrap_or_else(|| prettify(sym));
            conds.push(if *v {
                format!("it's read as “{phrase}”")
            } else {
                format!("it's read as “not {phrase}”")
            });
        }
        for (sym, v) in &world.ints {
            let measure = gloss_for(pact, sym)
                .map(|g| measure_phrase(&g))
                .unwrap_or_else(|| prettify(sym));
            conds.push(format!("{measure} is {v}"));
        }
        if !conds.is_empty() {
            return format!(
                "For example: {}. In that situation, no clause applies at all — the agreement \
                 simply says nothing. Add a rule for that world and the gap closes.",
                conds.join(" and ")
            );
        }
    }
    // Couldn't pin an exact world — keep the kernel's honest diagnosis, softened.
    soften(&gap.plain)
}

/// A concrete world: truth-values for the value-questions and integers for the
/// dials. Returned by [`find_uncovered_world`].
struct World {
    bools: Vec<(String, bool)>,
    ints: Vec<(String, i64)>,
}

/// Bounded search for a declared world that fires NO clause guard — the coverage
/// gap's witness. Mirrors the inconsistency-witness search: every value-question
/// assignment × every "interesting" integer (a guard literal and its immediate
/// neighbours) for each dial. Only handles the pact fragment (And/Or/Not/Le/Lt
/// over a free Bool + dial-vs-literal int comparisons); returns `None` otherwise,
/// so the caller falls back to the kernel's own diagnosis (never a false claim).
fn find_uncovered_world(pact: &Pact) -> Option<World> {
    let bools: Vec<String> = pact
        .predicates
        .iter()
        .filter(|s| is_value_predicate(s))
        .map(|s| s.name.clone())
        .collect();
    let ints: Vec<String> = pact
        .predicates
        .iter()
        .filter(|s| s.ret == Sort::Int && s.arg_sorts.is_empty())
        .map(|s| s.name.clone())
        .collect();
    let candidates = interesting_ints(pact);
    if candidates.is_empty() && !ints.is_empty() {
        return None;
    }

    let n_bool = bools.len();
    for mask in 0..(1usize << n_bool) {
        let bassign: Vec<(String, bool)> = bools
            .iter()
            .enumerate()
            .map(|(bit, name)| (name.clone(), (mask >> bit) & 1 == 1))
            .collect();
        for iassign in int_assignments(&ints, &candidates) {
            let env = Env { bools: &bassign, ints: &iassign };
            // A world is uncovered iff NO clause guard evaluates to true here.
            let mut covered = false;
            for c in &pact.clauses {
                match eval(&c.guard, &env) {
                    Some(true) => {
                        covered = true;
                        break;
                    }
                    Some(false) => {}
                    None => return None, // outside the fragment; don't guess
                }
            }
            if !covered {
                return Some(World { bools: bassign, ints: iassign });
            }
        }
    }
    None
}

/// The integer values worth probing: every literal that appears in a guard, and
/// its immediate neighbours (the boundary points where a linear guard flips).
fn interesting_ints(pact: &Pact) -> Vec<i64> {
    let mut lits: Vec<i64> = Vec::new();
    for c in &pact.clauses {
        collect_int_lits(&c.guard, &mut lits);
    }
    let mut out: Vec<i64> = Vec::new();
    for l in &lits {
        for v in [l - 1, *l, l + 1] {
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    // A baseline so a dial with no literal still gets probed.
    if out.is_empty() {
        out.push(0);
    }
    out
}

fn collect_int_lits(f: &Formula, out: &mut Vec<i64>) {
    match f {
        Formula::Le(a, b) | Formula::Lt(a, b) | Formula::Eq(a, b) => {
            for t in [a, b] {
                if let Term::IntLit(n) = t {
                    out.push(*n);
                }
            }
        }
        Formula::Not(x) => collect_int_lits(x, out),
        Formula::And(xs) | Formula::Or(xs) => xs.iter().for_each(|x| collect_int_lits(x, out)),
        _ => {}
    }
}

/// Every assignment of the dials over the candidate set (cartesian product).
fn int_assignments(ints: &[String], candidates: &[i64]) -> Vec<Vec<(String, i64)>> {
    let mut acc: Vec<Vec<(String, i64)>> = vec![Vec::new()];
    for name in ints {
        let mut next = Vec::new();
        for partial in &acc {
            for c in candidates {
                let mut p = partial.clone();
                p.push((name.clone(), *c));
                next.push(p);
            }
        }
        acc = next;
    }
    acc
}

/// The evaluation environment: a value-question's truth and a dial's integer.
struct Env<'a> {
    bools: &'a [(String, bool)],
    ints: &'a [(String, i64)],
}

impl Env<'_> {
    fn boolv(&self, sym: &str) -> Option<bool> {
        self.bools.iter().find(|(s, _)| s == sym).map(|(_, v)| *v)
    }
    fn intv(&self, sym: &str) -> Option<i64> {
        self.ints.iter().find(|(s, _)| s == sym).map(|(_, v)| *v)
    }
}

/// Evaluate a guard [`Formula`] in a world. `None` means "outside the supported
/// fragment" — the caller then declines to guess (falling back to the kernel's
/// own diagnosis), so this can never manufacture a false uncovered-world claim.
fn eval(f: &Formula, env: &Env) -> Option<bool> {
    match f {
        Formula::Atom(Term::App(sym, args)) if args.is_empty() => env.boolv(sym),
        Formula::Not(x) => eval(x, env).map(|b| !b),
        Formula::And(xs) => {
            let mut all = true;
            for x in xs {
                all &= eval(x, env)?;
            }
            Some(all)
        }
        Formula::Or(xs) => {
            let mut any = false;
            for x in xs {
                any |= eval(x, env)?;
            }
            Some(any)
        }
        Formula::Le(a, b) => Some(int_term(a, env)? <= int_term(b, env)?),
        Formula::Lt(a, b) => Some(int_term(a, env)? < int_term(b, env)?),
        _ => None,
    }
}

/// Evaluate an integer term (a literal or a declared dial) in a world.
fn int_term(t: &Term, env: &Env) -> Option<i64> {
    match t {
        Term::IntLit(n) => Some(*n),
        Term::App(sym, args) if args.is_empty() => env.intv(sym),
        _ => None,
    }
}

/// Strip the few machine words the room never shows, gently. Used for the
/// honesty-bearing diagnostic strings that originate in the kernel.
fn soften(s: &str) -> String {
    let mut t = s.to_string();
    t = t.replace("the contested predicate(s)", "the open question(s)");
    t = t.replace("predicate", "open question");
    t = t.replace("obligation", "rule");
    t = t.replace("uninterpreted", "left open");
    t
}

// ─────────────────────────────── the gallery ─────────────────────────────────

/// `GET /pacts` — the calm gallery of certified pact templates.
pub async fn pacts_gallery(State(state): State<SharedState>) -> Markup {
    page("Forward constitutions", html! {
        (brandbar(Some(html! { "forward constitutions" })))
        style { (PreEscaped(PACT_CSS)) }

        header .interior-head {
            h1 { "Forward constitutions" }
            p .lede {
                "Don't wait for the breakup to mediate it — settle, in advance, how every "
                "breakup you can name will be handled, and prove the agreement actually holds "
                "before anyone needs it. Each pact below was checked: it handles every situation "
                "the two people named, and no two of its rules can collide. The one genuinely "
                "human question stays theirs."
            }
        }

        @if state.pacts.is_empty() {
            section {
                div .card .empty {
                    p { "No certified pacts are loaded on this box yet." }
                    p .muted {
                        "Add a pact under " span .mono { "scenarios/pacts/" }
                        " beside its " span .mono { ".cert.json" }
                        " certificate, then restart."
                    }
                }
            }
        } @else {
            section .pact-gallery {
                @for rec in &state.pacts {
                    (pact_card(rec))
                }
            }
        }

        section .pacts-foot {
            p .muted {
                "These are templates, certified once and re-checkable by anyone. The proof runs "
                "where the prover lives; this page renders the signed result it sent back."
            }
            a .ghost-link href="/" { "← back to the room" }
        }
    })
}

/// One pact, as a gallery card: parties, title, a one-line verdict chip, and a
/// glimpse of the open question.
fn pact_card(rec: &PactRecord) -> Markup {
    let v = verdict_view(rec);
    let questions = declared_questions(&rec.pact);
    let open_q = questions.iter().find(|q| q.open);
    html! {
        a .pact-card href=(format!("/pact/{}", rec.id)) {
            div .pact-eyebrow { (parties_line(&rec.pact)) }
            h2 .pact-title { (pact_title(&rec.pact)) }
            @if let Some(q) = open_q {
                p .pact-open { "leaves open: " (lower_first(&q.text)) }
            }
            div .pact-foot {
                (verdict_chip(&v))
                span .pact-go { "open →" }
            }
        }
    }
}

/// A short, human title: the part before any " — " dash subtitle.
fn pact_title(pact: &Pact) -> String {
    pact.title.split(" — ").next().unwrap_or(&pact.title).trim().to_string()
}

/// The verdict as a small status chip for the card foot.
fn verdict_chip(v: &Verdict) -> Markup {
    let (cls, label) = match v.tone {
        "certified" => ("chip-green", "certified"),
        "inconsistent" => ("chip-amber", "rules collide"),
        "refused" => ("chip-amber", "a gap remains"),
        _ => ("chip-grey", "turned away"),
    };
    html! { span .chip .(cls) { (label) } }
}

/// "Whether the stain …" → "whether the stain …" (for inline use after a colon).
fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ─────────────────────────────── the detail page ─────────────────────────────

/// `GET /pact/:id` — one pact in full: open questions, warm clauses, the cached
/// verdict, the quiet re-verifiable provenance, and the draft-your-own explainer.
pub async fn pact_detail(
    AxPath(id): AxPath<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(rec) = state.pacts.iter().find(|r| r.id == id) else {
        return not_found("We don't have a record of that pact.");
    };
    let v = verdict_view(rec);
    let questions = declared_questions(&rec.pact);
    let clauses = clause_lines(&rec.pact);
    // The forward-constitution source — emitted live, no prover. This is the
    // machine-checkable promise the pact compiles to; we show it honestly as
    // "what gets certified where the prover lives", never as a live verdict.
    let source = mediator_pact::pact_codegen(&rec.pact);
    let record = &rec.cert.record;
    let verified = mediator_audit::verify(record);

    let markup = page(&format!("Pact — {}", pact_title(&rec.pact)), html! {
        (brandbar(Some(html! { a href="/pacts" { "forward constitutions" } })))
        style { (PreEscaped(PACT_CSS)) }

        header .interior-head {
            p .eyebrow { (parties_line(&rec.pact)) }
            h1 { (pact_title(&rec.pact)) }
            p .lede {
                "A two-party agreement, certified in advance. Below: the questions they chose to "
                "leave open between them, the rules they agreed on, and what the check found."
            }
        }

        // (1) The verdict — front and centre, in plain warm language.
        section {
            div .verdict .(format!("verdict-{}", v.tone)) {
                div .verdict-head {
                    span .verdict-mark { (verdict_glyph(v.tone)) }
                    h2 .verdict-headline { (v.headline) }
                }
                p .verdict-body { (v.body) }
                @if let Some(d) = &v.detail {
                    div .verdict-detail { p { (d) } }
                }
            }
        }

        // (2) The open questions — what they're agreeing to leave open.
        @if !questions.is_empty() {
            section .pact-section {
                h2 .pact-h { "The questions you're leaving open" }
                p .pact-sub {
                    "A forward constitution doesn't settle the human question — it settles "
                    "everything around it, so the only thing left is the part that was always "
                    "yours to decide."
                }
                ul .question-list {
                    @for q in &questions {
                        li .question .(if q.open { "q-open" } else { "q-measure" }) {
                            span .q-mark { @if q.open { "?" } @else { "·" } }
                            span .q-text {
                                (q.text)
                                @if q.open { span .q-tag { "left to you" } }
                                @else { span .q-tag .q-tag-measure { "agreed up front" } }
                            }
                        }
                    }
                }
            }
        }

        // (3) The clauses — warm if-this-then-that lines.
        @if !clauses.is_empty() {
            section .pact-section {
                h2 .pact-h { "What you agreed" }
                p .pact-sub { "Each rule says: in this situation, here's what happens." }
                ul .clause-list {
                    @for c in &clauses {
                        li .clause {
                            span .clause-name { (c.name) }
                            p .clause-line {
                                span .clause-if { "If " } (lower_first(&c.when)) ", "
                                span .clause-then { "then " } (lower_first(&c.then)) "."
                            }
                        }
                    }
                }
            }
        }

        // (4) Re-verifiable provenance — quiet, like the audit view.
        section .pact-section {
            (provenance_panel(rec, &verified))
        }

        // (5) Draft your own — the authoring explainer + the live source.
        (draft_your_own(&rec.pact, &source))

        section .pacts-foot {
            a .ghost-link href="/pacts" { "← all forward constitutions" }
        }
    });
    (StatusCode::OK, markup).into_response()
}

fn verdict_glyph(tone: &str) -> &'static str {
    match tone {
        "certified" => "✓",
        "inconsistent" => "≠",
        "refused" => "…",
        _ => "—",
    }
}

/// The quiet, checkable provenance — the same spirit as the audit view: a signed,
/// re-verifiable record, shown but not shouted. NEVER implies this box proved it.
fn provenance_panel(rec: &PactRecord, verified: &Result<(), mediator_audit::AuditError>) -> Markup {
    let record = &rec.cert.record;
    html! {
        details .provenance {
            summary { "Re-verifiable provenance" }
            div .provenance-body {
                p .pact-sub {
                    "This result was proved through the real prover off this box, then signed. "
                    "The record below is the receipt — anyone can re-check it wasn't altered, and "
                    "anyone with the prover can reproduce the proof from the pact itself."
                }
                @match verified {
                    Ok(()) => div .verify-ok { "✓ the signed certificate verifies — it has not been altered since it was sealed." },
                    Err(e) => div .verify-bad { "✗ this certificate did not verify: " (e.to_string()) },
                }
                table .audit-table {
                    thead { tr { th { "#" } th { "step" } th { "result" } } }
                    tbody {
                        @for e in &record.entries {
                            tr {
                                td { (e.seq) }
                                td .mono { (friendly_step(&e.kind)) }
                                td { @if let Some(v) = &e.verdict { (verdict_word(v)) } @else { "—" } }
                            }
                            // (the `friendly_step` label already speaks plainly)
                        }
                    }
                }
                div .provenance-sig {
                    div { span .k { "signed by" } span .mono { (short(&record.public_key)) } }
                    div { span .k { "signature" } span .mono { (short(&record.signature)) } }
                }
                p .muted .small {
                    "Scope, honestly: this checks the agreement is complete and non-contradictory "
                    "over the questions it actually named — never that it named every question "
                    "the world might raise. A situation nobody wrote down can't be checked for; "
                    "where that bites, the gap is shown above, not hidden."
                }
            }
        }
    }
}

/// Turn an internal step name into a quiet human label for the provenance table,
/// keeping the machine vocabulary off the page.
fn friendly_step(kind: &str) -> String {
    if kind == "coverage" {
        return "every situation is handled".to_string();
    }
    if kind == "pact_certified" {
        return "certified".to_string();
    }
    if kind == "pact_inconsistent" {
        return "a clash was found".to_string();
    }
    if kind == "pact_refused" {
        return "a gap was found".to_string();
    }
    if kind == "pact_rejected_at_authoring" {
        return "turned away before proving".to_string();
    }
    if let Some(rest) = kind.strip_prefix("consistent_") {
        // `consistent_0_2` → "rules 1 & 3 don't collide" (1-based, human).
        if let Some((i, j)) = rest.split_once('_') {
            if let (Ok(i), Ok(j)) = (i.parse::<usize>(), j.parse::<usize>()) {
                return format!("rules {} & {} don't collide", i + 1, j + 1);
            }
        }
    }
    prettify(kind)
}

/// Map the certificate's stored verdict string (the audit chain keeps it as a
/// plain `"Proved"`/`"Unknown"`/… label) to a quiet human word for the table.
fn verdict_word(v: &str) -> &'static str {
    match v {
        "Proved" => "held",
        "Refuted" => "broke",
        "Unknown" => "open",
        _ => "error",
    }
}

fn short(s: &str) -> String {
    if s.len() > 20 {
        format!("{}…{}", &s[..10], &s[s.len() - 6..])
    } else {
        s.to_string()
    }
}

// ─────────────────────────────── draft your own ──────────────────────────────

/// The "Draft your own" explainer: how a pact is authored, and the live
/// forward-constitution source it compiles to (emitted by `pact_codegen`, no
/// prover) — framed honestly as the machine-checkable promise that gets certified
/// *where the prover lives*. We never fake a verdict the box can't compute.
fn draft_your_own(pact: &Pact, source: &str) -> Markup {
    let n_open = pact.predicates.iter().filter(|s| is_value_predicate(s)).count();
    let n_clauses = pact.clauses.len();
    let rules_phrase = format!("{n_clauses} if-then rule{}", if n_clauses == 1 { "" } else { "s" });
    let open_phrase = format!("{n_open} open question{}", if n_open == 1 { "" } else { "s" });
    html! {
        section .pact-section .draft-own {
            h2 .pact-h { "Draft your own" }
            p .pact-sub {
                "A forward constitution is short to write. The one above is just "
                strong { (rules_phrase) }
                @if n_open > 0 { " over " strong { (open_phrase) } }
                ". You do two things:"
            }
            ol .draft-steps {
                li {
                    strong { "Name the open questions." }
                    " The genuinely human ones you want to leave to yourselves (\"whether the "
                    "stain is real damage\"), and any plain measures you'll agree on up front "
                    "(\"how many days' notice\")."
                }
                li {
                    strong { "Add your if-then rules." }
                    " One line per situation: " em { "if this is the world, then this is what "
                    "happens." } " Cover the situations you can name; give each a clear outcome."
                }
            }
            p .pact-sub {
                "That's it. From those two things, the machine writes the promise below — and "
                "checks, where the prover lives, that your rules cover every world you named and "
                "never collide. The human question stays free; it's handed back, unanswered, by "
                "design."
            }
            details .source-reveal {
                summary { "Show the machine-checkable promise" }
                div .source-body {
                    p .muted .small {
                        "This is generated from the pact above, right here — but it is "
                        strong { "not" } " proved here. It's the exact text the prover checks "
                        "where it runs. Reading it, you can see there's nowhere to hide a fudge: "
                        "the open question is a free symbol the machine is forbidden to decide."
                    }
                    pre .source-pre { (source) }
                }
            }
        }
    }
}

// ─────────────────────────────────── styling ─────────────────────────────────

/// Scoped styling for the pact pages — leans entirely on the shared palette
/// variables (warm neutrals, one accent, dark-mode aware via the root vars), so
/// it matches the room and the audit view without re-declaring colours.
const PACT_CSS: &str = r#"
.pact-gallery { display: grid; gap: 1.1rem; margin-top: .4rem; }
.pact-card {
  display: block; color: inherit;
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius); padding: 1.3rem 1.5rem; box-shadow: var(--shadow);
  transition: transform .18s ease, box-shadow .18s ease, border-color .18s ease;
}
.pact-card:hover { transform: translateY(-3px); box-shadow: var(--shadow-lg); border-color: var(--border-2); text-decoration: none; }
.pact-eyebrow { font-size: var(--t--1); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; font-weight: 600; }
.pact-title { font-family: var(--serif); font-size: var(--t-2); font-weight: 600; letter-spacing: -.01em; line-height: 1.2; margin-top: .35rem; }
.pact-open { color: var(--text-2); margin-top: .5rem; font-size: var(--t-0); }
.pact-foot { display: flex; align-items: center; gap: .5rem; margin-top: 1rem; }
.pact-go { margin-left: auto; color: var(--accent); font-weight: 600; font-size: var(--t--1); }

/* the verdict — the headline of a pact page */
.verdict { border: 1px solid var(--border); border-radius: var(--radius); padding: 1.6rem 1.7rem; box-shadow: var(--shadow); background: var(--surface); }
.verdict-head { display: flex; align-items: baseline; gap: .7rem; }
.verdict-mark { font-family: var(--serif); font-size: var(--t-2); line-height: 1; flex: none; }
.verdict-headline { font-family: var(--serif); font-size: var(--t-2); font-weight: 600; letter-spacing: -.01em; line-height: 1.2; }
.verdict-body { color: var(--text-2); margin-top: .8rem; font-size: var(--t-0); line-height: 1.6; max-width: 60ch; }
.verdict-detail { margin-top: 1rem; padding: .85rem 1.05rem; border-radius: var(--radius-sm); background: var(--bg-2); }
.verdict-detail p { margin: 0; font-size: var(--t-0); line-height: 1.55; color: var(--text); }
.verdict-certified { background: linear-gradient(180deg, var(--surface), var(--green-bg)); border-color: transparent; }
.verdict-certified .verdict-mark { color: var(--green); }
.verdict-inconsistent { background: linear-gradient(180deg, var(--surface), var(--amber-bg)); border-color: transparent; }
.verdict-inconsistent .verdict-mark { color: var(--amber); }
.verdict-refused { background: linear-gradient(180deg, var(--surface), var(--amber-bg)); border-color: transparent; }
.verdict-refused .verdict-mark { color: var(--amber); }
.verdict-rejected .verdict-mark { color: var(--muted); }

.pact-section { padding: 2rem 0 .6rem; border-top: 1px solid var(--border); margin-top: 1.4rem; }
.pact-h { font-family: var(--serif); font-size: var(--t-1); font-weight: 600; letter-spacing: -.005em; }
.pact-sub { color: var(--text-2); margin: .5rem 0 1.1rem; font-size: var(--t-0); max-width: 60ch; line-height: 1.6; }

/* the open questions */
.question-list { list-style: none; display: grid; gap: .7rem; }
.question { display: flex; gap: .7rem; align-items: flex-start; padding: .9rem 1.05rem; border-radius: var(--radius-sm); background: var(--surface); border: 1px solid var(--border); }
.question.q-open { border-color: var(--amber); background: var(--amber-bg); }
.q-mark { font-family: var(--serif); font-weight: 700; flex: none; color: var(--accent); line-height: 1.4; }
.q-open .q-mark { color: var(--amber); }
.q-text { color: var(--text); font-size: var(--t-0); line-height: 1.5; }
.q-tag { display: inline-block; margin-left: .55rem; font-size: var(--t--1); font-weight: 700; text-transform: uppercase; letter-spacing: .05em; color: var(--amber); }
.q-tag-measure { color: var(--muted); }

/* the clauses */
.clause-list { list-style: none; display: grid; gap: .8rem; }
.clause { padding: .9rem 1.05rem; border-radius: var(--radius-sm); background: var(--bg-2); border: 1px solid var(--border); }
.clause-name { display: inline-block; font-size: var(--t--1); text-transform: uppercase; letter-spacing: .05em; color: var(--muted); font-weight: 700; margin-bottom: .35rem; }
.clause-line { margin: 0; color: var(--text); font-size: var(--t-0); line-height: 1.6; }
.clause-if, .clause-then { font-weight: 700; color: var(--accent); }

/* provenance — quiet, foldable, like the audit view */
.provenance { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); }
.provenance > summary { cursor: pointer; padding: .9rem 1.1rem; color: var(--text-2); font-size: var(--t--1); list-style: none; }
.provenance > summary::-webkit-details-marker { display: none; }
.provenance > summary::before { content: "🔏 "; }
.provenance[open] > summary { border-bottom: 1px solid var(--border); }
.provenance-body { padding: 1.1rem; }
.verify-ok { background: var(--green-bg); color: var(--green); border: 1px solid var(--border); padding: .6rem .9rem; border-radius: var(--radius-sm); font-size: var(--t--1); }
.verify-bad { background: var(--red-bg); color: var(--red); border: 1px solid var(--border); padding: .6rem .9rem; border-radius: var(--radius-sm); font-size: var(--t--1); }
.audit-table { width: 100%; border-collapse: collapse; margin: 1rem 0; font-size: var(--t--1); }
.audit-table th, .audit-table td { text-align: left; padding: .4rem .55rem; border-bottom: 1px solid var(--border); }
.audit-table th { color: var(--muted); font-weight: 600; }
.provenance-sig { display: flex; flex-direction: column; gap: .35rem; margin: .6rem 0; font-size: var(--t--1); }
.provenance-sig .k { display: inline-block; width: 6.5rem; color: var(--muted); }

/* draft your own */
.draft-own .draft-steps { margin: .2rem 0 1.1rem 1.2rem; display: grid; gap: .7rem; }
.draft-own .draft-steps li { color: var(--text-2); line-height: 1.6; padding-left: .3rem; }
.draft-own .draft-steps strong { color: var(--text); }
.source-reveal { margin-top: .4rem; }
.source-reveal > summary { cursor: pointer; color: var(--accent); font-weight: 600; font-size: var(--t-0); list-style: none; }
.source-reveal > summary::-webkit-details-marker { display: none; }
.source-reveal > summary::before { content: "› "; }
.source-reveal[open] > summary::before { content: "⌄ "; }
.source-body { margin-top: .9rem; }
.source-pre {
  margin-top: .7rem; font-family: var(--mono); font-size: .72rem; line-height: 1.5;
  color: var(--text-2); white-space: pre-wrap; word-break: break-word;
  background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: .9rem 1rem;
  max-height: 32rem; overflow: auto;
}

.pacts-foot { margin-top: 2.4rem; padding-top: 1.4rem; border-top: 1px solid var(--border); }
.pacts-foot .ghost-link { font-size: var(--t--1); color: var(--muted); }
"#;
