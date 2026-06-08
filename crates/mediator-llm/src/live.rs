//! `live` — real, on-stage LLM formalization against **AWS Bedrock** (Converse
//! API). This is the *model-proposes* half of the kernel, live: a visitor's
//! free-text claim goes in, and a small council of models each return a typed
//! [`Formula`] proposal that the trusted gate can later check.
//!
//! # Trust boundary
//!
//! Nothing here is authoritative. Each model's output is parsed into the
//! `mediator-types` IR, structurally validated against the dispute's signature,
//! and rendered back to English by the *deterministic* renderer in
//! `mediator-core` — never by the model's own prose. The model's raw text is
//! kept only for transparency.
//!
//! # Offline safety
//!
//! Every Bedrock call is gated behind the async [`council_formalize_live`]. The
//! pure helpers ([`validate_formula`], [`extract_formula_json`],
//! [`normalize_formula`], the consensus logic) are network-free and are what the
//! offline tests exercise. The crate builds and its tests pass with no network
//! and no AWS credentials.
//!
//! # Cost / abuse guardrails
//!
//! This runs on a public, authed box, so we cap the work hard:
//! - the claim is length-limited ([`MAX_CLAIM_CHARS`]) before any model call;
//! - `maxTokens` is small ([`MAX_OUTPUT_TOKENS`]) — a `Formula` is tiny;
//! - temperature is 0 (deterministic, no resampling);
//! - the AWS client is built once and shared across the concurrent calls.

use mediator_core::render::formula_to_english;
use mediator_types::{Formula, Sig, Sort, Term};

/// Hard cap on the visitor's free-text claim. Anything longer is rejected
/// before a single token is spent (public-box abuse guard).
pub const MAX_CLAIM_CHARS: usize = 2_000;

/// Output token cap for the Converse call. A `Formula` JSON object is tiny;
/// this is plenty and keeps spend bounded.
pub const MAX_OUTPUT_TOKENS: i32 = 600;

// ─────────────────────────────── config ─────────────────────────────────

/// The provider behind a model. Used to surface **per-provider dissent**: when
/// models from *different* providers disagree, that is a stronger signal of
/// genuine ambiguity than two checkpoints of the same family splitting, because
/// their training biases are uncorrelated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    Anthropic,
    DeepSeek,
    Mistral,
    Amazon,
        /// NVIDIA — kept for inference of legacy model ids that contain "nvidia" or "nemotron".
    Nvidia,
    /// Alibaba — the lineage of the open Qwen models (incl. Qwen3-VL 235B, the flagship voice).
    Qwen,
    Other,
}

impl Provider {
    /// A short human label.
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::DeepSeek => "DeepSeek",
            Provider::Mistral => "Mistral",
            Provider::Amazon => "Amazon",
            Provider::Nvidia => "NVIDIA",
            Provider::Qwen => "Alibaba (Qwen)",
            Provider::Other => "Other",
        }
    }

    /// Whether this lineage is open (open-weights / open model) rather than a
    /// closed, proprietary one. DeepSeek, Mistral, NVIDIA, and Alibaba (Qwen)
    /// ship open models; Anthropic and Amazon are proprietary. `Other` is
    /// treated as not-known-open (conservative: only enthrone lineages we can
    /// vouch are open).
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            Provider::DeepSeek | Provider::Mistral | Provider::Nvidia | Provider::Qwen
        )
    }

    /// Best-effort inference of the provider from a Bedrock model id. Bedrock
    /// ids are provider-prefixed (`amazon.*`, `mistral.*`, `deepseek.*`) or
    /// carry a region-inference prefix in front of `anthropic.*`
    /// (`us.anthropic.*`), so a substring match is reliable.
    pub fn from_model_id(id: &str) -> Self {
        let id = id.to_ascii_lowercase();
        if id.contains("anthropic") || id.contains("claude") {
            Provider::Anthropic
        } else if id.contains("deepseek") {
            Provider::DeepSeek
        } else if id.contains("mistral") {
            Provider::Mistral
        } else if id.contains("amazon") || id.contains("nova") || id.contains("titan") {
            Provider::Amazon
        } else if id.contains("nvidia") || id.contains("nemotron") {
            Provider::Nvidia
        } else if id.contains("qwen") {
            Provider::Qwen
        } else {
            Provider::Other
        }
    }
}

/// A model id paired with a friendly display name.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelSpec {
    /// The Bedrock model id, e.g. `"us.anthropic.claude-haiku-4-5-20251001-v1:0"`.
    pub id: String,
    /// A human label shown in the UI, e.g. `"Claude Haiku 4.5"`.
    pub label: String,
}

impl ModelSpec {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    /// The inferred provider for this model (used for per-provider dissent).
    pub fn provider(&self) -> Provider {
        Provider::from_model_id(&self.id)
    }

    /// Whether this model is from a non-proprietary, open lineage. The members
    /// of the [`open_panel`] are the open models; Anthropic and Amazon are the
    /// proprietary lineages we explicitly do *not* enthrone in the open council.
    pub fn is_open(&self) -> bool {
        self.provider().is_open()
    }
}

// ── confirmed-working Bedrock model ids (Converse API is uniform across all) ──
//
// These are the verified ids on the `commonquant-ember` account. The Converse
// API path is identical for every one of them; only the id string differs.

/// Claude Haiku 4.5 (Anthropic) — fast, warm. Available for the mixed panel.
pub const MODEL_CLAUDE_HAIKU_45: &str = "us.anthropic.claude-haiku-4-5-20251001-v1:0";
/// DeepSeek V3.2 — strong, independent reasoning. Used as the *neutrality*
/// judge precisely because it is NOT the Qwen mediator voice (independent lineage,
/// uncorrelated blind spots).
pub const MODEL_DEEPSEEK_V32: &str = "deepseek.v3.2";
/// Mistral Large 3 (675B) — a big, uncorrelated European open model.
pub const MODEL_MISTRAL_LARGE_3: &str = "mistral.mistral-large-3-675b-instruct";
/// Amazon Nova Pro — available for mixed panels; proprietary lineage.
pub const MODEL_NOVA_PRO: &str = "amazon.nova-pro-v1:0";
/// Amazon Nova Lite — cheap, fast, for high-volume / latency-sensitive work.
pub const MODEL_NOVA_LITE: &str = "amazon.nova-lite-v1:0";

// ── fully-open council ids (Converse API, same uniform path) ──────────────
//
// Confirmed-working on `commonquant-ember`. These are the open-weights models
// that constitute the default open council — **no proprietary power enthroned**.

/// Qwen3-VL 235B (22B-active MoE, Alibaba) — **fully open**, the FLAGSHIP
/// voice of the mediator. The face the parties hear is an open model: values
/// made literal. `mediator-session` / `mediator-web` adopt this for the
/// mediator's prose.
pub const MODEL_QWEN3_VL_235B: &str = "qwen.qwen3-vl-235b-a22b";

/// The open model the mediator's **voice** speaks in. Values made literal: the
/// face the parties hear is itself a fully-open model, not a proprietary one.
/// `mediator-session` / `mediator-web` adopt this for the mediator's prose.
pub const FLAGSHIP_MODEL: &str = MODEL_QWEN3_VL_235B;

/// When set to a *falsy* value (`0`, `false`, `no`, `off`) this overrides the
/// default and selects the proprietary-diverse panel instead of the open one.
/// Unset or truthy ⇒ [`LiveConfig::default`] returns the fully-open council
/// (the default). Recognized falsy values: `0`, `false`, `no`, `off` (any case).
pub const OPEN_COUNCIL_ENV: &str = "MEDIATEOR_OPEN_COUNCIL";

