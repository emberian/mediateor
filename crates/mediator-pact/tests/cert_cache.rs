//! OFFLINE re-verification of the cached certificates (`scenarios/pacts/*.cert.json`).
//!
//! These run with NO Isabelle — exactly the situation of the public box. They
//! prove the cache discipline is honest: a machine with no prover can load each
//! cached `PactCertificate`, RE-VERIFY its signed record (ed25519 over the hash
//! chain), and — for an INCONSISTENT cache — RE-CHECK the exhibited witness
//! against the pact it came from, without re-proving anything. The cache is
//! trustworthy because it is re-verifiable, not because we say so.

use mediator_pact::{recheck_witness, Pact, PactCertificate, Status};
use std::path::PathBuf;

fn pacts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/pacts")
}

fn load_cert(name: &str) -> PactCertificate {
    let path = pacts_dir().join(format!("{name}.cert.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn load_pact(name: &str) -> Pact {
    let path = pacts_dir().join(format!("{name}.json"));
    Pact::load(path.to_str().unwrap()).unwrap_or_else(|e| panic!("load {name}: {e}"))
}

/// Every cached certificate's signed record re-verifies offline.
#[test]
fn every_cached_record_reverifies_offline() {
    let names = [
        "roommate_moveout",
        "roommate_moveout_broken",
        "roommate_overlap_inconsistent",
        "founder_vesting",
        "creative_credit",
        "cohabitation_lease",
    ];
    for name in names {
        let cert = load_cert(name);
        assert!(
            mediator_audit::verify(&cert.record).is_ok(),
            "cached record for {name} must re-verify with no Isabelle"
        );
        // The cache echoes the declared question-space and the honesty scope.
        assert!(!cert.declared_predicates.is_empty(), "{name}: declared preds");
        assert!(
            cert.scope_note.contains("DECLARED predicate space"),
            "{name}: scope note travels with the cache"
        );
    }
}

/// The certified caches really are CERTIFIED (every obligation Proved), so the
/// public box renders them as green without re-proving.
#[test]
fn certified_caches_are_green() {
    for name in ["roommate_moveout", "founder_vesting", "creative_credit", "cohabitation_lease"] {
        let cert = load_cert(name);
        assert!(matches!(cert.status, Status::Certified), "{name} must be CERTIFIED");
        assert!(
            cert.obligations.iter().all(|o| o.proved()),
            "{name}: every obligation Proved"
        );
        assert_eq!(
            cert.record.entries.last().unwrap().kind,
            "pact_certified",
            "{name}: record headline event"
        );
    }
}

/// The REFUSED cache names a coverage gap (the public box shows the uncovered
/// world without Isabelle).
#[test]
fn refused_cache_names_the_gap() {
    let cert = load_cert("roommate_moveout_broken");
    match &cert.status {
        Status::Refused { gap } => {
            assert_eq!(gap.obligation, "coverage");
            assert!(gap.plain.contains("does NOT cover every declared world"));
        }
        other => panic!("broken pact cache must be REFUSED, got {other:?}"),
    }
    assert_eq!(cert.record.entries.last().unwrap().kind, "pact_refused");
}

/// The INCONSISTENT cache carries a witness that RE-CHECKS against the pact —
/// the public box can re-evaluate the two guards at the named world and confirm
/// the clash by itself, with no prover.
#[test]
fn inconsistent_cache_witness_rechecks_offline() {
    let cert = load_cert("roommate_overlap_inconsistent");
    let pact = load_pact("roommate_overlap_inconsistent");

    let witness = match &cert.status {
        Status::Inconsistent { witness } => witness,
        other => panic!("must be INCONSISTENT with a witness, got {other:?}"),
    };

    // Re-check the witness from scratch: both guards fire at the named world and
    // the two demanded awards differ.
    assert!(
        recheck_witness(&pact, witness),
        "the cached witness must re-verify against the pact with no Isabelle"
    );
    assert_ne!(witness.award_i_cents, witness.award_j_cents);

    // The signed record's headline is the inconsistency, carrying the witness.
    let last = cert.record.entries.last().unwrap();
    assert_eq!(last.kind, "pact_inconsistent");
    assert_eq!(last.detail["non_contradictory"], serde_json::json!(false));
    assert!(last.detail["witness_plain"]
        .as_str()
        .unwrap()
        .contains("vs"));
}

/// A cached certificate's CLAIMED verdicts are internally re-checkable against
/// the pact for the cheap half — the witness — so a tampered "green" cache that
/// was actually inconsistent could be caught by re-running the bounded search.
/// (We can't re-run Isabelle here, but we CAN confirm a certified cache has no
/// findable clash for the pair it claims consistent — the search agrees.)
#[test]
fn certified_cache_has_no_findable_clash() {
    let pact = load_pact("founder_vesting");
    let n = pact.clauses.len();
    for i in 0..n {
        for j in (i + 1)..n {
            assert!(
                mediator_pact::find_clash(&pact, i, j).is_none(),
                "founder_vesting claims CERTIFIED, but clause pair ({i},{j}) has a findable clash"
            );
        }
    }
}
