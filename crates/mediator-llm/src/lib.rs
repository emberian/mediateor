//! `mediator-llm` — the **untrusted** operator + council layer.
//!
//! This crate proposes formalizations of natural-language claims; every
//! proposal is handed to `mediator-core` (the trusted gate), which runs it
//! through the Isabelle/HOL prover before it touches the record.  Nothing here
//! is authoritative; it is all data-to-be-checked.
//!
//! # Operators
//!
//! - [`ScriptedOperator`] — offline, pre-baked formalizations for the roommate
//!   demo.  Always works; no network, no credentials.
//! - [`LmStudioOperator`] — talks to a local LM Studio OpenAI-compatible
//!   endpoint (`http://localhost:1234/v1/chat/completions`).
//! - [`BedrockOperator`] — best-effort AWS Bedrock client for the
//!   `commonquant-ember` account; degrades to `Err` when credentials are
//!   absent so the crate always compiles and runs offline.
//!
//! # Council
//!
//! [`council_formalize`] runs `n` operators and takes a **majority vote over
//! the *normalized* (structurally equal) `Formula`s**.  Prose framing cannot
//! bias the verdict because the council never sees prose — it sees the parsed
//! IR.

use mediator_types::{Formula, LlmOperator, Sig, Sort, Term};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ─────────────────────────── ScriptedOperator ────────────────────────────

/// Offline fallback: returns pre-baked formalizations for the roommate demo.
///
/// Matching is done on a **stable substring** of the natural-language claim so
/// that minor whitespace/punctuation differences don't break the demo.  The
/// substrings are chosen from the canonical `nl` values in
/// `scenarios/roommate.json`.
pub struct ScriptedOperator;

/// Canonical formulas for the three roommate claims (mirrors `roommate.json`).
fn roommate_formula(nl: &str) -> Option<Formula> {
    // r1: Robin — "The carpet stain was ordinary wear and tear, not damage I should pay for."
    if nl.contains("ordinary wear and tear") || nl.contains("not damage I should pay for") {
        return Some(Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        )))));
    }
    // s1: Sam — "The stain is damage Robin caused, so Robin owes the carpet repair."
    if nl.contains("stain is damage Robin caused") || nl.contains("Robin owes the carpet repair") {
        return Some(Formula::Atom(Term::App("stain_is_damage".into(), vec![])));
    }
    // s2: Sam — "I told Robin the total deductions would come to about five hundred dollars."
    if nl.contains("total deductions would come to") || nl.contains("five hundred dollars") {
        return Some(Formula::Eq(
            Term::App("claimed_total".into(), vec![]),
            Term::IntLit(50000),
        ));
    }
    None
}

impl LlmOperator for ScriptedOperator {
    /// Return the pre-baked formula for the three roommate claims.
    ///
    /// Returns `Err` for unrecognized NL rather than fabricating something.
    fn formalize(&self, nl: &str, _sig: &[Sig]) -> Result<Formula, String> {
        roommate_formula(nl)
            .ok_or_else(|| format!("ScriptedOperator: no pre-baked formula for: {nl:?}"))
    }

    fn render_english(&self, f: &Formula) -> String {
        render_english(f)
    }
}

// ─────────────────────────── render_english ──────────────────────────────