/// Whether the open council is the current default. Returns `true` unless
/// `MEDIATEOR_OPEN_COUNCIL` is explicitly set to a falsy value. The open
/// council is the default — no proprietary power enthroned by default.
/// Reads the process environment each call (so a test can set/unset it).
pub fn open_council_requested() -> bool {
    std::env::var(OPEN_COUNCIL_ENV)
        .map(|v| is_truthy(&v))
        .unwrap_or(true) // open by default
}

fn is_truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The cheap, high-volume model. Use this where a single fast read suffices
/// (triage, pre-filtering, anything fired per-keystroke) rather than the full
/// panel — it keeps spend and latency bounded. Distinct from the depth-oriented
/// formalization panel below.
pub const HIGH_VOLUME_MODEL: &str = MODEL_NOVA_LITE;

/// The neutrality judge: it must be **independent of the mediator's voice** so
/// that a Claude utterance is not graded by a Claude judge (correlated blind
/// spots). Default: DeepSeek V3.2 — strong reasoning, distinct lineage. This
/// constant is the single source of truth shared with [`crate::neutrality`].
pub const NEUTRALITY_MODEL: &str = MODEL_DEEPSEEK_V32;

/// A mixed **formalization panel** spanning four independent providers
/// (Anthropic, DeepSeek, Mistral, Amazon). Useful when a proprietary-diverse
/// panel is explicitly preferred over the open default. Their biases are
/// uncorrelated, so where they disagree on the normalized IR the disagreement
/// localizes genuine ambiguity rather than one model's quirk.
///
/// Not the default — use [`open_panel`] / [`LiveConfig::default`] for the
/// values-aligned open council. Available when `MEDIATEOR_OPEN_COUNCIL=0`.
///
/// Latency note: four concurrent calls, one of them a 675B model. For
/// latency-sensitive / high-volume work use a single [`HIGH_VOLUME_MODEL`]
/// read instead (see [`LiveConfig::high_volume`]).
pub fn default_panel() -> Vec<ModelSpec> {
    vec![
        ModelSpec::new(MODEL_CLAUDE_HAIKU_45, "Claude Haiku 4.5"),
        ModelSpec::new(MODEL_DEEPSEEK_V32, "DeepSeek V3.2"),
        ModelSpec::new(MODEL_MISTRAL_LARGE_3, "Mistral Large 3 (675B)"),
        ModelSpec::new(MODEL_NOVA_PRO, "Nova Pro"),
    ]
}

/// The **fully-open council** — the default panel. Every member is an
/// open-weights model, so *no proprietary power is enthroned*. This is the
/// project's constitution made literal: a stranger can trust the council
/// without trusting any one company's closed model, because the whole panel is
/// inspectable lineage. It spans three uncorrelated open lineages (Mistral,
/// DeepSeek, Alibaba/Qwen).
///
/// Members (in order):
/// - Mistral Large 3 (675B) — large open European model;
/// - DeepSeek V3.2 — strong open reasoning (also the neutrality judge);
/// - Qwen3-VL 235B — the open flagship and the mediator's **voice**.
///
/// This is returned by [`LiveConfig::default`] and [`LiveConfig::open`].
pub fn open_panel() -> Vec<ModelSpec> {
    vec![
        ModelSpec::new(MODEL_MISTRAL_LARGE_3, "Mistral Large 3 (675B)"),
        ModelSpec::new(MODEL_DEEPSEEK_V32, "DeepSeek V3.2"),
        ModelSpec::new(MODEL_QWEN3_VL_235B, "Qwen3-VL 235B"),
    ]
}

/// Region + the council of models to consult.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveConfig {
    pub region: String,
    pub models: Vec<ModelSpec>,
}

impl Default for LiveConfig {
    /// The **fully-open council** ([`open_panel`]) — no proprietary power
    /// enthroned by default. The constitution made literal: the parties interact
    /// with an open-lineage panel they can trust without trusting any single
    /// company's closed model.
    ///
    /// Override with `MEDIATEOR_OPEN_COUNCIL=0` (or any falsy value) to use the
    /// proprietary-diverse [`default_panel`] instead. Every caller that uses
    /// `LiveConfig::default()` — session intake, the web front end, the
    /// `live-formalize` bin — inherits the choice transparently.
    fn default() -> Self {
        if open_council_requested() {
            Self::open()
        } else {
            Self {
                region: "us-east-1".to_string(),
                models: default_panel(),
            }
        }
    }
}

impl LiveConfig {
    /// The **fully-open council** ([`open_panel`]): no proprietary power
    /// enthroned, voiced by the open flagship Qwen3-VL 235B. This is also the
    /// result of [`LiveConfig::default`] — the open council is the default.
    pub fn open() -> Self {
        Self {
            region: "us-east-1".to_string(),
            models: open_panel(),
        }
    }

    /// Whether every model in this config is from a non-proprietary, open
    /// lineage. True for [`LiveConfig::open`]; the trust property the open
    /// council is built to guarantee.
    pub fn is_fully_open(&self) -> bool {
        !self.models.is_empty() && self.models.iter().all(|m| m.is_open())
    }

    /// A one-model config using the cheap [`HIGH_VOLUME_MODEL`]. For
    /// latency-sensitive / per-keystroke work where the full panel is overkill.
    pub fn high_volume() -> Self {
        Self {
            region: "us-east-1".to_string(),
            models: vec![ModelSpec::new(HIGH_VOLUME_MODEL, "Nova Lite")],
        }
    }

    /// The set of distinct providers represented in this panel. A panel that
    /// spans more providers has more uncorrelated bias and a more trustworthy
    /// disagreement signal.
    pub fn providers(&self) -> Vec<Provider> {
        let mut seen: Vec<Provider> = Vec::new();
        for m in &self.models {
            let p = m.provider();
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
        seen
    }
}

// ─────────────────────────────── results ────────────────────────────────

/// One model's reading of a free-text claim.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The model's display label.
    pub model: String,
    /// The provider behind the model (for per-provider dissent).
    pub provider: Provider,
    /// The parsed IR, or `None` if the model didn't produce valid `Formula` JSON.
    pub formula: Option<Formula>,
    /// Deterministic English render of `formula` (via `mediator-core`), or an
    /// error note when there is no formula.
    pub english: String,
    /// The model's raw text (trimmed), kept for transparency.
    pub raw: String,
    /// Whether `formula` is structurally valid against the signature.
    pub valid: bool,
    /// Structural problems, e.g. `"uses undeclared symbol 'foo'"`.
    pub issues: Vec<String>,
}

/// One candidate normalized formula in the council's ranking, with the support
/// it drew. Surfaced so the operator sees not just the winner but the full
/// field — and *which providers* backed each candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// The normalized-IR key (prose-independent identity of the proposal).
    pub normalized: String,
    /// A representative parsed formula for this key.
    pub formula: Formula,
    /// How many valid voters proposed this form.
    pub votes: usize,
    /// Distinct providers that backed this form (de-duplicated).
    pub providers: Vec<Provider>,
    /// Borda points: in each pairwise sense, a candidate beats every candidate
    /// ranked below it. With plurality scores this reduces to "votes-weighted
    /// rank", giving a stable total order even on ties. Higher = stronger.
    pub borda: usize,
}

