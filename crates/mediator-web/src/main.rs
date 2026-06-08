//! `mediator-web` binary.
//!
//! Loads `scenarios/roommate.json`, constructs a representative `Analysis`
//! (matching the hand-worked reduction from ARCHITECTURE.md), and serves
//! the web front end on 127.0.0.1:3000.
//!
//! Integration note: swap the hand-built `Analysis` here for a real kernel
//! call when `mediator-core` is ready. `AppState` does not depend on the
//! kernel; it just needs `(Dispute, Analysis)`.

use std::{net::SocketAddr, path::Path};

use anyhow::Context;
use mediator_types::{Analysis, Conflict, Dispute, Settlement};
use mediator_web::{router, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Load dispute from scenarios/roommate.json ─────────────────────────
    // The binary's cwd may vary; we search relative to CARGO_MANIFEST_DIR
    // (compile-time) then fall back to a runtime path.
    let scenario_path = {
        // CARGO_MANIFEST_DIR is crates/mediator-web; go up two levels.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let candidate = manifest.join("../../scenarios/roommate.json");
        if candidate.exists() {
            candidate
        } else {
            // Fallback: cwd-relative, for when the binary is run from the
            // workspace root.
            Path::new("scenarios/roommate.json").to_path_buf()
        }
    };

    let raw = std::fs::read_to_string(&scenario_path)
        .with_context(|| format!("reading {}", scenario_path.display()))?;
    let dispute: Dispute =
        serde_json::from_str(&raw).context("parsing roommate.json")?;

    // ── Hand-build a representative Analysis ─────────────────────────────
    // This matches the reduction described in ARCHITECTURE.md.
    // The real kernel (mediator-core) will produce this via Isabelle; for now
    // we demonstrate the shape so the web front end is useful immediately.
    let analysis = Analysis {
        shared_core: vec![
            "Robin lived at the property and has now moved out.".to_string(),
            "Sam holds the security deposit of $1,200.00.".to_string(),
            "Professional cleaning of $150.00 is undisputed.".to_string(),
            "Under the lease: tenant owes carpet repair if and only if the stain is chargeable damage.".to_string(),
        ],
        genuine_conflicts: vec![
            Conflict {
                description: "Is the carpet stain ordinary wear-and-tear or chargeable damage?".to_string(),
                parties: vec!["robin".to_string(), "sam".to_string()],
                claim_ids: vec!["r1".to_string(), "s1".to_string()],
            },
        ],
        dissolved: vec![
            "Sam's verbal $500 figure is refuted by the itemized ledger ($450). \
             This is not a new dispute — it dissolves once the arithmetic is checked."
             .to_string(),
        ],
        // The minimum refund is certain: deposit − cleaning = $1,050 (wear world).
        // If damage is proven, it falls to $900. We report the wear-world floor.
        ledger_refund_cents: Some(105_000),
        ledger_findings: vec![
            "Claimed total $500.00 (claim s2) is REFUTED: itemized total is $450.00.".to_string(),
            "Undisputed deduction: cleaning $150.00.".to_string(),
            "Contested deduction: carpet repair $300.00 — depends on the crux.".to_string(),
            "If stain = wear:   refund = $1,200 − $150          = $1,050.00  (PROVED)".to_string(),
            "If stain = damage: refund = $1,200 − $150 − $300   = $750.00   (conditional)".to_string(),
        ],
        crux: Some(
            "stain_is_damage — is the carpet stain chargeable damage (Robin's responsibility) \
             or ordinary wear and tear (landlord's responsibility)? \
             This single predicate is what the entire money question reduces to."
            .to_string(),
        ),
        settlements: vec![
            Settlement {
                label: "Wear-and-tear world: full refund minus cleaning".to_string(),
                allocations: vec![
                    ("couch".to_string(), "robin".to_string()),
                    ("standing_desk".to_string(), "robin".to_string()),
                    ("kitchenware".to_string(), "sam".to_string()),
                    ("bookshelf".to_string(), "sam".to_string()),
                ],
                splits: vec![],
                party_points: vec![
                    ("robin".to_string(), 75.0),
                    ("sam".to_string(), 45.0),
                ],
                envy_free: true,
                equitable: false,
                pareto_optimal: true,
                explanation: "Deposit refund $1,050 to Robin. Robin keeps the couch \
                    and standing desk (her highest-value items); Sam keeps the kitchenware \
                    and bookshelf. Robin gets 75 of her 100 valuation points; Sam gets 45."
                    .to_string(),
            },
            Settlement {
                label: "Damage world: refund minus cleaning and carpet".to_string(),
                allocations: vec![
                    ("couch".to_string(), "robin".to_string()),
                    ("standing_desk".to_string(), "robin".to_string()),
                    ("kitchenware".to_string(), "sam".to_string()),
                    ("bookshelf".to_string(), "sam".to_string()),
                ],
                splits: vec![],
                party_points: vec![
                    ("robin".to_string(), 75.0),
                    ("sam".to_string(), 45.0),
                ],
                envy_free: true,
                equitable: false,
                pareto_optimal: true,
                explanation: "Deposit refund $750 to Robin (accepting that the stain is damage). \
                    Same item allocation as the wear-and-tear world. Fairness certificates hold \
                    for the property division; the cash amount adjusts."
                    .to_string(),
            },
            Settlement {
                label: "Split-the-difference: $900 refund, shared items by preference".to_string(),
                allocations: vec![
                    ("couch".to_string(), "robin".to_string()),
                    ("standing_desk".to_string(), "sam".to_string()),
                    ("kitchenware".to_string(), "sam".to_string()),
                    ("bookshelf".to_string(), "robin".to_string()),
                ],
                splits: vec![],
                party_points: vec![
                    ("robin".to_string(), 55.0),
                    ("sam".to_string(), 60.0),
                ],
                envy_free: false,
                equitable: true,
                pareto_optimal: false,
                explanation: "$900 refund splits the difference on the carpet question. \
                    Item allocation trades standing desk for bookshelf to approximate equity \
                    in points. Neither party envies the other's cash; a reasonable compromise."
                    .to_string(),
            },
        ],
    };

    let state = AppState::new(dispute, analysis);
    let app = router(state);

    let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    println!("mediator-web listening on http://{}", addr);
    println!("  Landing:  http://{}/", addr);
    println!("  Robin:    http://{}/party/robin", addr);
    println!("  Sam:      http://{}/party/sam", addr);
    println!("  Operator: http://{}/operator", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
