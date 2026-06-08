//! `pact` — certify a FORWARD CONSTITUTION through the real Isabelle gate.
//!
//! Loads a pact (JSON), runs every coverage + consistency obligation through the
//! actual `isabelle` binary, and prints the certificate — one of the trichotomy
//! outcomes:
//!
//!   * CERTIFIED, with a signed, re-verifiable record;
//!   * INCONSISTENT, with the concrete witness world two clauses clash in;
//!   * REFUSED, with the coverage gap named in plain terms;
//!
//! or, before any of that, REJECTED AT AUTHORING if the unknown-gate firewall
//! ([`Pact::validate`]) catches a clause guard the prover could decide on its
//! own (a value-call in disguise).
//!
//!     cargo run -p mediator-pact --bin pact -- scenarios/pacts/roommate_moveout.json
//!
//! Flags:
//!   * `--thy` — print the generated `.thy` (the forward constitution as
//!     Isabelle source) instead of certifying;
//!   * `--validate` — run ONLY the authoring-time firewall (offline, no
//!     Isabelle) and report; exit nonzero if rejected;
//!   * `--cert` — certify, then print the full `PactCertificate` as JSON (the
//!     cached-certificate shape) to stdout;
//!   * `--write-cert` — certify, then write `<pact>.cert.json` beside the pact
//!     so a box with NO Isabelle can render the certified result (mirrors
//!     `mediator-demo … --write-cache`).

use anyhow::Result;
use mediator_pact::{certify_pact_for_cache, certify_pact_isabelle, pact_codegen, Pact};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "scenarios/pacts/roommate_moveout.json".to_string());
    let want_thy = args.iter().any(|a| a == "--thy");
    let want_validate_only = args.iter().any(|a| a == "--validate");
    let want_cert_json = args.iter().any(|a| a == "--cert");
    let want_write_cert = args.iter().any(|a| a == "--write-cert");

    let pact = Pact::load(&path)?;

    if want_thy {
        print!("{}", pact_codegen(&pact));
        return Ok(());
    }

    // The authoring-time firewall, runnable on its own (no Isabelle needed).
    if want_validate_only {
        let errors = pact.validate();
        if errors.is_empty() {
            println!(
                "✓ {} — passes the unknown-gate firewall: every clause guard branches on a \
                 declared free crux predicate.",
                pact.title
            );
            return Ok(());
        }
        eprintln!("✗ {} — REJECTED at authoring (the firewall fired):", pact.title);
        for e in &errors {
            eprintln!("    • {}", e.plain());
        }
        std::process::exit(1);
    }

    eprintln!(
        "☄  forward constitution — {}\n   certifying — driving Isabelle/HOL over the declared world, a few seconds…\n",
        pact.title
    );

    // For cache/JSON output, sign with the reproducible public-cache key so the
    // on-disk artifact is byte-stable across regenerations; for the interactive
    // human rendering, a fresh per-run key is fine.
    let for_cache = want_cert_json || want_write_cert;
    let cert = if for_cache {
        certify_pact_for_cache(&pact)
    } else {
        certify_pact_isabelle(&pact)
    };

    // Machine-readable cached-certificate JSON to stdout (for piping / inspection).
    if want_cert_json && !want_write_cert {
        println!("{}", serde_json::to_string_pretty(&cert)?);
    } else {
        print!("{}", cert.render());
    }

    // Write the sibling `<pact>.cert.json` cache so a no-Isabelle box can render
    // the certified result without re-proving.
    if want_write_cert {
        let cert_path = cert_cache_path(&path);
        std::fs::write(&cert_path, serde_json::to_string_pretty(&cert)?)?;
        eprintln!("✓ wrote cached certificate → {}", cert_path.display());
    }

    // Re-verify the signed record right here, so the printed certificate is one a
    // skeptic could re-check — and exit nonzero if the pact did not certify.
    match mediator_audit::verify(&cert.record) {
        Ok(()) => eprintln!(
            "\n✓ signed record re-verified ({} entries)",
            cert.record.entries.len()
        ),
        Err(e) => eprintln!("\n✗ signed record failed to verify: {e}"),
    }

    if !cert.certified() {
        std::process::exit(1);
    }
    Ok(())
}

/// `scenarios/pacts/roommate_moveout.json` →
/// `scenarios/pacts/roommate_moveout.cert.json`. Mirrors the `.analysis.json`
/// sibling-cache discipline in `mediator-web::load`.
fn cert_cache_path(pact_path: &str) -> std::path::PathBuf {
    let p = Path::new(pact_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("pact");
    p.with_file_name(format!("{stem}.cert.json"))
}
