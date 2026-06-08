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

/// A model id paired with a friendly display name.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelSpec {
    /// The Bedrock model id, e.g. `"us.anthropic.claude-haiku-4-5-20251001-v1:0"`.
    pub id: String,
    /// A human label shown in the UI, e.g. `"Claude Haiku 4.5"`.
    pub label: String,
}

/// Region + the council of models to consult.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveConfig {
    pub region: String,
    pub models: Vec<ModelSpec>,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            models: vec![
                ModelSpec {
                    id: "us.anthropic.claude-haiku-4-5-20251001-v1:0".to_string(),
                    label: "Claude Haiku 4.5".to_string(),
                },
                ModelSpec {
                    id: "amazon.nova-lite-v1:0".to_string(),
                    label: "Nova Lite".to_string(),
                },
            ],
        }
    }
}

// ─────────────────────────────── results ────────────────────────────────

/// One model's reading of a free-text claim.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The model's display label.
    pub model: String,
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
pub(crate) fn reading_from_raw(label: &str, raw: &str, sig: &[Sig]) -> Reading {
    let raw_trimmed = raw.trim().to_string();
    match extract_formula_json(&raw_trimmed) {
        Ok((formula, _slice)) => {
            let (valid, issues) = validate_formula(&formula, sig);
            let english = formula_to_english(&formula);
            Reading {
                model: label.to_string(),
                formula: Some(formula),
                english,
                raw: raw_trimmed,
                valid,
                issues,
            }
        }
        Err(e) => Reading {
            model: label.to_string(),
            formula: None,
            english: format!("(no valid formula: {e})"),
            raw: raw_trimmed,
            valid: false,
            issues: vec![e],
        },
    }
}