/// The council's combined reading.
#[derive(Clone, Debug, PartialEq)]
pub struct CouncilReading {
    pub claim: String,
    pub readings: Vec<Reading>,
    /// The majority formula over *structurally normalized* IR (prose can't bias
    /// it). `None` if there is no majority among the valid readings.
    pub agreed: Option<Formula>,
    /// A human one-liner describing the vote.
    pub consensus: String,
    /// The full ranked field of candidate formulas (winner first), by Borda
    /// score then vote count. Empty when no model produced a valid formula.
    pub ranking: Vec<Candidate>,
    /// Whether a single candidate is a *Condorcet winner*: it strictly
    /// out-votes every other candidate pairwise. With single-choice ballots
    /// this is exactly "strictly more votes than the runner-up". A Condorcet
    /// winner is a firmer proposal than a mere plurality lead.
    pub condorcet_winner: bool,
    /// A confidence signal in `[0.0, 1.0]`: the fraction of all readings that
    /// backed the winning form, scaled by provider agreement. `0.0` when there
    /// is no valid winner. This is advisory — the prover is still the only
    /// authority — but it lets the UI distinguish "everyone agreed" from
    /// "a bare plurality."
    pub confidence: f64,
}

// ───────────────────────── structural validation ────────────────────────

/// Structural validation of a proposed formula against a signature: every
/// applied symbol must be declared, with matching arity. Returns
/// `(is_valid, issues)`. Issues accumulate so the operator sees every problem,
/// not just the first.
pub fn validate_formula(f: &Formula, sig: &[Sig]) -> (bool, Vec<String>) {
    let mut issues = Vec::new();
    check_formula(f, sig, &mut issues);
    (issues.is_empty(), issues)
}

fn check_formula(f: &Formula, sig: &[Sig], issues: &mut Vec<String>) {
    match f {
        Formula::Atom(t) => check_term(t, sig, issues),
        Formula::Eq(a, b) | Formula::Le(a, b) | Formula::Lt(a, b) => {
            check_term(a, sig, issues);
            check_term(b, sig, issues);
        }
        Formula::Not(p) | Formula::Obligation(p) | Formula::Permission(p) => {
            check_formula(p, sig, issues)
        }
        Formula::And(ps) | Formula::Or(ps) => {
            for p in ps {
                check_formula(p, sig, issues);
            }
        }
        Formula::Implies(a, b) | Formula::Iff(a, b) => {
            check_formula(a, sig, issues);
            check_formula(b, sig, issues);
        }
        Formula::Forall(_, _, body) | Formula::Exists(_, _, body) => {
            check_formula(body, sig, issues)
        }
    }
}

fn check_term(t: &Term, sig: &[Sig], issues: &mut Vec<String>) {
    match t {
        // Bound/quantified variables and integer literals carry no signature
        // obligation. (We don't track quantifier scopes; an undeclared symbol
        // legitimately introduced as a `Var` is the model's choice.)
        Term::Var(_) | Term::IntLit(_) => {}
        Term::App(name, args) => {
            match sig.iter().find(|s| &s.name == name) {
                None => issues.push(format!("uses undeclared symbol '{name}'")),
                Some(decl) => {
                    if decl.arg_sorts.len() != args.len() {
                        issues.push(format!(
                            "symbol '{name}' applied to {} argument(s) but declared with arity {}",
                            args.len(),
                            decl.arg_sorts.len()
                        ));
                    }
                }
            }
            for a in args {
                check_term(a, sig, issues);
            }
        }
    }
}

// ─────────────────────── structural normalization ───────────────────────

/// A canonical, prose-independent form of a formula, used for the majority
/// vote. Two readings that mean the same thing in IR must produce byte-equal
/// keys regardless of how a model framed them.
///
/// Normalizations:
/// - commutative/associative `And`/`Or`: children are normalized then sorted,
///   so child order can't split the vote;
/// - `Iff` and `Eq` are symmetric: their two sides are sorted;
/// - everything else is structural recursion.
///
/// We intentionally keep this conservative (no logical rewriting like De
/// Morgan): the vote should reflect genuine structural agreement, not a theorem
/// prover's notion of equivalence.
pub fn normalize_formula(f: &Formula) -> String {
    match f {
        Formula::Atom(t) => format!("Atom({})", normalize_term(t)),
        Formula::Eq(a, b) => {
            // `=` is symmetric.
            let mut sides = [normalize_term(a), normalize_term(b)];
            sides.sort();
            format!("Eq({},{})", sides[0], sides[1])
        }
        Formula::Le(a, b) => format!("Le({},{})", normalize_term(a), normalize_term(b)),
        Formula::Lt(a, b) => format!("Lt({},{})", normalize_term(a), normalize_term(b)),
        Formula::Not(p) => format!("Not({})", normalize_formula(p)),
        Formula::And(ps) => format!("And({})", normalize_sorted(ps)),
        Formula::Or(ps) => format!("Or({})", normalize_sorted(ps)),
        Formula::Implies(a, b) => {
            format!("Implies({},{})", normalize_formula(a), normalize_formula(b))
        }
        Formula::Iff(a, b) => {
            // `↔` is symmetric.
            let mut sides = [normalize_formula(a), normalize_formula(b)];
            sides.sort();
            format!("Iff({},{})", sides[0], sides[1])
        }
        Formula::Forall(v, s, body) => {
            format!("Forall({v},{},{})", normalize_sort(s), normalize_formula(body))
        }
        Formula::Exists(v, s, body) => {
            format!("Exists({v},{},{})", normalize_sort(s), normalize_formula(body))
        }
        Formula::Obligation(p) => format!("Obl({})", normalize_formula(p)),
        Formula::Permission(p) => format!("Perm({})", normalize_formula(p)),
    }
}

fn normalize_sorted(ps: &[Formula]) -> String {
    let mut parts: Vec<String> = ps.iter().map(normalize_formula).collect();
    parts.sort();
    parts.join(",")
}

fn normalize_term(t: &Term) -> String {
    match t {
        Term::Var(v) => format!("Var({v})"),
        Term::IntLit(n) => format!("Int({n})"),
        Term::App(name, args) => {
            let inner: Vec<String> = args.iter().map(normalize_term).collect();
            format!("App({name},[{}])", inner.join(","))
        }
    }
}

fn normalize_sort(s: &Sort) -> String {
    match s {
        Sort::Bool => "Bool".to_string(),
        Sort::Int => "Int".to_string(),
        Sort::Real => "Real".to_string(),
        Sort::Uninterp(n) => format!("U({n})"),
    }
}

// ───────────────────────── JSON extraction / parse ───────────────────────

/// Extract a `Formula` from a model's raw text, tolerating code fences and
/// surrounding prose. We try, in order: the whole trimmed text; the contents of
/// a fenced ```` ```json ```` / ```` ``` ```` block; and finally the first
/// balanced `{...}` object found in the text. Returns the parsed `Formula` and
/// the exact JSON slice that parsed (for the record).
pub fn extract_formula_json(raw: &str) -> Result<(Formula, String), String> {
    let trimmed = raw.trim();

    // 1. Whole thing.
    if let Ok(f) = serde_json::from_str::<Formula>(trimmed) {
        return Ok((f, trimmed.to_string()));
    }

    // 2. Fenced block: ```json ... ``` or ``` ... ```.
    if let Some(inner) = fenced_block(trimmed) {
        let inner = inner.trim();
        if let Ok(f) = serde_json::from_str::<Formula>(inner) {
            return Ok((f, inner.to_string()));
        }
    }

    // 3. First balanced JSON object anywhere in the text.
    if let Some(obj) = first_json_object(trimmed) {
        if let Ok(f) = serde_json::from_str::<Formula>(obj) {
            return Ok((f, obj.to_string()));
        }
    }

    Err("no parseable Formula JSON found in model output".to_string())
}

