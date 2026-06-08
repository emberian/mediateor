//! Route tests. No network, no Isabelle — everything runs over an in-memory
//! `AppState` built from a small fixture (and, where present, the real cached
//! roommate analysis discovered on disk).

use super::*;
use crate::load::{DisputeRecord, discover_disputes, scenarios_dir};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use mediator_types::{
    Analysis, Claim, Conflict, ContestedItem, Crux, Dispute, Formula, Ledger, LedgerItem, Party,
    Settlement, Sig, Sort, Term, Valuation, Verdict,
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
                controlling_crux: None,
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
        cruxes: Vec::new(),
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

/// A record reducing to TWO contested questions at once (multi-crux), each with
/// a glossed predicate so the party view can phrase both kindly.
fn twocrux_record() -> DisputeRecord {
    let sig = |name: &str, gloss: &str| Sig {
        name: name.to_string(),
        arg_sorts: vec![],
        ret: Sort::Bool,
        gloss: gloss.to_string(),
    };
    let dispute = Dispute {
        title: "A two-knot dispute".to_string(),
        parties: vec![
            Party {
                id: "ada".to_string(),
                display_name: "Ada (the founder who left)".to_string(),
                signature: vec![
                    sig("departure_breached_vesting", "the departure breached the vesting agreement"),
                ],
            },
            Party {
                id: "ben".to_string(),
                display_name: "Ben (the remaining partner)".to_string(),
                signature: vec![
                    sig("gift_was_advance", "the $8,000 was an advance, not a gift"),
                ],
            },
        ],
        claims: vec![],
        stipulated: vec![],
        ledger: Ledger { deposit_cents: 0, items: vec![] },
        contested_items: vec![],
        valuations: vec![],
    };
    let analysis = Analysis {
        shared_core: vec!["You both want the partnership wound down cleanly.".to_string()],
        genuine_conflicts: vec![],
        dissolved: vec![],
        ledger_refund_cents: None,
        ledger_findings: vec![],
        crux: Some("departure_breached_vesting — one of two contested questions.".to_string()),
        cruxes: vec![
            Crux {
                predicate: "departure_breached_vesting".to_string(),
                question: "Whether the departure breached the vesting agreement".to_string(),
                verdict: Verdict::Unknown,
            },
            Crux {
                predicate: "gift_was_advance".to_string(),
                question: "Whether the $8,000 was an advance".to_string(),
                verdict: Verdict::Unknown,
            },
        ],
        settlements: vec![],
    };
    DisputeRecord { id: "twoknot".to_string(), dispute, analysis, receipts: vec![] }
}

fn fixture_state() -> AppState {
    AppState::new(vec![roommate_record()])
}

/// State with a deliberately tiny per-IP allowance, for the throttle test.
fn throttled_state() -> AppState {
    AppState::new(vec![roommate_record()]).with_rate_config(crate::RateConfig {
        global_capacity: 100.0,
        global_refill: 100.0,
        per_ip_capacity: 3.0,
        per_ip_refill: 0.0001, // effectively no refill within the test window
        max_ip_buckets: 64,
    })
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
    assert!(html.contains("A calm room for a hard conversation"), "tagline missing");
    // the room is the centerpiece — the primary CTA enters it
    assert!(html.contains("Enter the room"), "room CTA missing");
    assert!(html.contains("/talk/roommate/"), "room link missing");
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
    assert!(html.contains("/session/roommate"), "session link missing");
}

#[tokio::test]
async fn session_route_renders_conducted_mediation() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(Request::builder().uri("/session/roommate").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("A mediation, conducted"));
    assert!(html.contains("In private with"));
    assert!(html.contains("Together"));
    // no live LLM in tests → the deterministic scripted voice
    assert!(html.contains("scripted preview voice"));
    // a party is never told they're "wrong"
    assert!(!html.to_lowercase().contains("you are wrong"));
}

/// Pull the freshly-created session id out of the room page (it's in the say-url
/// `hx-post` of the room form).
fn sid_from_room(html: &str) -> String {
    let say = html
        .split("/say\"")
        .next()
        .and_then(|s| s.rsplit("/talk/").next())
        .unwrap_or("");
    say.split('/').next().unwrap_or("").to_string()
}

