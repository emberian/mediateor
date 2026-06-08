//! `neutrality` — the mediator-neutrality guardrail.
//!
//! **The problem it solves.**  A prompted model can be told "don't take sides",
//! but prompting is not a *check*.  This module provides a second, independent
//! judge (Nova Lite) that reads the mediator's own utterance and asks two
//! specific questions:
//!
//! 1. Does this utterance *decide* the crux — pick a side, declare one party
//!    right?
//! 2. Does it *assert* a fact that is not entailed by the certified facts (i.e.,
//!    does it invent something)?
//!
//! If either answer is "yes", the utterance fails the neutrality check and the
//! caller can suppress it, request a revision, or log it for review.
//!
//! # Framing-bias resistance
//!
//! The crux is passed to the judge as its **normalized form** (the canonical IR
//! string from [`crate::live::normalize_formula`] when a `Formula` is available,
//! or the plain crux text otherwise).  Prose framing of the crux cannot bias the
//! judge because the judge sees the canonical representation, not the original
//! wording.  This mirrors the council's own vote mechanism: evaluate the
//! normalized form, not the prose.
//!
//! # Offline safety
//!
//! [`check_neutrality`] calls Bedrock (Nova Lite) and is `async`.  All pure
//! helpers — [`build_neutrality_prompt`], [`parse_neutrality_verdict`] — are
//! synchronous, network-free, and fully tested offline.
//!
//! # The `deliberate` helper
//!
//! [`deliberate`] is a thin wrapper around [`council_formalize_live`] that
//! returns a structured [`DeliberationSummary`]: the agreed formula (if any),
//! whether the council reached consensus, and a plain-English summary of any
//! dissent.  It exists so callers that want "run the council, give me a clean
//! summary" don't have to look inside `CouncilReading`.

use crate::live::{council_formalize_live, normalize_formula, CouncilReading, LiveConfig};
use anyhow::Context;
use mediator_types::{Formula, Sig};
use serde::{Deserialize, Serialize};

// ─────────────────────────────── types ──────────────────────────────────

/// The result of a neutrality check on a single mediator utterance.
///
/// `neutral == false` means the utterance failed: it either decided the crux
/// or invented a fact not in the certified set.  `issues` lists what was
/// wrong — it is non-empty when `neutral == false` and (by construction)
/// empty when `neutral == true`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NeutralityVerdict {
    /// `true` iff the utterance is neutral (decides nothing, invents nothing).
    pub neutral: bool,
    /// The judge's specific objections.  Empty when `neutral == true`.
    pub issues: Vec<String>,
}

/// A structured summary of what the formalization council decided.
///
/// Returned by [`deliberate`]; its primary consumer is the mediation session
/// that needs to know whether the council reached a firm proposal before
/// forwarding it to the trusted gate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeliberationSummary {
    /// The claim that was deliberated.
    pub claim: String,
    /// The council's agreed formula, if a majority was reached.
    pub agreed_formula: Option<Formula>,
    /// The normalized canonical form of `agreed_formula`, for bias-resistant
    /// downstream checks.
    pub agreed_normalized: Option<String>,
    /// Plain English rendering of each model's reading (in council order).
    pub readings_english: Vec<String>,
    /// A human one-liner summarizing the vote.
    pub consensus: String,
    /// `true` if every model with a valid formula agreed.
    pub unanimous: bool,
    /// Human-readable list of dissenting readings (label + their formula's
    /// normalized form), if any.
    pub dissent_notes: Vec<String>,
}

// ──────────────────────────── prompt helpers ─────────────────────────────

/// Model id for the neutrality judge (Nova Lite — cheap and fast).
pub const NEUTRALITY_MODEL: &str = "amazon.nova-lite-v1:0";

/// Token cap for the verdict call: we need only a small JSON object.
pub const NEUTRALITY_MAX_TOKENS: i32 = 256;