/// Deterministic, readable English rendering of a [`Formula`].
///
/// Advisory only — the authoritative renderer lives in `mediator-core`.
/// This one is used by all operators as a consistent fallback.
pub fn render_english(f: &Formula) -> String {
    match f {
        Formula::Atom(t) => render_term(t),
        Formula::Eq(a, b) => format!("{} = {}", render_term(a), render_term(b)),
        Formula::Le(a, b) => format!("{} ≤ {}", render_term(a), render_term(b)),
        Formula::Lt(a, b) => format!("{} < {}", render_term(a), render_term(b)),
        Formula::Not(inner) => format!("not ({})", render_english(inner)),
        Formula::And(fs) if fs.is_empty() => "⊤".into(),
        Formula::And(fs) => fs
            .iter()
            .map(render_english)
            .collect::<Vec<_>>()
            .join(" ∧ "),
        Formula::Or(fs) if fs.is_empty() => "⊥".into(),
        Formula::Or(fs) => fs
            .iter()
            .map(render_english)
            .collect::<Vec<_>>()
            .join(" ∨ "),
        Formula::Implies(a, b) => {
            format!("({}) → ({})", render_english(a), render_english(b))
        }
        Formula::Iff(a, b) => format!("({}) ↔ ({})", render_english(a), render_english(b)),
        Formula::Forall(x, s, body) => {
            format!("∀ {} : {}. {}", x, render_sort(s), render_english(body))
        }
        Formula::Exists(x, s, body) => {
            format!("∃ {} : {}. {}", x, render_sort(s), render_english(body))
        }
        Formula::Obligation(inner) => format!("O({})", render_english(inner)),
        Formula::Permission(inner) => format!("P({})", render_english(inner)),
    }
}

fn render_term(t: &Term) -> String {
    match t {
        Term::Var(x) => x.clone(),
        Term::IntLit(n) => {
            // Display cents as dollars for readability.
            let dollars = n / 100;
            let cents = n.unsigned_abs() % 100;
            format!("${dollars}.{cents:02}")
        }
        Term::App(f, args) if args.is_empty() => f.clone(),
        Term::App(f, args) => {
            let rendered: Vec<_> = args.iter().map(render_term).collect();
            format!("{}({})", f, rendered.join(", "))
        }
    }
}

fn render_sort(s: &Sort) -> &str {
    match s {
        Sort::Bool => "Bool",
        Sort::Int => "Int",
        Sort::Real => "Real",
        Sort::Uninterp(n) => n.as_str(),
    }
}

// ─────────────────────────── LmStudioOperator ────────────────────────────

/// Talks to a local LM Studio OpenAI-compatible endpoint.
///
/// Endpoint: `http://localhost:1234/v1/chat/completions`
///
/// The model is prompted to emit **only** a JSON object that deserializes into
/// a [`Formula`] (using the same serde shape as `mediator-types`).  On any
/// parse or HTTP error this returns `Err`; it never fabricates a formula.
pub struct LmStudioOperator {
    /// Base URL, e.g. `"http://localhost:1234"`.
    pub base_url: String,
    /// Timeout in milliseconds.  Default: 10 000 ms.
    pub timeout_ms: u64,
}

impl LmStudioOperator {
    pub fn new() -> Self {
        Self {
            base_url: "http://localhost:1234".into(),
            timeout_ms: 10_000,
        }
    }

    /// Check whether the endpoint is reachable (used by [`default_operator`]).
    pub fn is_reachable(&self) -> bool {
        let url = format!("{}/v1/models", self.base_url);
        ureq::get(&url)
            .timeout(std::time::Duration::from_millis(self.timeout_ms.min(2_000)))
            .call()
            .is_ok()
    }
}

