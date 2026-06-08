//! The **CERTIFIED CONTROVERSY** — the distribution of the choices people
//! actually made at a structurally-identical decision point, all signed.
//!
//! # The decision point
//!
//! Within a template, a [`DecisionPoint`] is a *normalized world*: which side of
//! a dial's threshold the world sits on ([`WorldSide`]), and the truth-value of
//! the crux. For the corpus's deposit/vesting/credit template (one crux, one
//! dial) there are exactly four — `(AtOrAbove, crux=false)`, `(AtOrAbove,
//! crux=true)`, `(Below, false)`, `(Below, true)` — one per clause. A decision
//! point is template-relative: it is defined by ROLES (dial #0, crux #0), so the
//! same point lands in every pact of the template regardless of names.
//!
//! # What is tallied, and why it is comparable across domains
//!
//! At a decision point, each pact awards *some amount* — but in its own units
//! (cents for a deposit, basis points for equity or a royalty share). Comparing
//! the raw numbers across pacts would be meaningless, and worse, declaring a
//! cross-pact "fair number" would re-import exactly the normative gravity the
//! constitution forbids. So the controversy is reported as the
//! domain-independent **[`CruxEffect`]**: holding the dial-side fixed, did the
//! crux being TRUE *raise*, *lower*, or *not change* the award versus the crux
//! being FALSE? That sign is the actual value-call the two humans made ("does the
//! contested thing, when true, help or hurt the party here?"), and it IS
//! comparable across domains. The raw award is still reported alongside, in the
//! pact's own units, for full transparency and re-checkability — but it is never
//! averaged into a recommendation.
//!
//! # The constitutional line (never "yours should be X")
//!
//! A [`Tally`] reports a DISTRIBUTION: "of the N pacts of this template, here is
//! how the crux's effect at this world came out — k raised, m lowered, p left it
//! unchanged — and here is each, attributed to its signed pact." It exposes no
//! mean, no median, no mode-as-recommendation, no "the usual choice is". The
//! mode is computed only to detect *agreement vs. live controversy* (all one way
//! ⇒ settled; split ⇒ a certified live controversy), which is a description of
//! the corpus, not advice to a new pair. The whole point of a forward
//! constitution is that the new pair decides their own awards; the commons only
//! ever *shows them what others, facing the identical structural question, chose*.
//!
//! # Honesty: the support count is the headline
//!
//! Every tally carries its support count (`N`) and a [`Tally::confidence_note`]
//! that states it in plain words — "backed by 2 pacts (tiny — not a norm)". A
//! controversy over a handful of pacts is reported as exactly that handful, never
//! dressed as a settled rule.

use crate::CorpusPact;
use crate::template::template_key;
use mediator_pact::{Pact, PactCertificate};
use mediator_types::{Formula, Sort, Term};
use std::collections::BTreeMap;

// ───────────────────────────── the decision point ───────────────────────────

/// Which side of a dial's threshold a world sits on. Template-relative (defined
/// by the dial's *role*, not its name), so the same side lands in every pact of
/// the template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldSide {
    /// The dial is at or above its threshold (the "settled / vested / qualifying"
    /// side, structurally — `dial ≥ THRESH`).
    AtOrAbove,
    /// The dial is below its threshold (the "brief / pre-cliff / short" side —
    /// `dial < THRESH`).
    Below,
}

impl WorldSide {
    fn label(self) -> &'static str {
        match self {
            WorldSide::AtOrAbove => "dial at-or-above threshold",
            WorldSide::Below => "dial below threshold",
        }
    }
}

/// A normalized world within a template: a dial-side and a crux truth-value.
/// Defined by ROLE index (crux #`crux_role`, dial #`dial_role`) so it is the same
/// point in every pact of the template. For the single-crux/single-dial corpus,
/// both roles are 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionPoint {
    /// The dial whose threshold defines the side (role index among sorted dials).
    pub dial_role: usize,
    /// Which side of that dial's threshold.
    pub side: WorldSide,
    /// The crux whose truth-value this point fixes (role index among sorted
    /// cruxes).
    pub crux_role: usize,
    /// The crux's truth-value at this world.
    pub crux_value: bool,
}

impl DecisionPoint {
    /// A plain-language description of the world, in role terms (a reader maps the
    /// role back to their own crux/dial via the template descriptor).
    pub fn plain(&self) -> String {
        format!(
            "world: {} and crux#{} is {}",
            self.side.label(),
            self.crux_role,
            self.crux_value
        )
    }
}

