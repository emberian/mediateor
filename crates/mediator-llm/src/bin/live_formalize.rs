//! `live-formalize` — manual / SSH smoke test for the live Bedrock council.
//!
//! Loads the roommate scenario's signature, sends a free-text claim to the
//! council ([`mediator_llm::live::council_formalize_live`]), and prints each
//! model's reading plus the consensus.
//!
//! This binary needs AWS credentials at runtime (the default credential chain:
//! the EC2 instance role on the box, or `~/.aws` locally). That's fine — it is
//! NOT part of the test suite, it's a hands-on probe.
//!
//! # Usage
//!
//! ```sh
//! cargo run -p mediator-llm --bin live-formalize -- "the deposit should be 1200 dollars"
//! ```

use mediator_core::render::formula_to_isabelle;
use mediator_llm::live::{council_formalize_live, LiveConfig};
use mediator_types::Sig;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let claim = match args.get(1) {
        Some(s) => s.as_str(),
        None => {
            eprintln!("Usage: live-formalize <natural-language-claim>");
            eprintln!();
            eprintln!("Example:");
            eprintln!(
                r#"  cargo run -p mediator-llm --bin live-formalize -- "the deposit should be 1200 dollars""#
            );
            std::process::exit(2);
        }
    };

    let sig = match load_roommate_sig() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to load roommate signature: {e}");
            std::process::exit(1);
        }
    };

    let cfg = LiveConfig::default();

    println!();
    println!("LIVE COUNCIL — Bedrock ({})", cfg.region);
    println!("  models: {}", cfg
        .models
        .iter()
        .map(|m| m.label.as_str())
        .collect::<Vec<_>>()
        .join(", "));
    println!("  claim : {claim}");
    println!();

    match council_formalize_live(claim, &sig, &cfg).await {
        Ok(council) => {
            for r in &council.readings {
                println!("── {} ─────────────────────────────────────────", r.model);
                if let Some(f) = &r.formula {
                    let json = serde_json::to_string(f).unwrap_or_default();
                    println!("  formula : {json}");
                    println!("  isabelle: {}", formula_to_isabelle(f));
                    println!("  english : {}", r.english);
                    println!("  valid   : {}", r.valid);
                    if !r.issues.is_empty() {
                        println!("  issues  : {}", r.issues.join("; "));
                    }
                } else {
                    println!("  (no valid formula)");
                    if !r.issues.is_empty() {
                        println!("  issues  : {}", r.issues.join("; "));
                    }
                    if !r.raw.is_empty() {
                        println!("  raw     : {}", truncate(&r.raw, 300));
                    }
                }
                println!();
            }

            println!("══ CONSENSUS ═════════════════════════════════════════");
            println!("  {}", council.consensus);
            if let Some(f) = &council.agreed {
                println!("  agreed  : {}", serde_json::to_string(f).unwrap_or_default());
            }
            println!();
            println!("  (untrusted until gated by Isabelle/HOL)");
            println!();
        }
        Err(e) => {
            eprintln!("council_formalize_live failed: {e:#}");
            eprintln!();
            eprintln!("This binary needs AWS credentials (instance role on the box, or ~/.aws).");
            std::process::exit(1);
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n).collect();
        format!("{head}…")
    }
}

/// Load the roommate scenario's signature (the union of both parties' symbols)
/// from `scenarios/roommate.json`, falling back to a relative path so it works
/// from the workspace root.
fn load_roommate_sig() -> anyhow::Result<Vec<Sig>> {
    let candidates = [
        "scenarios/roommate.json",
        "../../scenarios/roommate.json",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/roommate.json"),
    ];
    let mut last_err = None;
    for path in candidates {
        match std::fs::read_to_string(path) {
            Ok(text) => return sig_from_scenario(&text),
            Err(e) => last_err = Some(e),
        }
    }
    Err(anyhow::anyhow!(
        "could not read scenarios/roommate.json: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "no path tried".into())
    ))
}

fn sig_from_scenario(text: &str) -> anyhow::Result<Vec<Sig>> {
    let dispute: mediator_types::Dispute = serde_json::from_str(text)?;
    let mut sig: Vec<Sig> = Vec::new();
    for party in &dispute.parties {
        for s in &party.signature {
            if !sig.iter().any(|d| d.name == s.name) {
                sig.push(s.clone());
            }
        }
    }
    Ok(sig)
}
