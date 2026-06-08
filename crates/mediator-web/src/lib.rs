//! `mediator-web` — a simple, delightful, portfolio-quality **htmx** front end.
//!
//! Server-rendered HTML via `maud` + a few tiny htmx interactions. No SPA, no
//! build step. Two faces of one `Analysis`, plus a calm gallery over *every*
//! dispute in `scenarios/`:
//!
//!   - `GET /`                          — the gallery of disputes
//!   - `GET /dispute/:id`               — choose your seat
//!   - `GET /party/:dispute/:party`     — the gentle, guided party reveal
//!   - `GET /operator/:dispute`         — the operator cockpit (provenance)
//!
//! The people in a dispute never see a formula or the word "wrong". The
//! operator sees the certified ledger findings, the crux verdict, and the
//! hash-chained receipt ledger — provenance you can point at.

mod load;
mod theme;

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use mediator_types::{Analysis, Dispute, Formula, Party, Receipt, Settlement, Term};
use tokio::sync::RwLock;

pub use load::{DisputeRecord, discover_disputes, load_record, scenarios_dir};
use theme::CSS;

// ──────────────────────────────── AppState ───────────────────────────────────

/// One loaded dispute and everything the views render from it.
pub struct LoadedDispute {
    pub id: String,
    /// A warm, one-line human framing for the gallery card (sidecar copy,
    /// independent of the kernel's internal text).
    pub blurb: String,
    pub dispute: Dispute,
    pub analysis: Analysis,
    pub receipts: Vec<Receipt>,
    /// Per-settlement acceptances: index → set of party ids who said "ok".
    pub accepted: RwLock<Vec<HashSet<String>>>,
}

impl LoadedDispute {
    pub fn new(rec: DisputeRecord) -> Self {
        let n = rec.analysis.settlements.len();
        let blurb = blurb_for(&rec.id, &rec.dispute);
        Self {
            id: rec.id,
            blurb,
            dispute: rec.dispute,
            analysis: rec.analysis,
            receipts: rec.receipts,
            accepted: RwLock::new(vec![HashSet::new(); n]),
        }
    }

    fn party(&self, id: &str) -> Option<&Party> {
        self.dispute.parties.iter().find(|p| p.id == id)
    }
}

/// What the server renders from. Holds many disputes; decoupled from the kernel.
pub struct AppState {
    pub disputes: Vec<LoadedDispute>,
}

impl AppState {
    pub fn new(records: Vec<DisputeRecord>) -> Self {
        let mut disputes: Vec<LoadedDispute> =
            records.into_iter().map(LoadedDispute::new).collect();
        // Stable, friendly order: the canonical roommate case first, then a-z.
        disputes.sort_by(|a, b| {
            let rank = |id: &str| if id == "roommate" { 0 } else { 1 };
            rank(&a.id)
                .cmp(&rank(&b.id))
                .then_with(|| a.id.cmp(&b.id))
        });
        Self { disputes }
    }

