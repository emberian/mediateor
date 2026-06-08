//! `mediator-web` — a simple, delightful, minimalist **htmx** front end.
//!
//! Server-rendered HTML via `maud` + tiny htmx interactions.
//! Two faces of one `Analysis`, same as the TUI:
//!   - A kind, warm party view (for Robin or Sam)
//!   - An operator cockpit (raw facts, hashes, crux status)
//!
//! No build step, no SPA. Just warm pages a frightened roommate could love.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    routing::{get, post},
    response::IntoResponse,
    http::StatusCode,
};
use maud::{html, Markup, DOCTYPE, PreEscaped};
use mediator_types::{Analysis, Dispute, Settlement};
use tokio::sync::RwLock;

// ──────────────────────────────── AppState ───────────────────────────────────

/// What the server renders from. Decoupled from the kernel.
/// The real pipeline hands a freshly computed `Analysis` in at startup.
pub struct AppState {
    pub dispute: Dispute,
    pub analysis: Analysis,
    /// Settlement acceptances: index → set of party ids who said "ok".
    pub accepted: RwLock<Vec<std::collections::HashSet<String>>>,
}

impl AppState {
    pub fn new(dispute: Dispute, analysis: Analysis) -> Self {
        let n = analysis.settlements.len();
        Self {
            dispute,
            analysis,
            accepted: RwLock::new(vec![std::collections::HashSet::new(); n]),
        }
    }
}

type SharedState = Arc<AppState>;

// ──────────────────────────────── router ─────────────────────────────────────

/// Build the axum router. The static directory is resolved at call time using
/// `CARGO_MANIFEST_DIR` (set at compile time) so the binary can locate
/// `static/htmx.min.js` regardless of the working directory.
pub fn router(state: AppState) -> Router {
    let shared = Arc::new(state);

    // Path to the `static/` directory, baked in at compile time.
    let static_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/static");

    Router::new()
        .route("/", get(landing))
        .route("/party/{id}", get(party_view))
        .route("/operator", get(operator_view))
        .route("/settlement/{idx}/accept", post(settlement_accept))
        .nest_service("/static", tower_http::services::ServeDir::new(static_dir))
        .with_state(shared)
}

// ─────────────────────────── shared page chrome ──────────────────────────────