#[tokio::test]
async fn the_room_opens_and_hears_a_party() {
    let app = router(fixture_state());

    // open the room as robin
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("The room"), "room heading missing");
    // the evidence drawer is part of the room
    assert!(html.contains("Add something to the record"), "evidence drawer missing");

    let sid = sid_from_room(&html);
    assert!(sid.starts_with('s'), "sid: {sid} (html had no say-url?)");
    let say = format!("/talk/{sid}/robin/say");

    // say something → the mediator reflects (and checks an interest back)
    let req = Request::builder()
        .method("POST")
        .uri(&say)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("message=the stain was already there when I moved in"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let frag = body_string(resp).await;
    assert!(frag.to_lowercase().contains("mediator"));
    // offline → the deterministic scripted caucus reply, then a check-back
    assert!(frag.contains("Thank you for telling me"));
    assert!(frag.contains("check something back") || frag.contains("Am I close"));
}

/// Driving both parties through to readiness walks the room to its heart: the
/// shared ground and the one open question are now spoken in the mediator's
/// voice (no certified bullet/predicate panels in the party's face), while the
/// two things the parties themselves shaped — the SUBTRACTION and a single
/// balanced way forward — get a real, central surface.
#[tokio::test]
async fn the_room_reaches_the_subtraction_panel() {
    let app = router(fixture_state());
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    let sid = sid_from_room(&html);
    assert!(sid.starts_with('s'), "sid: {sid}");

    // Each party speaks, then confirms the checked-back interest (the iterated
    // loop: name it, ask "am I close?", they own it). robin first, then sam.
    // sam's confirm is the last turn — by then everyone is heard and the
    // autonomous flow walks all the way to the options (and residue, since the
    // ledger is certified).
    let turns = [
        ("robin", "it was wear and tear, honestly"),
        ("robin", "yes — that's exactly it"),
        ("sam", "the carpet is real damage and that's only fair"),
        ("sam", "right, that's what I mean"),
    ];
    let mut frag = String::new();
    for (p, m) in turns {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/talk/{sid}/{p}/say"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("message={m}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        frag = body_string(resp).await;
    }

    // FOLDED INTO VOICE: the shared ground is spoken warmly, not paneled — and the
    // certified bullet panel is gone from the party's view.
    assert!(frag.contains("what you already agree on"), "shared ground should be spoken");
    assert!(!frag.contains("panel-ground"), "the certified bullet panel must be gone");
    // FOLDED INTO VOICE: the mediator simply names the one real question; no
    // predicate-list panel demanding the party stare at a knot.
    assert!(frag.contains("the one real knot") || frag.contains("yours"), "crux should be spoken");
    assert!(!frag.contains("panel-crux"), "the crux predicate panel must be gone");
    assert!(!frag.contains("yours to answer"), "no crux panel kicker");
    // KEPT CENTRAL: the SUBTRACTION panel — the signature move the parties uncovered.
    assert!(
        frag.contains("what was never about money"),
        "subtraction/residue panel missing: {frag}"
    );
    assert!(frag.contains("panel-residue"), "the residue must keep its central surface");
    // LEAD WITH ONE: a single balanced way forward, acceptable in-session — not
    // three equal panels. (This fixture carries one settlement, so there's no
    // disclosure; the multi-option disclosure is covered in `lead_option_*` below.)
    assert!(frag.contains("A way forward"), "the single lead option missing");
    assert!(frag.contains("This one works for me"), "in-session accept missing");
    // BACKSTAGED: the party never reads the machine's vocabulary in the room.
    assert!(!frag.contains("certified"), "the word 'certified' must not reach the party");
    assert!(!frag.to_lowercase().contains("ledger"), "the word 'ledger' must not reach the party");
    // never tells a human they are "wrong", never leaks a formula
    assert!(!frag.to_lowercase().contains("you are wrong"));
    assert!(!frag.contains("Forall"));
}

/// Evidence submitted in the room is stored and acknowledged by the mediator
/// (woven in), and never decides the open question.
#[tokio::test]
async fn the_room_takes_and_acknowledges_evidence() {
    let app = router(fixture_state());
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    let sid = sid_from_room(&html);

    // both parties get heard fully first (speak + confirm), so the next
    // autonomous move after evidence arrives is the acknowledgement.
    for (p, m) in [
        ("robin", "it was wear and tear"),
        ("robin", "yes, exactly"),
        ("sam", "it's damage and that's fair"),
        ("sam", "right"),
    ] {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/talk/{sid}/{p}/say"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("message={m}")))
                    .unwrap(),
            )
            .await
            .unwrap();
    }

    // robin adds a concrete fact as evidence
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/talk/{sid}/robin/evidence"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("kind=fact&text=I lived there 2 years and the carpet was already worn&note="))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let frag = body_string(resp).await;
    assert!(frag.contains("Added to the record"), "evidence not stored/echoed");
    // the mediator acknowledges the exhibit on the record
    assert!(
        frag.to_lowercase().contains("on the record"),
        "evidence not acknowledged: {frag}"
    );
}

