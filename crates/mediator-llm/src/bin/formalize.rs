//! `formalize` — the "wow, it understood my sentence" demo binary.
//!
//! Given a natural-language claim on the command line, picks the best available
//! operator (LM Studio → fallback to ScriptedOperator), formalizes the claim,
//! and prints:
//!
//!   1. Which operator was used (and any fallback note).
//!   2. The proposed Formula as pretty-printed JSON.
//!   3. The Isabelle/HOL rendering of the formula.
//!   4. The plain-English back-render.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p mediator-llm --bin formalize -- "the deposit should be 1200 dollars"
//! cargo run -p mediator-llm --bin formalize -- "the carpet stain was ordinary wear"
//! ```
//!
//! Falls back gracefully when no model is reachable:
//!
//! ```sh
//! # (with no LM Studio running)
//! cargo run -p mediator-llm --bin formalize -- "The stain is damage Robin caused."
//! # → Note: using ScriptedOperator (no model reachable at localhost:1234)
//! ```
//!
//! The binary always exits 0 when a formula was produced (possibly via
//! fallback), and exits 1 only when every operator failed.

use mediator_core::render::formula_to_isabelle;
use mediator_llm::{
    formalize_nl, BedrockOperator, FormalizationResult, LmStudioOperator, ScriptedOperator,
};
use mediator_types::{Sig, Sort};
use serde_json;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let nl = match args.get(1) {
        Some(s) => s.as_str(),
        None => {
            eprintln!("Usage: formalize <natural-language-claim>");
            eprintln!();
            eprintln!("Example:");
            eprintln!(
                r#"  cargo run -p mediator-llm --bin formalize -- "the deposit should be 1200 dollars""#
            );
            std::process::exit(2);
        }
    };

    // Build the roommate signature (the canonical context for the demo).
    // A production version would load this from a Dispute file; for the demo
    // this is the signature that the roommate scenario uses.
    let sig = roommate_sig();

    // Try operators in preference order, collecting the first success.
    let result = try_operators(nl, &sig);

    match result {
        Ok(res) => print_result(&res, nl),
        Err(errors) => {
            eprintln!("All operators failed to formalize the claim.");
            eprintln!();
            for (name, err) in &errors {
                eprintln!("  [{name}] {err}");
            }
            std::process::exit(1);
        }
    }
}

/// Try operators in order: LM Studio → Bedrock → Scripted.
///
/// Returns the first success, or all errors if every operator failed.
fn try_operators(nl: &str, sig: &[Sig]) -> Result<FormalizationResult, Vec<(String, String)>> {
    let mut errors: Vec<(String, String)> = Vec::new();

    // ── LM Studio ──────────────────────────────────────────────────────────
    // We check that a model is actually loaded (not just reachable) before
    // attempting inference, to avoid hanging on an idle server.
    // Inference itself gets a generous timeout since model generation may be slow.
    let lm_probe = LmStudioOperator {
        base_url: "http://localhost:1234".into(),
        timeout_ms: 2_000,
    };
    let lm = LmStudioOperator {
        base_url: "http://localhost:1234".into(),
        timeout_ms: 30_000,
    };
    if lm_has_model(&lm_probe) {
        match formalize_nl(&lm, "LmStudioOperator", nl, sig) {
            Ok(mut r) => {
                // No fallback note — a live model was used.
                r.fallback_note = None;
                return Ok(r);
            }
            Err(e) => errors.push(("LmStudioOperator".into(), e)),
        }
    } else {
        errors.push((
            "LmStudioOperator".into(),
            "endpoint not reachable or no model loaded (http://localhost:1234)".into(),
        ));
    }

    // ── Bedrock ────────────────────────────────────────────────────────────
    if BedrockOperator::has_credentials() {
        let bedrock = BedrockOperator::new();
        match formalize_nl(&bedrock, "BedrockOperator", nl, sig) {
            Ok(mut r) => {
                r.fallback_note = None;
                return Ok(r);
            }
            Err(e) => errors.push(("BedrockOperator".into(), e)),
        }
    } else {
        errors.push((
            "BedrockOperator".into(),
            "no AWS credentials found (AWS_ACCESS_KEY_ID / AWS_PROFILE not set)".into(),
        ));
    }

    // ── ScriptedOperator (offline fallback — always runs) ──────────────────
    let scripted = ScriptedOperator;
    match formalize_nl(&scripted, "ScriptedOperator", nl, sig) {
        Ok(mut r) => {
            r.fallback_note = Some(
                "no live model was reachable; using pre-baked offline formalizations \
                 (ScriptedOperator). For live formalization, start LM Studio at \
                 localhost:1234 or set AWS credentials."
                    .into(),
            );
            return Ok(r);
        }
        Err(e) => errors.push(("ScriptedOperator".into(), e)),
    }

    Err(errors)
}

/// Pretty-print the formalization result.
fn print_result(res: &FormalizationResult, nl: &str) {
    // Header separator
    println!();
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║              mediateor ☄️  — formalize                ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    // Input
    println!("INPUT CLAIM");
    println!("───────────");
    println!("  {nl}");
    println!();

    // Operator
    println!("OPERATOR");
    println!("────────");
    println!("  {}", res.operator_name);
    if let Some(note) = &res.fallback_note {
        println!();
        println!("  Note: {note}");
    }
    println!();

    // Formula (JSON)
    println!("PROPOSED FORMULA (JSON IR)");
    println!("──────────────────────────");
    let json = serde_json::to_string_pretty(&res.formula)
        .unwrap_or_else(|e| format!("<serialization error: {e}>"));
    for line in json.lines() {
        println!("  {line}");
    }
    println!();

    // Isabelle/HOL
    println!("ISABELLE/HOL RENDERING");
    println!("──────────────────────");
    let isa = formula_to_isabelle(&res.formula);
    println!("  {isa}");
    println!();

    // Plain English
    println!("PLAIN ENGLISH BACK-RENDER");
    println!("─────────────────────────");
    println!("  {}", res.english);
    println!();

    // Reminder about trust boundary
    println!("─────────────────────────────────────────────────────────");
    println!("  This proposal is UNTRUSTED until gated by Isabelle/HOL.");
    println!("  Run the full pipeline to certify: cargo run -p mediator-demo");
    println!();
}

/// Check that the LM Studio endpoint is up *and* has at least one model loaded.
///
/// LM Studio will answer `/v1/models` even when nothing is loaded (returning an
/// empty `data` list), but inference calls will then hang. We treat "no models"
/// the same as "not reachable" for the formalize binary.
fn lm_has_model(lm: &LmStudioOperator) -> bool {
    let url = format!("{}/v1/models", lm.base_url);
    match ureq::get(&url)
        .timeout(std::time::Duration::from_millis(lm.timeout_ms.min(2_000)))
        .call()
    {
        Err(_) => false,
        Ok(resp) => {
            // Parse the model list; non-empty `data` array means a model is loaded.
            resp.into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| v["data"].as_array().map(|arr| !arr.is_empty()))
                .unwrap_or(false)
        }
    }
}

/// The canonical roommate-dispute signature, matching `scenarios/roommate.json`.
///
/// A real CLI would load this from a `--scenario` file; for the offline demo
/// this is the known context for the three scripted claims.
fn roommate_sig() -> Vec<Sig> {
    vec![
        Sig {
            name: "stain_is_damage".into(),
            arg_sorts: vec![],
            ret: Sort::Bool,
            gloss: "the carpet stain counts as chargeable damage (vs. ordinary wear)".into(),
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