/// The canonical decision points of a pact's template. For a one-crux/one-dial
/// template this is the four `(side, crux)` worlds. For richer templates it is
/// the cross-product of (each dial's two sides) × (each crux's two values),
/// capped so it stays small and deterministic; the corpus only exercises the
/// one-crux/one-dial case, and we document the cap rather than explode.
///
/// We enumerate per-dial sides one dial at a time (holding others at-or-above)
/// and per-crux values one crux at a time (holding others false), which yields
/// the structurally-distinct "single-knob" worlds — enough to surface the
/// crux-effect controversy without a combinatorial blowup. The honest boundary:
/// for multi-crux templates this does not enumerate every joint world; it
/// enumerates the single-variable decision points, which is where the value-call
/// controversy lives.
pub fn decision_points(pact: &Pact) -> Vec<DecisionPoint> {
    let n_cruxes = pact
        .predicates
        .iter()
        .filter(|s| s.ret == Sort::Bool && s.arg_sorts.is_empty())
        .count();
    let n_dials = pact.predicates.iter().filter(|s| s.ret == Sort::Int).count();

    let mut pts = Vec::new();
    // The crux-effect controversy is read off a fixed dial-side while flipping the
    // crux. Enumerate (dial_role, side) × (crux_role) and emit BOTH crux values,
    // so a tally over the pair (false,true) can compute the effect.
    let dials = if n_dials == 0 { 1 } else { n_dials };
    let cruxes = if n_cruxes == 0 { 0 } else { n_cruxes };
    for dial_role in 0..dials {
        for side in [WorldSide::AtOrAbove, WorldSide::Below] {
            for crux_role in 0..cruxes {
                for crux_value in [false, true] {
                    pts.push(DecisionPoint {
                        dial_role: if n_dials == 0 { 0 } else { dial_role },
                        side,
                        crux_role,
                        crux_value,
                    });
                }
            }
        }
    }
    pts
}

// ───────────────────────────── the tally ─────────────────────────────────────

/// The direction the crux moves the award at a fixed dial-side: the
/// domain-independent value-call. Computed as `award(crux=true) − award(crux=
/// false)` at the same world-side, reported only as its *sign*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CruxEffect {
    /// The crux being true RAISES the award versus it being false.
    Raises,
    /// The crux being true LOWERS the award.
    Lowers,
    /// The crux does not change the award at this world-side.
    NoChange,
    /// The pact does not resolve cleanly to a single award at one of the two
    /// worlds (e.g. an outcome that is not a bare `award = const`). Reported
    /// honestly rather than guessed.
    Indeterminate,
}

impl CruxEffect {
    fn label(self) -> &'static str {
        match self {
            CruxEffect::Raises => "crux-true RAISES the award",
            CruxEffect::Lowers => "crux-true LOWERS the award",
            CruxEffect::NoChange => "crux makes NO difference",
            CruxEffect::Indeterminate => "indeterminate (non-numeric outcome)",
        }
    }
}

/// One pact's contribution to a tally at a decision point: how the crux moved the
/// award there (the comparable value-call), the raw award at this exact world (in
/// the pact's own units, for transparency), and the attribution.
#[derive(Clone, Debug, PartialEq)]
pub struct TallyEntry {
    /// Index of the pact in the commons.
    pub pact_index: usize,
    /// The pact's label (provenance).
    pub label: String,
    /// The crux's effect at this decision point's dial-side — the cross-domain
    /// value-call.
    pub effect: CruxEffect,
    /// The raw award (in the pact's own units) at exactly this decision point's
    /// world (this `side` × this `crux_value`). `None` if the world fires no
    /// award-clause or the outcome is non-numeric.
    pub award_here: Option<i64>,
    /// The root hash of the pact's signed certificate record — the thing a
    /// skeptic re-verifies to trust this entry. Binds the tally to the signature.
    pub record_root: String,
}

/// The certified controversy at one decision point over the pacts of a template:
/// the DISTRIBUTION of the choices people made, attributed and signed.
///
/// This is the load-bearing, falsifiable output. It reports counts of each
/// [`CruxEffect`], the per-pact entries, and a [`Tally::shape`] (settled vs. a
/// certified live controversy) — but never a recommendation.
#[derive(Clone, Debug, PartialEq)]
pub struct Tally {
    /// The template this tally is over.
    pub template_key: String,
    /// The decision point tallied.
    pub point: DecisionPoint,
    /// One entry per supporting pact, in ascending pact-index order.
    pub entries: Vec<TallyEntry>,
    /// The distribution: how many pacts landed each crux-effect.
    pub distribution: BTreeMap<String, usize>,
}