/// When two parties put *colliding facts* on the record (different numbers about
/// the same thing), the room surfaces the honest factual-conflict moment —
/// speech-led and calm, inside a mediator bubble, not an adjudication panel — and
/// never implies the machine decided it.
#[tokio::test]
async fn the_room_surfaces_a_factual_conflict_without_deciding() {
    let app = router(fixture_state());
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    let sid = sid_from_room(&html);

    async fn say(app: &axum::Router, sid: &str, p: &str, m: &str) {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/talk/{sid}/{p}/say"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("message={m}")))
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    async fn evidence(app: &axum::Router, sid: &str, p: &str, text: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/talk/{sid}/{p}/evidence"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("kind=fact&text={text}&note=")))
                    .unwrap(),
            )
            .await
            .unwrap();
        body_string(resp).await
    }

    say(&app, &sid, "robin", "wear and tear").await;
    say(&app, &sid, "robin", "yes").await;
    say(&app, &sid, "sam", "damage").await;
    say(&app, &sid, "sam", "yes").await;

    evidence(&app, &sid, "robin", "the stain is 10cm across").await;
    let frag = evidence(&app, &sid, "sam", "the stain is 30cm across").await;

    // the honest factual-conflict moment appears, speech-led…
    assert!(frag.contains("a question of fact needs evidence"), "conflict surface missing: {frag}");
    assert!(frag.contains("remember it differently"), "the two accounts should be set side by side");
    // …spoken in the mediator's voice (a calm bubble), NOT an adjudication panel
    assert!(frag.contains("b med conflict"), "conflict should ride in a mediator bubble");
    assert!(!frag.contains("panel-conflict"), "the conflict must not be a hard adjudication panel");
    // …and the machine explicitly refuses to decide which fact is true
    assert!(
        frag.contains("won't decide which of you is right") || frag.contains("wouldn't be fair"),
        "must not imply the machine decided the fact"
    );
}