/// Compute the majority `agreed` formula and a human consensus line from the
/// readings. Only *structurally valid* readings with a parsed formula vote.
/// The vote is over [`normalize_formula`] keys so prose can't bias it.
pub(crate) fn tally_consensus(readings: &[Reading]) -> (Option<Formula>, String) {
    let total = readings.len();

    // Collect (normalized-key, formula) for the valid voters.
    let voters: Vec<(String, &Formula)> = readings
        .iter()
        .filter(|r| r.valid)
        .filter_map(|r| r.formula.as_ref().map(|f| (normalize_formula(f), f)))
        .collect();

    if voters.is_empty() {
        return (
            None,
            "No model produced a valid formula — that's the signal.".to_string(),
        );
    }

    // Tally by normalized key, remembering one representative formula per key.
    let mut tally: Vec<(String, &Formula, usize)> = Vec::new();
    for (key, f) in &voters {
        if let Some(entry) = tally.iter_mut().find(|(k, _, _)| k == key) {
            entry.2 += 1;
        } else {
            tally.push((key.clone(), f, 1));
        }
    }
    tally.sort_by_key(|(_, _, votes)| std::cmp::Reverse(*votes));

    let (_, winner_f, top_votes) = &tally[0];
    let winner = (*winner_f).clone();
    let top_votes = *top_votes;

    // No structural dissent among the valid voters: every model that produced a
    // valid formula landed on the SAME normalized IR.
    let no_dissent = tally.len() == 1;
    // Unanimous: no dissent AND every reading was a valid voter.
    let unanimous = no_dissent && voters.len() == total;
    // A genuine *majority* of all readings (used when there IS dissent).
    let is_majority = top_votes * 2 > total;

    let consensus = if unanimous {
        "All models agreed.".to_string()
    } else if no_dissent {
        // One agreed form, but some readings produced no valid formula.
        format!(
            "{} of {} agreed (others produced no valid formula).",
            top_votes, total
        )
    } else if is_majority {
        format!("{} of {} agreed.", top_votes, total)
    } else {
        "The models disagreed — that's the signal.".to_string()
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

    (agreed, consensus)
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
            (idx, label, raw)
        });
    }

    // Collect, then re-sort to cfg order for stable output.
    let mut out: Vec<(usize, Reading)> = Vec::with_capacity(cfg.models.len());
    while let Some(joined) = set.join_next().await {
        let (idx, label, raw) = joined.map_err(|e| anyhow::anyhow!("task join error: {e}"))?;
        let reading = match raw {
            Ok(text) => reading_from_raw(&label, &text, sig),
            Err(e) => Reading {
                model: label,
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

    let (agreed, consensus) = tally_consensus(&readings);

    Ok(CouncilReading {
        claim: claim.to_string(),
        readings,
        agreed,
        consensus,
    })
}

// ───────────────────────────────── tests ─────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::Sort;

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

    #[test]
    fn reading_marks_invalid_for_undeclared() {
        let r = reading_from_raw(
            "TestModel",
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
        let readings = vec![
            reading_from_raw("A", raw, &roommate_sig()),
            reading_from_raw("B", raw, &roommate_sig()),
        ];
        let (agreed, consensus) = tally_consensus(&readings);
        assert_eq!(
            agreed,
            Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
        );
        assert_eq!(consensus, "All models agreed.");
    }

    #[test]
    fn tally_disagreement_yields_no_agreed() {
        let r1 = reading_from_raw(
            "A",
            r#"{"Atom":{"App":["stain_is_damage",[]]}}"#,
            &roommate_sig(),
        );
        let r2 = reading_from_raw(
            "B",
            r#"{"Not":{"Atom":{"App":["stain_is_damage",[]]}}}"#,
            &roommate_sig(),
        );
        let (agreed, consensus) = tally_consensus(&[r1, r2]);
        assert_eq!(agreed, None);
        assert!(consensus.contains("disagreed"), "{consensus}");
    }

    #[test]
    fn tally_majority_over_three() {
        let same = r#"{"Atom":{"App":["stain_is_damage",[]]}}"#;
        let other = r#"{"Not":{"Atom":{"App":["stain_is_damage",[]]}}}"#;
        let readings = vec![
            reading_from_raw("A", same, &roommate_sig()),
            reading_from_raw("B", same, &roommate_sig()),
            reading_from_raw("C", other, &roommate_sig()),
        ];
        let (agreed, consensus) = tally_consensus(&readings);
        assert_eq!(
            agreed,
            Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
        );
        assert_eq!(consensus, "2 of 3 agreed.");
    }

    #[test]
    fn tally_invalid_voters_excluded() {
        // One valid, one structurally-invalid (undeclared) reading.
        let valid = reading_from_raw(
            "A",
            r#"{"Atom":{"App":["stain_is_damage",[]]}}"#,
            &roommate_sig(),
        );
        let invalid = reading_from_raw("B", r#"{"Atom":{"App":["bogus",[]]}}"#, &roommate_sig());
        let (agreed, consensus) = tally_consensus(&[valid, invalid]);
        // The lone valid voter is the only proposal and faces no structural
        // dissent, so we surface it (the gate checks it regardless). The
        // consensus line stays honest that one reading produced nothing valid.
        assert_eq!(
            agreed,
            Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
        );
        assert!(consensus.contains("1 of 2"), "{consensus}");
        assert!(consensus.contains("no valid formula"), "{consensus}");
    }

    #[test]
    fn tally_all_invalid() {
        let r = reading_from_raw("A", "not json at all", &roommate_sig());
        let (agreed, consensus) = tally_consensus(&[r]);
        assert_eq!(agreed, None);
        assert!(consensus.contains("No model"), "{consensus}");
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
    fn default_config_has_two_models() {
        let cfg = LiveConfig::default();
        assert_eq!(cfg.region, "us-east-1");
        assert_eq!(cfg.models.len(), 2);
        assert_eq!(cfg.models[0].label, "Claude Haiku 4.5");
        assert_eq!(cfg.models[1].label, "Nova Lite");
    }
}