/// Whether a tally shows agreement or a live controversy — a description of the
/// corpus, not advice. `Settled` does NOT mean a new pair *should* choose that
/// way; it means everyone in this tiny corpus happened to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TallyShape {
    /// No supporting pacts (this template has no certified member yet).
    Empty,
    /// One supporting pact: a singleton, not yet a controversy at all.
    Singleton,
    /// Every supporting pact made the same value-call here.
    Settled,
    /// The supporting pacts split: formally identical worlds, opposite human
    /// value-calls. A certified live controversy.
    LiveControversy,
}

impl Tally {
    /// The number of pacts backing this tally — the headline error bar.
    pub fn support(&self) -> usize {
        self.entries.len()
    }

    /// The shape of the tally (agreement vs. live controversy vs. too-few).
    /// Computed over the *determinate* effects only; an `Indeterminate` entry
    /// does not count toward agreement.
    pub fn shape(&self) -> TallyShape {
        let determinate: Vec<&TallyEntry> = self
            .entries
            .iter()
            .filter(|e| e.effect != CruxEffect::Indeterminate)
            .collect();
        match determinate.len() {
            0 => TallyShape::Empty,
            1 => TallyShape::Singleton,
            _ => {
                let first = determinate[0].effect;
                if determinate.iter().all(|e| e.effect == first) {
                    TallyShape::Settled
                } else {
                    TallyShape::LiveControversy
                }
            }
        }
    }

    /// The honest confidence note: the support count stated in plain words, with
    /// a loud "tiny" for a handful. This is the headline that travels with every
    /// claim — an error bar, never a green checkmark.
    pub fn confidence_note(&self) -> String {
        let n = self.support();
        let qualifier = match n {
            0 => "NO pacts — nothing is known here",
            1 => "1 pact — a singleton, not a norm",
            2 | 3 => "tiny — not a norm, just what these few chose",
            4..=9 => "small — a handful of signed choices, not a settled rule",
            _ => "still modest — read it as a distribution, not a verdict",
        };
        format!("backed by {n} pact(s) ({qualifier})")
    }

    /// A plain-text rendering of the controversy — what the `pact-commons` bin
    /// prints. Headlines the support count, lists the distribution, then each
    /// signed contribution. Never prints a recommendation.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  decision point — {}\n", self.point.plain()));
        out.push_str(&format!("    {}\n", self.confidence_note()));
        let shape_word = match self.shape() {
            TallyShape::Empty => "no signal",
            TallyShape::Singleton => "singleton (one signed choice)",
            TallyShape::Settled => "SETTLED here (every pact agreed) — still advisory only",
            TallyShape::LiveControversy => {
                "LIVE CONTROVERSY (identical world, opposite human value-calls)"
            }
        };
        out.push_str(&format!("    shape: {shape_word}\n"));
        out.push_str("    distribution of the crux's effect (how the contested thing, when true, moved the award):\n");
        for (effect, count) in &self.distribution {
            out.push_str(&format!("      {effect}: {count}\n"));
        }
        out.push_str("    the signed choices (each re-verifiable; NEVER 'yours should be X'):\n");
        for e in &self.entries {
            let award = e
                .award_here
                .map(|a| a.to_string())
                .unwrap_or_else(|| "—".to_string());
            out.push_str(&format!(
                "      · {:<22} {:<28} award@world={:<10} root={}…\n",
                e.label,
                e.effect.label(),
                award,
                &e.record_root[..e.record_root.len().min(12)]
            ));
        }
        out
    }
}