/// Return the inside of the first fenced code block, if any.
fn fenced_block(s: &str) -> Option<&str> {
    let start = s.find("```")?;
    let after = &s[start + 3..];
    // Skip an optional language tag up to the first newline.
    let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
    let body = &after[body_start..];
    let end = body.find("```")?;
    Some(&body[..end])
}

/// Find the first balanced `{ ... }` object, respecting string literals and
/// escapes so braces inside strings don't fool the matcher.
fn first_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

// ───────────────────────────── prompting ────────────────────────────────

/// The system prompt: who the model is and the exact IR contract.
fn system_prompt() -> String {
    // Few-shot examples use the SAME serde shape mediator-types emits.
    let ex_not = serde_json::to_string(&Formula::Not(Box::new(Formula::Atom(Term::App(
        "p".to_string(),
        vec![],
    )))))
    .unwrap();
    let ex_eq = serde_json::to_string(&Formula::Eq(
        Term::App("total".to_string(), vec![]),
        Term::IntLit(50000),
    ))
    .unwrap();

    format!(
        "You translate a person's plain-language claim into a typed logical \
formula (IR), reusing a fixed set of declared symbols.\n\
\n\
Output ONLY a single JSON object that deserializes into this `Formula`:\n\
  Formula = one of:\n\
    {{\"Atom\": <Term>}}                         (a Bool-valued predicate)\n\
    {{\"Eq\": [<Term>, <Term>]}}                 (equality)\n\
    {{\"Le\": [<Term>, <Term>]}}  {{\"Lt\": [<Term>, <Term>]}}\n\
    {{\"Not\": <Formula>}}\n\
    {{\"And\": [<Formula>, ...]}}  {{\"Or\": [<Formula>, ...]}}\n\
    {{\"Implies\": [<Formula>, <Formula>]}}  {{\"Iff\": [<Formula>, <Formula>]}}\n\
    {{\"Forall\": [\"x\", <Sort>, <Formula>]}}  {{\"Exists\": [\"x\", <Sort>, <Formula>]}}\n\
    {{\"Obligation\": <Formula>}}  {{\"Permission\": <Formula>}}\n\
  Term = one of:\n\
    {{\"Var\": \"x\"}}   {{\"IntLit\": 50000}}   {{\"App\": [\"symbol\", [<Term>, ...]]}}\n\
  Sort = \"Bool\" | \"Int\" | \"Real\" | {{\"Uninterp\": \"name\"}}\n\
\n\
A nullary predicate/constant is `{{\"App\": [\"name\", []]}}`.\n\
Money is ALWAYS integer cents: $500.00 is `{{\"IntLit\": 50000}}`.\n\
\n\
Examples:\n\
  \"it is not the case that p\"            -> {ex_not}\n\
  \"the total is five hundred dollars\"    -> {ex_eq}\n\
\n\
RULES:\n\
- REUSE the declared symbols below verbatim. Do NOT invent synonyms or new \
symbol names; if the claim doesn't map onto the declared symbols, pick the \
closest declared symbol rather than coining one.\n\
- Do NOT include prose, explanation, or markdown. Output the JSON object only."
    )
}

/// The user prompt: the visitor's claim plus the declared signature.
fn user_prompt(claim: &str, sig: &[Sig]) -> String {
    let mut s = String::new();
    s.push_str("Declared symbols (name : arg_sorts -> ret  — gloss):\n");
    if sig.is_empty() {
        s.push_str("  (none declared)\n");
    } else {
        for d in sig {
            let args = if d.arg_sorts.is_empty() {
                "()".to_string()
            } else {
                let parts: Vec<String> = d.arg_sorts.iter().map(sort_name).collect();
                format!("({})", parts.join(", "))
            };
            s.push_str(&format!(
                "  {} : {} -> {}  — {}\n",
                d.name,
                args,
                sort_name(&d.ret),
                d.gloss
            ));
        }
    }
    s.push_str("\nClaim:\n  ");
    s.push_str(claim);
    s.push_str("\n\nReturn the single JSON Formula object now.");
    s
}

fn sort_name(s: &Sort) -> String {
    match s {
        Sort::Bool => "Bool".to_string(),
        Sort::Int => "Int".to_string(),
        Sort::Real => "Real".to_string(),
        Sort::Uninterp(n) => n.clone(),
    }
}

// ───────────────────────── reading construction ──────────────────────────

/// Turn one model's raw text into a [`Reading`] (pure; no network). Exposed at
/// crate level so tests can exercise it without Bedrock.
///
/// `provider` is the lineage behind the model (for per-provider dissent).
/// Tests that don't care may pass [`Provider::Other`].
pub(crate) fn reading_from_raw(
    label: &str,
    provider: Provider,
    raw: &str,
    sig: &[Sig],
) -> Reading {
    let raw_trimmed = raw.trim().to_string();
    match extract_formula_json(&raw_trimmed) {
        Ok((formula, _slice)) => {
            let (valid, issues) = validate_formula(&formula, sig);
            let english = formula_to_english(&formula);
            Reading {
                model: label.to_string(),
                provider,
                formula: Some(formula),
                english,
                raw: raw_trimmed,
                valid,
                issues,
            }
        }
        Err(e) => Reading {
            model: label.to_string(),
            provider,
            formula: None,
            english: format!("(no valid formula: {e})"),
            raw: raw_trimmed,
            valid: false,
            issues: vec![e],
        },
    }
}

/// The full outcome of tallying a panel: the agreed winner (if any), a human
/// consensus line, the ranked field, the Condorcet flag, and a confidence
/// signal. Returned by [`tally_consensus`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tally {
    pub agreed: Option<Formula>,
    pub consensus: String,
    pub ranking: Vec<Candidate>,
    pub condorcet_winner: bool,
    pub confidence: f64,
}

