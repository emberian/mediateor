//! Real-Isabelle integration test. Generates the preamble + obligations for
//! `scenarios/roommate.json`, runs the ACTUAL `isabelle` binary on each
//! obligation (preamble + lemma + `end`, in an isolated session), and asserts
//! the verdicts the reduction promises:
//!
//!   refund_damage_world, refund_wear_world, over_claim_refuted, crux_iff
//!       -> Proved
//!   crux_is_damage, crux_is_wear
//!       -> Unknown  (the kernel must NOT decide the human question)
//!
//! This deliberately does NOT depend on the `mediator-prover` crate: core owns
//! the codegen; here we just run the generated theory ourselves to prove it is
//! valid Isabelle2025 and that the verdicts come out as designed.

use mediator_core::codegen::{build_obligations, build_preamble, standalone_theory};
use mediator_core::load_dispute;
use mediator_types::Verdict;
use std::path::{Path, PathBuf};
use std::process::Command;

const ISABELLE: &str = "/Users/ember/isabelle/Isabelle2025-2.app/bin/isabelle";

/// Run one obligation through real Isabelle and classify the outcome.
///
/// `home_user` is a private `ISABELLE_HOME_USER` shared across this test's
/// obligations so the HOL image is built once, but isolated from any other
/// agent's Isabelle store. Each obligation gets a *unique* theory + session
/// name so they never collide in the shared build database.
fn run_obligation(
    preamble: &str,
    ob: &mediator_types::Obligation,
    idx: usize,
    home_user: &Path,
) -> Verdict {
    // Give this obligation its own theory name to avoid sqlite primary-key
    // collisions in the shared session-sources database.
    let theory_name = format!("Mediator_Probe_{idx}");
    let preamble_renamed = preamble.replacen("theory Mediator_Probe", &format!("theory {theory_name}"), 1);
    let theory = standalone_theory(&preamble_renamed, ob);

    let dir: PathBuf = std::env::temp_dir().join(format!(
        "mt-core-isa-{}-{}-{}",
        std::process::id(),
        idx,
        ob.name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create session dir");

    let session = format!("Probe_{idx}");
    std::fs::write(
        dir.join("ROOT"),
        format!("session {session} = HOL +\n  theories\n    {theory_name}\n"),
    )
    .unwrap();
    std::fs::write(dir.join(format!("{theory_name}.thy")), &theory).unwrap();

    let mut cmd = Command::new(ISABELLE);
    cmd.arg("build").arg("-d").arg(&dir).arg(&session);
    // Only override the user home if asked (keeps the prebuilt system HOL image
    // reachable by default, so we don't rebuild HOL from scratch).
    if !home_user.as_os_str().is_empty() {
        cmd.env("ISABELLE_HOME_USER", home_user);
    }
    let output = cmd.output().expect("invoke isabelle");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    let verdict = if output.status.success() {
        Verdict::Proved
    } else if combined.contains("Failed to apply")
        || combined.contains("Unfinished session")
        || combined.contains("Failed to finish proof")
    {
        // The proof method genuinely did not close the goal — honest Unknown.
        Verdict::Unknown
    } else {
        // Syntax/type error or environment problem: surface it.
        Verdict::Error(combined.lines().take(8).collect::<Vec<_>>().join(" | "))
    };

    let _ = std::fs::remove_dir_all(&dir);
    verdict
}

#[test]
fn roommate_reduction_checks_against_real_isabelle() {
    if !PathBuf::from(ISABELLE).exists() {
        eprintln!("skipping: isabelle not found at {ISABELLE}");
        return;
    }

    let dispute = load_dispute(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scenarios/roommate.json"
    ))
    .expect("load roommate scenario");

    let preamble = build_preamble(&dispute);
    let obligations = build_obligations(&dispute);

    // Sanity: the preamble is well-formed at the shape level.
    assert!(preamble.contains("theory Mediator_Probe"));
    assert!(!preamble.trim_end().ends_with("end"), "preamble must not close the theory");

    // Use the default Isabelle user home so the prebuilt system HOL image is
    // reused (an empty path means "don't override"). Unique per-obligation
    // theory + session names keep the shared build database collision-free.
    let home_user = PathBuf::new();

    let mut verdicts = std::collections::HashMap::new();
    for (i, ob) in obligations.iter().enumerate() {
        let v = run_obligation(&preamble, ob, i, &home_user);
        eprintln!("obligation {:<22} -> {:?}", ob.name, v);
        // No obligation should be an Error — that would mean invalid Isabelle.
        assert!(
            !matches!(v, Verdict::Error(_)),
            "obligation {} produced invalid Isabelle / errored: {:?}\n--- theory ---\n{}",
            ob.name,
            v,
            standalone_theory(&preamble, ob)
        );
        verdicts.insert(ob.name.clone(), v);
    }

    let get = |n: &str| verdicts.get(n).cloned().unwrap();

    // The certifiable core: Proved.
    assert_eq!(get("refund_damage_world"), Verdict::Proved, "refund (damage world)");
    assert_eq!(get("refund_wear_world"), Verdict::Proved, "refund (wear world)");
    assert_eq!(get("over_claim_refuted"), Verdict::Proved, "over-claim refuted");
    assert_eq!(get("crux_iff"), Verdict::Proved, "crux iff (the reduction)");

    // The human question: honestly Unknown. The kernel must not decide it.
    assert_eq!(get("crux_is_damage"), Verdict::Unknown, "crux is_damage must be undecided");
    assert_eq!(get("crux_is_wear"), Verdict::Unknown, "crux is_wear must be undecided");
}
