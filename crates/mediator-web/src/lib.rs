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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    Form,
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use mediator_types::{Analysis, Dispute, Formula, Party, Receipt, Settlement, Sig, Term};
use mediator_session::{
    conduct, LiveBrain, MediatorBrain, ScriptedBrain, ScriptedInputs, Session, Utterance,
};
use tokio::sync::RwLock;

pub use load::{DisputeRecord, discover_disputes, load_record, scenarios_dir};
use theme::CSS;

// ─────────────────────────── rate limiter ────────────────────────────────────

/// A very simple token-bucket rate limiter.
/// Allows one call per `min_interval`; shared across all /formalize requests.
struct RateLimiter {
    min_interval: Duration,
    last_allowed: Mutex<Option<Instant>>,
}

impl RateLimiter {
    fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_allowed: Mutex::new(None),
        }
    }

    /// Returns `true` if the call is allowed (and records the time).
    fn allow(&self) -> bool {
        let mut guard = self.last_allowed.lock().unwrap();
        let now = Instant::now();
        match *guard {
            None => {
                *guard = Some(now);
                true
            }
            Some(last) => {
                if now.duration_since(last) >= self.min_interval {
                    *guard = Some(now);
                    true
                } else {
                    false
                }
            }
        }
    }
}

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

    /// Combined signature: the union of all parties' symbols (deduped by name).
    fn combined_sig(&self) -> Vec<Sig> {
        let mut sig: Vec<Sig> = Vec::new();
        for party in &self.dispute.parties {
            for s in &party.signature {
                if !sig.iter().any(|d| d.name == s.name) {
                    sig.push(s.clone());
                }
            }
        }
        sig
    }
}

/// What the server renders from. Holds many disputes; decoupled from the kernel.
pub struct AppState {
    pub disputes: Vec<LoadedDispute>,
    /// Whether the live LLM feature is enabled (env `MEDIATEOR_LIVE_LLM`).
    pub live_llm_enabled: bool,
    /// Simple global rate limiter for /formalize calls.
    rate_limiter: RateLimiter,
    /// In-memory store of live interactive mediation sessions, keyed by a short
    /// id. Ephemeral (lost on restart) — fine for a demo behind auth.
    sessions: RwLock<HashMap<String, Session>>,
    next_sid: AtomicU64,
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
        let live_llm_enabled = std::env::var("MEDIATEOR_LIVE_LLM")
            .map(|v| {
                !v.is_empty()
                    && v != "0"
                    && v.to_ascii_lowercase() != "false"
                    && v.to_ascii_lowercase() != "no"
            })
            .unwrap_or(false);
        Self {
            disputes,
            live_llm_enabled,
            // One call per 4 seconds globally — cheap but prevents spam.
            rate_limiter: RateLimiter::new(Duration::from_secs(4)),
            sessions: RwLock::new(HashMap::new()),
            next_sid: AtomicU64::new(1),
        }
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
        .route("/session/:dispute_id", get(mediation_session))
        .route("/talk/:dispute_id/:party_id", get(talk_start))
        .route("/talk/:sid/:party_id/say", post(talk_say))
        .route("/talk/:sid/:party_id/where", get(talk_where))
        .route(
            "/settlement/:dispute_id/:idx/accept",
            post(settlement_accept),
        )
        .route(
            "/settlement/:dispute_id/:idx/counter",
            post(settlement_counter),
        )
        .route("/formalize/:dispute_id", post(formalize_claim))
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

// ─────────────────────────── the mediation session ──────────────────────────

/// Conduct a full mediation over a dispute and render it as a readable session.
/// On the box (`MEDIATEOR_LIVE_LLM` set) the model conducts it (Claude Haiku
/// 4.5); otherwise a deterministic scripted voice. The certified facts and
/// fair-division settlements are never model-invented — they're the trust spine.
async fn mediation_session(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&id) else {
        return not_found("We don't have a record of that dispute.");
    };