/// Build the system prompt for the neutrality judge.
///
/// The framing-bias-resistance rationale: the judge is asked to evaluate the
/// *normalized* crux representation (canonical IR, not prose).  This means a
/// mediator utterance that subtly rephrases the crux cannot game the check —
/// the judge reasons about the formal predicate, not the surface language.
pub fn build_neutrality_system_prompt() -> String {
    "You are a neutrality auditor for an AI mediator.  Your only job is to \
decide whether a mediator utterance (a) DECIDES the disputed question (the \
crux) by favouring one side, or (b) ASSERTS a factual claim that is not \
entailed by the provided certified facts.\n\
\n\
You must output ONLY a JSON object with exactly these two fields:\n\
  { \"neutral\": <bool>, \"issues\": [<string>, ...] }\n\
\n\
Rules:\n\
- `neutral` is true iff the utterance does NOT decide the crux AND does NOT \
invent a fact absent from the certified list.\n\
- `issues` is an array of short English sentences describing each problem \
found.  It MUST be empty when `neutral` is true.\n\
- The crux is provided in its NORMALIZED formal form — evaluate it as a \
logical predicate, not as prose.\n\
- Do not output anything besides the JSON object."
        .to_string()
}

/// Build the user prompt for the neutrality judge.
///
/// `crux_normalized` should be the output of [`normalize_formula`] when a
/// `Formula` is available; otherwise the plain crux text (e.g. from
/// [`mediator_types::Analysis::crux`]).  Either way the judge sees a stable,
/// prose-independent representation.
pub fn build_neutrality_user_prompt(
    utterance: &str,
    crux_normalized: &str,
    certified_facts: &[String],
) -> String {
    let mut s = String::new();
    s.push_str("CRUX (normalized predicate — the ONE question that must stay undecided):\n  ");
    s.push_str(crux_normalized);
    s.push_str("\n\nCERTIFIED FACTS (the only factual claims the mediator may assert):\n");
    if certified_facts.is_empty() {
        s.push_str("  (none certified yet)\n");
    } else {
        for (i, fact) in certified_facts.iter().enumerate() {
            s.push_str(&format!("  {}. {}\n", i + 1, fact));
        }
    }
    s.push_str("\nMEDIATOR UTTERANCE TO AUDIT:\n  ");
    s.push_str(utterance);
    s.push_str("\n\nRespond with the JSON verdict now.");
    s
}

// ─────────────────────────── verdict parser ───────────────────────────────

/// Parse the neutrality verdict from a model's raw text.
///
/// Tolerates:
/// - bare JSON: `{"neutral":false,"issues":["..."]}
/// - fenced JSON blocks (` ```json ... ``` ` or ` ``` ... ``` `)
/// - JSON buried in prose (first balanced `{...}` object)
///
/// This is a pure, synchronous function — fully testable offline.
pub fn parse_neutrality_verdict(raw: &str) -> Result<NeutralityVerdict, String> {
    // The shape we expect from the model.
    #[derive(Deserialize)]
    struct Raw {
        neutral: bool,
        issues: Vec<String>,
    }

    let trimmed = raw.trim();

    // 1. Whole text.
    if let Ok(r) = serde_json::from_str::<Raw>(trimmed) {
        return Ok(NeutralityVerdict {
            neutral: r.neutral,
            issues: r.issues,
        });
    }

    // 2. Fenced block.
    if let Some(inner) = fenced_block(trimmed) {
        let inner = inner.trim();
        if let Ok(r) = serde_json::from_str::<Raw>(inner) {
            return Ok(NeutralityVerdict {
                neutral: r.neutral,
                issues: r.issues,
            });
        }
    }

    // 3. First balanced JSON object.
    if let Some(obj) = first_json_object(trimmed) {
        if let Ok(r) = serde_json::from_str::<Raw>(obj) {
            return Ok(NeutralityVerdict {
                neutral: r.neutral,
                issues: r.issues,
            });
        }
    }

    Err(format!(
        "neutrality: could not parse verdict from model output: {trimmed:?}"
    ))
}

// Copied from live.rs (private) so this module is self-contained.

fn fenced_block(s: &str) -> Option<&str> {
    let start = s.find("```")?;
    let after = &s[start + 3..];
    let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
    let body = &after[body_start..];
    let end = body.find("```")?;
    Some(&body[..end])
}

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

// ──────────────────────────── live check ────────────────────────────────