/// Compute the council's outcome from the readings. Only *structurally valid*
/// readings with a parsed formula vote, and the vote is over [`normalize_formula`]
/// keys so prose can't bias it.
///
/// # Ranking (Borda / Condorcet over normalized formulas)
///
/// With single-choice ballots, every candidate formula is one bin. We build a
/// total order by **Borda count**: a candidate's score is the number of
/// (candidate-instance ranked below it), summed across the implied pairwise
/// races. For single-choice ballots this collapses to a clean, ties-stable
/// ordering driven by vote count and then provider breadth. The top candidate
/// is a **Condorcet winner** iff it strictly out-votes every rival pairwise —
/// i.e. has strictly more votes than the runner-up.
///
/// # Per-provider dissent
///
/// Each [`Candidate`] records which *providers* backed it. Two checkpoints of
/// the same provider splitting is weak evidence of ambiguity; two *different*
/// providers splitting is strong evidence (uncorrelated biases). The
/// confidence signal weights provider agreement accordingly.
pub(crate) fn tally_consensus(readings: &[Reading]) -> Tally {
    let total = readings.len();

    // Collect (normalized-key, formula, provider) for the valid voters.
    let voters: Vec<(String, &Formula, Provider)> = readings
        .iter()
        .filter(|r| r.valid)
        .filter_map(|r| {
            r.formula
                .as_ref()
                .map(|f| (normalize_formula(f), f, r.provider))
        })
        .collect();

    if voters.is_empty() {
        return Tally {
            agreed: None,
            consensus: "No model produced a valid formula — that's the signal.".to_string(),
            ranking: Vec::new(),
            condorcet_winner: false,
            confidence: 0.0,
        };
    }

    // Bin by normalized key, remembering one representative formula, the vote
    // count, and the set of providers that backed each form.
    struct Bin<'a> {
        key: String,
        formula: &'a Formula,
        votes: usize,
        providers: Vec<Provider>,
    }
    let mut bins: Vec<Bin> = Vec::new();
    for (key, f, provider) in &voters {
        if let Some(b) = bins.iter_mut().find(|b| &b.key == key) {
            b.votes += 1;
            if !b.providers.contains(provider) {
                b.providers.push(*provider);
            }
        } else {
            bins.push(Bin {
                key: key.clone(),
                formula: f,
                votes: 1,
                providers: vec![*provider],
            });
        }
    }

    // Order the field: by votes desc, then provider breadth desc (a form backed
    // by 3 providers beats one backed by 1 at equal votes), then the normalized
    // key for a deterministic tiebreak.
    bins.sort_by(|a, b| {
        b.votes
            .cmp(&a.votes)
            .then(b.providers.len().cmp(&a.providers.len()))
            .then(a.key.cmp(&b.key))
    });

    // Borda over the ordered field. With single-choice ballots, every ballot
    // for candidate i ranks i above all candidates it out-votes; the Borda
    // points a candidate accrues is votes_i * (number of candidates strictly
    // below it in the order). We compute it directly from the sorted field so
    // it stays consistent with the displayed ranking.
    let n = bins.len();
    let ranking: Vec<Candidate> = bins
        .iter()
        .enumerate()
        .map(|(rank, b)| {
            let below = n - 1 - rank; // candidates ranked strictly below this one
            Candidate {
                normalized: b.key.clone(),
                formula: b.formula.clone(),
                votes: b.votes,
                providers: b.providers.clone(),
                borda: b.votes * below,
            }
        })
        .collect();

    let top = &ranking[0];
    let top_votes = top.votes;
    let winner = top.formula.clone();

    // No structural dissent among the valid voters: every valid model landed on
    // the SAME normalized IR.
    let no_dissent = ranking.len() == 1;
    // Unanimous: no dissent AND every reading was a valid voter.
    let unanimous = no_dissent && voters.len() == total;
    // A genuine *majority* of all readings (used when there IS dissent).
    let is_majority = top_votes * 2 > total;
    // Condorcet: the top strictly out-votes the runner-up (pairwise win over
    // every rival, since ballots are single-choice).
    let runner_up_votes = ranking.get(1).map(|c| c.votes).unwrap_or(0);
    let condorcet_winner = top_votes > runner_up_votes;

    // Provider agreement of the winner: a form backed by many distinct
    // providers is more trustworthy than one backed by a single lineage.
    let winner_providers = top.providers.len() as f64;
    let total_providers = {
        let mut seen: Vec<Provider> = Vec::new();
        for (_, _, p) in &voters {
            if !seen.contains(p) {
                seen.push(*p);
            }
        }
        seen.len().max(1) as f64
    };
    let provider_share = winner_providers / total_providers;

    // Confidence: fraction of ALL readings backing the winner, lifted toward
    // 1.0 by provider breadth. Bounded to [0, 1].
    let vote_share = top_votes as f64 / total as f64;
    let confidence = (0.5 * vote_share + 0.5 * provider_share * vote_share).clamp(0.0, 1.0);

    let consensus = if unanimous {
        let provs = top.providers.len();
        if provs >= 2 {
            format!("All {} models agreed (across {} providers).", total, provs)
        } else {
            "All models agreed.".to_string()
        }
    } else if no_dissent {
        // One agreed form, but some readings produced no valid formula.
        format!(
            "{} of {} agreed (others produced no valid formula).",
            top_votes, total
        )
    } else if is_majority {
        format!(
            "{} of {} agreed across {} provider(s); {} dissenting form(s).",
            top_votes,
            total,
            top.providers.len(),
            ranking.len() - 1
        )
    } else {
        format!(
            "The models split {} ways — that's the signal.",
            ranking.len()
        )
    };

    // Surface `agreed` when the valid voters did not structurally disagree
    // (their shared form is the only proposal — the gate checks it anyway), or
    // when one form holds a strict majority of all readings. A genuine split
    // without a majority yields `None`: the disagreement is itself the signal.
    let agreed = if no_dissent || is_majority {
        Some(winner)
    } else {
        None
    };

    Tally {
        agreed,
        consensus,
        ranking,
        condorcet_winner,
        confidence,
    }
}

// ─────────────────────────── live Bedrock call ───────────────────────────

/// Formalize a claim with the live Bedrock council, reusing the dispute's
/// signature symbols. Uses the **default AWS credential chain** (the EC2
/// instance role on the box; `~/.aws` locally) — credentials are never
/// hardcoded.
///
/// The models run **concurrently**. A model that errors or returns unparseable
/// text yields a `Reading` with `formula: None` rather than failing the whole
/// council, so one flaky model can't sink the panel.
/// A single Bedrock Converse call returning the model's text. Reused by the
/// mediation session for the mediator's *voice* — its prose, not load-bearing
/// facts (those stay certified by the prover). Uses the default AWS credential
/// chain (the EC2 instance role on the box; `~/.aws` locally).
pub async fn converse_text(
    region: &str,
    model_id: &str,
    system: &str,
    user: &str,
    max_tokens: i32,
    temperature: f32,
) -> anyhow::Result<String> {
    use aws_sdk_bedrockruntime::types::{
        ContentBlock, ConversationRole, InferenceConfiguration, Message, SystemContentBlock,
    };

    let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(region.to_string()))
        .load()
        .await;
    let client = aws_sdk_bedrockruntime::Client::new(&shared);

    let message = Message::builder()
        .role(ConversationRole::User)
        .content(ContentBlock::Text(user.to_string()))
        .build()
        .map_err(|e| anyhow::anyhow!("build message: {e}"))?;
    let inference = InferenceConfiguration::builder()
        .max_tokens(max_tokens)
        .temperature(temperature)
        .build();

    let resp = client
        .converse()
        .model_id(model_id)
        .system(SystemContentBlock::Text(system.to_string()))
        .messages(message)
        .inference_config(inference)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("converse({model_id}): {e}"))?;

    let out = resp
        .output()
        .ok_or_else(|| anyhow::anyhow!("converse({model_id}): no output"))?;
    let msg = out
        .as_message()
        .map_err(|_| anyhow::anyhow!("converse({model_id}): output not a message"))?;
    let mut text = String::new();
    for block in msg.content() {
        if let ContentBlock::Text(t) = block {
            text.push_str(t);
        }
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("converse({model_id}): empty text output");
    }
    Ok(text)
}