impl Default for LmStudioOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmOperator for LmStudioOperator {
    fn formalize(&self, nl: &str, sig: &[Sig]) -> Result<Formula, String> {
        let sig_json = serde_json::to_string(sig)
            .map_err(|e| format!("sig serialization error: {e}"))?;

        let system_prompt = format!(
            "You are a formal-logic assistant. \
             Given a natural-language claim and a signature of available symbols, \
             output ONLY a single JSON object that is a valid `Formula` in the following Rust serde shape:\n\
             Formula variants: Atom(Term), Eq(Term,Term), Le(Term,Term), Lt(Term,Term), \
             Not(Formula), And(Vec<Formula>), Or(Vec<Formula>), Implies(Formula,Formula), \
             Iff(Formula,Formula), Forall(var,Sort,Formula), Exists(var,Sort,Formula), \
             Obligation(Formula), Permission(Formula).\n\
             Term variants: Var(String), IntLit(i64), App(String, Vec<Term>).\n\
             Sort variants: Bool, Int, Real, Uninterp(String).\n\
             Money is always integer cents (e.g. $5.00 = 500).\n\
             Available signature symbols: {sig_json}\n\
             Output ONLY the JSON object, no explanation, no markdown fences."
        );

        let body = json!({
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user",   "content": nl }
            ],
            "temperature": 0.0,
            "max_tokens": 512
        });

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = ureq::post(&url)
            .timeout(std::time::Duration::from_millis(self.timeout_ms))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("LmStudio HTTP error: {e}"))?;

        // OpenAI-compatible response shape: choices[0].message.content
        let resp_json: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("LmStudio JSON parse error: {e}"))?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| "LmStudio: missing choices[0].message.content".to_string())?;

        // Strip any accidental markdown fences the model may have emitted.
        let trimmed = content.trim();
        let stripped = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed);
        let stripped = stripped.strip_suffix("```").unwrap_or(stripped);

        serde_json::from_str::<Formula>(stripped.trim())
            .map_err(|e| format!("LmStudio formula parse error: {e}\n  raw content: {stripped}"))
    }

    fn render_english(&self, f: &Formula) -> String {
        render_english(f)
    }
}

// ─────────────────────────── BedrockOperator ─────────────────────────────

/// AWS Bedrock operator for the `commonquant-ember` account.
///
/// # Status
///
/// A working implementation requires SigV4 request signing and a Bedrock
/// `InvokeModel` call.  This implementation is **structurally complete** but
/// requires one of:
///
/// - The `aws-sigv4` + `aws-credential-types` crates (or `aws-sdk-bedrockruntime`)
///   for proper signing, OR
/// - Valid AWS credentials in the environment (`AWS_ACCESS_KEY_ID`,
///   `AWS_SECRET_ACCESS_KEY`, optionally `AWS_SESSION_TOKEN`).
///
/// ## How to finish this
///
/// 1. Add to `Cargo.toml`:
///    ```toml
///    aws-sdk-bedrockruntime = "1"
///    tokio = { version = "1", features = ["rt"] }
///    ```
///    (or `aws-sigv4` + `ureq` for a sync path without tokio).
///
/// 2. Replace [`BedrockOperator::invoke_model`] with a real SigV4-signed POST
///    to:
///    `https://bedrock-runtime.{region}.amazonaws.com/model/{model_id}/invoke`
///
/// 3. The request body for Anthropic Claude models on Bedrock is:
///    ```json
///    { "anthropic_version": "bedrock-2023-05-31",
///      "max_tokens": 512,
///      "messages": [ ... ] }
///    ```
///    The response body contains `content[0].text`.
///
/// Until credentials are present, `formalize` returns `Err` immediately so
/// the demo and offline tests are not blocked.
pub struct BedrockOperator {
    /// AWS region, e.g. `"us-east-1"`.
    pub region: String,
    /// Bedrock model ID, e.g. `"anthropic.claude-3-haiku-20240307-v1:0"`.
    pub model_id: String,
}

impl BedrockOperator {
    pub fn new() -> Self {
        Self {
            region: "us-east-1".into(),
            model_id: "anthropic.claude-3-haiku-20240307-v1:0".into(),
        }
    }

    /// Returns `true` when AWS credentials are present in the environment.
    ///
    /// This is a lightweight env-var check; it does not attempt a real API call.
    pub fn has_credentials() -> bool {
        std::env::var("AWS_ACCESS_KEY_ID").is_ok() || std::env::var("AWS_PROFILE").is_ok()
    }