/// Tally the certified controversy at `point` over the `members` (indices into
/// `corpus`). The caller guarantees every member shares the same template (this
/// is enforced by `Commons::controversy`); we re-derive the template key from the
/// first member so the tally is self-describing.
///
/// For each member, we compute the crux's effect at `point.side` (the comparable
/// value-call) and the raw award at exactly `point`'s world (for transparency).
pub fn tally(corpus: &[CorpusPact], members: &[usize], point: &DecisionPoint) -> Tally {
    let template = members
        .first()
        .map(|&i| template_key(&corpus[i].pact))
        .unwrap_or_default();

    let mut entries = Vec::new();
    let mut distribution: BTreeMap<String, usize> = BTreeMap::new();
    // Seed the distribution keys so a zero count for a category is explicit.
    for k in [
        CruxEffect::Raises,
        CruxEffect::Lowers,
        CruxEffect::NoChange,
        CruxEffect::Indeterminate,
    ] {
        distribution.insert(k.label().to_string(), 0);
    }

    for &idx in members {
        let cp = &corpus[idx];
        let effect = crux_effect(&cp.pact, point);
        let award_here = award_at_world(&cp.pact, point);
        *distribution.entry(effect.label().to_string()).or_insert(0) += 1;
        entries.push(TallyEntry {
            pact_index: idx,
            label: cp.label.clone(),
            effect,
            award_here,
            record_root: record_root(&cp.cert),
        });
    }
    entries.sort_by_key(|e| e.pact_index);

    Tally {
        template_key: template,
        point: *point,
        entries,
        distribution,
    }
}

// ───────────────────── evaluating a pact at a world ──────────────────────────

/// The crux's effect at `point.side`: compare the award the pact gives at
/// `(side, crux=false)` vs `(side, crux=true)`, holding everything else at a
/// fixed reference world. Reported as the sign of `award(true) − award(false)`.
fn crux_effect(pact: &Pact, point: &DecisionPoint) -> CruxEffect {
    let world_false = build_world(pact, point.dial_role, point.side, point.crux_role, false);
    let world_true = build_world(pact, point.dial_role, point.side, point.crux_role, true);
    let a_false = fired_award(pact, &world_false);
    let a_true = fired_award(pact, &world_true);
    match (a_false, a_true) {
        (Some(f), Some(t)) => {
            use std::cmp::Ordering::*;
            match t.cmp(&f) {
                Greater => CruxEffect::Raises,
                Less => CruxEffect::Lowers,
                Equal => CruxEffect::NoChange,
            }
        }
        _ => CruxEffect::Indeterminate,
    }
}

/// The raw award (pact's own units) at exactly `point`'s world.
fn award_at_world(pact: &Pact, point: &DecisionPoint) -> Option<i64> {
    let world = build_world(pact, point.dial_role, point.side, point.crux_role, point.crux_value);
    fired_award(pact, &world)
}

/// A concrete assignment: crux truth-values + dial integer values, chosen to
/// realize the decision point. The dial at `dial_role` is set to a value on the
/// requested `side` of its threshold; all other dials are set at-or-above their
/// thresholds; the crux at `crux_role` is set to `crux_value`; all other cruxes
/// are false.
fn build_world(
    pact: &Pact,
    dial_role: usize,
    side: WorldSide,
    crux_role: usize,
    crux_value: bool,
) -> World {
    let cruxes = sorted_names(pact, |s| s.ret == Sort::Bool && s.arg_sorts.is_empty());
    let dials = sorted_names(pact, |s| s.ret == Sort::Int);

    let mut bools: BTreeMap<String, bool> = BTreeMap::new();
    for (k, name) in cruxes.iter().enumerate() {
        bools.insert(name.clone(), k == crux_role && crux_value);
    }

    let mut ints: BTreeMap<String, i64> = BTreeMap::new();
    for (k, name) in dials.iter().enumerate() {
        let thresh = dial_threshold(pact, name).unwrap_or(0);
        let want_side = if k == dial_role { side } else { WorldSide::AtOrAbove };
        let val = match want_side {
            // At or above: pick threshold itself (satisfies `dial ≥ THRESH`).
            WorldSide::AtOrAbove => thresh,
            // Below: pick threshold − 1 (satisfies `dial < THRESH`).
            WorldSide::Below => thresh.saturating_sub(1),
        };
        ints.insert(name.clone(), val);
    }

    World { bools, ints }
}

/// The threshold a dial is compared against in the guards: the integer literal
/// that appears in a comparison with this dial. If several appear, the smallest
/// (the most common single-cliff case has exactly one). `None` if the dial is
/// never compared to a literal.
fn dial_threshold(pact: &Pact, dial: &str) -> Option<i64> {
    let mut found: Option<i64> = None;
    for c in &pact.clauses {
        collect_dial_thresholds(&c.guard, dial, &mut found);
    }
    found
}

