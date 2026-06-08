//! `pact` — certify a FORWARD CONSTITUTION through the real Isabelle gate.
//!
//! Loads a pact (JSON), runs every coverage + consistency obligation through the
//! actual `isabelle` binary, and prints the certificate — CERTIFIED with a
//! signed, re-verifiable record, or REFUSED with the gap named in plain terms.
//!
//!     cargo run -p mediator-pact --bin pact -- scenarios/pacts/roommate_moveout.json
//!
//! With `--thy` it prints the generated `.thy` (the forward constitution as
//! Isabelle source) instead of certifying.

use anyhow::Result;
use mediator_pact::{certify_pact_isabelle, pact_codegen, Pact};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "scenarios/pacts/roommate_moveout.json".to_string());
    let want_thy = args.iter().any(|a| a == "--thy");

    let pact = Pact::load(&path)?;

    if want_thy {
        print!("{}", pact_codegen(&pact));
        return Ok(());
    }

    eprintln!(
        "☄  forward constitution — {}\n   certifying — driving Isabelle/HOL over the declared world, a few seconds…\n",
        pact.title
    );

    let cert = certify_pact_isabelle(&pact);
    print!("{}", cert.render());

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