    let session = Session::new(d.dispute.clone(), d.analysis.clone(), d.receipts.clone());
    let inputs = scripted_inputs_for(&d.dispute);
    let live = state.live_llm_enabled;

    // `conduct` is sync and a live brain blocks on its own runtime, so run it off
    // the async executor.
    let out = tokio::task::spawn_blocking(move || {
        if live {
            if let Ok(b) = LiveBrain::new() {
                return conduct(session, &b, &inputs);
            }
        }
        conduct(session, &ScriptedBrain, &inputs)
    })
    .await
    .expect("session conduct task");

    let voice = if live { "Claude Haiku 4.5" } else { "a scripted preview voice" };
    let markup = page(
        &format!("Mediation — {}", out.title),
        render_session(&out, voice),
    );
    (StatusCode::OK, markup).into_response()
}

/// Opening caucus lines per party — rich, hand-written for the roommate demo;
/// otherwise gentle generic openers so any scenario runs.
fn scripted_inputs_for(dispute: &Dispute) -> ScriptedInputs {
    if dispute.parties.iter().any(|p| p.id == "robin") {
        return ScriptedInputs::new()
            .with(
                "robin",
                &[
                    "Honestly I just don't think I should pay for that stain — it was wear and tear.",
                    "And look, I wasn't around for the deep clean, but I pulled my weight the whole lease.",
                ],
            )
            .with(
                "sam",
                &["The carpet is real damage and it's only fair that Robin covers it. I just want this to be fair."],
            );
    }
    let mut inp = ScriptedInputs::new();
    for p in &dispute.parties {
        inp = inp.with(
            &p.id,
            &[
                "Here's how I see it, in my own words.",
                "I want an outcome that's actually fair to both of us.",
            ],
        );
    }
    inp
}

fn render_session(s: &Session, voice: &str) -> Markup {
    html! {
        (brandbar(Some(html! { (&s.title) })))
        style { (PreEscaped(SESSION_CSS)) }

        header .interior-head {
            h1 { "A mediation, conducted" }
            p .lede {
                "Not a verdict — a " em { "mediation" } ". An impartial guide with no "
                "stake in the outcome speaks with each person, finds what they "
                "already agree on, names the one real disagreement and hands it "
                "back, and offers fair options. The facts it leans on were checked "
                "by the prover, so it can't fudge a number or paper over a "
                "contradiction."
            }
        }

        @for th in &s.parties {
            @if !th.caucus.is_empty() {
                section .caucus {
                    h2 .session-h { "In private with " (party_first_name(&th.display_name)) }
                    div .chat {
                        @for u in &th.caucus { (bubble(s, u)) }
                    }
                    @if !th.interests.is_empty() {
                        p .interest {
                            "What the mediator heard underneath: " (th.interests.join("; "))
                        }
                    }
                }
            }
        }

        section .joint {
            h2 .session-h { "Together" }
            div .chat {
                @for u in &s.joint_transcript { (bubble(s, u)) }
            }
        }

        @if !s.proposals.is_empty() {
            section .session-options {
                h2 .session-h { "On the table" }
                @for p in &s.proposals {
                    div .opt {
                        span .opt-badge[p.coherent] { @if p.coherent { "fair · certified" } @else { "uncertified" } }
                        span .opt-sum { (p.summary) }
                    }
                }
            }
        }

        p .session-foot {
            "Conducted by " strong { (voice) } ". Everything it relied on is certified — "
            a href=(format!("/operator/{}", s.id)) { "see the receipts" } "."
        }
    }
}

fn bubble(s: &Session, u: &mediator_session::Utterance) -> Markup {
    use mediator_session::Speaker;
    match &u.speaker {
        Speaker::Mediator => html! { div .b.med { span .who { "mediator" } p { (u.text) } } },
        Speaker::Party(p) => html! { div .b.party { span .who { (s.party_name(p)) } p { (u.text) } } },
        Speaker::System => html! { div .b.sys { p { (u.text) } } },
    }
}