    pub fn get(&self, id: &str) -> Option<&LoadedDispute> {
        self.disputes.iter().find(|d| d.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.disputes.is_empty()
    }
}

type SharedState = Arc<AppState>;

/// A warm, human one-liner for the gallery card. Keyed by scenario id so the
/// copy reads well regardless of the kernel's internal (carpet-flavoured) text;
/// falls back to a gentle generic framing for unknown scenarios.
fn blurb_for(id: &str, dispute: &Dispute) -> String {
    match id {
        "roommate" => {
            "Robin is moving out; Sam holds the $1,200 deposit. A carpet stain \
             and some shared furniture stand between them — and they hate each \
             other, but not that badly."
        }
        "freelance" => {
            "A website handed off, an invoice unpaid. Was the work in scope, or \
             half-finished? One contested milestone, and the project assets to \
             divide."
        }
        "siblings" => {
            "Two siblings sorting through what a parent left behind. One earlier \
             gift, remembered differently — and a houseful of things that each \
             mean more than money."
        }
        _ => return generic_blurb(dispute),
    }
    .to_string()
}

fn generic_blurb(dispute: &Dispute) -> String {
    let names: Vec<&str> = dispute
        .parties
        .iter()
        .map(|p| p.display_name.as_str())
        .collect();
    match names.as_slice() {
        [a, b] => format!("{a} and {b} have something to work through — let's see the shape of it."),
        _ => "A disagreement to work through, gently and in the open.".to_string(),
    }
}

// ──────────────────────────────── router ─────────────────────────────────────

/// Build the axum router over a fully loaded [`AppState`]. The `static/`
/// directory is resolved at compile time via `CARGO_MANIFEST_DIR`, so the
/// binary finds `static/htmx.min.js` regardless of the working directory.
pub fn router(state: AppState) -> Router {
    let shared = Arc::new(state);
    let static_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/static");

    Router::new()
        .route("/", get(gallery))
        .route("/dispute/:id", get(seat_picker))
        .route("/party/:dispute_id/:party_id", get(party_view))
        .route("/operator/:dispute_id", get(operator_view))
        .route(
            "/settlement/:dispute_id/:idx/accept",
            post(settlement_accept),
        )
        .route(
            "/settlement/:dispute_id/:idx/counter",
            post(settlement_counter),
        )
        .nest_service("/static", tower_http::services::ServeDir::new(static_dir))
        .with_state(shared)
}

// ─────────────────────────── shared page chrome ──────────────────────────────

const WORDMARK: &str = "Mediateor ☄";

fn page(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — " (WORDMARK) }
                style { (PreEscaped(CSS)) }
                script src="/static/htmx.min.js" defer {}
            }
            body {
                div .page {
                    (body)
                    footer .site-footer {
                        span { (WORDMARK) }
                        span .dot { "·" }
                        span { "the prover's kindest move is knowing where to stop" }
                    }
                }
            }
        }
    }
}

/// The small wordmark that sits atop every interior page and links home.
fn brandbar(crumb: Option<Markup>) -> Markup {
    html! {
        div .brandbar {
            a .wordmark href="/" { (WORDMARK) }
            @if let Some(c) = crumb {
                span .crumb-sep { "/" }
                span .crumb { (c) }
            }
        }
    }
}

fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let c = cents.unsigned_abs();
    format!("{sign}${}.{:02}", c / 100, c % 100)
}

// ─────────────────────────────── the gallery ─────────────────────────────────

async fn gallery(State(state): State<SharedState>) -> Markup {
    page("Disputes", html! {
        header .hero {
            div .hero-mark { (WORDMARK) }
            h1 .hero-title { "See the true shape of a disagreement." }
            p .hero-lede {
                "A trusted mediator. It doesn't judge — it clears away the parts "
                "that were never really the fight, certifies the few facts that "
                "must not be fudged, and hands back the one question that's "
                "honestly yours to answer."
            }
        }

        @if state.is_empty() {
            section {
                div .card .empty {
                    p { "No disputes are loaded yet." }
                    p .muted {
                        "Add a scenario under " span .mono { "scenarios/" }
                        " and (optionally) its " span .mono { ".analysis.json" }
                        " cache, then restart."
                    }
                }
            }
        } @else {
            section .gallery {
                @for d in &state.disputes {
                    a .case-card href=(format!("/dispute/{}", d.id)) {
                        div .case-eyebrow {
                            @for (i, p) in d.dispute.parties.iter().enumerate() {
                                @if i > 0 { span .vs { "vs" } }
                                span .case-party { (party_first_name(&p.display_name)) }
                            }
                        }
                        h2 .case-title { (&d.dispute.title) }
                        p .case-blurb { (&d.blurb) }
                        div .case-foot {
                            @if d.analysis.crux.is_some() {
                                span .chip .chip-amber { "1 open question" }
                            }
                            @if !d.analysis.dissolved.is_empty() {
                                span .chip .chip-green { "a misunderstanding cleared" }
                            }
                            @if d.analysis.ledger_refund_cents.is_some() {
                                span .chip .chip-blue { "ledger certified" }
                            }
                            span .case-go { "open →" }
                        }
                    }
                }
            }
        }
    })
}