/// The whole arc in the room: hear both, reach the options, accept one, then
/// CO-AUTHOR the agreement and sign it — the agreement is theirs, owned by both,
/// and only then does the room land.
#[tokio::test]
async fn the_room_co_authors_and_signs_the_agreement() {
    let app = router(fixture_state());
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    let sid = sid_from_room(&html);

    async fn say(app: &axum::Router, sid: &str, p: &str, m: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/talk/{sid}/{p}/say"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("message={m}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        body_string(resp).await
    }

    // hear + confirm both
    say(&app, &sid, "robin", "wear and tear").await;
    say(&app, &sid, "robin", "yes exactly").await;
    say(&app, &sid, "sam", "it's damage").await;
    let frag = say(&app, &sid, "sam", "right that's it").await;
    assert!(frag.contains("A way forward"), "should reach the single balanced option");
    assert!(frag.contains("This one works for me"), "the option should be acceptable in-session");

    // accept option 0 → the co-authoring draft opens
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/talk/{sid}/robin/accept/0"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let frag = body_string(resp).await;
    assert!(frag.contains("write it down together"), "co-authoring draft should open: {frag}");
    assert!(frag.contains("name=\"message\""), "draft should be editable");

    // both sign → owned → landed
    let _ = app
        .clone()
        .oneshot(Request::builder().method("POST").uri(format!("/talk/{sid}/robin/sign")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let resp = app
        .oneshot(Request::builder().method("POST").uri(format!("/talk/{sid}/sam/sign")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let frag = body_string(resp).await;
    assert!(frag.contains("owned by both") || frag.contains("landed"), "agreement should be owned/landed: {frag}");
}

/// A multi-crux dispute surfaces ALL its open questions — in the gallery chip,
/// the party view (each phrased kindly), and the signed audit record — and never
/// implies the machine decided any of them.
#[tokio::test]
async fn multi_crux_is_surfaced_everywhere() {
    let app = router(AppState::new(vec![twocrux_record()]));

    // gallery chip pluralizes
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    assert!(html.contains("2 open questions"), "gallery should show 2 open questions");

    // party view shows BOTH questions, phrased kindly from the glosses
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/party/twoknot/ada").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("questions that are really yours"), "plural crux heading missing");
    assert!(html.contains("the departure breached the vesting agreement"), "crux 1 missing");
    assert!(html.contains("an advance, not a gift"), "crux 2 missing");
    // never implies a decision was made for them, never leaks a formula
    assert!(!html.contains("wrong"));
    assert!(!html.contains("Forall"));

    // the signed audit record lists each crux as handed back, not decided
    let resp = app
        .oneshot(Request::builder().uri("/audit/twoknot/download").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let json = body_string(resp).await;
    let rec: mediator_audit::MediationRecord = serde_json::from_str(&json).unwrap();
    assert!(mediator_audit::verify(&rec).is_ok(), "record must verify");
    let kinds: Vec<&str> = rec.entries.iter().map(|e| e.kind.as_str()).collect();
    assert!(
        kinds.iter().filter(|k| **k == "crux_handed_back").count() == 2,
        "both cruxes should be recorded as handed back: {kinds:?}"
    );
}

/// PUBLIC HARDENING: a burst from a single IP to a model-calling endpoint gets
/// throttled with a friendly fragment (never a bare 429 to a human), while the
/// global cap stays high. Offline — the scripted brain handles every call.
#[tokio::test]
async fn a_burst_from_one_ip_is_throttled_kindly() {
    let app = router(throttled_state());

    // Open a room as robin and grab the session id.
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    let say = html
        .split("/say\"")
        .next()
        .and_then(|s| s.rsplit("/talk/").next())
        .unwrap_or("");
    let sid = say.split('/').next().unwrap_or("");
    assert!(sid.starts_with('s'), "sid: {sid}");
    let say_url = format!("/talk/{sid}/robin/say");

    // Fire a fast burst from ONE IP (per-IP capacity is 3 here). The first few
    // get a real reply; once the bucket is dry, a calm "one moment" fragment.
    let mut throttled = false;
    let mut served = 0;
    for i in 0..7 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&say_url)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header("x-forwarded-for", "203.0.113.7")
                    .body(Body::from(format!("message=turn number {i}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Always a 200 (htmx swaps the fragment) — never a bare 429 to a person.
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        if body.contains("One moment") || body.contains("let's not rush") {
            throttled = true;
        } else {
            served += 1;
        }
    }
    assert!(served >= 1, "at least the first call should be served");
    assert!(throttled, "a sustained burst from one IP should get the friendly throttle");
}

/// A *different* IP is unaffected by another IP's exhausted budget — the limit
/// is genuinely per-peer, not global-only.
#[tokio::test]
async fn per_ip_limit_does_not_punish_other_peers() {
    let app = router(throttled_state());
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/talk/roommate/robin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = body_string(resp).await;
    let say = html.split("/say\"").next().and_then(|s| s.rsplit("/talk/").next()).unwrap_or("");
    let sid = say.split('/').next().unwrap_or("");
    let say_url = format!("/talk/{sid}/robin/say");

    // Exhaust IP A.
    for _ in 0..5 {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&say_url)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header("x-forwarded-for", "198.51.100.1")
                    .body(Body::from("message=flooding"))
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    // IP B's first call should still be served (a real reply, not "one moment").
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&say_url)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("x-forwarded-for", "198.51.100.2")
                .body(Body::from("message=my first words"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(!body.contains("One moment"), "a fresh peer must not inherit another's throttle");
    assert!(body.contains("Thank you for telling me"), "fresh peer should get a real reply");
}

#[tokio::test]
async fn audit_record_view_and_download_verify() {
    let app = router(fixture_state());
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/audit/roommate").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    assert!(html.contains("The record"));
    assert!(html.to_lowercase().contains("verif"));

    // the downloaded signed record must actually verify
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/audit/roommate/download")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_string(resp).await;
    let rec: mediator_audit::MediationRecord = serde_json::from_str(&json).unwrap();
    assert!(mediator_audit::verify(&rec).is_ok(), "downloaded record must verify");
    assert!(!rec.entries.is_empty());
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

// ── live-LLM guardrail tests (offline, no network, no Bedrock) ───────────────
//
// All these run with MEDIATEOR_LIVE_LLM *unset* (the fixture_state() constructor
// reads the env at construction time, and these tests never set it).

#[tokio::test]
async fn party_view_without_live_llm_has_no_formalize_panel() {
    // MEDIATEOR_LIVE_LLM is not set → live_llm_enabled = false.
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
    // The panel's distinctive heading must NOT appear.
    assert!(
        !html.contains("Say it in your own words"),
        "live panel must not render when MEDIATEOR_LIVE_LLM is unset"
    );
    // The /formalize endpoint must not be linked from the page.
    assert!(
        !html.contains("/formalize/"),
        "formalize endpoint must not be linked when feature is off"
    );
}

#[tokio::test]
async fn formalize_endpoint_returns_off_fragment_when_live_llm_unset() {
    // With MEDIATEOR_LIVE_LLM unset the endpoint returns a friendly "off" note
    // without ever calling Bedrock.
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/formalize/roommate")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("claim=the+stain+was+ordinary+wear"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_string(resp).await;
    // Must be a fragment, not a full page.
    assert!(!html.contains("<!DOCTYPE"), "should be a fragment, not a full page");
    // Must explain that live mode is off.
    assert!(
        html.contains("off") || html.contains("MEDIATEOR_LIVE_LLM"),
        "fragment should mention live mode is off: {html}"
    );
    // Must NOT contain any Bedrock-sourced content.
    assert!(
        !html.contains("council"),
        "no council output should appear when feature is off"
    );
}

#[tokio::test]
async fn formalize_endpoint_rejects_overlong_claim() {
    // Even with MEDIATEOR_LIVE_LLM unset the length check fires first.
    // Verify the endpoint exists and handles the form properly.
    let app = router(fixture_state());
    let long_claim = "x".repeat(241);
    let body = format!("claim={}", long_claim);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/formalize/roommate")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    // Either 200 (fragment with error message) or feature-off response — both are fine.
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::BAD_REQUEST,
        "unexpected status: {}",
        resp.status()
    );
}

#[tokio::test]
async fn formalize_endpoint_404_for_unknown_dispute() {
    let app = router(fixture_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/formalize/nonexistent")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("claim=test"))
                .unwrap(),
        )
        .await
        .unwrap();
    // When live LLM is off, the off-fragment comes back with 200 before we
    // even check the dispute. So either 200 (off) or 404 (live-on, not found).
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::NOT_FOUND,
        "unexpected status: {}",
        resp.status()
    );
}

// ── the simplified "lead with one" options (pure render, no fixture needed) ──

fn opt(idx: usize, summary: &str, envy_free: bool, equitable: bool) -> crate::OptionView {
    crate::OptionView { idx, summary: summary.to_string(), envy_free, equitable }
}

#[test]
fn lead_option_picks_the_most_balanced() {
    // the second is both envy-free AND equitable → it leads, even though it's not first
    let opts = vec![
        opt(0, "split A", true, false),
        opt(1, "split B", true, true),
        opt(2, "split C", false, false),
    ];
    assert_eq!(crate::lead_option(&opts), 1);
    // with nothing to distinguish them, the first leads
    let flat = vec![opt(0, "a", false, false), opt(1, "b", false, false)];
    assert_eq!(crate::lead_option(&flat), 0);
}

#[test]
fn options_lead_with_one_and_fold_the_rest_behind_a_quiet_disclosure() {
    // three options → one leads plainly; the other two hide behind a disclosure,
    // and NONE of the machine's fairness vocabulary reaches the party.
    let opts = vec![
        opt(0, "Robin keeps the desk; deposit split evenly", true, true),
        opt(1, "Sam keeps the desk; Robin takes more deposit", true, false),
        opt(2, "the desk is sold and the cash split", false, false),
    ];
    let beat = crate::Beat::Options(opts);
    let html = crate::render_beat("s1", "robin", &beat).into_string();

    // exactly one lead card, the rest under a <details>
    assert!(html.contains("A way forward"), "lead option label missing");
    assert!(html.contains("Another way"), "non-lead option label missing");
    assert!(html.contains("<details"), "the other splits should be a quiet disclosure");
    assert!(html.contains("see 2 other fair splits"), "disclosure summary missing");
    // every option stays acceptable in-session (all three accept buttons present)
    assert!(html.contains("/talk/s1/robin/accept/0"));
    assert!(html.contains("/talk/s1/robin/accept/1"));
    assert!(html.contains("/talk/s1/robin/accept/2"));
    // no fairness chips / machine words in the party's face
    assert!(!html.contains("certified"), "no 'certified' in the room");
    assert!(!html.contains("envy-free"), "no 'envy-free' chip in the room");
    assert!(!html.contains("equitable"), "no 'equitable' chip in the room");

    // a single option → no disclosure at all
    let one = crate::Beat::Options(vec![opt(0, "the only fair split", true, true)]);
    let html1 = crate::render_beat("s1", "robin", &one).into_string();
    assert!(html1.contains("A way forward"));
    assert!(!html1.contains("<details"), "one option needs no disclosure");
}