pub async fn council_formalize_live(
    claim: &str,
    sig: &[Sig],
    cfg: &LiveConfig,
) -> anyhow::Result<CouncilReading> {
    use aws_sdk_bedrockruntime::types::{
        ContentBlock, ConversationRole, InferenceConfiguration, Message, SystemContentBlock,
    };

    let claim = claim.trim();
    if claim.is_empty() {
        anyhow::bail!("empty claim");
    }
    if claim.chars().count() > MAX_CLAIM_CHARS {
        anyhow::bail!(
            "claim too long ({} chars; max {})",
            claim.chars().count(),
            MAX_CLAIM_CHARS
        );
    }

    // Build the Bedrock client once (default credential chain), share it.
    let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(cfg.region.clone()))
        .load()
        .await;
    let client = aws_sdk_bedrockruntime::Client::new(&shared);

    let sys = system_prompt();
    let usr = user_prompt(claim, sig);

    // Fire all models concurrently.
    let mut set = tokio::task::JoinSet::new();
    for (idx, spec) in cfg.models.iter().enumerate() {
        let client = client.clone();
        let model_id = spec.id.clone();
        let label = spec.label.clone();
        let provider = spec.provider();
        let sys = sys.clone();
        let usr = usr.clone();
        set.spawn(async move {
            let raw: anyhow::Result<String> = async {
                let message = Message::builder()
                    .role(ConversationRole::User)
                    .content(ContentBlock::Text(usr))
                    .build()
                    .map_err(|e| anyhow::anyhow!("build message: {e}"))?;

                let inference = InferenceConfiguration::builder()
                    .max_tokens(MAX_OUTPUT_TOKENS)
                    .temperature(0.0)
                    .build();

                let resp = client
                    .converse()
                    .model_id(&model_id)
                    .system(SystemContentBlock::Text(sys))
                    .messages(message)
                    .inference_config(inference)
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("converse({model_id}): {e}"))?;

                let out = resp
                    .output()
                    .ok_or_else(|| anyhow::anyhow!("converse({model_id}): no output"))?;
                let msg = out
                    .as_message()
                    .map_err(|_| anyhow::anyhow!("converse({model_id}): output not a message"))?;

                let mut text = String::new();
                for block in msg.content() {
                    if let ContentBlock::Text(t) = block {
                        text.push_str(t);
                    }
                }
                if text.trim().is_empty() {
                    anyhow::bail!("converse({model_id}): empty text output");
                }
                Ok(text)
            }
            .await;
            (idx, label, provider, raw)
        });
    }

    // Collect, then re-sort to cfg order for stable output.
    let mut out: Vec<(usize, Reading)> = Vec::with_capacity(cfg.models.len());
    while let Some(joined) = set.join_next().await {
        let (idx, label, provider, raw) =
            joined.map_err(|e| anyhow::anyhow!("task join error: {e}"))?;
        let reading = match raw {
            Ok(text) => reading_from_raw(&label, provider, &text, sig),
            Err(e) => Reading {
                model: label,
                provider,
                formula: None,
                english: format!("(model call failed: {e})"),
                raw: String::new(),
                valid: false,
                issues: vec![format!("model call failed: {e}")],
            },
        };
        out.push((idx, reading));
    }
    out.sort_by_key(|(idx, _)| *idx);
    let readings: Vec<Reading> = out.into_iter().map(|(_, r)| r).collect();

    let tally = tally_consensus(&readings);

    Ok(CouncilReading {
        claim: claim.to_string(),
        readings,
        agreed: tally.agreed,
        consensus: tally.consensus,
        ranking: tally.ranking,
        condorcet_winner: tally.condorcet_winner,
        confidence: tally.confidence,
    })
}