/// "Robin (moving out)" → "Robin"; keeps a clean party chip.
fn party_first_name(display: &str) -> String {
    display
        .split([' ', '(', ','])
        .next()
        .unwrap_or(display)
        .trim()
        .to_string()
}

// ──────────────────────────── the seat picker ────────────────────────────────

async fn seat_picker(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&id) else {
        return not_found("We don't have a record of that dispute.");
    };

    let markup = page(&d.dispute.title, html! {
        (brandbar(Some(html! { (&d.dispute.title) })))

        header .interior-head {
            h1 { "Choose your seat" }
            p .lede {
                "How you read this depends on where you sit. Pick a seat — you "
                "can switch any time. Nothing here is a verdict; it's a way to "
                "see the disagreement clearly."
            }
        }

        section .seats {
            @for p in &d.dispute.parties {
                a .seat href=(format!("/party/{}/{}", d.id, p.id)) {
                    span .seat-i { "I'm " (party_first_name(&p.display_name)) }
                    span .seat-sub { (party_role(&p.display_name)) }
                    span .seat-hint { "A gentle, guided walk-through of where things stand for you." }
                }
            }
        }

        section {
            a .seat .seat-operator href=(format!("/operator/{}", d.id)) {
                span .seat-i { "Watch as the mediator" }
                span .seat-sub { "operator · the full picture" }
                span .seat-hint {
                    "The cockpit: certified ledger findings, the crux verdict, the "
                    "settlement table, and the hash-chained receipt ledger."
                }
            }
        }
    });
    (StatusCode::OK, markup).into_response()
}

/// "Robin (moving out)" → "moving out"; the parenthetical, lightly cased.
fn party_role(display: &str) -> String {
    if let (Some(a), Some(b)) = (display.find('('), display.find(')')) {
        if b > a + 1 {
            return display[a + 1..b].to_string();
        }
    }
    "in this dispute".to_string()
}

// ─────────────────────────────── party view ──────────────────────────────────
//
// A gentle, progressively-revealed scrollytelling walk. Sections fade/slide in
// as they enter the viewport (pure CSS + a touch of inline JS; htmx for the
// settlement interactions). Never a formula, never the word "wrong".