    /// Stub: would perform a SigV4-signed `POST` to Bedrock InvokeModel.
    /// Returns `Err` without credentials or until real signing is wired.
    fn invoke_model(&self, _prompt_body: &serde_json::Value) -> Result<String, String> {
        if !Self::has_credentials() {
            return Err(
                "BedrockOperator: no AWS credentials found; \
                 set AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY or AWS_PROFILE"
                    .into(),
            );
        }

        // ── TODO: replace this block with real SigV4 + ureq ──────────────
        //
        // let endpoint = format!(
        //     "https://bedrock-runtime.{}.amazonaws.com/model/{}/invoke",
        //     self.region, self.model_id
        // );
        // let body_bytes = serde_json::to_vec(_prompt_body)
        //     .map_err(|e| format!("Bedrock body serialization: {e}"))?;
        //
        // Build SigV4 signature:
        //   let creds = AwsCredentials::from_env()?;
        //   let signed_headers = aws_sigv4::sign(
        //       &body_bytes, "execute-api", "bedrock", &self.region, &creds
        //   )?;
        //
        //   let resp = ureq::post(&endpoint)
        //       .set("Content-Type", "application/json")
        //       .set_many(signed_headers)
        //       .send_bytes(&body_bytes)
        //       .map_err(|e| format!("Bedrock HTTP error: {e}"))?;
        //
        //   let v: serde_json::Value = resp.into_json()
        //       .map_err(|e| format!("Bedrock response parse error: {e}"))?;
        //   Ok(v["content"][0]["text"].as_str().unwrap_or("").to_string())
        //
        // ─────────────────────────────────────────────────────────────────

        Err("BedrockOperator: SigV4 signing not yet implemented (see invoke_model comments)".into())
    }
}

impl Default for BedrockOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmOperator for BedrockOperator {
    fn formalize(&self, nl: &str, sig: &[Sig]) -> Result<Formula, String> {
        let sig_json = serde_json::to_string(sig)
            .map_err(|e| format!("sig serialization error: {e}"))?;

        let system_prompt = format!(
            "You are a formal-logic assistant. \
             Output ONLY a JSON Formula (Atom/Eq/Le/Lt/Not/And/Or/Implies/Iff/\
             Forall/Exists/Obligation/Permission). \
             Term: Var(String)|IntLit(i64)|App(String,Vec<Term>). \
             Sort: Bool|Int|Real|Uninterp(String). Money=cents. \
             Signature: {sig_json}"
        );

        let body = json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": 512,
            "system": system_prompt,
            "messages": [
                { "role": "user", "content": nl }
            ]
        });

        let text = self.invoke_model(&body)?;

        let trimmed = text.trim();
        let stripped = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed);
        let stripped = stripped.strip_suffix("```").unwrap_or(stripped);

        serde_json::from_str::<Formula>(stripped.trim())
            .map_err(|e| format!("Bedrock formula parse error: {e}\n  raw: {stripped}"))
    }

    fn render_english(&self, f: &Formula) -> String {
        render_english(f)
    }
}

// ──────────────────────────── council_formalize ───────────────────────────

/// The result of a council vote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CouncilResult {
    /// The formula that won the majority vote (structural equality, not prose).
    pub agreed: Formula,
    /// How many operators voted for the winning formula.
    pub votes: usize,
    /// Total operators whose proposals were counted.
    pub total: usize,
    /// Distinct formulas that did NOT win, with their vote counts.
    pub dissent: Vec<(Formula, usize)>,
}