const SESSION_CSS: &str = r#"
.caucus, .joint, .session-options { margin: 1.6rem 0; }
.session-h { font-size: 1.05rem; letter-spacing: .02em; opacity: .8; margin: 0 0 .7rem; }
.chat { display: flex; flex-direction: column; gap: .7rem; }
.b { max-width: 46rem; padding: .7rem .95rem; border-radius: 14px; }
.b p { margin: 0; white-space: pre-wrap; line-height: 1.5; }
.b .who { display: block; font-size: .72rem; text-transform: uppercase; letter-spacing: .06em; opacity: .55; margin-bottom: .25rem; }
.b.med { background: #fbf6ee; border: 1px solid #efe4d2; align-self: flex-start; }
.b.party { background: #f3f4f6; border: 1px solid #e5e7eb; align-self: flex-end; }
.b.sys { background: transparent; border: 1px dashed #d6c8b0; align-self: center; font-style: italic; opacity: .8; }
@media (prefers-color-scheme: dark) {
  .b.med { background: #2a2620; border-color: #3a3328; }
  .b.party { background: #232427; border-color: #34363b; }
}
.interest { margin: .6rem .2rem 0; font-size: .9rem; opacity: .75; font-style: italic; }
.session-options .opt { display: flex; gap: .8rem; align-items: baseline; padding: .55rem 0; border-bottom: 1px solid #00000010; }
.opt-badge { font-size: .7rem; padding: .1rem .5rem; border-radius: 999px; background: #e9f3ec; color: #2f6b46; white-space: nowrap; }
.session-foot { margin-top: 1.6rem; font-size: .92rem; opacity: .8; }
"#;

// ──────────────────────── the interactive session (talk) ─────────────────────

#[derive(serde::Deserialize)]
struct TalkForm {
    message: String,
}

/// Start a live, interactive caucus: the visitor speaks as one party and the
/// mediator (live on the box, scripted locally) replies in real back-and-forth.
async fn talk_start(
    Path((dispute_id, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&dispute_id) else {
        return not_found("We don't have a record of that dispute.");
    };
    let Some(party) = d.dispute.parties.iter().find(|p| p.id == party_id) else {
        return not_found("That person isn't part of this dispute.");
    };
    let name = party_first_name(&party.display_name);

    let sid = format!("s{}", state.next_sid.fetch_add(1, Ordering::Relaxed));
    let session = Session::new(d.dispute.clone(), d.analysis.clone(), d.receipts.clone());
    state.sessions.write().await.insert(sid.clone(), session);

    let opener = format!(
        "I'm really glad you're here, {name}. This is just between us — nothing you say \
         is shared with the other person without your okay. Tell me, in your own words: \
         what's going on?"
    );
    let say_url = format!("/talk/{}/{}/say", sid, party.id);

    let markup = page(&format!("Talk it through — {}", d.dispute.title), html! {
        (brandbar(Some(html! { (&d.dispute.title) })))
        style { (PreEscaped(SESSION_CSS)) (PreEscaped(TALK_CSS)) }
        header .interior-head {
            h1 { "A private word with the mediator" }
            p .lede {
                "You're speaking as " strong { (party.display_name.clone()) } ". Say what's "
                "on your mind — the mediator listens and reflects, has no stake in how this "
                "turns out, and you can stop any time."
            }
        }
        div #chat .chat {
            div .b.med { span .who { "mediator" } p { (opener) } }
        }
        form .talkform hx-post=(say_url) hx-target="#chat" hx-swap="beforeend"
             "hx-on::after-request"="this.reset(); this.querySelector('input').focus()" {
            input .talkin type="text" name="message" autocomplete="off" required
                  placeholder=(format!("Speak as {name}…"));
            button type="submit" { "Say it" }
        }
        button .where-btn hx-get=(format!("/talk/{}/{}/where", sid, party.id))
               hx-target="#chat" hx-swap="beforeend" {
            "When you're ready — see where this could land →"
        }
        p .session-foot {
            @if state.live_llm_enabled { "The mediator is Claude Haiku 4.5, live." }
            @else { "The mediator is a scripted preview voice (the live model runs on the deployed site)." }
        }
    });
    (StatusCode::OK, markup).into_response()
}

/// The visitor said something → append it, get the mediator's reply, return the
/// exchange as an htmx fragment appended to the chat.
async fn talk_say(
    Path((sid, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
    Form(form): Form<TalkForm>,
) -> impl IntoResponse {
    let msg = form.message.trim().to_string();
    if msg.is_empty() {
        return (StatusCode::OK, html! {}).into_response();
    }
    if msg.chars().count() > 600 {
        return (
            StatusCode::OK,
            html! { div .b.sys { p { "(let's keep it to a few sentences at a time)" } } },
        )
            .into_response();
    }

    // Snapshot the session for the brain; don't hold the lock across the model call.
    let session = {
        let map = state.sessions.read().await;
        match map.get(&sid) {
            Some(s) => s.clone(),
            None => {
                return (
                    StatusCode::OK,
                    html! { div .b.sys { p { "That conversation expired — start again from the seat picker." } } },
                )
                    .into_response()
            }
        }
    };

    let live = state.live_llm_enabled;
    let p2 = party_id.clone();
    let m2 = msg.clone();
    let mv = tokio::task::spawn_blocking(move || {
        if live {
            if let Ok(b) = LiveBrain::new() {
                return b.caucus(&session, &p2, &m2);
            }
        }
        ScriptedBrain.caucus(&session, &p2, &m2)
    })
    .await
    .expect("caucus task");

    // Persist the exchange into the stored session.
    {
        let mut map = state.sessions.write().await;
        if let Some(s) = map.get_mut(&sid) {
            if let Some(th) = s.parties.iter_mut().find(|t| t.id == party_id) {
                th.caucus.push(Utterance::party(&party_id, msg.clone()));
                th.caucus.push(Utterance::mediator(mv.reply.clone()));
                for i in &mv.interests {
                    if !th.interests.contains(i) {
                        th.interests.push(i.clone());
                    }
                }
            }
        }
    }

    let frag = html! {
        div .b.party { span .who { "you" } p { (msg) } }
        div .b.med { span .who { "mediator" } p { (mv.reply) } }
    };
    (StatusCode::OK, frag).into_response()
}

/// "Where this could land": the mediator steps back from the private caucus and
/// shows the certified shared ground, the crux (handed back), and the fair
/// options — tying the felt conversation to the trustworthy resolution.
async fn talk_where(
    Path((sid, _party)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let session = {
        let map = state.sessions.read().await;
        match map.get(&sid) {
            Some(s) => s.clone(),
            None => {
                return (
                    StatusCode::OK,
                    html! { div .b.sys { p { "That conversation expired — start again from the seat picker." } } },
                )
                    .into_response()
            }
        }
    };

    let live = state.live_llm_enabled;
    let (shared, crux, opts) = tokio::task::spawn_blocking(move || {
        let brain: Box<dyn MediatorBrain> = if live {
            LiveBrain::new()
                .map(|b| Box::new(b) as Box<dyn MediatorBrain>)
                .unwrap_or_else(|_| Box::new(ScriptedBrain))
        } else {
            Box::new(ScriptedBrain)
        };
        let shared = brain.shared_ground(&session);
        let crux = brain.crux(&session);
        let opts: Vec<String> = brain.proposals(&session).into_iter().map(|d| d.summary).collect();
        (shared, crux, opts)
    })
    .await
    .expect("where task");

    let frag = html! {
        div .b.med { span .who { "mediator" } p { "Okay — let me step back and show you the bigger picture, with both sides in view." } }
        div .b.med { span .who { "mediator" } p { (shared) } }
        div .b.med { span .who { "mediator" } p { (crux) } }
        @if !opts.is_empty() {
            div .session-options {
                @for o in &opts {
                    div .opt { span .opt-badge { "fair · certified" } span .opt-sum { (o) } }
                }
            }
        }
        p .session-foot {
            "Every fact shown here was checked by the prover — the mediator can't "
            "fudge a number or invent a fact. The open question above is yours to answer."
        }
    };
    (StatusCode::OK, frag).into_response()
}

const TALK_CSS: &str = r#"
.talkform { display: flex; gap: .5rem; margin: 1rem 0 .4rem; }
.talkin { flex: 1; padding: .65rem .8rem; border-radius: 12px; border: 1px solid #d8cbb6; font: inherit; background: #fff; }
.talkform button { padding: .65rem 1.1rem; border-radius: 12px; border: 0; background: #c06a3e; color: #fff; font: inherit; cursor: pointer; }
.talkform button:hover { background: #a85a32; }
#chat { min-height: 7rem; }
.where-btn { margin-top: .6rem; background: transparent; border: 1px solid #d8cbb6; border-radius: 12px; padding: .6rem 1rem; cursor: pointer; font: inherit; opacity: .9; }
.where-btn:hover { background: #fbf6ee; opacity: 1; }
@media (prefers-color-scheme: dark) { .talkin { background: #1f1d1a; color: #eee; border-color: #3a3328; } .where-btn { color: inherit; } .where-btn:hover { background: #2a2620; } }
"#;

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

        section {
            a .seat .seat-session href=(format!("/session/{}", d.id)) {
                span .seat-i { "Sit in on the whole mediation" }
                span .seat-sub { "the session · how it actually mediates" }
                span .seat-hint {
                    "Watch a full mediation conducted end to end — the private "
                    "caucuses, the common ground, the one open question handed "
                    "back, and the fair options. The facts underneath are checked."
                }
            }
        }

        section .talk-invite {
            style { (PreEscaped(".talk-invite{margin-top:1.5rem}.talk-links{display:flex;gap:.6rem;flex-wrap:wrap;margin-top:.55rem}.talk-link{padding:.55rem .95rem;border:1px solid #d8cbb6;border-radius:10px;text-decoration:none;color:inherit}.talk-link:hover{background:#fbf6ee}")) }
            p { "Or try it yourself — speak privately with the mediator, live:" }
            div .talk-links {
                @for p in &d.dispute.parties {
                    a .talk-link href=(format!("/talk/{}/{}", d.id, p.id)) {
                        "Talk as " (party_first_name(&p.display_name))
                    }
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

        // (6) "Say it in your own words" — live formalization panel (gated).
        @if state.live_llm_enabled {
            (live_formalize_panel(&d.id))
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

// ─────────────────────── live formalization panel ────────────────────────────

/// The "Say it in your own words" panel. Shown only when `live_llm_enabled`.
fn live_formalize_panel(dispute_id: &str) -> Markup {
    html! {
        section .reveal .step #live-formalize {
            span .step-n { "?" }
            h2 { "Say it in your own words" }
            p .step-lede {
                "Describe what you believe is true in plain English. "
                "The council will try to read it as a formal statement — "
                "so you can see how a prover would understand it."
            }
            div .card .formalize-card {
                form
                    hx-post=(format!("/formalize/{dispute_id}"))
                    hx-target="#formalize-result"
                    hx-swap="innerHTML"
                    hx-indicator="#formalize-spinner"
                    {
                    div .formalize-input-row {
                        input .formalize-input
                            type="text"
                            name="claim"
                            placeholder="e.g. the stain was ordinary wear and tear"
                            maxlength="240"
                            autocomplete="off"
                            {}
                        button .btn .btn-accept type="submit" { "Formalize" }
                        span #formalize-spinner .htmx-indicator .formalize-spinner { "…" }
                    }
                    p .formalize-hint {
                        "Up to 240 characters. "
                        "Use the symbols from this dispute — the more specific, the better."
                    }
                }
                div #formalize-result .formalize-result {}
                p .formalize-caption {
                    "The council proposes; a prover would check this next — "
                    "this is the formalization step, live. Nothing here has been proved."
                }
            }
        }
    }
}

/// The compact HTML fragment for one model's reading.
fn reading_fragment(r: &mediator_llm::live::Reading) -> Markup {
    let valid_pill = if r.valid {
        html! { span .pill .pill-green { "valid" } }
    } else {
        html! { span .pill .pill-amber { "needs review" } }
    };

    let formal_ir = r.formula.as_ref().map(|f| {
        use mediator_core::render::formula_to_isabelle;
        let isa = formula_to_isabelle(f);
        let json = serde_json::to_string_pretty(f).unwrap_or_default();
        html! {
            details .reading-details {
                summary { "Formal IR" }
                pre .reading-ir { (isa) }
                pre .reading-ir { (json) }
            }
        }
    });

    html! {
        div .reading-card {
            div .reading-head {
                (valid_pill)
                strong .reading-model { (r.model.clone()) }
            }
            @if !r.issues.is_empty() {
                ul .reading-issues {
                    @for issue in &r.issues {
                        li { (issue) }
                    }
                }
            }
            // The wow — prominent English render.
            p .reading-english { (r.english.clone()) }
            // Formal IR in a collapsible.
            @if let Some(frag) = formal_ir { (frag) }
            // Raw text in a collapsible.
            details .reading-details {
                summary { "Raw model output" }
                pre .reading-raw { (r.raw.clone()) }
            }
        }
    }
}

/// Form payload for /formalize.
#[derive(serde::Deserialize)]
struct FormalizeForm {
    #[serde(default)]
    claim: String,
}

/// `POST /formalize/:dispute_id` — returns an htmx HTML fragment.
async fn formalize_claim(
    Path(dispute_id): Path<String>,
    State(state): State<SharedState>,
    Form(form): Form<FormalizeForm>,
) -> impl IntoResponse {
    // Feature gate.
    if !state.live_llm_enabled {
        let frag = html! {
            p .formalize-off {
                "Live formalization is off in this build. "
                "Set " span .mono { "MEDIATEOR_LIVE_LLM=1" } " to enable it."
            }
        };
        return (StatusCode::OK, frag).into_response();
    }

    // Dispute must exist.
    let Some(d) = state.get(&dispute_id) else {
        let frag = html! { p .formalize-error { "Unknown dispute." } };
        return (StatusCode::NOT_FOUND, frag).into_response();
    };

    // Input length guard (≤ 240 chars).
    let claim = form.claim.trim().to_string();
    if claim.is_empty() {
        let frag = html! {
            p .formalize-hint { "Please enter a claim to formalize." }
        };
        return (StatusCode::OK, frag).into_response();
    }
    if claim.chars().count() > 240 {
        let frag = html! {
            p .formalize-error {
                "That's a bit long (max 240 characters). "
                "Try a shorter, more direct statement."
            }
        };
        return (StatusCode::OK, frag).into_response();
    }

    // Rate limiter.
    if !state.rate_limiter.allow() {
        let frag = html! {
            p .formalize-hint {
                "One moment — the council is still thinking. Try again in a few seconds."
            }
        };
        return (StatusCode::OK, frag).into_response();
    }

    // Call the live council.
    let sig = d.combined_sig();
    let cfg = mediator_llm::live::LiveConfig::default();
    match mediator_llm::live::council_formalize_live(&claim, &sig, &cfg).await {
        Ok(council) => {
            let frag = html! {
                div .council-result {
                    @for r in &council.readings {
                        (reading_fragment(r))
                    }
                    div .consensus-line {
                        span .pill .pill-blue { "consensus" }
                        " "
                        (council.consensus.clone())
                    }
                }
            };
            (StatusCode::OK, frag).into_response()
        }
        Err(e) => {
            let frag = html! {
                p .formalize-error {
                    "The council couldn't reach Bedrock right now. "
                    "(" (e.to_string()) ")"
                }
            };
            (StatusCode::OK, frag).into_response()
        }
    }
}

// ─────────────────────────────────── tests ───────────────────────────────────

#[cfg(test)]
mod tests;