const CSS: &str = r#"
:root {
  --bg:      #faf9f7;
  --surface: #ffffff;
  --border:  #e8e4df;
  --muted:   #8a8278;
  --text:    #2c2925;
  --accent:  #5b7fa6;
  --green:   #4a7c59;
  --amber:   #8c6d2f;
  --danger:  #8c3a2f;
  --radius:  10px;
  --shadow:  0 1px 4px rgba(0,0,0,.07);
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: -apple-system, "Segoe UI", Helvetica, Arial, sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.65;
  font-size: 1rem;
  padding: 2rem 1rem;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
.container { max-width: 760px; margin: 0 auto; }
header { margin-bottom: 2.5rem; }
header h1 { font-size: 1.4rem; font-weight: 600; letter-spacing: -.01em; }
header .subtitle { color: var(--muted); margin-top: .3rem; font-size: .95rem; }
nav { margin-top: .8rem; font-size: .9rem; display: flex; gap: 1.2rem; }
section { margin-bottom: 2rem; }
section h2 {
  font-size: 1rem; font-weight: 600; color: var(--muted);
  text-transform: uppercase; letter-spacing: .07em;
  margin-bottom: .8rem;
}
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 1.1rem 1.3rem;
  box-shadow: var(--shadow);
  margin-bottom: .8rem;
}
.card + .card { margin-top: .6rem; }
.pill {
  display: inline-block;
  padding: .15rem .55rem;
  border-radius: 99px;
  font-size: .8rem;
  font-weight: 500;
  margin-left: .4rem;
  vertical-align: middle;
}
.pill-green  { background: #dff0e5; color: var(--green); }
.pill-amber  { background: #f5ecda; color: var(--amber); }
.pill-red    { background: #f5e0de; color: var(--danger); }
.pill-blue   { background: #dce8f5; color: var(--accent); }
.pill-grey   { background: #eeebe7; color: var(--muted);  }
.amount { font-size: 1.5rem; font-weight: 700; color: var(--text); }
.amount-note { font-size: .85rem; color: var(--muted); margin-top: .2rem; }
.two-doors {
  display: grid; grid-template-columns: 1fr 1fr; gap: 1rem;
  margin-top: 1.5rem;
}
.door {
  display: block; padding: 1.4rem 1.2rem;
  background: var(--surface); border: 1.5px solid var(--border);
  border-radius: var(--radius); box-shadow: var(--shadow);
  text-align: center; transition: border-color .15s;
}
.door:hover { border-color: var(--accent); text-decoration: none; }
.door .door-name { font-weight: 600; font-size: 1.05rem; }
.door .door-hint { font-size: .85rem; color: var(--muted); margin-top: .25rem; }
.operator-door {
  display: block; padding: .8rem 1.2rem;
  background: #f2f0ec; border: 1px solid var(--border);
  border-radius: var(--radius); margin-top: .8rem;
  font-size: .9rem; color: var(--muted); text-align: center;
}
.operator-door:hover { color: var(--text); }
.hash { font-family: "SFMono-Regular", "Menlo", monospace; font-size: .78rem; color: var(--muted); word-break: break-all; }
.mono { font-family: "SFMono-Regular", "Menlo", monospace; }
.accept-btn {
  margin-top: .8rem;
  padding: .45rem 1rem;
  background: var(--accent); color: #fff;
  border: none; border-radius: 6px;
  font-size: .88rem; cursor: pointer;
  transition: background .12s;
}
.accept-btn:hover { background: #4a6e92; }
.htmx-indicator { color: var(--muted); font-size: .82rem; margin-left: .5rem; }
hr.divider { border: none; border-top: 1px solid var(--border); margin: 1.5rem 0; }
ul.plain { list-style: none; }
ul.plain li { padding: .2rem 0; }
ul.plain li::before { content: "· "; color: var(--muted); }
.crux-box {
  border: 1.5px solid var(--amber);
  border-radius: var(--radius);
  padding: 1rem 1.2rem;
  background: #fffdf5;
}
.crux-box p { margin-top: .4rem; font-size: .92rem; }
"#;

fn page(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — trusted mediator" }
                style { (PreEscaped(CSS)) }
                script src="/static/htmx.min.js" defer {}
            }
            body {
                div .container {
                    (body)
                }
            }
        }
    }
}

fn nav_links(current: &str) -> Markup {
    html! {
        nav {
            @if current != "home" { a href="/" { "← Home" } }
            @if current != "robin" { a href="/party/robin" { "Robin's view" } }
            @if current != "sam"   { a href="/party/sam"   { "Sam's view" }   }
            @if current != "op"    { a href="/operator"    { "Operator" }      }
        }
    }
}

// ─────────────────────────────── landing page ────────────────────────────────

async fn landing(State(state): State<SharedState>) -> Markup {
    let d = &state.dispute;
    let party_links: Vec<Markup> = d.parties.iter().map(|p| {
        html! {
            a .door href=(format!("/party/{}", p.id)) {
                div .door-name { (&p.display_name) }
                div .door-hint { "See the situation from this perspective" }
            }
        }
    }).collect();

    page(&d.title, html! {
        header {
            h1 { (&d.title) }
            p .subtitle {
                "This is a space for working through a disagreement with help. "
                "The facts that can be verified have been, and the ones that cannot "
                "are named honestly. You are not alone in this."
            }
            (nav_links("home"))
        }

        section {
            h2 { "Choose a perspective" }
            div .two-doors {
                @for link in &party_links {
                    (link)
                }
            }
        }

        section {
            a .operator-door href="/operator" {
                "Operator / mediator view (all conflicts, ledger, receipts)"
            }
        }
    })
}

// ─────────────────────────────── party view ──────────────────────────────────

async fn party_view(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let d = &state.dispute;
    let a = &state.analysis;

    // Find the party — return 404 with a kind message if unknown.
    let Some(party) = d.parties.iter().find(|p| p.id == id) else {
        let body = page("Not found", html! {
            header { h1 { "Party not found" } (nav_links("")) }
            p { "We don't have a record of a party with that id. " a href="/" { "Go home." } }
        });
        return (StatusCode::NOT_FOUND, body).into_response();
    };

    let accepted = state.accepted.read().await;
    let title = format!("{}'s view", party.display_name);

    let markup = page(&title, html! {
        header {
            h1 { (&party.display_name) }
            p .subtitle { "Here is where things stand — as clearly and kindly as we can put them." }
            (nav_links(&id))
        }

        // ── Shared ground ────────────────────────────────────────────────
        @if !a.shared_core.is_empty() {
            section {
                h2 { "What you both agree on" }
                div .card {
                    ul .plain {
                        @for fact in &a.shared_core {
                            li { (fact) }
                        }
                    }
                }
            }
        }

        // ── The one knot ─────────────────────────────────────────────────
        @if let Some(crux) = &a.crux {
            section {
                h2 { "The question that everything turns on" }
                div .crux-box {
                    p { (crux) }
                    p style="margin-top:.6rem;font-size:.85rem;color:#8a8278;" {
                        "The mediator has confirmed this is the genuine crux. "
                        "Everything else has been resolved or dissolved."
                    }
                }
            }
        }

        // ── Dissolved misunderstandings ───────────────────────────────────
        @if !a.dissolved.is_empty() {
            section {
                h2 { "Things that turned out not to be disagreements" }
                div .card {
                    ul .plain {
                        @for item in &a.dissolved {
                            li { (item) }
                        }
                    }
                }
            }
        }

        // ── Ledger ───────────────────────────────────────────────────────
        section {
            h2 { "The money" }
            @if let Some(refund_cents) = a.ledger_refund_cents {
                div .card {
                    div .amount { (format!("${:.2}", refund_cents as f64 / 100.0)) }
                    div .amount-note { "certified minimum refund (ledger proven)" }
                }
            }
            @if !a.ledger_findings.is_empty() {
                div .card {
                    ul .plain {
                        @for finding in &a.ledger_findings {
                            li { (finding) }
                        }
                    }
                }
            }
            @if a.ledger_refund_cents.is_none() && a.ledger_findings.is_empty() {
                div .card {
                    p style="color:#8a8278;" { "Ledger analysis pending." }
                }
            }
        }

        // ── Settlement options ─────────────────────────────────────────
        @if !a.settlements.is_empty() {
            section {
                h2 { "Fair settlement options" }
                p style="font-size:.88rem;color:#8a8278;margin-bottom:1rem;" {
                    "Each option below has been certified for fairness. "
                    "You can mark one as acceptable — the other party will see your answer."
                }
                @for (idx, s) in a.settlements.iter().enumerate() {
                    (settlement_card(idx, s, &accepted[idx], &id))
                }
            }
        }
    });

    (StatusCode::OK, markup).into_response()
}

fn settlement_card(
    idx: usize,
    s: &Settlement,
    accepted_by: &std::collections::HashSet<String>,
    viewer: &str,
) -> Markup {
    let already_accepted = accepted_by.contains(viewer);
    let all_accepted = accepted_by.len() >= 2;

    html! {
        div .card id=(format!("settlement-{}", idx)) {
            div style="display:flex;align-items:baseline;gap:.6rem;flex-wrap:wrap;" {
                strong { (&s.label) }
                @if s.envy_free    { span .pill.pill-green  { "envy-free" }    }
                @if s.equitable    { span .pill.pill-green  { "equitable" }    }
                @if s.pareto_optimal { span .pill.pill-blue { "Pareto" }       }
            }
            p style="margin-top:.5rem;font-size:.9rem;" { (&s.explanation) }

            @if !s.allocations.is_empty() {
                div style="margin-top:.6rem;font-size:.85rem;color:#5c5752;" {
                    @for (item_id, party_id) in &s.allocations {
                        span style="margin-right:.8rem;" {
                            strong { (item_id) } " → " (party_id)
                        }
                    }
                }
            }

            @if !already_accepted && !all_accepted {
                form {
                    button
                        .accept-btn
                        hx-post=(format!("/settlement/{}/accept", idx))
                        hx-vals=(format!("{{\"party\":\"{}\"}}", viewer))
                        hx-target=(format!("#settlement-{}", idx))
                        hx-swap="outerHTML"
                        { "Mark as acceptable" }
                    span .htmx-indicator { "..." }
                }
            } @else if all_accepted {
                p style="margin-top:.7rem;color:#4a7c59;font-weight:600;" {
                    "✓ Both parties have accepted this option."
                }
            } @else {
                p style="margin-top:.7rem;color:#5b7fa6;" {
                    "Marked as acceptable — waiting on the other party."
                }
            }
        }
    }
}

// ────────────────────────────── operator view ────────────────────────────────

async fn operator_view(State(state): State<SharedState>) -> Markup {
    let d = &state.dispute;
    let a = &state.analysis;

    page("Operator cockpit", html! {
        header {
            h1 { "Operator cockpit" }
            p .subtitle { "All conflicts, ledger findings, crux status, and receipt hashes." }
            (nav_links("op"))
        }

        // ── Crux status ─────────────────────────────────────────────────
        section {
            h2 { "Crux" }
            @match &a.crux {
                Some(crux) => {
                    div .card {
                        span .pill.pill-amber { "ISOLATED" }
                        " "
                        (crux)
                    }
                }
                None => {
                    div .card {
                        span .pill.pill-grey { "UNKNOWN" }
                        " Crux not yet isolated."
                    }
                }
            }
        }

        // ── Genuine conflicts ───────────────────────────────────────────
        section {
            h2 { (format!("Conflicts ({})", a.genuine_conflicts.len())) }
            @if a.genuine_conflicts.is_empty() {
                div .card { p style="color:#8a8278;" { "No conflicts on record." } }
            } @else {
                @for c in &a.genuine_conflicts {
                    div .card {
                        div style="display:flex;gap:.5rem;align-items:center;flex-wrap:wrap;" {
                            strong { (&c.description) }
                            @for pid in &c.parties {
                                span .pill.pill-blue { (pid) }
                            }
                        }
                        @if !c.claim_ids.is_empty() {
                            div style="margin-top:.4rem;font-size:.82rem;color:#8a8278;" {
                                "Claims: "
                                @for cid in &c.claim_ids {
                                    span .mono style="margin-right:.4rem;" { (cid) }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Ledger findings ─────────────────────────────────────────────
        section {
            h2 { "Ledger findings" }
            div .card {
                @if a.ledger_findings.is_empty() {
                    p style="color:#8a8278;" { "No findings." }
                } @else {
                    ul .plain {
                        @for f in &a.ledger_findings {
                            li { (f) }
                        }
                    }
                }
                @if let Some(r) = a.ledger_refund_cents {
                    hr .divider;
                    p {
                        span .pill.pill-green { "PROVED" }
                        " Minimum refund: "
                        strong { (format!("${:.2}", r as f64 / 100.0)) }
                    }
                }
            }
        }

        // ── Dispute claims ──────────────────────────────────────────────
        section {
            h2 { "Claims" }
            @for claim in &d.claims {
                div .card {
                    div style="display:flex;gap:.5rem;align-items:center;flex-wrap:wrap;" {
                        span .mono { (&claim.id) }
                        span .pill.pill-blue { (&claim.party) }
                        @if claim.defeasible { span .pill.pill-amber { "defeasible" } }
                        @if !claim.active  { span .pill.pill-grey  { "inactive" } }
                    }
                    p style="margin-top:.4rem;" { (&claim.nl) }
                    p style="margin-top:.2rem;font-size:.82rem;color:#8a8278;" {
                        em { (&claim.english_render) }
                    }
                }
            }
        }

        // ── Dissolved items ─────────────────────────────────────────────
        @if !a.dissolved.is_empty() {
            section {
                h2 { "Dissolved (vocabulary, not substance)" }
                div .card {
                    ul .plain {
                        @for item in &a.dissolved {
                            li { (item) }
                        }
                    }
                }
            }
        }

        // ── Ledger items ────────────────────────────────────────────────
        section {
            h2 { "Ledger" }
            div .card {
                p {
                    "Deposit: "
                    strong { (format!("${:.2}", d.ledger.deposit_cents as f64 / 100.0)) }
                }
                hr .divider;
                @for item in &d.ledger.items {
                    div style="display:flex;justify-content:space-between;padding:.25rem 0;border-bottom:1px solid #e8e4df;" {
                        span {
                            (&item.label)
                            @if item.disputed { span .pill.pill-amber { "disputed" } }
                        }
                        span .mono { (format!("${:.2}", item.amount_cents as f64 / 100.0)) }
                    }
                }
            }
        }

        // ── Settlements ─────────────────────────────────────────────────
        @if !a.settlements.is_empty() {
            section {
                h2 { "Settlement options" }
                @for s in &a.settlements {
                    div .card {
                        div style="display:flex;gap:.5rem;align-items:baseline;flex-wrap:wrap;" {
                            strong { (&s.label) }
                            @if s.envy_free    { span .pill.pill-green { "envy-free" }  }
                            @if s.equitable    { span .pill.pill-green { "equitable" }  }
                            @if s.pareto_optimal { span .pill.pill-blue { "Pareto" }    }
                        }
                        p style="margin-top:.4rem;font-size:.9rem;" { (&s.explanation) }
                        @for (pid, pts) in &s.party_points {
                            span style="font-size:.82rem;margin-right:.8rem;color:#5c5752;" {
                                (pid) ": " (format!("{:.1} pts", pts))
                            }
                        }
                    }
                }
            }
        }
    })
}

// ──────────────────────── htmx: settlement accept ────────────────────────────

/// Receives a party's acceptance of a settlement option.
/// Returns an HTML fragment that replaces the card in place.
async fn settlement_accept(
    Path(idx): Path<usize>,
    State(state): State<SharedState>,
    axum::Form(form): axum::Form<AcceptForm>,
) -> impl IntoResponse {
    // Bounds check.
    if idx >= state.analysis.settlements.len() {
        return (StatusCode::BAD_REQUEST, html! { p { "Invalid settlement index." } })
            .into_response();
    }

    // Record the acceptance.
    {
        let mut accepted = state.accepted.write().await;
        accepted[idx].insert(form.party.clone());
    }

    let accepted = state.accepted.read().await;
    let s = &state.analysis.settlements[idx];
    let card = settlement_card(idx, s, &accepted[idx], &form.party);
    (StatusCode::OK, card).into_response()
}

#[derive(serde::Deserialize)]
struct AcceptForm {
    party: String,
}
