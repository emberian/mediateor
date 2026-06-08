//! Real-Isabelle integration test for FORWARD CONSTITUTIONS.
//!
//! Generates the preamble + obligations for the seed pacts and runs the ACTUAL
//! `isabelle` binary on each obligation (preamble + theorem + `end`, in an
//! isolated session), asserting the certificate the reduction promises:
//!
//!   * `scenarios/pacts/roommate_moveout.json` (COMPLETE, consistent) →
//!     coverage Proved AND every consistency Proved → CERTIFIED, signed.
//!   * `scenarios/pacts/roommate_moveout_broken.json` (short clause dropped) →
//!     coverage NOT Proved → REFUSED, with the uncovered world named.
//!
//! Gated EXACTLY the way `mediator-core`'s real-Isabelle test is gated: it
//! checks the `isabelle` binary exists and returns early (skips) if not, builds
//! the prebuilt system HOL image, and gives every obligation a unique theory +
//! session name so concurrent runs never collide. It hits real Isabelle the same
//! way (no dependency on a mock) — but here we additionally route through the
//! crate's own `certify_pact` + `IsabelleProver` so the WHOLE pipeline (codegen →
//! gate → signed record) is exercised end to end.

use mediator_pact::{certify_pact, Pact, Status};
use mediator_prover::IsabelleProver;
use mediator_types::Verdict;
use std::path::PathBuf;
use std::time::Duration;

/// Same path the rest of the workspace pins; the spec's binary location.
const ISABELLE: &str = "/Users/ember/isabelle/Isabelle2025-2.app/bin/isabelle";

fn load(rel: &str) -> Pact {
    let path = format!("{}/../../{rel}", env!("CARGO_MANIFEST_DIR"));
    Pact::load(&path).unwrap_or_else(|e| panic!("load {rel}: {e}"))
}

/// The complete pact certifies through the REAL gate, and the signed record
/// verifies.
#[test]
fn complete_pact_certifies_through_real_isabelle() {
    if !PathBuf::from(ISABELLE).exists() {
        eprintln!("skipping: isabelle not found at {ISABELLE}");
        return;
    }

    let pact = load("scenarios/pacts/roommate_moveout.json");

    // Real gate. Generous-but-bounded per-obligation budget; the HOL image is
    // prebuilt so each `auto` closes in a couple seconds.
    let prover = IsabelleProver::new(ISABELLE).with_timeout(Duration::from_secs(180));
    let cert = certify_pact(&pact, &prover);

    for o in &cert.obligations {
        eprintln!("obligation {:<18} -> {:?}", o.name, o.verdict);
        assert!(
            !matches!(o.verdict, Verdict::Error(_)),
            "obligation {} produced invalid Isabelle / errored: {:?}",
            o.name,
            o.verdict
        );
    }

    // Coverage Proved AND every consistency Proved ⇒ CERTIFIED.
    assert!(
        matches!(cert.status, Status::Certified),
        "expected CERTIFIED, got {:?}",
        cert.status
    );
    assert!(cert
        .obligations
        .iter()
        .all(|o| matches!(o.verdict, Verdict::Proved)));

    // The signed record is anyone-re-verifiable.
    assert!(
        mediator_audit::verify(&cert.record).is_ok(),
        "signed certificate record must verify"
    );
    // One receipt per obligation (coverage + 3 pairs) + the certification event.
    assert_eq!(cert.record.entries.len(), 4 + 1);
    assert_eq!(cert.record.entries.last().unwrap().kind, "pact_certified");

    eprintln!("\n{}", cert.render());
}

/// The broken pact (short-notice clause dropped) is REFUSED through the REAL
/// gate: coverage does not close (a declared world fires no clause), and the gap
/// is named. Crucially the consistency obligations that DO remain still close —
/// so the refusal is specifically a coverage gap, not a malformed pact.
#[test]
fn broken_pact_is_refused_through_real_isabelle() {
    if !PathBuf::from(ISABELLE).exists() {
        eprintln!("skipping: isabelle not found at {ISABELLE}");
        return;
    }

    let pact = load("scenarios/pacts/roommate_moveout_broken.json");

    let prover = IsabelleProver::new(ISABELLE).with_timeout(Duration::from_secs(180));
    let cert = certify_pact(&pact, &prover);

    for o in &cert.obligations {
        eprintln!("obligation {:<18} -> {:?}", o.name, o.verdict);
        // No obligation may be an Error: the theory must be valid Isabelle. A
        // coverage gap is an honest Unknown (open subgoal), never a syntax error.
        assert!(
            !matches!(o.verdict, Verdict::Error(_)),
            "obligation {} produced invalid Isabelle / errored: {:?}",
            o.name,
            o.verdict
        );
    }

    // Coverage must NOT close: the disjunction of the two remaining guards is not
    // valid (a short-notice world fires no clause). The gate reports this as the
    // honest Unknown — the open subgoal IS the diagnosis.
    let coverage = cert
        .obligations
        .iter()
        .find(|o| o.name == "coverage")
        .expect("a coverage obligation");
    assert_eq!(
        coverage.verdict,
        Verdict::Unknown,
        "coverage of the broken pact must be an open subgoal (Unknown)"
    );

    // The remaining consistency pair (clean vs damaged) still closes — proving
    // the refusal is a coverage GAP, not a malformed pact.
    if let Some(c01) = cert.obligations.iter().find(|o| o.name == "consistent_0_1") {
        assert_eq!(
            c01.verdict,
            Verdict::Proved,
            "the kept clause pair is still consistent"
        );
    }

    // Overall REFUSED, with the uncovered world named in plain terms.
    match &cert.status {
        Status::Refused { gap } => {
            assert_eq!(gap.obligation, "coverage");
            assert_eq!(gap.verdict, Verdict::Unknown);
            assert!(
                gap.plain.contains("does NOT cover every declared world"),
                "gap should name the uncovered world: {}",
                gap.plain
            );
            assert!(
                gap.plain.contains("notice_days"),
                "gap should point at the integer dial: {}",
                gap.plain
            );
        }
        Status::Certified => panic!("broken pact must NOT certify"),
    }

    // The refusal is recorded in the signed, verifiable chain too.
    assert!(mediator_audit::verify(&cert.record).is_ok());
    assert_eq!(cert.record.entries.last().unwrap().kind, "pact_refused");

    eprintln!("\n{}", cert.render());
}