async fn party_view(
    Path((dispute_id, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&dispute_id) else {
        return not_found("We don't have a record of that dispute.");
    };
    let Some(party) = d.party(&party_id) else {
        return not_found("We don't have a record of a party with that id.");
    };
    let a = &d.analysis;
    let accepted = d.accepted.read().await;
    let you = party_first_name(&party.display_name);
    let other = d
        .dispute
        .parties
        .iter()
        .find(|p| p.id != party_id)
        .map(|p| party_first_name(&p.display_name))
        .unwrap_or_else(|| "the other party".to_string());

    let markup = page(&format!("{you}'s view"), html! {
        (brandbar(Some(html! { a href=(format!("/dispute/{}", d.id)) { (&d.dispute.title) } })))

        header .interior-head .party-head {
            p .eyebrow { "For " (you) }
            h1 { "Let's walk through this, gently." }
            p .lede {
                "Take it one step at a time. We'll start with what you already "
                "agree on — it's usually more than it feels like — and end with a "
                "few fair ways forward."
            }
            p .scroll-cue { "scroll ↓" }
        }

        // (1) What you already agree on.
        @if !a.shared_core.is_empty() {
            section .reveal .step {
                span .step-n { "1" }
                h2 { "What you already agree on" }
                p .step-lede { "The ground you share. None of this is in question." }
                div .card .soft {
                    ul .plain {
                        @for fact in &a.shared_core { li { (humanize(fact)) } }
                    }
                }
            }
        }

        // (2) Things that were just different words.
        @if !a.dissolved.is_empty() {
            section .reveal .step {
                span .step-n { "2" }
                h2 { "Things that turned out not to be disagreements" }
                p .step-lede {
                    "These looked like fights but were just different words, or a "
                    "number remembered roughly. Cleared, kindly — no fault in it."
                }
                @for item in &a.dissolved {
                    div .card .dissolved {
                        span .dissolved-mark { "✓" }
                        span { (humanize(item)) }
                    }
                }
            }
        }

        // (3) The one open question — the crux, phrased kindly.
        @if let Some(crux) = &a.crux {
            section .reveal .step {
                span .step-n { "3" }
                h2 { "The one question that's really yours" }
                p .step-lede {
                    "Everything else has been settled or set aside. This is the "
                    "single thing left — and it's not ours to decide. It's a "
                    "judgement only the two of you can make."
                }
                div .crux-box {
                    p .crux-q { (crux_question(d, crux)) }
                    p .crux-note {
                        "We've confirmed this is the genuine crux: answer it, and "
                        "the numbers below follow on their own."
                    }
                }
            }
        }

        // (4) What the numbers show — certified, with the refund range.
        section .reveal .step {
            span .step-n { "4" }
            h2 { "What the numbers show" }
            p .step-lede {
                "Checked carefully, so no one has to take anyone's word for it."
            }
            @if let Some(refund) = a.ledger_refund_cents {
                div .card .figure {
                    div .figure-amount { (money(refund)) }
                    div .figure-note {
                        "the amount that's settled either way — the rest depends on "
                        "the one open question above"
                    }
                }
            }
            @if !a.ledger_findings.is_empty() {
                div .card .soft {
                    ul .plain {
                        @for f in &a.ledger_findings { li { (humanize(f)) } }
                    }
                }
            }
            @if a.ledger_refund_cents.is_none() && a.ledger_findings.is_empty() {
                div .card .soft { p .muted { "The money side is still being worked out." } }
            }
        }

        // (5) Fair ways forward — settlement cards.
        @if !a.settlements.is_empty() {
            section .reveal .step {
                span .step-n { "5" }
                h2 { "A few fair ways forward" }
                p .step-lede {
                    "Each option splits the shared things so neither of you would "
                    "rather have the other's share. Mark any that you'd be okay "
                    "with — " (other) " will see your answer, never your reasons."
                }
                @for (idx, s) in a.settlements.iter().enumerate() {
                    (settlement_card(&d.id, idx, s, &accepted[idx], &party_id, &you))
                }
            }
        }

        section .reveal .closing {
            p {
                "That's the whole shape of it. Not a winner and a loser — just the "
                "smallest world that holds you both, and the one honest question in "
                "the middle of it."
            }
            a .ghost-link href=(format!("/dispute/{}", d.id)) { "← back to seats" }
        }

        (reveal_script())
    });
    (StatusCode::OK, markup).into_response()
}

/// A small bit of JS that adds `.in` to `.reveal` sections as they scroll into
/// view (progressive reveal). Degrades gracefully: if JS is off, a CSS fallback
/// shows everything.
fn reveal_script() -> Markup {
    html! {
        script {
            (PreEscaped(r#"
            (function () {
              var els = document.querySelectorAll('.reveal');
              if (!('IntersectionObserver' in window)) {
                els.forEach(function (e) { e.classList.add('in'); });
                return;
              }
              var io = new IntersectionObserver(function (entries) {
                entries.forEach(function (en) {
                  if (en.isIntersecting) { en.target.classList.add('in'); io.unobserve(en.target); }
                });
              }, { threshold: 0.12 });
              els.forEach(function (e) { io.observe(e); });
            })();
            "#))
        }
    }
}

/// Phrase the crux as a kind question, derived from the *dispute's own
/// structure* rather than the kernel's crux prose.
///
/// The kernel renders the crux in a fixed (carpet-flavoured) idiom that is only
/// right for the roommate case. But every dispute carries a clean, faithful
/// source of the contested predicate: the stipulated bridge
/// `Iff(consequence, crux_predicate)`, whose predicate has a plain-English
/// `gloss` in some party's signature. We build the question from that.
///
/// Order of preference:
///   1. the crux predicate's signature gloss → "Is it true that {gloss}?"
///   2. a "Whether …" genuine-conflict description, if present
///   3. the kernel's crux sentence (last resort)
fn crux_question(d: &LoadedDispute, kernel_crux: &str) -> String {
    if let Some(gloss) = crux_gloss(&d.dispute) {
        let g = gloss.trim().trim_end_matches('.');
        return format!("Is it true that {g}?");
    }
    if let Some(c) = d.analysis.genuine_conflicts.first() {
        let desc = c.description.trim();
        if let Some(rest) = desc.strip_prefix("Whether ") {
            let core = rest.split(" — ").next().unwrap_or(rest).trim();
            return format!("Is it true that {core}?");
        }
        if !desc.is_empty() {
            return desc.split(" — ").next().unwrap_or(desc).trim().to_string();
        }
    }
    kernel_crux.to_string()
}

/// Find the gloss of the crux predicate. The crux is the right-hand atom of a
/// stipulated `Iff(consequence, crux)`; look its symbol up in the parties'
/// signatures and return that symbol's human gloss.
fn crux_gloss(dispute: &Dispute) -> Option<String> {
    let crux_sym = dispute.stipulated.iter().find_map(crux_symbol)?;
    dispute
        .parties
        .iter()
        .flat_map(|p| &p.signature)
        .find(|s| s.name == crux_sym && !s.gloss.trim().is_empty())
        .map(|s| s.gloss.clone())
}

/// From a stipulated `Iff(_, Atom(App(sym, [])))`, pull `sym` — the predicate
/// the whole obligation reduces to.
fn crux_symbol(f: &Formula) -> Option<String> {
    let Formula::Iff(_, rhs) = f else { return None };
    match rhs.as_ref() {
        Formula::Atom(Term::App(sym, args)) if args.is_empty() => Some(sym.clone()),
        _ => None,
    }
}

fn settlement_card(
    dispute_id: &str,
    idx: usize,
    s: &Settlement,
    accepted_by: &HashSet<String>,
    viewer: &str,
    viewer_name: &str,
) -> Markup {
    let already = accepted_by.contains(viewer);
    let all = accepted_by.len() >= 2;

    html! {
        div .card .settlement id=(format!("settlement-{idx}")) {
            div .settlement-head {
                strong { "Option " (idx + 1) }
                @if s.envy_free { span .chip .chip-green { "envy-free" } }
                @if s.equitable { span .chip .chip-green { "equitable" } }
                @if s.pareto_optimal { span .chip .chip-blue { "no waste" } }
            }
            p .settlement-text { (humanize_settlement(&s.explanation)) }

            @if !s.allocations.is_empty() || !s.splits.is_empty() {
                ul .alloc {
                    @for (item_id, party_id) in &s.allocations {
                        li { span .alloc-item { (prettify_id(item_id)) } span .alloc-arrow { "→" } span .alloc-who { (prettify_id(party_id)) } }
                    }
                    @for (item_id, frac) in &s.splits {
                        li { span .alloc-item { (prettify_id(item_id)) } span .alloc-arrow { "→" } span .alloc-who { "shared (" (format!("{:.0}%", frac * 100.0)) " / " (format!("{:.0}%", (1.0 - frac) * 100.0)) ")" } }
                    }
                }
            }

            (settlement_actions(dispute_id, idx, viewer, viewer_name, already, all))
        }
    }
}

/// The action row of a settlement card — the only part that changes after an
/// htmx round-trip, but we re-emit the whole card so the swap is self-contained.
fn settlement_actions(
    dispute_id: &str,
    idx: usize,
    viewer: &str,
    viewer_name: &str,
    already: bool,
    all: bool,
) -> Markup {
    html! {
        @if all {
            p .settle-done { "✓ You both find this acceptable." }
        } @else if already {
            p .settle-waiting { "Marked acceptable — waiting on the other party ✓" }
        } @else {
            div .settle-actions {
                button .btn .btn-accept
                    hx-post=(format!("/settlement/{dispute_id}/{idx}/accept"))
                    hx-vals=(format!("{{\"party\":\"{viewer}\",\"name\":\"{viewer_name}\"}}"))
                    hx-target=(format!("#settlement-{idx}"))
                    hx-swap="outerHTML"
                    { "Mark acceptable" }
                button .btn .btn-counter
                    hx-post=(format!("/settlement/{dispute_id}/{idx}/counter"))
                    hx-vals=(format!("{{\"name\":\"{viewer_name}\"}}"))
                    hx-target=(format!("#settlement-{idx}"))
                    hx-swap="outerHTML"
                    { "I'd like to counter" }
                span .htmx-indicator { "…" }
            }
        }
    }
}

// ────────────────────────────── operator view ────────────────────────────────

async fn operator_view(
    Path(dispute_id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&dispute_id) else {
        return not_found("We don't have a record of that dispute.");
    };
    let a = &d.analysis;
    let disp = &d.dispute;

    let markup = page("Operator cockpit", html! {
        (brandbar(Some(html! { a href=(format!("/dispute/{}", d.id)) { (&disp.title) } " · operator" })))

        header .interior-head {
            h1 { "Operator cockpit" }
            p .lede {
                "The full picture: genuine conflicts, the crux verdict, certified "
                "ledger findings, the settlement table, and the receipt chain. "
                "Everything here is provenance you can point at."
            }
        }

        // ── Crux ─────────────────────────────────────────────────────────
        section {
            h2 .op-h { "Crux" }
            @match &a.crux {
                Some(crux) => div .card {
                    div .op-row { span .pill .pill-amber { "ISOLATED" } span .pill .pill-grey { "verdict: Unknown (by design)" } }
                    p .op-crux { (crux) }
                    p .muted .small { "The kernel proves the whole question reduces here, then refuses to decide it — that refusal is the honest answer." }
                },
                None => div .card { span .pill .pill-grey { "NOT ISOLATED" } " No single crux on record." },
            }
        }

        // ── Genuine conflicts ────────────────────────────────────────────
        section {
            h2 .op-h { "Genuine conflicts (" (a.genuine_conflicts.len()) ")" }
            @if a.genuine_conflicts.is_empty() {
                div .card { p .muted { "No genuine conflicts on record." } }
            } @else {
                @for c in &a.genuine_conflicts {
                    div .card {
                        div .op-row { strong { (&c.description) } }
                        div .op-meta {
                            @for pid in &c.parties { span .pill .pill-blue { (pid) } }
                            @if !c.claim_ids.is_empty() {
                                span .op-claims {
                                    "claims: "
                                    @for cid in &c.claim_ids { span .mono { (cid) } " " }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Certified ledger findings ────────────────────────────────────
        section {
            h2 .op-h { "Certified ledger" }
            div .card {
                @if a.ledger_findings.is_empty() {
                    p .muted { "No findings." }
                } @else {
                    ul .plain { @for f in &a.ledger_findings { li { (f) } } }
                }
                @if let Some(r) = a.ledger_refund_cents {
                    hr .divider;
                    p { span .pill .pill-green { "PROVED" } " Settled-either-way refund: " strong { (money(r)) } }
                }
            }
        }

        // ── Settlement table ─────────────────────────────────────────────
        @if !a.settlements.is_empty() {
            section {
                h2 .op-h { "Settlement options" }
                div .card .tablewrap {
                    table .op-table {
                        thead {
                            tr {
                                th { "option" }
                                @for p in &disp.parties { th .num { (party_first_name(&p.display_name)) " pts" } }
                                th { "envy-free" } th { "equitable" } th { "Pareto" }
                            }
                        }
                        tbody {
                            @for s in &a.settlements {
                                tr {
                                    td { (&s.label) }
                                    @for p in &disp.parties {
                                        td .num {
                                            @let pts = s.party_points.iter().find(|(id, _)| id == &p.id).map(|(_, v)| *v);
                                            @match pts { Some(v) => (format!("{v:.1}")), None => "—" }
                                        }
                                    }
                                    td { (yesno(s.envy_free)) }
                                    td { (yesno(s.equitable)) }
                                    td { (yesno(s.pareto_optimal)) }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Claims ───────────────────────────────────────────────────────
        section {
            h2 .op-h { "Claims" }
            @for claim in &disp.claims {
                div .card {
                    div .op-meta {
                        span .mono { (&claim.id) }
                        span .pill .pill-blue { (&claim.party) }
                        @if claim.defeasible { span .pill .pill-amber { "defeasible" } }
                        @if !claim.active { span .pill .pill-grey { "inactive" } }
                        span .muted .small { "weight " (claim.weight) }
                    }
                    p .op-claim-nl { (&claim.nl) }
                    p .muted .small { em { (&claim.english_render) } }
                }
            }
        }

        // ── Ledger ───────────────────────────────────────────────────────
        section {
            h2 .op-h { "Ledger" }
            div .card {
                p { "Deposit held: " strong { (money(disp.ledger.deposit_cents)) } }
                hr .divider;
                @for item in &disp.ledger.items {
                    div .ledger-line {
                        span { (&item.label) @if item.disputed { span .pill .pill-amber { "disputed" } } }
                        span .mono { (money(item.amount_cents)) }
                    }
                }
            }
        }

        // ── Receipt chain ────────────────────────────────────────────────
        section {
            h2 .op-h { "Receipt ledger (" (d.receipts.len()) " links, hash-chained)" }
            div .card .tablewrap {
                table .op-table .receipts {
                    thead { tr { th { "seq" } th { "op" } th { "hash" } th { "verdict" } } }
                    tbody {
                        @for r in &d.receipts {
                            tr {
                                td .num { (r.seq) }
                                td .mono { (&r.op) }
                                td .mono .hash { (short_hash(&r.hash)) }
                                td { (verdict_pill(&r.verdict)) }
                            }
                        }
                    }
                }
                p .muted .small .chain-note { "Each link's hash folds in the one before it; tamper with any row and the chain breaks." }
            }
        }
    });
    (StatusCode::OK, markup).into_response()
}

fn yesno(b: bool) -> Markup {
    if b {
        html! { span .yes { "✓" } }
    } else {
        html! { span .no { "—" } }
    }
}

fn short_hash(h: &str) -> String {
    if h.len() >= 12 {
        format!("{}…", &h[..12])
    } else if h.is_empty() {
        "—".to_string()
    } else {
        h.to_string()
    }
}

fn verdict_pill(v: &Option<mediator_types::Verdict>) -> Markup {
    use mediator_types::Verdict::*;
    match v {
        Some(Proved) => html! { span .pill .pill-green { "Proved" } },
        Some(Refuted) => html! { span .pill .pill-red { "Refuted" } },
        Some(Unknown) => html! { span .pill .pill-grey { "Unknown" } },
        Some(Error(e)) => html! { span .pill .pill-red { "Error" } span .muted .small { " " (e) } },
        None => html! { span .muted { "—" } },
    }
}

// ──────────────────────── htmx: settlement actions ────────────────────────────

#[derive(serde::Deserialize)]
struct AcceptForm {
    party: String,
    #[serde(default)]
    name: String,
}

async fn settlement_accept(
    Path((dispute_id, idx)): Path<(String, usize)>,
    State(state): State<SharedState>,
    axum::Form(form): axum::Form<AcceptForm>,
) -> impl IntoResponse {
    let Some(d) = state.get(&dispute_id) else {
        return (StatusCode::NOT_FOUND, html! { p { "Unknown dispute." } }).into_response();
    };
    if idx >= d.analysis.settlements.len() {
        return (StatusCode::BAD_REQUEST, html! { p { "Invalid settlement index." } })
            .into_response();
    }
    {
        let mut accepted = d.accepted.write().await;
        accepted[idx].insert(form.party.clone());
    }
    let accepted = d.accepted.read().await;
    let name = if form.name.is_empty() { &form.party } else { &form.name };
    let card = settlement_card(
        &dispute_id,
        idx,
        &d.analysis.settlements[idx],
        &accepted[idx],
        &form.party,
        name,
    );
    (StatusCode::OK, card).into_response()
}

#[derive(serde::Deserialize)]
struct CounterForm {
    #[serde(default)]
    name: String,
}

/// A "counter" affordance: the party signals they'd like to propose a change.
/// We don't (yet) capture a structured counter — we acknowledge it warmly and
/// note the mediator will reach out. Returns a self-contained card fragment.
async fn settlement_counter(
    Path((dispute_id, idx)): Path<(String, usize)>,
    State(state): State<SharedState>,
    axum::Form(form): axum::Form<CounterForm>,
) -> impl IntoResponse {
    let Some(d) = state.get(&dispute_id) else {
        return (StatusCode::NOT_FOUND, html! { p { "Unknown dispute." } }).into_response();
    };
    if idx >= d.analysis.settlements.len() {
        return (StatusCode::BAD_REQUEST, html! { p { "Invalid settlement index." } })
            .into_response();
    }
    let name = if form.name.trim().is_empty() { "You" } else { form.name.trim() };
    let fragment = html! {
        div .card .settlement id=(format!("settlement-{idx}")) {
            div .settlement-head { strong { "Option " (idx + 1) } span .chip .chip-amber { "counter requested" } }
            p .settle-counter {
                (name) " would like to talk this one through before agreeing. That's "
                "completely fine — the mediator will help you shape a counter-offer, "
                "and the other party will be told a conversation is open, not that "
                "anything was refused."
            }
        }
    };
    (StatusCode::OK, fragment).into_response()
}

// ───────────────────────────── small helpers ─────────────────────────────────

fn not_found(msg: &str) -> axum::response::Response {
    let body = page("Not found", html! {
        (brandbar(None))
        header .interior-head {
            h1 { "Nothing here" }
            p .lede { (msg) " " a href="/" { "Back to the gallery." } }
        }
    });
    (StatusCode::NOT_FOUND, body).into_response()
}

/// "couch" → "Couch"; "standing_desk" → "Standing desk".
fn prettify_id(id: &str) -> String {
    let spaced = id.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => spaced,
    }
}

/// Light-touch softening of kernel prose for a party's eyes.
///
/// The kernel is already kind, but its English renderer is phrased for the
/// roommate case: it talks about "the stain", "ordinary wear", "damage", and
/// "deductions" even in a freelance or estate dispute. We rewrite those fixed
/// idioms into dispute-neutral language so the party view reads naturally for
/// *every* scenario. (The operator cockpit keeps the kernel's verbatim text.)
fn humanize(s: &str) -> String {
    let mut t = s.replace("You both stipulate: ", "You both agree: ");
    // The two-world refund line: "$X back if the stain is ordinary wear; $Y
    // back if it counts as damage." → neutral "gentler / stricter reading".
    t = t.replace(
        "back if the stain is ordinary wear",
        "settled in the gentler reading of the open question",
    );
    t = t.replace(
        "back if it counts as damage",
        "settled in the stricter reading",
    );
    // Over-claim / itemization wording.
    t = t.replace(" in deductions is not what the itemization supports", " doesn't match the itemized figures");
    t = t.replace("the itemized deductions total", "the itemized figures come to");
    t = t.replace("gap on the deductions is", "gap is");
    t
}

/// The Adjusted Winner explanation is precise but operator-flavoured (point
/// totals, "Pareto-optimal"). For a party, lead with the human sentence and
/// drop the bare arithmetic dump; the allocation list already shows the split.
fn humanize_settlement(explanation: &str) -> String {
    // Keep only the first human-readable line if the kernel dumped a multi-line
    // mechanism trace; the structured allocation list carries the specifics.
    let first = explanation
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Adjusted Winner"))
        .unwrap_or(explanation.trim());
    if first.starts_with("The shared")
        || first.contains("→")
        || first.starts_with("Final point")
    {
        // It's mechanism detail, not a sentence — give a calm generic line.
        return "A fair split of the shared things, with the deposit settled \
                accordingly. Neither of you would rather have the other's share."
            .to_string();
    }
    first.to_string()
}

// ─────────────────────────────────── tests ───────────────────────────────────

#[cfg(test)]
mod tests;