/// Run all operators and return the majority-vote formula.
///
/// # Framing-bias resistance
///
/// Each operator sees the same `nl` and `sig`.  The vote is taken over the
/// **normalized structural representation** (`Formula`'s derived `PartialEq`),
/// not over any prose or English rendering.  An operator that rephrases the
/// claim before formalizing still lands on the same IR if its semantics are
/// correct — and that IR is the only thing that counts.
///
/// Operators that return `Err` are silently dropped from the vote (they
/// propose nothing, which is the honest answer to "I can't tell").  If *all*
/// operators fail, this returns `Err`.
pub fn council_formalize(
    ops: &[&dyn LlmOperator],
    nl: &str,
    sig: &[Sig],
) -> Result<CouncilResult, String> {
    // Collect all successful proposals.
    let proposals: Vec<Formula> = ops
        .iter()
        .filter_map(|op| op.formalize(nl, sig).ok())
        .collect();

    if proposals.is_empty() {
        return Err(
            "council_formalize: all operators failed to propose a formula".into(),
        );
    }

    // Tally by structural equality (PartialEq on Formula).
    // Walk the list; for each formula check if it matches an existing bin.
    let mut tally: Vec<(Formula, usize)> = Vec::new();
    for f in proposals {
        if let Some(entry) = tally.iter_mut().find(|(rep, _)| *rep == f) {
            entry.1 += 1;
        } else {
            tally.push((f, 1));
        }
    }

    // Sort descending by vote count — winner is index 0.
    tally.sort_by(|a, b| b.1.cmp(&a.1));

    let (winner, votes) = tally.remove(0);
    let total = votes + tally.iter().map(|(_, c)| c).sum::<usize>();

    Ok(CouncilResult {
        agreed: winner,
        votes,
        total,
        dissent: tally,
    })
}

// ───────────────────────── default_operator ──────────────────────────────

/// Returns an [`LmStudioOperator`] if the local endpoint is reachable,
/// otherwise falls back to [`ScriptedOperator`].
///
/// Intended for use in the demo binary and wherever a single concrete operator
/// is needed without explicit configuration.
pub fn default_operator() -> Box<dyn LlmOperator> {
    let lm = LmStudioOperator::new();
    if lm.is_reachable() {
        Box::new(lm)
    } else {
        Box::new(ScriptedOperator)
    }
}

// ──────────────────────── clean public entry points ──────────────────────

/// The result of formalizing a natural-language claim with a single operator.
///
/// `operator_name` identifies which concrete operator was used so the caller
/// can log and the `formalize` binary can print it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormalizationResult {
    /// Human-readable name of the operator that produced the proposal.
    pub operator_name: String,
    /// The proposed [`Formula`].
    pub formula: Formula,
    /// A plain-English back-render of the formula (advisory; the deterministic
    /// renderer in `mediator-core` is the trusted one for display, but this is
    /// consistent with what the operator produces).
    pub english: String,
    /// If the operator was a fallback (no model reachable), this note says so.
    pub fallback_note: Option<String>,
}

/// Formalize a natural-language claim using a single operator.
///
/// On success, returns a [`FormalizationResult`] with the proposed formula,
/// its English rendering, and which operator was used.
///
/// On failure, returns an `Err` with the operator's error message.
pub fn formalize_nl(
    op: &dyn LlmOperator,
    operator_name: &str,
    nl: &str,
    sig: &[Sig],
) -> Result<FormalizationResult, String> {
    let formula = op.formalize(nl, sig)?;
    let english = op.render_english(&formula);
    Ok(FormalizationResult {
        operator_name: operator_name.to_string(),
        formula,
        english,
        fallback_note: None,
    })
}

/// Formalize a natural-language claim using a council of operators, then
/// return the majority-agreed formula together with the per-operator details.
///
/// This is the framing-bias-resistant path: the vote is over normalized
/// [`Formula`] IR, never over prose. See [`council_formalize`] for the
/// vote mechanics.
///
/// Returns an `Err` only when *all* operators fail.
pub fn council_formalize_nl(
    ops: &[(&dyn LlmOperator, &str)],
    nl: &str,
    sig: &[Sig],
) -> Result<(FormalizationResult, Vec<(Formula, usize)>), String> {
    let raw_ops: Vec<&dyn LlmOperator> = ops.iter().map(|(op, _)| *op).collect();
    let result = council_formalize(&raw_ops, nl, sig)?;

    // Use the first operator that produced the winning formula to render English.
    // If none can be identified, fall back to the module-level renderer.
    let english = {
        let winner_op = ops.iter().find_map(|(op, _)| {
            op.formalize(nl, sig)
                .ok()
                .filter(|f| *f == result.agreed)
                .map(|_| *op)
        });
        winner_op
            .map(|op| op.render_english(&result.agreed))
            .unwrap_or_else(|| render_english(&result.agreed))
    };

    let operator_names: Vec<&str> = ops.iter().map(|(_, name)| *name).collect();
    let agreed_result = FormalizationResult {
        operator_name: format!("council[{}]", operator_names.join(", ")),
        formula: result.agreed,
        english,
        fallback_note: None,
    };

    Ok((agreed_result, result.dissent))
}

