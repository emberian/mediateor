//! `mediator-web` binary.
//!
//! Discovers every `scenarios/*.json` and its sibling `*.analysis.json` cache,
//! builds an in-memory map of disputes, and serves the htmx front end on
//! 127.0.0.1:3000. The cache is preferred (instant, no Isabelle); a scenario
//! with no cache is computed via the kernel if a prover is present, else
//! skipped gracefully.

use std::net::SocketAddr;

use mediator_web::{AppState, discover_disputes, router, scenarios_dir};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = scenarios_dir();
    let records = discover_disputes(&dir);

    if records.is_empty() {
        eprintln!(
            "⚠  no disputes loaded from {} — the gallery will be empty.\n   \
             (add scenarios/<name>.json and, ideally, run the demo with \
             --write-cache.)",
            dir.display()
        );
    } else {
        println!("loaded {} dispute(s) from {}", records.len(), dir.display());
        for r in &records {
            let cached = !r.receipts.is_empty();
            println!(
                "  · {:<12} {}{}",
                r.id,
                r.dispute.title,
                if cached { "" } else { "  (computed live)" }
            );
        }
    }

    let state = AppState::new(records);
    let app = router(state);

    let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    println!("\n☄  Mediateor — listening on http://{addr}");
    println!("   Gallery:  http://{addr}/");
    println!("   (pick a dispute, then a seat — or watch as the mediator)\n");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