/// Check whether a mediator utterance is neutral with respect to a crux and
/// a set of certified facts.
///
/// # What it checks
///
/// - **Crux non-decision**: does the utterance pick a side, declare one party
///   right, or imply the crux is resolved?
/// - **Fact non-invention**: does the utterance assert something not covered by
///   `certified_facts`?
///
/// # Framing-bias resistance
///
/// `crux` is normalized via [`normalize_formula`] if a `Formula` can be parsed
/// from it; otherwise the plain string is used.  The judge sees the canonical
/// representation, so prose framing of the crux cannot influence its decision.
///
/// # Offline safety
///
/// This function calls Bedrock.  Tests must not call it; see the `#[cfg(test)]`
/// module for offline tests of the pure helpers.
pub async fn check_neutrality(
    utterance: &str,
    crux: &str,
    certified_facts: &[String],
    cfg: &LiveConfig,
) -> anyhow::Result<NeutralityVerdict> {
    // Normalize the crux to a prose-independent representation.  Try to
    // parse it as a Formula first; fall back to the plain string.
    let crux_normalized: String = {
        // Attempt to interpret crux as a serialized Formula JSON.
        match serde_json::from_str::<Formula>(crux) {
            Ok(f) => normalize_formula(&f),
            Err(_) => crux.to_string(),
        }
    };

    let system = build_neutrality_system_prompt();
    let user = build_neutrality_user_prompt(utterance, &crux_normalized, certified_facts);

    let raw = crate::live::converse_text(
        &cfg.region,
        NEUTRALITY_MODEL,
        &system,
        &user,
        NEUTRALITY_MAX_TOKENS,
        0.0,
    )
    .await
    .context("neutrality: Bedrock call failed")?;

    parse_neutrality_verdict(&raw)
        .map_err(|e| anyhow::anyhow!(e))
}

// ───────────────────────────── deliberate ────────────────────────────────

/// Run the formalization council on `claim` and return a structured
/// [`DeliberationSummary`].
///
/// This is a convenience wrapper around [`council_formalize_live`]: it calls
/// the full Bedrock council, tallies the vote, and packages the result into a
/// clean summary for the mediation session.
///
/// # Fields of interest
///
/// - `agreed_formula` / `agreed_normalized`: the proposal to send to the gate;
///   `None` when the council split without a majority.
/// - `unanimous`: `true` when every model with a valid formula agreed.
/// - `dissent_notes`: human-readable lines for each dissenting reading.
///
/// # Offline safety
///
/// Calls Bedrock. Tests must not call it directly.
pub async fn deliberate(
    claim: &str,
    sig: &[Sig],
    cfg: &LiveConfig,
) -> anyhow::Result<DeliberationSummary> {
    let council: CouncilReading = council_formalize_live(claim, sig, cfg)
        .await
        .context("deliberate: council call failed")?;

    let agreed_normalized = council
        .agreed
        .as_ref()
        .map(normalize_formula);

    // Collect dissent notes: readings that are valid but disagree with the winner.
    let winner_norm = agreed_normalized.as_deref();
    let dissent_notes: Vec<String> = council
        .readings
        .iter()
        .filter(|r| r.valid)
        .filter(|r| {
            r.formula.as_ref().map(normalize_formula).as_deref() != winner_norm
        })
        .map(|r| {
            let norm = r
                .formula
                .as_ref()
                .map(normalize_formula)
                .unwrap_or_else(|| "(no formula)".to_string());
            format!("{}: {}", r.model, norm)
        })
        .collect();

    // Unanimity: every reading is valid and agreed on the same form.
    let total = council.readings.len();
    let valid_count = council.readings.iter().filter(|r| r.valid).count();
    let unanimous = dissent_notes.is_empty()
        && valid_count == total
        && valid_count > 0;

    let readings_english: Vec<String> = council
        .readings
        .iter()
        .map(|r| r.english.clone())
        .collect();

    Ok(DeliberationSummary {
        claim: council.claim,
        agreed_formula: council.agreed,
        agreed_normalized,
        readings_english,
        consensus: council.consensus,
        unanimous,
        dissent_notes,
    })
}