fn collect_dial_thresholds(f: &Formula, dial: &str, out: &mut Option<i64>) {
    match f {
        Formula::Le(a, b) | Formula::Lt(a, b) | Formula::Eq(a, b) => {
            // If one side is this dial and the other an int literal, record it.
            let lit = match (a, b) {
                (Term::App(n, args), Term::IntLit(v)) if n == dial && args.is_empty() => Some(*v),
                (Term::IntLit(v), Term::App(n, args)) if n == dial && args.is_empty() => Some(*v),
                _ => None,
            };
            if let Some(v) = lit {
                *out = Some(match *out {
                    Some(cur) => cur.min(v),
                    None => v,
                });
            }
        }
        Formula::Not(p) | Formula::Obligation(p) | Formula::Permission(p) => {
            collect_dial_thresholds(p, dial, out)
        }
        Formula::And(ps) | Formula::Or(ps) => {
            for p in ps {
                collect_dial_thresholds(p, dial, out);
            }
        }
        Formula::Implies(a, b) | Formula::Iff(a, b) => {
            collect_dial_thresholds(a, dial, out);
            collect_dial_thresholds(b, dial, out);
        }
        Formula::Forall(_, _, body) | Formula::Exists(_, _, body) => {
            collect_dial_thresholds(body, dial, out)
        }
        Formula::Atom(_) => {}
    }
}

/// A concrete world for evaluating guards.
struct World {
    bools: BTreeMap<String, bool>,
    ints: BTreeMap<String, i64>,
}

/// The award demanded by the (first) clause that fires in `world`. A certified
/// pact is consistent, so at most one award fires (a clash would have been
/// caught); if several fired with the same award it is unambiguous anyway. `None`
/// if no clause fires or the firing clause's outcome is non-numeric.
fn fired_award(pact: &Pact, world: &World) -> Option<i64> {
    for c in &pact.clauses {
        if eval_guard(&c.guard, world) == Some(true) {
            if let Some(a) = demanded_award(&c.outcome) {
                return Some(a);
            }
        }
    }
    None
}

/// Read `award = <const>` (either order) from an outcome formula.
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

/// Evaluate a guard formula over a concrete world (the pact fragment: bool crux
/// atoms + linear int comparisons + ∧/∨/¬). `None` if it uses something outside
/// the fragment.
fn eval_guard(f: &Formula, world: &World) -> Option<bool> {
    match f {
        Formula::Atom(Term::App(name, args)) if args.is_empty() => world.bools.get(name).copied(),
        Formula::Atom(_) => None,
        Formula::Eq(a, b) => Some(eval_int(a, world)? == eval_int(b, world)?),
        Formula::Le(a, b) => Some(eval_int(a, world)? <= eval_int(b, world)?),
        Formula::Lt(a, b) => Some(eval_int(a, world)? < eval_int(b, world)?),
        Formula::Not(p) => Some(!eval_guard(p, world)?),
        Formula::And(ps) => {
            let mut acc = true;
            for p in ps {
                acc &= eval_guard(p, world)?;
            }
            Some(acc)
        }
        Formula::Or(ps) => {
            let mut acc = false;
            for p in ps {
                acc |= eval_guard(p, world)?;
            }
            Some(acc)
        }
        Formula::Implies(a, b) => Some(!eval_guard(a, world)? || eval_guard(b, world)?),
        Formula::Iff(a, b) => Some(eval_guard(a, world)? == eval_guard(b, world)?),
        _ => None,
    }
}

fn eval_int(t: &Term, world: &World) -> Option<i64> {
    match t {
        Term::IntLit(n) => Some(*n),
        Term::App(name, args) if args.is_empty() => world.ints.get(name).copied(),
        _ => None,
    }
}

/// Declared symbol names matching a predicate, sorted (the role order).
fn sorted_names(pact: &Pact, pred: impl Fn(&mediator_types::Sig) -> bool) -> Vec<String> {
    let mut v: Vec<String> = pact
        .predicates
        .iter()
        .filter(|s| pred(s))
        .map(|s| s.name.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The root (final-entry) hash of a certificate's signed record — the handle a
/// skeptic re-verifies. Empty chain ⇒ the genesis sentinel. (We touch only the
/// certificate's `record` here; the binding to the signature is what makes a
/// tally entry re-checkable.)
fn record_root(cert: &PactCertificate) -> String {
    match cert.record.entries.last() {
        Some(e) => e.hash.clone(),
        None => "0".repeat(64),
    }
}