// ───────────────────────────────── tests ─────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::Sort;

    /// Serializes the tests that touch the `MEDIATEOR_OPEN_COUNCIL` process env,
    /// so a set-var in one can't race the `LiveConfig::default()` read in
    /// another (Rust runs tests multithreaded in one binary). Poison is fine to
    /// ignore — we only use it for ordering.
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn roommate_sig() -> Vec<Sig> {
        vec![
            Sig {
                name: "stain_is_damage".into(),
                arg_sorts: vec![],
                ret: Sort::Bool,
                gloss: "the carpet stain counts as chargeable damage".into(),
            },
            Sig {
                name: "claimed_total".into(),
                arg_sorts: vec![],
                ret: Sort::Int,
                gloss: "the verbally claimed deduction total, in cents".into(),
            },
        ]
    }

    // ── validate_formula ─────────────────────────────────────────────────

    #[test]
    fn validate_accepts_declared_symbols() {
        let f = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        let (ok, issues) = validate_formula(&f, &roommate_sig());
        assert!(ok, "declared symbol should validate: {issues:?}");
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_flags_undeclared_symbol() {
        let f = Formula::Atom(Term::App("totally_made_up".into(), vec![]));
        let (ok, issues) = validate_formula(&f, &roommate_sig());
        assert!(!ok, "undeclared symbol must fail");
        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].contains("undeclared symbol 'totally_made_up'"),
            "issue text: {}",
            issues[0]
        );
    }

    #[test]
    fn validate_flags_arity_mismatch() {
        // claimed_total is nullary; apply it to an argument.
        let f = Formula::Eq(
            Term::App("claimed_total".into(), vec![Term::IntLit(1)]),
            Term::IntLit(50000),
        );
        let (ok, issues) = validate_formula(&f, &roommate_sig());
        assert!(!ok);
        assert!(
            issues.iter().any(|s| s.contains("arity")),
            "should flag arity: {issues:?}"
        );
    }

    #[test]
    fn validate_intlit_and_var_carry_no_obligation() {
        let f = Formula::Eq(
            Term::App("claimed_total".into(), vec![]),
            Term::IntLit(50000),
        );
        let (ok, issues) = validate_formula(&f, &roommate_sig());
        assert!(ok, "{issues:?}");
    }

    // ── extract_formula_json ─────────────────────────────────────────────

    #[test]
    fn extract_parses_bare_json() {
        let raw = r#"{"Atom":{"App":["stain_is_damage",[]]}}"#;
        let (f, _) = extract_formula_json(raw).expect("bare json should parse");
        assert_eq!(f, Formula::Atom(Term::App("stain_is_damage".into(), vec![])));
    }

    #[test]
    fn extract_parses_fenced_json_block() {
        let raw = "Here is the formula:\n\
            ```json\n\
            {\"Not\":{\"Atom\":{\"App\":[\"stain_is_damage\",[]]}}}\n\
            ```\n\
            Hope that helps!";
        let (f, slice) = extract_formula_json(raw).expect("fenced json should parse");
        assert_eq!(
            f,
            Formula::Not(Box::new(Formula::Atom(Term::App(
                "stain_is_damage".into(),
                vec![]
            ))))
        );
        assert!(slice.contains("\"Not\""), "slice is the json: {slice}");
    }

    #[test]
    fn extract_parses_json_buried_in_prose() {
        let raw = "Sure! The answer is {\"Eq\":[{\"App\":[\"claimed_total\",[]]},{\"IntLit\":50000}]} and that's it.";
        let (f, _) = extract_formula_json(raw).expect("buried json should parse");
        assert_eq!(
            f,
            Formula::Eq(Term::App("claimed_total".into(), vec![]), Term::IntLit(50000))
        );
    }

    #[test]
    fn extract_ignores_braces_inside_strings() {
        // A name containing a brace shouldn't confuse the balanced matcher.
        let raw = r#"prefix {"Atom":{"App":["weird}name",[]]}} suffix"#;
        let (f, _) = extract_formula_json(raw).expect("should parse past string braces");
        assert_eq!(f, Formula::Atom(Term::App("weird}name".into(), vec![])));
    }

    #[test]
    fn extract_rejects_nonjson() {
        let raw = "I'm not sure how to formalize that, sorry.";
        assert!(extract_formula_json(raw).is_err());
    }

    // ── normalize_formula ────────────────────────────────────────────────

    #[test]
    fn normalize_is_order_insensitive_for_and() {
        let a = Formula::Atom(Term::App("a".into(), vec![]));
        let b = Formula::Atom(Term::App("b".into(), vec![]));
        let f1 = Formula::And(vec![a.clone(), b.clone()]);
        let f2 = Formula::And(vec![b, a]);
        assert_eq!(normalize_formula(&f1), normalize_formula(&f2));
    }

    #[test]
    fn normalize_distinguishes_different_formulas() {
        let p = Formula::Atom(Term::App("p".into(), vec![]));
        let np = Formula::Not(Box::new(p.clone()));
        assert_ne!(normalize_formula(&p), normalize_formula(&np));
    }

    // ── reading_from_raw + tally_consensus (offline council mechanics) ────

    // A reading helper for tests: model label, provider, raw, sig.
    fn reading(label: &str, provider: Provider, raw: &str) -> Reading {
        reading_from_raw(label, provider, raw, &roommate_sig())
    }

    #[test]
    fn reading_marks_invalid_for_undeclared() {
        let r = reading_from_raw(
            "TestModel",
            Provider::Other,
            r#"{"Atom":{"App":["bogus",[]]}}"#,
            &roommate_sig(),
        );
        assert!(r.formula.is_some());
        assert!(!r.valid);
        assert!(!r.issues.is_empty());
    }

    #[test]
    fn tally_unanimous() {
        let raw = r#"{"Atom":{"App":["stain_is_damage",[]]}}"#;
        // Same provider on both: the unanimous line should NOT claim multiple
        // providers.
        let readings = vec![
            reading("A", Provider::Anthropic, raw),
            reading("B", Provider::Anthropic, raw),
        ];
        let t = tally_consensus(&readings);
        assert_eq!(
            t.agreed,
            Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
        );
        assert_eq!(t.consensus, "All models agreed.");
        assert_eq!(t.ranking.len(), 1);
        assert!(t.condorcet_winner);
        // Single-provider unanimity: full vote share, single provider.
        assert!(t.confidence > 0.49, "confidence={}", t.confidence);
    }

    #[test]
    fn tally_unanimous_across_providers_is_noted() {
        let raw = r#"{"Atom":{"App":["stain_is_damage",[]]}}"#;
        let readings = vec![
            reading("A", Provider::Anthropic, raw),
            reading("B", Provider::DeepSeek, raw),
            reading("C", Provider::Mistral, raw),
        ];
        let t = tally_consensus(&readings);
        assert!(t.consensus.contains("3 providers"), "{}", t.consensus);
        // Unanimous across all providers → confidence 1.0.
        assert!((t.confidence - 1.0).abs() < 1e-9, "confidence={}", t.confidence);
        assert!(t.condorcet_winner);
    }

    #[test]
    fn tally_disagreement_yields_no_agreed() {
        let r1 = reading("A", Provider::Anthropic, r#"{"Atom":{"App":["stain_is_damage",[]]}}"#);
        let r2 = reading(
            "B",
            Provider::DeepSeek,
            r#"{"Not":{"Atom":{"App":["stain_is_damage",[]]}}}"#,
        );
        let t = tally_consensus(&[r1, r2]);
        assert_eq!(t.agreed, None);
        assert!(t.consensus.contains("split"), "{}", t.consensus);
        // A 1-1 split: no Condorcet winner (no strict pairwise lead).
        assert!(!t.condorcet_winner);
        assert_eq!(t.ranking.len(), 2);
    }

    #[test]
    fn tally_majority_over_three() {
        let same = r#"{"Atom":{"App":["stain_is_damage",[]]}}"#;
        let other = r#"{"Not":{"Atom":{"App":["stain_is_damage",[]]}}}"#;
        let readings = vec![
            reading("A", Provider::Anthropic, same),
            reading("B", Provider::DeepSeek, same),
            reading("C", Provider::Mistral, other),
        ];
        let t = tally_consensus(&readings);
        assert_eq!(
            t.agreed,
            Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
        );
        assert!(t.consensus.contains("2 of 3"), "{}", t.consensus);
        // 2 votes vs 1: a Condorcet winner.
        assert!(t.condorcet_winner);
        // Borda: winner ranked above one rival, votes 2 → borda 2.
        assert_eq!(t.ranking[0].borda, 2);
        assert_eq!(t.ranking[1].borda, 0);
    }

    #[test]
    fn tally_provider_breadth_breaks_tie() {
        // Two forms each get 2 votes, but one is backed by 2 distinct providers
        // and the other by 1. Provider breadth should rank the broader one first.
        let form_x = r#"{"Atom":{"App":["stain_is_damage",[]]}}"#;
        let form_y = r#"{"Not":{"Atom":{"App":["stain_is_damage",[]]}}}"#;
        let readings = vec![
            reading("A", Provider::Anthropic, form_x),
            reading("B", Provider::DeepSeek, form_x), // x: 2 providers
            reading("C", Provider::Mistral, form_y),
            reading("D", Provider::Mistral, form_y), // y: 1 provider
        ];
        let t = tally_consensus(&readings);
        assert_eq!(t.ranking.len(), 2);
        assert_eq!(t.ranking[0].votes, 2);
        assert_eq!(t.ranking[1].votes, 2);
        // x (2 providers) ranks first.
        assert_eq!(t.ranking[0].providers.len(), 2);
        assert_eq!(t.ranking[1].providers.len(), 1);
        // Tied on votes → no strict pairwise lead → not a Condorcet winner, and
        // not a majority (2 of 4), so no agreed formula.
        assert!(!t.condorcet_winner);
        assert_eq!(t.agreed, None);
    }

    #[test]
    fn tally_invalid_voters_excluded() {
        // One valid, one structurally-invalid (undeclared) reading.
        let valid = reading("A", Provider::Anthropic, r#"{"Atom":{"App":["stain_is_damage",[]]}}"#);
        let invalid = reading("B", Provider::DeepSeek, r#"{"Atom":{"App":["bogus",[]]}}"#);
        let t = tally_consensus(&[valid, invalid]);
        // The lone valid voter is the only proposal and faces no structural
        // dissent, so we surface it (the gate checks it regardless). The
        // consensus line stays honest that one reading produced nothing valid.
        assert_eq!(
            t.agreed,
            Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
        );
        assert!(t.consensus.contains("1 of 2"), "{}", t.consensus);
        assert!(t.consensus.contains("no valid formula"), "{}", t.consensus);
    }

    #[test]
    fn tally_all_invalid() {
        let r = reading("A", Provider::Anthropic, "not json at all");
        let t = tally_consensus(&[r]);
        assert_eq!(t.agreed, None);
        assert!(t.consensus.contains("No model"), "{}", t.consensus);
        assert_eq!(t.confidence, 0.0);
        assert!(t.ranking.is_empty());
    }

    // ── prompt construction is total / sane ──────────────────────────────

    #[test]
    fn prompts_mention_claim_and_symbols() {
        let sys = system_prompt();
        assert!(sys.contains("Formula"));
        assert!(sys.contains("IntLit"));
        let usr = user_prompt("the deposit should be 1200 dollars", &roommate_sig());
        assert!(usr.contains("1200 dollars"));
        assert!(usr.contains("stain_is_damage"));
    }

    #[test]
    fn default_config_is_the_open_panel() {
        // Hold the env guard and ensure the open-council var is unset so the
        // implicit default (open council) is observed regardless of test ordering.
        let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(OPEN_COUNCIL_ENV);
        let cfg = LiveConfig::default();
        assert_eq!(cfg.region, "us-east-1");
        // The open panel: three fully-open models.
        assert_eq!(cfg.models.len(), 3);
        assert_eq!(cfg.models[0].label, "Mistral Large 3 (675B)");
        assert_eq!(cfg.models[2].label, "Qwen3-VL 235B");
        // Fully open — no proprietary model enthroned.
        assert!(cfg.is_fully_open());
        let provs = cfg.providers();
        assert_eq!(provs.len(), 3, "panel should span 3 open providers: {provs:?}");
        assert!(provs.contains(&Provider::Mistral));
        assert!(provs.contains(&Provider::DeepSeek));
        assert!(provs.contains(&Provider::Qwen));
    }

    #[test]
    fn high_volume_config_is_one_cheap_model() {
        let cfg = LiveConfig::high_volume();
        assert_eq!(cfg.models.len(), 1);
        assert_eq!(cfg.models[0].id, HIGH_VOLUME_MODEL);
        // The cheap model is an Amazon Nova (high-volume lineage).
        assert_eq!(cfg.models[0].provider(), Provider::Amazon);
    }

    #[test]
    fn provider_inference_from_bedrock_ids() {
        assert_eq!(
            Provider::from_model_id("us.anthropic.claude-haiku-4-5-20251001-v1:0"),
            Provider::Anthropic
        );
        assert_eq!(Provider::from_model_id("deepseek.v3.2"), Provider::DeepSeek);
        assert_eq!(
            Provider::from_model_id("mistral.mistral-large-3-675b-instruct"),
            Provider::Mistral
        );
        assert_eq!(
            Provider::from_model_id("amazon.nova-pro-v1:0"),
            Provider::Amazon
        );
        assert_eq!(Provider::from_model_id("some.unknown-model"), Provider::Other);
    }

    #[test]
    fn neutrality_model_is_independent_of_the_qwen_mediator() {
        // The neutrality judge must NOT share the mediator's (Qwen) lineage,
        // or it inherits correlated blind spots. DeepSeek is independent of Qwen.
        assert_ne!(
            Provider::from_model_id(NEUTRALITY_MODEL),
            Provider::Qwen,
            "neutrality judge must be independent of the Qwen mediator voice"
        );
        assert_eq!(Provider::from_model_id(NEUTRALITY_MODEL), Provider::DeepSeek);
    }

    // ── fully-open council ────────────────────────────────────────────────

    #[test]
    fn open_panel_is_three_open_models_voiced_by_qwen() {
        let panel = open_panel();
        assert_eq!(panel.len(), 3);
        // Mistral leads, Qwen3-VL 235B is the flagship voice at the end.
        assert_eq!(panel[0].id, MODEL_MISTRAL_LARGE_3);
        assert_eq!(panel[0].provider(), Provider::Mistral);
        assert_eq!(panel[2].id, MODEL_QWEN3_VL_235B);
        assert_eq!(panel[2].provider(), Provider::Qwen);
        // The exact open membership, in order.
        let ids: Vec<&str> = panel.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                MODEL_MISTRAL_LARGE_3,
                MODEL_DEEPSEEK_V32,
                MODEL_QWEN3_VL_235B,
            ]
        );
    }

    #[test]
    fn open_panel_has_no_proprietary_power_enthroned() {
        // Every member is an open lineage — the values made literal.
        for m in open_panel() {
            assert!(m.is_open(), "{} ({:?}) must be open", m.label, m.provider());
            assert_ne!(m.provider(), Provider::Anthropic);
            assert_ne!(m.provider(), Provider::Amazon);
        }
        assert!(LiveConfig::open().is_fully_open());
    }

    #[test]
    fn open_panel_spans_three_uncorrelated_open_providers() {
        let cfg = LiveConfig::open();
        let provs = cfg.providers();
        assert_eq!(provs.len(), 3, "open council spans 3 providers: {provs:?}");
        assert!(provs.contains(&Provider::Mistral));
        assert!(provs.contains(&Provider::DeepSeek));
        assert!(provs.contains(&Provider::Qwen));
    }

    #[test]
    fn flagship_is_open_qwen3_vl() {
        assert_eq!(FLAGSHIP_MODEL, MODEL_QWEN3_VL_235B);
        assert_eq!(Provider::from_model_id(FLAGSHIP_MODEL), Provider::Qwen);
        assert!(Provider::from_model_id(FLAGSHIP_MODEL).is_open());
        // The flagship the mediator's voice uses must itself be open.
        assert!(ModelSpec::new(FLAGSHIP_MODEL, "flagship").is_open());
    }

    #[test]
    fn open_model_constants_infer_open_providers() {
        // Qwen3-VL 235B — the flagship voice.
        assert_eq!(
            Provider::from_model_id("qwen.qwen3-vl-235b-a22b"),
            Provider::Qwen
        );
        assert!(Provider::Qwen.is_open());
        // Nvidia ids still resolve correctly (legacy / available-but-not-in-open-panel).
        assert_eq!(
            Provider::from_model_id("nvidia.nemotron-super-3-120b"),
            Provider::Nvidia
        );
        assert!(Provider::Nvidia.is_open());
        // The proprietary lineages are explicitly NOT open.
        assert!(!Provider::Anthropic.is_open());
        assert!(!Provider::Amazon.is_open());
        assert!(!Provider::Other.is_open());
    }

    #[test]
    fn default_panel_is_not_fully_open() {
        // The diverse default does enthrone proprietary models (Claude/Nova),
        // so it is NOT fully open — that's the whole point of the open council.
        assert!(!LiveConfig {
            region: "us-east-1".into(),
            models: default_panel(),
        }
        .is_fully_open());
    }

    #[test]
    fn truthy_env_values_recognized() {
        for v in ["1", "true", "TRUE", "Yes", "on", " on "] {
            assert!(is_truthy(v), "{v:?} should be truthy");
        }
        for v in ["0", "false", "no", "off", "", "open"] {
            assert!(!is_truthy(v), "{v:?} should be falsy");
        }
    }

    #[test]
    fn env_selects_open_council_for_default() {
        // Serialize with other env-touching tests; clear on the way out.
        let _g = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());

        // Unset (the normal case): open council is the default.
        std::env::remove_var(OPEN_COUNCIL_ENV);
        assert!(
            LiveConfig::default().is_fully_open(),
            "unset env must yield the open council (the default)"
        );
        assert_eq!(LiveConfig::default().models, open_panel());

        // Explicitly truthy: also the open council.
        std::env::set_var(OPEN_COUNCIL_ENV, "1");
        let cfg = LiveConfig::default();
        assert!(
            cfg.is_fully_open(),
            "MEDIATEOR_OPEN_COUNCIL=1 must yield the open council: {:?}",
            cfg.models
        );
        assert_eq!(cfg.models, open_panel());

        // Explicitly falsy: the proprietary-diverse panel is the override.
        std::env::set_var(OPEN_COUNCIL_ENV, "0");
        assert!(
            !LiveConfig::default().is_fully_open(),
            "MEDIATEOR_OPEN_COUNCIL=0 must override to the proprietary-diverse panel"
        );
        assert_eq!(LiveConfig::default().models, default_panel());

        // Restore.
        std::env::remove_var(OPEN_COUNCIL_ENV);
    }
}
