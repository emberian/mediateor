//! `mediator` — the end-to-end demo binary.
//!
//! Loads a dispute scenario, drives the real Isabelle/HOL gate through
//! `mediator-core::analyze`, and renders the result. With `--tui` it launches
//! the interactive terminal app; otherwise it prints the party views, the
//! operator cockpit, and the receipt chain.
//!
//!     cargo run -p mediator-demo -- scenarios/roommate.json
//!     cargo run -p mediator-demo -- scenarios/roommate.json --tui

use anyhow::Result;
use mediator_core::receipts::verify_chain;
use mediator_core::{analyze, load_dispute};
use mediator_fairdiv::AdjustedWinner;
use mediator_prover::IsabelleProver;
use mediator_tui::{operator_view, party_view, run_with_receipts};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let scenario = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "scenarios/roommate.json".to_string());
    let want_tui = args.iter().any(|a| a == "--tui");
    let want_cache = args.iter().any(|a| a == "--write-cache");

    let dispute = load_dispute(&scenario)?;
    eprintln!(
        "☄  {}\n   analyzing — driving Isabelle/HOL, a few seconds…\n",
        dispute.title
    );

    let prover = IsabelleProver::locate();
    let fair = AdjustedWinner;
    let (analysis, receipts) = analyze(&dispute, &prover, &fair);

    // Persist a precomputed analysis next to the scenario so the web app loads
    // instantly (and works with no prover present). `<name>.json` →
    // `<name>.analysis.json`, carrying the certified Analysis + receipt chain.
    if want_cache {
        let cache_path = scenario
            .strip_suffix(".json")
            .map(|s| format!("{s}.analysis.json"))
            .unwrap_or_else(|| format!("{scenario}.analysis.json"));
        let payload = serde_json::json!({ "analysis": analysis, "receipts": receipts });
        std::fs::write(&cache_path, serde_json::to_string_pretty(&payload)?)?;
        eprintln!("✓ wrote analysis cache → {cache_path}");
    }

    if want_tui {
        run_with_receipts(analysis, dispute, receipts, verify_chain)?;
        return Ok(());
    }

    for p in &dispute.parties {
        println!("{}\n", party_view(&analysis, &p.id));
    }
    println!("{}", operator_view(&analysis));

    println!("\n── receipt ledger ({} entries) ──", receipts.len());
    for r in &receipts {
        let v = r
            .verdict
            .as_ref()
            .map(|v| format!("{v:?}"))
            .unwrap_or_default();
        let h = if r.hash.len() >= 12 { &r.hash[..12] } else { &r.hash };
        println!("  #{:<2} {:<16} {h}…  {v}", r.seq, r.op);
    }
    match verify_chain(&receipts) {
        Ok(()) => println!("  ✓ hash chain verified ({} links)", receipts.len()),
        Err(i) => println!("  ✗ hash chain broken at entry {i}"),
    }

    Ok(())
}
