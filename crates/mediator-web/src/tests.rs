//! Route tests. No network, no Isabelle — everything runs over an in-memory
//! `AppState` built from a small fixture (and, where present, the real cached
//! roommate analysis discovered on disk).

use super::*;
use crate::load::{DisputeRecord, discover_disputes, scenarios_dir};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use mediator_types::{
    Analysis, Claim, Conflict, ContestedItem, Dispute, Formula, Ledger, LedgerItem, Party,
    Settlement, Term, Valuation,
};
use tower::ServiceExt; // for `.oneshot()`

/// A self-contained roommate-shaped record (no disk, no prover).
fn roommate_record() -> DisputeRecord {
    let dispute = Dispute {
        title: "Roommate security-deposit dispute — Robin moves out".to_string(),
        parties: vec![
            Party {
                id: "robin".to_string(),
                display_name: "Robin (moving out)".to_string(),
                signature: vec![],
            },
            Party {
                id: "sam".to_string(),
                display_name: "Sam (staying, holds the deposit)".to_string(),
                signature: vec![],
            },
        ],
        claims: vec![Claim {
            id: "r1".to_string(),
            party: "robin".to_string(),
            nl: "The carpet stain was ordinary wear and tear.".to_string(),
            formula: Formula::Not(Box::new(Formula::Atom(Term::App(
                "stain_is_damage".to_string(),
                vec![],
            )))),
            english_render: "It is not the case that the stain is damage.".to_string(),
            weight: 7,
            defeasible: false,
            active: true,
        }],
        stipulated: vec![],
        ledger: Ledger {
            deposit_cents: 120_000,
            items: vec![LedgerItem {
                id: "cleaning".to_string(),
                label: "Professional cleaning".to_string(),
                amount_cents: 15_000,
                asserted_by: "sam".to_string(),
                disputed: false,
            }],
        },
        contested_items: vec![ContestedItem {
            id: "standing_desk".to_string(),
            label: "Standing desk".to_string(),
            divisible: false,
        }],
        valuations: vec![
            Valuation { party: "robin".to_string(), item: "standing_desk".to_string(), points: 60 },
            Valuation { party: "sam".to_string(), item: "standing_desk".to_string(), points: 40 },
        ],
    };

    let analysis = Analysis {
        shared_core: vec!["You both stipulate: Robin lived there and has moved out.".to_string()],
        genuine_conflicts: vec![Conflict {
            description: "Whether the carpet stain is chargeable damage or ordinary wear — a real disagreement of fact and judgment, not just different words.".to_string(),
            parties: vec!["robin".to_string(), "sam".to_string()],
            claim_ids: vec!["r1".to_string(), "s1".to_string()],
        }],
        dissolved: vec!["The $500.00-vs-$450.00 gap is a number to correct, not a deception.".to_string()],
        ledger_refund_cents: Some(105_000),
        ledger_findings: vec!["$1050.00 back if the stain is ordinary wear; $750.00 back if it counts as damage.".to_string()],
        crux: Some("stain_is_damage — the whole question reduces to this.".to_string()),
        settlements: vec![Settlement {
            label: "Adjusted Winner".to_string(),
            allocations: vec![("standing_desk".to_string(), "robin".to_string())],
            splits: vec![],
            party_points: vec![("robin".to_string(), 60.0), ("sam".to_string(), 40.0)],
            envy_free: true,
            equitable: true,
            pareto_optimal: true,
            explanation: "Robin keeps the standing desk; deposit refunded.".to_string(),
        }],
    };

    DisputeRecord {
        id: "roommate".to_string(),
        dispute,
        analysis,
        receipts: vec![],
    }
}

fn fixture_state() -> AppState {
    AppState::new(vec![roommate_record()])
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn gallery_returns_200_with_copy() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Mediateor"), "wordmark missing");
    assert!(html.contains("See the true shape of a disagreement"), "tagline missing");
    assert!(html.contains("Robin"), "dispute card party missing");
    assert!(html.contains("/dispute/roommate"), "dispute link missing");
}

#[tokio::test]
async fn seat_picker_returns_200() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(Request::builder().uri("/dispute/roommate").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Choose your seat"));
    assert!(html.contains("I'm Robin"));
    assert!(html.contains("/operator/roommate"));
}

#[tokio::test]
async fn party_robin_returns_200_with_kind_copy() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/party/roommate/robin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("For Robin"), "party eyebrow missing");
    assert!(html.contains("What you already agree on"), "shared-core section missing");
    assert!(html.contains("$1050.00"), "refund figure missing");
    assert!(html.contains("fair ways forward"), "settlement section missing");
    // Never the word "wrong"; never a raw formula token.
    assert!(!html.contains("wrong"), "party view must never say 'wrong'");
    assert!(!html.contains("Forall"), "party view must not leak formula syntax");
}

#[tokio::test]
async fn operator_returns_200_with_provenance() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/operator/roommate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("Operator cockpit"));
    assert!(html.contains("ISOLATED"), "crux status missing");
    assert!(html.contains("Receipt ledger"), "receipt section missing");
    assert!(html.contains("Settlement options"), "settlement table missing");
}

#[tokio::test]
async fn unknown_dispute_and_party_404() {
    let app = router(fixture_state());
    let r1 = app
        .clone()
        .oneshot(Request::builder().uri("/dispute/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::NOT_FOUND);
    let r2 = app
        .oneshot(Request::builder().uri("/party/roommate/nobody").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn static_htmx_route_is_wired() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/static/htmx.min.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::NOT_FOUND, "htmx.min.js not served");
}

#[tokio::test]
async fn settlement_accept_returns_fragment() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/settlement/roommate/0/accept")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("party=robin&name=Robin"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(!html.contains("<!DOCTYPE"), "should be a fragment, not a page");
    assert!(html.contains("waiting on the other party"), "acceptance copy missing");
}

#[tokio::test]
async fn settlement_counter_returns_fragment() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/settlement/roommate/0/counter")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("name=Robin"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("counter requested"), "counter chip missing");
    assert!(html.contains("conversation is open"), "counter should frame as a conversation");
}

/// If the real scenarios + caches are present on disk, the discovery path must
/// load them and the canonical roommate case must come first.
#[tokio::test]
async fn discovery_loads_disk_scenarios_if_present() {
    let dir = scenarios_dir();
    let records = discover_disputes(&dir);
    if records.is_empty() {
        // Scenarios not present in this checkout context — nothing to assert.
        return;
    }
    let state = AppState::new(records);
    assert_eq!(state.disputes[0].id, "roommate", "roommate should sort first");
}
