//! OFFLINE end-to-end test over the REAL shipped corpus (`scenarios/pacts/`).
//!
//! No Isabelle — exactly the public box's situation. These prove the COMMONS OF
//! PACTS works on the actually-certified pacts (whose `.cert.json` were produced
//! through the real gate), not just on synthetic fixtures:
//!
//!   * the four certified pacts ADMIT and COLLIDE onto one template key
//!     (signal-by-construction), while the broken / inconsistent pacts are
//!     REFUSED by the aggregation gate;
//!   * a real CERTIFIED LIVE CONTROVERSY exists over the corpus and re-checks
//!     (creative-credit's crux RAISES the award where the others LOWER it);
//!   * the whole corpus RE-VERIFIES from the signed artifacts alone, and a
//!     tampered cited record FAILS — on the real on-disk certificates.

use std::path::PathBuf;

use mediator_pact_commons::{
    reverify_corpus, AdmitError, CommonsError, Commons, DecisionPoint, TallyShape, WorldSide,
};

fn pacts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/pacts")
}

const CERTIFIED: &[&str] = &[
    "roommate_moveout",
    "cohabitation_lease",
    "creative_credit",
    "founder_vesting",
];

fn certified_commons() -> Commons {
    let dir = pacts_dir();
    let mut c = Commons::new();
    for name in CERTIFIED {
        c.admit_from_dir(&dir, name)
            .unwrap_or_else(|e| panic!("admit {name}: {e}"));
    }
    c
}

/// The four shipped certified pacts admit and collapse onto ONE template key —
/// the whole thesis: pacts of one template collide by construction.
#[test]
fn the_certified_corpus_collides_onto_one_template() {
    let c = certified_commons();
    assert_eq!(c.len(), 4, "all four certified pacts admit");
    let templates = c.templates();
    assert_eq!(
        templates.len(),
        1,
        "the four certified pacts share ONE template (collide by construction)"
    );
    let (_key, members) = templates.into_iter().next().unwrap();
    assert_eq!(members.len(), 4);
}

/// The broken (REFUSED) and overlap (INCONSISTENT) pacts are refused by the
/// aggregation gate — only certified, crux-free pacts back a controversy.
#[test]
fn uncertified_pacts_are_refused_by_the_gate() {
    let dir = pacts_dir();
    for name in ["roommate_moveout_broken", "roommate_overlap_inconsistent"] {
        let mut c = Commons::new();
        let err = c.admit_from_dir(&dir, name);
        assert!(
            err.is_err(),
            "{name} is not certified and must be refused by the gate"
        );
    }
}

/// A REAL certified live controversy exists over the corpus and re-checks: at the
/// at-or-above world, the contested predicate being true LOWERS the award in the
/// deposit/vesting pacts but RAISES it in creative-credit — formally identical
/// worlds, opposite human value-calls. This is the load-bearing output, computed
/// from the signed on-disk certificates.
#[test]
fn a_real_certified_live_controversy_is_present_and_signed() {
    let c = certified_commons();
    let (key, _members) = c.templates().into_iter().next().unwrap();

    let point = DecisionPoint {
        dial_role: 0,
        side: WorldSide::AtOrAbove,
        crux_role: 0,
        crux_value: false,
    };
    let tally = c.controversy(&key, &point).expect("a tally over the template");

    assert_eq!(tally.support(), 4, "all four back this decision point");
    assert_eq!(
        tally.shape(),
        TallyShape::LiveControversy,
        "the corpus genuinely splits on the crux's effect here"
    );
    // 3 pacts: crux-true LOWERS; 1 pact (creative_credit): crux-true RAISES.
    assert_eq!(tally.distribution["crux-true LOWERS the award"], 3);
    assert_eq!(tally.distribution["crux-true RAISES the award"], 1);

    // The dissenting pact is creative_credit, and its entry carries the signed
    // record root that a skeptic re-verifies.
    let raiser = tally
        .entries
        .iter()
        .find(|e| e.effect == mediator_pact_commons::CruxEffect::Raises)
        .expect("one pact raises");
    assert_eq!(raiser.label, "creative_credit");
    assert_eq!(raiser.record_root.len(), 64, "a full sha256 record root");

    // The render headlines support and refuses to recommend.
    let r = tally.render();
    assert!(r.contains("backed by 4 pact(s)"));
    assert!(r.contains("NEVER 'yours should be X'"));
    assert!(!r.to_lowercase().contains("you should"));
}

/// The whole shipped corpus re-verifies from the signed artifacts alone (no
/// Isabelle, no AI, no trust in the commons).
#[test]
fn the_real_corpus_reverifies_without_trust() {
    let c = certified_commons();
    assert!(c.reverify().is_ok(), "the real certified corpus re-verifies");

    let triples: Vec<_> = c
        .entries()
        .iter()
        .map(|e| (e.pact.clone(), e.cert.clone(), e.label.clone()))
        .collect();
    let outcome = reverify_corpus(&triples).expect("skeptic re-verifies the real corpus");
    assert_eq!(outcome.verified, 4);
    // All four recompute to the same key.
    assert!(outcome.recomputed_keys.windows(2).all(|w| w[0] == w[1]));
}

/// On the REAL certificates: tampering with one cited record's signed payload
/// makes re-verification fail — the commons cannot launder a forged precedent,
/// even from the on-disk cache.
#[test]
fn tampering_a_real_cited_record_fails_reverification() {
    let c = certified_commons();
    let mut triples: Vec<_> = c
        .entries()
        .iter()
        .map(|e| (e.pact.clone(), e.cert.clone(), e.label.clone()))
        .collect();

    // Forge the headline certification event of the first (real) certificate.
    let last = triples[0]
        .1
        .record
        .entries
        .last_mut()
        .expect("a record entry");
    last.detail["complete"] = serde_json::Value::Bool(false);

    match reverify_corpus(&triples) {
        Err(AdmitError::Gate { index, error, .. }) => {
            assert_eq!(index, 0);
            assert!(matches!(error, CommonsError::RecordInvalid(_)));
        }
        Ok(_) => panic!("a tampered real cited record MUST fail re-verification"),
    }
}