// ─────────────────────────────── tests ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::{Formula, Term};

    fn roommate_sig() -> Vec<Sig> {
        vec![
            Sig {
                name: "stain_is_damage".into(),
                arg_sorts: vec![],
                ret: Sort::Bool,
                gloss: "the carpet stain counts as chargeable damage".into(),
            },
            Sig {
                name: "tenant_owes_carpet".into(),
                arg_sorts: vec![],
                ret: Sort::Bool,
                gloss: "Robin must bear the carpet repair cost".into(),
            },
            Sig {
                name: "claimed_total".into(),
                arg_sorts: vec![],
                ret: Sort::Int,
                gloss: "the total deduction Sam verbally claimed, in cents".into(),
            },
        ]
    }

    // ── ScriptedOperator: all three roommate claims ───────────────────────

    #[test]
    fn scripted_r1_robin_wear_tear() {
        let op = ScriptedOperator;
        let nl = "The carpet stain was ordinary wear and tear, not damage I should pay for.";
        let f = op.formalize(nl, &roommate_sig()).expect("r1 should succeed");
        let expected = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        assert_eq!(f, expected);
    }

    #[test]
    fn scripted_s1_sam_damage() {
        let op = ScriptedOperator;
        let nl = "The stain is damage Robin caused, so Robin owes the carpet repair.";
        let f = op.formalize(nl, &roommate_sig()).expect("s1 should succeed");
        let expected = Formula::Atom(Term::App("stain_is_damage".into(), vec![]));
        assert_eq!(f, expected);
    }

    #[test]
    fn scripted_s2_sam_five_hundred() {
        let op = ScriptedOperator;
        let nl = "I told Robin the total deductions would come to about five hundred dollars.";
        let f = op.formalize(nl, &roommate_sig()).expect("s2 should succeed");
        let expected = Formula::Eq(
            Term::App("claimed_total".into(), vec![]),
            Term::IntLit(50000),
        );
        assert_eq!(f, expected);
    }

    #[test]
    fn scripted_unknown_returns_err() {
        let op = ScriptedOperator;
        let result = op.formalize("We should split the utility bills evenly.", &roommate_sig());
        assert!(result.is_err(), "unknown NL should return Err, not fabricate");
    }

    // ── Council: majority over two ScriptedOperators ─────────────────────

    #[test]
    fn council_two_scripted_agree() {
        let a = ScriptedOperator;
        let b = ScriptedOperator;
        let ops: Vec<&dyn LlmOperator> = vec![&a, &b];
        let nl = "The carpet stain was ordinary wear and tear, not damage I should pay for.";
        let result = council_formalize(&ops, nl, &roommate_sig()).expect("council should succeed");

        let expected = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        assert_eq!(result.agreed, expected);
        assert_eq!(result.votes, 2);
        assert_eq!(result.total, 2);
        assert!(result.dissent.is_empty(), "no dissent when both agree");
    }

    #[test]
    fn council_majority_wins() {
        // Two ScriptedOperators agree; one impostor always returns a different formula.
        struct AlwaysAtom;
        impl LlmOperator for AlwaysAtom {
            fn formalize(&self, _nl: &str, _sig: &[Sig]) -> Result<Formula, String> {
                Ok(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
            }
            fn render_english(&self, f: &Formula) -> String {
                render_english(f)
            }
        }

        let a = ScriptedOperator;
        let b = ScriptedOperator;
        let c = AlwaysAtom;
        let ops: Vec<&dyn LlmOperator> = vec![&a, &b, &c];
        let nl = "The carpet stain was ordinary wear and tear, not damage I should pay for.";
        let result = council_formalize(&ops, nl, &roommate_sig()).expect("council should succeed");

        // ScriptedOperator (×2) returns Not(Atom(stain_is_damage)); AlwaysAtom is minority.
        let expected_winner = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        assert_eq!(result.agreed, expected_winner);
        assert_eq!(result.votes, 2);
        assert_eq!(result.total, 3);
        assert_eq!(result.dissent.len(), 1);
        assert_eq!(result.dissent[0].1, 1, "minority got 1 vote");
    }

    #[test]
    fn council_all_fail_returns_err() {
        struct AlwaysFail;
        impl LlmOperator for AlwaysFail {
            fn formalize(&self, _nl: &str, _sig: &[Sig]) -> Result<Formula, String> {
                Err("simulated failure".into())
            }
            fn render_english(&self, f: &Formula) -> String {
                render_english(f)
            }
        }

        let a = AlwaysFail;
        let b = AlwaysFail;
        let ops: Vec<&dyn LlmOperator> = vec![&a, &b];
        let result = council_formalize(&ops, "anything", &[]);
        assert!(result.is_err());
    }

    // ── render_english: readable round-trips ─────────────────────────────

    #[test]
    fn render_atom() {
        let f = Formula::Atom(Term::App("stain_is_damage".into(), vec![]));
        let s = render_english(&f);
        assert!(s.contains("stain_is_damage"), "atom renders the predicate name: {s}");
    }

    #[test]
    fn render_not_atom() {
        let f = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        let s = render_english(&f);
        assert!(s.contains("not"), "negation renders 'not': {s}");
        assert!(s.contains("stain_is_damage"), "inner predicate present: {s}");
    }

    #[test]
    fn render_eq_cents() {
        let f = Formula::Eq(
            Term::App("claimed_total".into(), vec![]),
            Term::IntLit(50000),
        );
        let s = render_english(&f);
        // Should mention claimed_total and the dollar amount.
        assert!(s.contains("claimed_total"), "lhs present: {s}");
        assert!(s.contains("500"), "dollar amount present: {s}");
    }

    #[test]
    fn render_implies() {
        let ant = Formula::Atom(Term::App("stain_is_damage".into(), vec![]));
        let con = Formula::Atom(Term::App("tenant_owes_carpet".into(), vec![]));
        let f = Formula::Implies(Box::new(ant), Box::new(con));
        let s = render_english(&f);
        assert!(s.contains('→'), "implication arrow present: {s}");
    }

    #[test]
    fn render_iff() {
        let a = Formula::Atom(Term::App("p".into(), vec![]));
        let b = Formula::Atom(Term::App("q".into(), vec![]));
        let f = Formula::Iff(Box::new(a), Box::new(b));
        let s = render_english(&f);
        assert!(s.contains('↔'), "biconditional arrow present: {s}");
    }

    #[test]
    fn render_obligation_and_permission() {
        let inner = Formula::Atom(Term::App("pay".into(), vec![]));
        let ob = Formula::Obligation(Box::new(inner.clone()));
        let pe = Formula::Permission(Box::new(inner));
        let ob_s = render_english(&ob);
        let pe_s = render_english(&pe);
        assert!(ob_s.contains("O("), "obligation prefix: {ob_s}");
        assert!(pe_s.contains("P("), "permission prefix: {pe_s}");
    }

    #[test]
    fn render_forall_exists() {
        let body = Formula::Atom(Term::App("pos".into(), vec![Term::Var("x".into())]));
        let fa = Formula::Forall("x".into(), Sort::Int, Box::new(body.clone()));
        let ex = Formula::Exists("x".into(), Sort::Int, Box::new(body));
        let fas = render_english(&fa);
        let exs = render_english(&ex);
        assert!(fas.contains('∀') && fas.contains("Int"), "forall: {fas}");
        assert!(exs.contains('∃') && exs.contains("Int"), "exists: {exs}");
    }

    #[test]
    fn render_and_or_empty() {
        assert_eq!(render_english(&Formula::And(vec![])), "⊤");
        assert_eq!(render_english(&Formula::Or(vec![])), "⊥");
    }

    #[test]
    fn render_le_lt() {
        let a = Term::IntLit(100);
        let b = Term::IntLit(200);
        let le = render_english(&Formula::Le(a.clone(), b.clone()));
        let lt = render_english(&Formula::Lt(a, b));
        assert!(le.contains('≤'), "Le: {le}");
        assert!(lt.contains('<'), "Lt: {lt}");
    }

    // ── formalize_nl / council_formalize_nl ─────────────────────────────

    #[test]
    fn formalize_nl_basic() {
        let op = ScriptedOperator;
        let nl = "The carpet stain was ordinary wear and tear, not damage I should pay for.";
        let result = formalize_nl(&op, "ScriptedOperator", nl, &roommate_sig())
            .expect("should succeed");
        assert_eq!(result.operator_name, "ScriptedOperator");
        let expected = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        assert_eq!(result.formula, expected, "formula should match");
        assert!(result.english.contains("not"), "english should mention negation: {}", result.english);
        assert!(result.fallback_note.is_none());
    }

    #[test]
    fn council_formalize_nl_basic() {
        let a = ScriptedOperator;
        let b = ScriptedOperator;
        let ops: Vec<(&dyn LlmOperator, &str)> = vec![(&a, "ScriptedA"), (&b, "ScriptedB")];
        let nl = "The stain is damage Robin caused, so Robin owes the carpet repair.";
        let (result, dissent) = council_formalize_nl(&ops, nl, &roommate_sig())
            .expect("council should succeed");
        let expected = Formula::Atom(Term::App("stain_is_damage".into(), vec![]));
        assert_eq!(result.formula, expected);
        assert!(result.operator_name.contains("council"), "name: {}", result.operator_name);
        assert!(result.operator_name.contains("ScriptedA"));
        assert!(result.operator_name.contains("ScriptedB"));
        assert!(dissent.is_empty(), "no dissent when all agree");
    }

    #[test]
    fn council_formalize_nl_with_dissent() {
        struct AlwaysAtom;
        impl LlmOperator for AlwaysAtom {
            fn formalize(&self, _nl: &str, _sig: &[Sig]) -> Result<Formula, String> {
                Ok(Formula::Atom(Term::App("stain_is_damage".into(), vec![])))
            }
            fn render_english(&self, f: &Formula) -> String {
                render_english(f)
            }
        }

        let a = ScriptedOperator;
        let b = ScriptedOperator;
        let c = AlwaysAtom;
        let ops: Vec<(&dyn LlmOperator, &str)> = vec![(&a, "ScriptedA"), (&b, "ScriptedB"), (&c, "AlwaysAtom")];
        let nl = "The carpet stain was ordinary wear and tear, not damage I should pay for.";
        let (result, dissent) = council_formalize_nl(&ops, nl, &roommate_sig())
            .expect("council should succeed");

        // ScriptedOperator wins (×2), AlwaysAtom is minority
        let expected_winner = Formula::Not(Box::new(Formula::Atom(Term::App(
            "stain_is_damage".into(),
            vec![],
        ))));
        assert_eq!(result.formula, expected_winner);
        assert_eq!(dissent.len(), 1, "one dissenting formula");
        assert_eq!(dissent[0].1, 1, "minority got 1 vote");
    }
}