// ─────────────────────────────── tests ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_neutrality_verdict ─────────────────────────────────────────

    /// A clean `{neutral:false, issues:[...]}` object should parse correctly.
    #[test]
    fn verdict_parse_non_neutral() {
        let raw = r#"{"neutral":false,"issues":["Utterance declares the stain is damage.","Asserts unlisted fact about repair cost."]}"#;
        let v = parse_neutrality_verdict(raw).expect("should parse");
        assert!(!v.neutral);
        assert_eq!(v.issues.len(), 2);
        assert!(v.issues[0].contains("damage"));
    }

    /// A clean `{neutral:true, issues:[]}` should parse correctly.
    #[test]
    fn verdict_parse_neutral_clean() {
        let raw = r#"{"neutral":true,"issues":[]}"#;
        let v = parse_neutrality_verdict(raw).expect("should parse");
        assert!(v.neutral);
        assert!(v.issues.is_empty());
    }

    /// The model may emit a fenced code block.
    #[test]
    fn verdict_parse_fenced_block() {
        let raw = "Here is my verdict:\n\
            ```json\n\
            {\"neutral\":false,\"issues\":[\"Picks a side by saying Robin is wrong.\"]}\n\
            ```\n\
            That is the assessment.";
        let v = parse_neutrality_verdict(raw).expect("fenced block should parse");
        assert!(!v.neutral);
        assert_eq!(v.issues.len(), 1);
        assert!(v.issues[0].contains("Robin"));
    }

    /// JSON buried in prose should still parse via the balanced-brace extractor.
    #[test]
    fn verdict_parse_buried_in_prose() {
        let raw = r#"The verdict is {"neutral":true,"issues":[]} as I see it."#;
        let v = parse_neutrality_verdict(raw).expect("buried json should parse");
        assert!(v.neutral);
    }

    /// Non-JSON gibberish should return an Err.
    #[test]
    fn verdict_parse_rejects_nonjson() {
        let raw = "I cannot determine the neutrality of the statement.";
        assert!(parse_neutrality_verdict(raw).is_err());
    }

    /// Extra fields should not break parsing (forward compat).
    #[test]
    fn verdict_parse_extra_fields_ok() {
        let raw = r#"{"neutral":true,"issues":[],"confidence":0.95}"#;
        let v = parse_neutrality_verdict(raw).expect("extra fields ok");
        assert!(v.neutral);
        assert!(v.issues.is_empty());
    }

    // ── build_neutrality_user_prompt ─────────────────────────────────────

    /// The user prompt must contain the crux and all certified facts.
    #[test]
    fn prompt_contains_crux_and_facts() {
        let crux = "Atom(App(stain_is_damage,[]))";
        let facts = vec![
            "The deposit was $1,050.".to_string(),
            "Itemized deductions total $450.".to_string(),
        ];
        let utterance = "Both parties have valid perspectives on this situation.";
        let p = build_neutrality_user_prompt(utterance, crux, &facts);
        assert!(p.contains(crux), "prompt must contain crux: {p}");
        assert!(
            p.contains("deposit was $1,050"),
            "prompt must contain first fact: {p}"
        );
        assert!(
            p.contains("Itemized deductions"),
            "prompt must contain second fact: {p}"
        );
        assert!(
            p.contains(utterance),
            "prompt must contain the utterance: {p}"
        );
    }

    /// Empty certified facts should produce a graceful note, not a crash.
    #[test]
    fn prompt_empty_facts() {
        let p = build_neutrality_user_prompt("hello", "Atom(App(p,[]))", &[]);
        assert!(p.contains("none certified"), "should mention empty: {p}");
    }

    // ── system prompt sanity ─────────────────────────────────────────────

    #[test]
    fn system_prompt_mentions_normalized_form() {
        let s = build_neutrality_system_prompt();
        assert!(
            s.contains("NORMALIZED"),
            "system prompt should mention normalized: {s}"
        );
        assert!(
            s.contains("neutral"),
            "system prompt should mention neutral: {s}"
        );
        assert!(
            s.contains("issues"),
            "system prompt should mention issues: {s}"
        );
    }

    // ── NeutralityVerdict serde round-trip ───────────────────────────────

    #[test]
    fn verdict_roundtrip_serde() {
        let v = NeutralityVerdict {
            neutral: false,
            issues: vec!["Decides the crux.".to_string()],
        };
        let json = serde_json::to_string(&v).expect("serialize");
        let v2: NeutralityVerdict = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v, v2);
    }
}
