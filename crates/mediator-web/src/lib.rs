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
mod pacts;
mod theme;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::{
    Form,
    Router,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use mediator_types::{Analysis, Crux, Dispute, Formula, Party, Receipt, Settlement, Sig, Term};
use mediator_session::{
    conduct, next_action, Evidence, EvidenceKind, LiveBrain, MediatorAction, MediatorBrain,
    ScriptedBrain, ScriptedInputs, Session, Utterance,
};
use std::net::SocketAddr;
use tokio::sync::RwLock;

pub use load::{DisputeRecord, discover_disputes, load_record, scenarios_dir};
use theme::CSS;

// ─────────────────────────── rate limiting ───────────────────────────────────
//
// Two layers guard every endpoint that can reach a model (a caucus reply, the
// "where this lands" recap, the live formalizer). The $50 AWS budget action is
// the true backstop; *these* are the primary, friendly defense that lets the
// Caddy password come off for a public demo:
//
//   1. a GLOBAL token bucket (a hard ceiling on model calls across all visitors),
//   2. a PER-IP token bucket (so one peer can't monopolize or run up the bill),
//
// keyed on the peer IP — preferring the left-most `X-Forwarded-For` address that
// Caddy sets, falling back to the socket address. All limits are env-tunable
// (see `RateConfig::from_env`). A human who trips a limit gets a calm "one
// moment…" fragment, never a bare 429.

/// A token bucket: `capacity` tokens, refilling at `refill_per_sec`. One model
/// call costs one token. Cheap, lock-guarded, no background task.
#[derive(Debug)]
struct TokenBucket {
    capacity: f64,
    refill_per_sec: f64,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self { capacity, refill_per_sec, tokens: capacity, last: Instant::now() }
    }

    /// Try to spend one token. Refills lazily based on elapsed wall time.
    fn try_take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Tunable limits for the model-calling endpoints. Every value is overridable by
/// an environment variable so the public box can be tightened without a rebuild.
#[derive(Clone, Copy, Debug)]
struct RateConfig {
    /// Global burst capacity (tokens) across *all* visitors.
    global_capacity: f64,
    /// Global steady-state refill (tokens per second).
    global_refill: f64,
    /// Per-IP burst capacity (tokens) for a single peer.
    per_ip_capacity: f64,
    /// Per-IP steady-state refill (tokens per second).
    per_ip_refill: f64,
    /// Max distinct IP buckets kept in memory (oldest evicted past this).
    max_ip_buckets: usize,
}

impl RateConfig {
    fn from_env() -> Self {
        // Defaults: a single visitor gets a burst of ~8 model calls, then ~1 every
        // 5s; across everyone, a burst of 40 then ~2/sec. Comfortable for a real,
        // unhurried conversation (you think between turns); hostile to a script
        // hammering the endpoint. Tune any of these via env on the public box:
        //   MEDIATEOR_RL_IP_BURST, MEDIATEOR_RL_IP_PER_SEC      (per peer IP)
        //   MEDIATEOR_RL_GLOBAL_BURST, MEDIATEOR_RL_GLOBAL_PER_SEC (all visitors)
        //   MEDIATEOR_RL_MAX_IPS (cap on tracked IP buckets)
        fn num(key: &str, default: f64) -> f64 {
            std::env::var(key).ok().and_then(|v| v.parse().ok()).filter(|v: &f64| *v > 0.0).unwrap_or(default)
        }
        fn usize_env(key: &str, default: usize) -> usize {
            std::env::var(key).ok().and_then(|v| v.parse().ok()).filter(|v: &usize| *v > 0).unwrap_or(default)
        }
        RateConfig {
            global_capacity: num("MEDIATEOR_RL_GLOBAL_BURST", 40.0),
            global_refill: num("MEDIATEOR_RL_GLOBAL_PER_SEC", 2.0),
            per_ip_capacity: num("MEDIATEOR_RL_IP_BURST", 8.0),
            per_ip_refill: num("MEDIATEOR_RL_IP_PER_SEC", 0.2),
            max_ip_buckets: usize_env("MEDIATEOR_RL_MAX_IPS", 4096),
        }
    }
}

/// The two-layer (global + per-IP) limiter behind every model-calling endpoint.
struct RateLimiter {
    cfg: RateConfig,
    global: Mutex<TokenBucket>,
    per_ip: Mutex<HashMap<String, TokenBucket>>,
}

impl RateLimiter {
    fn new(cfg: RateConfig) -> Self {
        Self {
            cfg,
            global: Mutex::new(TokenBucket::new(cfg.global_capacity, cfg.global_refill)),
            per_ip: Mutex::new(HashMap::new()),
        }
    }

    /// Allow a model call from `ip`? Spends one global *and* one per-IP token.
    /// (Spends global first; if the per-IP bucket is dry we don't refund the
    /// global token — conservative, and the buckets refill on the same clock.)
    fn allow(&self, ip: &str) -> bool {
        {
            let mut g = self.global.lock().unwrap();
            if !g.try_take() {
                return false;
            }
        }
        let mut map = self.per_ip.lock().unwrap();
        // Bound the table so a flood of distinct IPs can't grow it unboundedly.
        if map.len() >= self.cfg.max_ip_buckets && !map.contains_key(ip) {
            // Evict the bucket idle the longest (its `last` is furthest in the past).
            if let Some(stale) = map.iter().min_by_key(|(_, b)| b.last).map(|(k, _)| k.clone()) {
                map.remove(&stale);
            }
        }
        let bucket = map
            .entry(ip.to_string())
            .or_insert_with(|| TokenBucket::new(self.cfg.per_ip_capacity, self.cfg.per_ip_refill));
        bucket.try_take()
    }
}

/// The friendly throttle fragment shown to a human who's gone too fast — a calm
/// "one moment…", never a bare 429. Returned with 200 so htmx swaps it in place.
fn one_moment() -> Markup {
    html! {
        div .b.sys .one-moment {
            p { "One moment — let's not rush this. Take a breath; try again in a few seconds." }
        }
    }
}

/// The peer's IP for rate-limiting: the left-most `X-Forwarded-For` entry Caddy
/// sets, else the real socket address. (Behind our own trusted Caddy, XFF is
/// safe to trust; with no proxy we fall back to the connection's address.)
fn client_ip(headers: &HeaderMap, conn: Option<SocketAddr>) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            let ip = first.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }
    conn.map(|c| c.ip().to_string()).unwrap_or_else(|| "unknown".to_string())
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
    /// The certified FORWARD CONSTITUTIONS corpus: each pact + the CACHED, signed
    /// certificate the anchor worker proved through the real gate. This box only
    /// renders them — it never runs Isabelle. May be empty (the gallery shows a
    /// calm empty state then).
    pub pacts: Vec<pacts::PactRecord>,
    /// Whether the live LLM feature is enabled (env `MEDIATEOR_LIVE_LLM`).
    pub live_llm_enabled: bool,
    /// Global + per-IP rate limiter guarding every model-calling endpoint.
    rate_limiter: RateLimiter,
    /// Max characters accepted in a single utterance / exhibit / claim. A human
    /// says a few sentences; this just keeps a payload from being abusive. Tunable
    /// via `MEDIATEOR_MAX_INPUT_CHARS` (default 600, floor 40).
    max_input_chars: usize,
    /// In-memory store of live interactive mediation sessions, keyed by a short
    /// id. Ephemeral (lost on restart) — fine for a demo behind auth.
    sessions: RwLock<HashMap<String, Session>>,
    next_sid: AtomicU64,
    /// Server signing key for the tamper-evident audit records (ed25519,
    /// ephemeral per process — each record carries its own public key, so it is
    /// self-verifying regardless).
    audit_key: ed25519_dalek::SigningKey,
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
        let max_input_chars = std::env::var("MEDIATEOR_MAX_INPUT_CHARS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &usize| *v >= 40)
            .unwrap_or(600);
        // The certified-pact corpus (renders the CACHED certs; never runs the gate).
        // Discovered beside the scenarios, under `scenarios/pacts/`. Missing/empty
        // is fine — the gallery shows a calm empty state.
        let pacts = pacts::discover_pacts(&pacts::pacts_dir());
        Self {
            disputes,
            pacts,
            live_llm_enabled,
            rate_limiter: RateLimiter::new(RateConfig::from_env()),
            max_input_chars,
            sessions: RwLock::new(HashMap::new()),
            next_sid: AtomicU64::new(1),
            audit_key: mediator_audit::generate_keypair(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&LoadedDispute> {
        self.disputes.iter().find(|d| d.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.disputes.is_empty()
    }

    /// Test-only: install an explicit rate-limit config (so a burst test is
    /// deterministic without racing on process-global env vars).
    #[cfg(test)]
    fn with_rate_config(mut self, cfg: RateConfig) -> Self {
        self.rate_limiter = RateLimiter::new(cfg);
        self
    }

    /// Test-only: install an explicit certified-pact corpus, so the forward-
    /// constitution routes can be exercised on deterministic fixtures rather than
    /// whatever happens to sit on disk.
    #[cfg(test)]
    fn with_pacts(mut self, pacts: Vec<pacts::PactRecord>) -> Self {
        self.pacts = pacts;
        self
    }
}

type SharedState = Arc<AppState>;

/// Max live interactive sessions kept in memory at once (oldest evicted past it).
const MAX_SESSIONS: usize = 500;

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
        .route("/talk/:sid/:party_id/evidence", post(talk_evidence))
        .route("/talk/:sid/:party_id/accept/:idx", post(talk_accept))
        .route("/talk/:sid/:party_id/revise", post(talk_revise))
        .route("/talk/:sid/:party_id/sign", post(talk_sign))
        .route("/audit/:dispute_id", get(audit_view))
        .route("/audit/:dispute_id/download", get(audit_download))
        .route("/pacts", get(pacts::pacts_gallery))
        .route("/pact/:id", get(pacts::pact_detail))
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
/// On the box (`MEDIATEOR_LIVE_LLM` set) the model conducts it (the open
/// flagship, Qwen3-VL 235B); otherwise a deterministic scripted voice. The
/// certified facts and fair-division settlements are never model-invented —
/// they're the trust spine.
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

    let voice = if live { "Qwen3-VL 235B — open" } else { "a scripted preview voice" };
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

// ════════════════════════ the room (interactive talk) ════════════════════════
//
// The centerpiece. Not a chat widget bolted onto a dashboard — *a room*. The
// visitor sits with the mediator and speaks freely; the mediator listens, checks
// the interest under the position *back* to them, weaves in any evidence, and —
// only when the room is ready — steps the two of them through the certified
// shared ground, the one open question (handed back), the subtraction ("here's
// what was never about the money"), the fair options, and an agreement the
// parties write in their own words.
//
// The flow is **not a fixed march**. After every human turn we ask
// `mediator_session::next_action(&session)` what a real mediator would do next
// and perform exactly that, looping through the autonomous moves and pausing at
// the human checkpoints (confirm an interest, add evidence, accept an option,
// sign the agreement). A late exhibit re-opens acknowledgement; the crux is never
// reached before the person is heard. The certified facts underneath are the
// trust spine — the mediator can't fudge a number or decide the crux.

#[derive(serde::Deserialize)]
struct TalkForm {
    message: String,
}

/// One rendered unit of the room's transcript, produced by the autonomous driver
/// while it holds the (blocking-safe) brain. Rendered to maud *after* the
/// blocking task returns, so no markup crosses the task boundary.
enum Beat {
    /// The mediator speaks (a caucus reply, a reflection, the crux, …).
    Mediator(String),
    /// A system aside (gentle, centered).
    System(String),
    /// The signature **subtraction** panel: the money is handled; here's the
    /// residue that was never about money. One of the two things the parties
    /// themselves shaped — kept visually central.
    Residue(Vec<String>),
    /// The honest **factual-conflict** surface — needs evidence, not logic.
    /// Speech-led and calm, not an adjudication panel.
    Conflict(Vec<ConflictView>),
    /// Fair ways forward: the single most balanced one shown plainly, the rest
    /// folded behind a quiet disclosure. Each acceptable in-session.
    Options(Vec<OptionView>),
    /// Open the co-authored agreement for the parties to shape and sign.
    Draft { text: String, signed: Vec<String>, parties: Vec<(String, String)> },
    /// The room has landed.
    Landed(String),
}

struct ConflictView {
    about: String,
    a_name: String,
    a_claim: String,
    b_name: String,
    b_claim: String,
}

struct OptionView {
    idx: usize,
    summary: String,
    envy_free: bool,
    equitable: bool,
}

/// Make a brain (live on the box if it initializes; the scripted floor otherwise).
fn make_brain(live: bool) -> Box<dyn MediatorBrain> {
    if live {
        LiveBrain::new()
            .map(|b| Box::new(b) as Box<dyn MediatorBrain>)
            .unwrap_or_else(|_| Box::new(ScriptedBrain))
    } else {
        Box::new(ScriptedBrain)
    }
}

/// Drive the session forward from its current state via `next_action`, performing
/// the **autonomous** mediator moves (reflect, name crux, subtract, propose, …)
/// and collecting the beats to render. Pauses — returns — at any move that needs
/// the human (more space, a check-back to confirm, the invitation to agree, the
/// co-authoring). Mutates `s` in place. Pure-ish: only the brain may call out.
fn drive(s: &mut Session, brain: &dyn MediatorBrain) -> Vec<Beat> {
    let mut beats = Vec::new();
    // A generous bound; each branch makes progress or breaks, so this is just a
    // belt-and-suspenders guard against a logic bug looping forever.
    for _ in 0..32 {
        match next_action(s) {
            MediatorAction::Welcome => {
                let intro = brain.intro(s);
                s.joint_transcript.push(Utterance::mediator(intro.clone()));
                s.events.push("intake".into());
                beats.push(Beat::Mediator(intro));
            }
            // Needs the human: the mediator has asked; we wait for their reply.
            MediatorAction::AskParty(_) => break,
            MediatorAction::CheckBackInterest(_) => break,
            MediatorAction::AcknowledgeEvidence => {
                let msg = brain.acknowledge_evidence(s);
                s.joint_transcript.push(Utterance::mediator(msg.clone()));
                s.evidence_acknowledged = true;
                s.events.push("acknowledge_evidence".into());
                beats.push(Beat::Mediator(msg));
            }
            MediatorAction::SurfaceFactualConflict => {
                let intro = brain.surface_factual_conflict(s);
                let views: Vec<ConflictView> = s
                    .factual_conflicts
                    .iter()
                    .map(|c| ConflictView {
                        about: c.about.clone(),
                        a_name: party_first_name(&s.party_name(&c.between.0)),
                        a_claim: c.claims.0.clone(),
                        b_name: party_first_name(&s.party_name(&c.between.1)),
                        b_claim: c.claims.1.clone(),
                    })
                    .collect();
                s.joint_transcript.push(Utterance::mediator(intro.clone()));
                s.conflicts_surfaced = true;
                s.events.push("factual_conflict".into());
                beats.push(Beat::Mediator(intro));
                beats.push(Beat::Conflict(views));
            }
            MediatorAction::ReflectSharedGround => {
                // FOLDED INTO THE MEDIATOR'S VOICE. The mediator simply says, warmly,
                // what the two already agree on — no certified bullet panel in the
                // party's face. The shared ground lives backstage, in "see the record".
                let msg = brain.shared_ground(s);
                s.joint_transcript.push(Utterance::mediator(msg.clone()));
                s.events.push("shared_ground".into());
                beats.push(Beat::Mediator(msg));
            }
            MediatorAction::NameCrux => {
                // FOLDED INTO THE MEDIATOR'S VOICE. The mediator just *says* "here's
                // the one real question, and it's yours" — no predicate-list panel.
                // The full crux machinery stays backstage in the audit record.
                let msg = brain.crux(s);
                s.joint_transcript.push(Utterance::mediator(msg.clone()));
                s.crux_named = true;
                s.events.push("crux".into());
                beats.push(Beat::Mediator(msg));
            }
            MediatorAction::NameResidue => {
                if s.residue.is_empty() {
                    s.residue = s.residue_candidates();
                }
                // The residue is the signature *visual* — let the panel carry it
                // (it has its own warm framing), rather than also dumping the long
                // bulleted prose as a bubble. Record the spoken line in the
                // transcript for the audit/voice, but render only the panel.
                let msg = brain.name_residue(s);
                let residue = s.residue.clone();
                s.joint_transcript.push(Utterance::mediator(msg));
                s.residue_named = true;
                s.events.push("residue".into());
                beats.push(Beat::Residue(residue));
            }
            MediatorAction::ProposeOptions => {
                let drafts = brain.proposals(s);
                for (i, d) in drafts.into_iter().enumerate() {
                    s.proposals.push(mediator_session::Proposal {
                        id: format!("p{}", i + 1),
                        summary: d.summary,
                        settlement: d.settlement,
                        coherent: true,
                        accepted_by: Vec::new(),
                    });
                }
                let intro = brain.present_proposals(s);
                s.joint_transcript.push(Utterance::mediator(intro.clone()));
                s.events.push("proposals".into());
                beats.push(Beat::Mediator(intro));
                beats.push(Beat::Options(option_views(s)));
            }
            // Needs the human: invite, then wait for accept / counter / hold.
            MediatorAction::InviteAgreement => break,
            MediatorAction::CoAuthorAgreement => {
                // Open the draft (idempotent) and present it for the parties to
                // shape; then pause so they can edit and sign in their own words.
                if s.draft_agreement.is_none() {
                    let text = brain.co_author_agreement(s);
                    s.open_draft_agreement(text.clone(), "mediator");
                    s.joint_transcript.push(Utterance::mediator(text));
                }
                beats.push(draft_beat(s));
                break;
            }
            MediatorAction::Escalate => {
                s.escalate("the room asked for a human");
                beats.push(Beat::System(
                    "Handing this to a person, with the full record of everything so far. \
                     You're not starting over — they'll arrive already knowing where you are."
                        .into(),
                ));
                break;
            }
            MediatorAction::Close => {
                let landed = s
                    .proposals
                    .iter()
                    .find(|p| !p.accepted_by.is_empty())
                    .cloned()
                    .map(|p| brain.closing(s, &p))
                    .unwrap_or_else(|| {
                        "You've done the hard part — staying at the table. Everything \
                         you leaned on was checked; the rest is yours."
                            .to_string()
                    });
                beats.push(Beat::Landed(landed));
                break;
            }
        }
    }
    beats
}

/// Map this session's open cruxes to kind, dispute-faithful questions. Prefers
/// the multi-crux set (`Analysis.cruxes`); falls back to the single crux.
fn crux_questions(s: &Session) -> Vec<String> {
    if !s.analysis.cruxes.is_empty() {
        return s
            .analysis
            .cruxes
            .iter()
            .map(|c| crux_view_question(&s.dispute, c))
            .collect();
    }
    match &s.analysis.crux {
        Some(c) => vec![crux_question_from(&s.dispute, &s.analysis, c)],
        None => Vec::new(),
    }
}

fn option_views(s: &Session) -> Vec<OptionView> {
    s.proposals
        .iter()
        .enumerate()
        .map(|(i, p)| OptionView {
            idx: i,
            summary: p.summary.clone(),
            envy_free: p.settlement.as_ref().map(|x| x.envy_free).unwrap_or(false),
            equitable: p.settlement.as_ref().map(|x| x.equitable).unwrap_or(false),
        })
        .collect()
}

/// Pick the *single most balanced* option to lead with: prefer one that's both
/// envy-free and equitable, then envy-free, then equitable, else the first. The
/// room leads with one calm suggestion rather than three demanding a choice; the
/// others stay one quiet disclosure away.
fn lead_option(opts: &[OptionView]) -> usize {
    let score = |o: &OptionView| (o.envy_free as u8) + (o.equitable as u8);
    // On a tie, keep the *earliest* option (the kernel's first, usually Adjusted
    // Winner): only a strictly higher score displaces the current lead.
    let mut lead = 0;
    let mut best = opts.first().map(score).unwrap_or(0);
    for (i, o) in opts.iter().enumerate().skip(1) {
        let s = score(o);
        if s > best {
            best = s;
            lead = i;
        }
    }
    lead
}

/// One option, rendered plainly: a name, the human summary, and a warm accept.
/// No fairness chips in the party's face — the fairness lives in the record.
fn option_card(sid: &str, party: &str, o: &OptionView, lead: bool) -> Markup {
    html! {
        div .room-opt id=(format!("room-opt-{}", o.idx)) {
            div .room-opt-head {
                strong { @if lead { "A way forward" } @else { "Another way" } }
            }
            p .room-opt-sum { (o.summary) }
            button .room-accept
                hx-post=(format!("/talk/{sid}/{party}/accept/{}", o.idx))
                hx-target="#room" hx-swap="beforeend" {
                "This one works for me"
            }
        }
    }
}

/// Render a sequence of beats to a chat fragment (appended into `#room`).
fn render_beats(sid: &str, party: &str, beats: &[Beat]) -> Markup {
    html! {
        @for b in beats { (render_beat(sid, party, b)) }
    }
}

fn render_beat(sid: &str, party: &str, beat: &Beat) -> Markup {
    match beat {
        Beat::Mediator(t) => html! { div .b.med { span .who { "mediator" } p { (t) } } },
        Beat::System(t) => html! { div .b.sys { p { (t) } } },
        Beat::Conflict(views) => html! {
            // Speech-led, not an adjudication panel: a quiet aside in the room, the
            // colliding accounts set side by side, and the mediator's honest line
            // that this is the one thing it won't decide.
            div .b.med .conflict {
                span .who { "mediator" }
                @for c in views {
                    p .conflict-about { "On " strong { (c.about) } ", the two of you remember it differently:" }
                    div .conflict-claims {
                        div .conflict-claim { span .who { (c.a_name) } p { "“" (c.a_claim) "”" } }
                        div .conflict-claim { span .who { (c.b_name) } p { "“" (c.b_claim) "”" } }
                    }
                }
                p {
                    "That's a question of fact, and a question of fact needs evidence — "
                    "not argument, and not me. I won't decide which of you is right; that "
                    "wouldn't be fair or honest. We'll keep working everything that doesn't hang on it."
                }
            }
        },
        Beat::Residue(items) => html! {
            // KEPT CENTRAL. The subtraction the parties themselves uncovered — the
            // money is handled; here's what was never about money. Plain, quiet,
            // beautiful; no machine vocabulary in the party's face.
            div .panel .panel-residue {
                div .panel-head {
                    span .residue-mark { "—" }
                    span .panel-kicker { "the money is handled. here's what was never about money." }
                }
                ul .residue-list { @for it in items { li { (it) } } }
                p .panel-note {
                    "The money part is settled, and it's fair — that's done. What's left "
                    "isn't something a number can settle, and I won't pretend it could. But "
                    "naming it is worth something: it's the real thing, and it's yours."
                }
            }
        },
        Beat::Options(opts) => {
            // LEAD WITH ONE. The room offers a single, balanced way forward — calm,
            // not a wall of three demanding a choice. The other fair splits are one
            // quiet disclosure away for anyone who wants to compare.
            let lead = lead_option(opts);
            let rest: Vec<&OptionView> = opts.iter().enumerate().filter(|(i, _)| *i != lead).map(|(_, o)| o).collect();
            html! {
                div .room-options {
                    @if let Some(o) = opts.get(lead) { (option_card(sid, party, o, true)) }
                    @if !rest.is_empty() {
                        details .more-options {
                            summary { @if rest.len() == 1 { "see another fair split" } @else { "see " (rest.len()) " other fair splits" } }
                            @for o in &rest { (option_card(sid, party, o, false)) }
                        }
                    }
                    p .options-note { "It's a fair starting point, and it works whichever way the open question goes. Take it, change it, or hold — your call." }
                }
            }
        },
        Beat::Draft { text, signed, parties } => {
            let revise_url = format!("/talk/{sid}/{party}/revise");
            let sign_url = format!("/talk/{sid}/{party}/sign");
            let all_signed = !parties.is_empty() && parties.iter().all(|(id, _)| signed.contains(id));
            html! {
                div #draft .panel .panel-draft {
                    div .panel-head { span .panel-kicker { "write it down together — in your words" } }
                    form .draft-form hx-post=(revise_url) hx-target="#draft" hx-swap="outerHTML" {
                        textarea .draft-text name="message" rows="7" { (text) }
                        div .draft-actions {
                            button .btn .btn-counter type="submit" { "Save these words" }
                            button .btn .btn-accept type="button"
                                hx-post=(sign_url) hx-target="#draft" hx-swap="outerHTML" {
                                @if signed.iter().any(|s| s == party) { "Signed ✓" } @else { "Sign it as it stands" }
                            }
                        }
                    }
                    div .draft-status {
                        @if signed.is_empty() {
                            p .panel-note { "Change any word until it says what you both mean. Editing it clears any signatures — a changed agreement has to be re-owned by both." }
                        } @else {
                            p .panel-note {
                                @if all_signed { "Signed and owned by both: " } @else { "Signed so far: " }
                                @for (i, (id, name)) in parties.iter().enumerate() {
                                    @if i > 0 { ", " }
                                    @let did = signed.contains(id);
                                    span .sign-name .signed[did] { (name) @if did { " ✓" } }
                                }
                                ". I only check it doesn't contradict the facts already settled — I never decide it's the right outcome. That was always yours."
                            }
                        }
                    }
                }
            }
        }
        Beat::Landed(t) => html! {
            div .panel .panel-landed {
                div .panel-head { span .panel-kicker { "landed" } }
                p .landed-text { (t) }
            }
        },
    }
}

/// Start the room: the visitor takes a seat and the mediator opens. Renders the
/// full calm page (the chat scaffold, the say box, the evidence drawer).
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
    let other = d
        .dispute
        .parties
        .iter()
        .find(|p| p.id != party_id)
        .map(|p| party_first_name(&p.display_name))
        .unwrap_or_else(|| "the other person".to_string());

    let sid = format!("s{}", state.next_sid.fetch_add(1, Ordering::Relaxed));
    let session = Session::new(d.dispute.clone(), d.analysis.clone(), d.receipts.clone());
    {
        let mut map = state.sessions.write().await;
        if map.len() >= MAX_SESSIONS {
            if let Some(oldest) = map
                .keys()
                .min_by_key(|k| k.trim_start_matches('s').parse::<u64>().unwrap_or(u64::MAX))
                .cloned()
            {
                map.remove(&oldest);
            }
        }
        map.insert(sid.clone(), session);
    }

    let opener = format!(
        "I'm really glad you're here, {name}. Take your time — there's no clock in this \
         room. This part is just between us; nothing you say reaches {other} without your \
         okay. So tell me, in your own words: what's going on?"
    );
    let say_url = format!("/talk/{}/{}/say", sid, party.id);
    let ev_url = format!("/talk/{}/{}/evidence", sid, party.id);

    let markup = page(&format!("The room — {}", d.dispute.title), html! {
        (brandbar(Some(html! { a href=(format!("/dispute/{}", d.id)) { (&d.dispute.title) } })))
        style { (PreEscaped(ROOM_CSS)) }
        header .room-head {
            p .eyebrow { "you are " (party.display_name.clone()) }
            h1 { "The room" }
            p .lede {
                "A calm, private place to be heard. There's no clock and no judgement. "
                "The mediator has no stake in how this lands, and you can pause or ask for "
                "a person any time."
            }
        }

        div #room .room {
            div .b.med { span .who { "mediator" } p { (opener) } }
        }

        form .room-form hx-post=(say_url) hx-target="#room" hx-swap="beforeend"
             "hx-on::after-request"="this.reset(); this.querySelector('textarea').focus(); window.scrollTo(0, document.body.scrollHeight);" {
            textarea .room-in name="message" autocomplete="off" required rows="2"
                  placeholder=(format!("Speak as {name}…  (take your time)")) {}
            button .room-say type="submit" { "Say it" }
        }

        details .evidence-drawer {
            summary { "Add something to the record — your account, a fact, or an exhibit" }
            form .evidence-form hx-post=(ev_url) hx-target="#room" hx-swap="beforeend"
                 "hx-on::after-request"="this.reset(); window.scrollTo(0, document.body.scrollHeight);" {
                div .evidence-row {
                    select .evidence-kind name="kind" {
                        option value="statement" { "My account (my side, in my words)" }
                        option value="fact" { "A specific fact (a number, a date, a measurement)" }
                        option value="artifact" { "An exhibit (a photo, a receipt, a clause)" }
                    }
                }
                input .evidence-text name="text" autocomplete="off"
                      placeholder="What is it? (e.g. “the stain is 30cm across”, or “photo of the carpet”)";
                input .evidence-note name="note" autocomplete="off"
                      placeholder="Optional note or link (for an exhibit)";
                button .btn .btn-counter type="submit" { "Put it on the record" }
            }
            p .evidence-caption {
                "The mediator weighs everything on the record and acknowledges it — so you "
                "feel heard — but it never lets evidence decide the open question. That stays yours."
            }
        }

        p .session-foot {
            @if state.live_llm_enabled { "The mediator's voice is Qwen3-VL 235B — an open model, live. " }
            @else { "The mediator is a scripted preview voice here (the live model runs on the deployed site). " }
            a .record-link href=(format!("/audit/{}", d.id)) { "see the record" }
            " — the quiet, checkable account of everything underneath."
        }
    });
    (StatusCode::OK, markup).into_response()
}

/// The visitor said something. Append it, let the mediator reply in caucus, then
/// **drive the readiness-based flow forward** and render every resulting beat.
async fn talk_say(
    headers: HeaderMap,
    conn: Option<ConnectInfo<SocketAddr>>,
    Path((sid, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
    Form(form): Form<TalkForm>,
) -> impl IntoResponse {
    let msg = form.message.trim().to_string();
    if msg.is_empty() {
        return (StatusCode::OK, html! {}).into_response();
    }
    if msg.chars().count() > state.max_input_chars {
        return (
            StatusCode::OK,
            html! { div .b.sys { p { "(let's keep it to a few sentences at a time — say a little, and we'll go from there)" } } },
        )
            .into_response();
    }

    // Per-IP + global rate limit on this model-calling endpoint.
    let ip = client_ip(&headers, conn.map(|c| c.0));
    if !state.rate_limiter.allow(&ip) {
        return (StatusCode::OK, one_moment()).into_response();
    }

    let session = {
        let map = state.sessions.read().await;
        match map.get(&sid) {
            Some(s) => s.clone(),
            None => return (StatusCode::OK, expired_fragment()).into_response(),
        }
    };

    let live = state.live_llm_enabled;
    let p2 = party_id.clone();
    let m2 = msg.clone();

    // All brain work + session driving happens off the async executor and is
    // returned as data; maud rendering stays out of the blocking task.
    let driven = tokio::task::spawn_blocking(move || {
        let brain = make_brain(live);
        let mut s = session;

        // Was the mediator mid-check-back? Then this turn confirms the interest.
        let pending_check = s.needs_interest_check_back();
        // Has this party already confirmed an interest? Then we don't open a fresh
        // check-back loop on later turns — the interest is owned; we just reflect.
        let already_confirmed = s
            .parties
            .iter()
            .find(|t| t.id == p2)
            .map(|t| t.confirmed_interest.is_some())
            .unwrap_or(false);

        let mv = brain.caucus(&s, &p2, &m2);
        if let Some(th) = s.parties.iter_mut().find(|t| t.id == p2) {
            th.caucus.push(Utterance::party(&p2, m2.clone()));
            th.caucus.push(Utterance::mediator(mv.reply.clone()));
            for i in &mv.interests {
                if !th.interests.contains(i) {
                    th.interests.push(i.clone());
                }
            }
            if mv.heard_fully {
                th.heard_fully = true;
            }
        }

        let mut beats = vec![Beat::Mediator(mv.reply)];

        // If a check-back was owed, the human's message just confirmed it (their
        // words win, per the brain's discipline) — promote it and move on.
        if pending_check.as_deref() == Some(p2.as_str()) {
            s.confirm_interest(&p2, Some(m2.clone()));
        }

        // The brain may newly name an interest to check back — but only open that
        // loop once per party. If they've already confirmed one (this turn or
        // earlier), we don't keep re-checking; the interest is owned and we move on.
        let just_confirmed = pending_check.as_deref() == Some(p2.as_str());
        if let Some(interest) = mv.interest_to_check {
            if !already_confirmed && !just_confirmed {
                s.name_interest(&p2, &interest);
                let cb = brain.check_back_interest(&s, &p2);
                if let Some(th) = s.parties.iter_mut().find(|t| t.id == p2) {
                    th.caucus.push(Utterance::mediator(cb.clone()));
                }
                beats.push(Beat::Mediator(cb));
            }
        }

        // Now run the autonomous moves the room is ready for.
        beats.extend(drive(&mut s, brain.as_ref()));
        (s, beats)
    })
    .await
    .expect("room turn task");

    let (new_session, beats) = driven;
    {
        let mut map = state.sessions.write().await;
        map.insert(sid.clone(), new_session);
    }

    let frag = html! {
        div .b.party { span .who { "you" } p { (msg) } }
        (render_beats(&sid, &party_id, &beats))
    };
    (StatusCode::OK, frag).into_response()
}

#[derive(serde::Deserialize)]
struct EvidenceForm {
    kind: String,
    text: String,
    #[serde(default)]
    note: String,
}

/// The visitor adds evidence to the record. Stored on their thread; the mediator
/// re-weaves and acknowledges it (and, if it collides with the other side's fact,
/// surfaces the honest factual conflict). Drives the flow so the acknowledgement
/// and any conflict surface appear immediately.
async fn talk_evidence(
    headers: HeaderMap,
    conn: Option<ConnectInfo<SocketAddr>>,
    Path((sid, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
    Form(form): Form<EvidenceForm>,
) -> impl IntoResponse {
    let text = form.text.trim().to_string();
    if text.is_empty() {
        return (StatusCode::OK, html! {}).into_response();
    }
    if text.chars().count() > state.max_input_chars {
        return (
            StatusCode::OK,
            html! { div .b.sys { p { "(let's keep each exhibit short — a line or two)" } } },
        )
            .into_response();
    }
    let ip = client_ip(&headers, conn.map(|c| c.0));
    if !state.rate_limiter.allow(&ip) {
        return (StatusCode::OK, one_moment()).into_response();
    }

    let note = {
        let n = form.note.trim();
        if n.is_empty() { None } else { Some(n.chars().take(state.max_input_chars).collect::<String>()) }
    };
    let ev = match form.kind.as_str() {
        "fact" => Evidence::fact(&party_id, text.clone()),
        "artifact" => Evidence::artifact(&party_id, text.clone(), note),
        _ => {
            let mut e = Evidence::statement(&party_id, text.clone());
            e.note = note;
            e
        }
    };
    let kind_word = match ev.kind {
        EvidenceKind::Statement => "your account",
        EvidenceKind::Fact => "that fact",
        EvidenceKind::Artifact => "that exhibit",
    }
    .to_string();
    let ev_render = ev.render();

    let session = {
        let map = state.sessions.read().await;
        match map.get(&sid) {
            Some(s) => s.clone(),
            None => return (StatusCode::OK, expired_fragment()).into_response(),
        }
    };

    let live = state.live_llm_enabled;
    let driven = tokio::task::spawn_blocking(move || {
        let brain = make_brain(live);
        let mut s = session;
        let stored = s.submit_evidence(ev).is_ok();
        let mut beats = Vec::new();
        if stored {
            beats.push(Beat::System(format!("Added to the record — {ev_render}.")));
            beats.extend(drive(&mut s, brain.as_ref()));
        } else {
            beats.push(Beat::System(
                "I couldn't attach that — let's keep going and you can tell me about it.".into(),
            ));
        }
        (s, beats, stored)
    })
    .await
    .expect("evidence task");

    let (new_session, beats, _stored) = driven;
    {
        let mut map = state.sessions.write().await;
        map.insert(sid.clone(), new_session);
    }

    let frag = html! {
        div .b.party { span .who { "you" } p { "I'd like to put " (kind_word) " on the record: " (text) } }
        (render_beats(&sid, &party_id, &beats))
    };
    (StatusCode::OK, frag).into_response()
}

/// The visitor accepts a certified-fair option in-session. Records it, then opens
/// the co-authored agreement (the parties make the words theirs and sign).
async fn talk_accept(
    Path((sid, party_id, idx)): Path<(String, String, usize)>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let live = state.live_llm_enabled;
    let session = {
        let map = state.sessions.read().await;
        match map.get(&sid) {
            Some(s) => s.clone(),
            None => return (StatusCode::OK, expired_fragment()).into_response(),
        }
    };
    let pid = party_id.clone();
    let driven = tokio::task::spawn_blocking(move || {
        let brain = make_brain(live);
        let mut s = session;
        let mut beats = Vec::new();
        if let Some(p) = s.proposals.get_mut(idx) {
            if !p.accepted_by.contains(&pid) {
                p.accepted_by.push(pid.clone());
            }
            beats.push(Beat::System(format!(
                "Noted — Option {} works for you. The other party will see that you're \
                 okay with it, never your reasons.",
                idx + 1
            )));
        }
        beats.extend(drive(&mut s, brain.as_ref()));
        (s, beats)
    })
    .await
    .expect("accept task");
    let (new_session, beats) = driven;
    {
        let mut map = state.sessions.write().await;
        map.insert(sid.clone(), new_session);
    }
    (StatusCode::OK, render_beats(&sid, &party_id, &beats)).into_response()
}

/// The visitor revises the co-authored agreement text (their words). Clears
/// signatures — a changed agreement must be re-owned by both — and re-renders the
/// draft panel in place. (No model call here, so no rate-limit; the length cap
/// still bounds the input.)
async fn talk_revise(
    Path((sid, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
    Form(form): Form<TalkForm>,
) -> impl IntoResponse {
    let text = form.message.trim().to_string();
    if text.is_empty() {
        return (StatusCode::OK, expired_fragment()).into_response();
    }
    // The agreement can be a paragraph; allow generous room, still bounded.
    let text: String = text.chars().take(state.max_input_chars.max(2000)).collect();

    let mut map = state.sessions.write().await;
    let Some(s) = map.get_mut(&sid) else {
        drop(map);
        return (StatusCode::OK, expired_fragment()).into_response();
    };
    s.revise_draft(&party_id, text);
    let beat = draft_beat(s);
    drop(map);
    (StatusCode::OK, render_beat(&sid, &party_id, &beat)).into_response()
}

/// The visitor signs the current agreement text. If both have now signed, the
/// room lands; otherwise the draft panel updates to show who's signed.
async fn talk_sign(
    Path((sid, party_id)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let live = state.live_llm_enabled;
    let session = {
        let map = state.sessions.read().await;
        match map.get(&sid) {
            Some(s) => s.clone(),
            None => return (StatusCode::OK, expired_fragment()).into_response(),
        }
    };
    let pid = party_id.clone();
    let driven = tokio::task::spawn_blocking(move || {
        let brain = make_brain(live);
        let mut s = session;
        s.sign_draft(&pid);
        let draft = draft_beat(&s);
        let mut beats = vec![draft];
        if s.agreement_owned() {
            beats.extend(drive(&mut s, brain.as_ref()));
        }
        (s, beats)
    })
    .await
    .expect("sign task");
    let (new_session, beats) = driven;
    {
        let mut map = state.sessions.write().await;
        map.insert(sid.clone(), new_session);
    }
    // The first beat re-renders #draft in place; any landed beat appends after.
    let head = render_beat(&sid, &party_id, &beats[0]);
    let rest = render_beats(&sid, &party_id, &beats[1..]);
    (StatusCode::OK, html! { (head) (rest) }).into_response()
}

/// Build the draft beat for a session (the co-authoring panel, with sign state).
fn draft_beat(s: &Session) -> Beat {
    Beat::Draft {
        text: s.draft_agreement.as_ref().map(|d| d.text.clone()).unwrap_or_default(),
        signed: s.draft_agreement.as_ref().map(|d| d.signed_by.clone()).unwrap_or_default(),
        parties: s
            .parties
            .iter()
            .map(|t| (t.id.clone(), party_first_name(&t.display_name)))
            .collect(),
    }
}

fn expired_fragment() -> Markup {
    html! {
        div .b.sys { p { "This conversation has wound down — start a fresh one from the seat picker whenever you're ready." } }
    }
}

const ROOM_CSS: &str = r#"
/* The room is open space, not a boxed widget: no surrounding frame, generous
   air between turns, an unhurried calm. The cathedral stays backstage. */
.room-head { margin-bottom: 2rem; }
.room-head h1 { font-family: var(--serif); font-size: var(--t-3); font-weight: 600; letter-spacing: -.015em; }
.room {
  display: flex; flex-direction: column; gap: 1.5rem;
  min-height: 8rem; padding: .5rem 0 1rem; margin-bottom: 1.5rem;
}
.room .b { max-width: 38rem; padding: .9rem 1.15rem; border-radius: 18px; }
.room .b p { margin: 0; white-space: pre-wrap; line-height: 1.65; }
.room .b p + p { margin-top: .7rem; }
.room .b .who { display: block; font-size: .68rem; text-transform: uppercase; letter-spacing: .07em; color: var(--muted); margin-bottom: .3rem; }
.room .b.med { background: var(--surface); align-self: flex-start; box-shadow: var(--shadow); }
.room .b.party { background: var(--accent); color: #fff; align-self: flex-end; }
.room .b.party .who { color: rgba(255,255,255,.8); }
.room .b.sys { background: transparent; align-self: center; font-style: italic; color: var(--muted); text-align: center; max-width: 36rem; }
.room .b.sys.one-moment { color: var(--amber); }

/* The factual-conflict moment: speech, not adjudication. It rides inside a
   mediator bubble; the two remembered accounts sit quietly side by side. */
.room .b.med.conflict { max-width: 42rem; }
.conflict-about { color: var(--text-2); margin: .2rem 0 .6rem; }
.conflict-claims { display: grid; gap: .5rem; margin: .2rem 0 .8rem; }
.conflict-claim { background: var(--blue-bg); border-radius: var(--radius-sm); padding: .55rem .8rem; }
.conflict-claim .who { display: block; font-size: .66rem; text-transform: uppercase; letter-spacing: .06em; color: var(--blue); font-weight: 700; margin-bottom: .2rem; }
.conflict-claim p { margin: 0; }

.room-form { display: flex; gap: .55rem; align-items: flex-end; margin: 0 0 .9rem; }
.room-in {
  flex: 1; resize: vertical; min-height: 2.8rem; font: inherit; line-height: 1.5;
  padding: .75rem .9rem; border-radius: 14px; border: 1px solid var(--border-2);
  background: var(--surface); color: var(--text);
}
.room-in:focus { outline: none; border-color: var(--accent); }
.room-say { padding: .75rem 1.3rem; border-radius: 14px; border: 0; background: var(--accent); color: #fff; font: inherit; font-weight: 600; cursor: pointer; transition: background .12s; }
.room-say:hover { background: var(--accent-2); }

/* The evidence drawer stays a quiet, closed affordance. */
.evidence-drawer { margin: .2rem 0 1.6rem; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); }
.evidence-drawer > summary { cursor: pointer; padding: .85rem 1rem; color: var(--text-2); font-size: var(--t--1); list-style: none; }
.evidence-drawer > summary::-webkit-details-marker { display: none; }
.evidence-drawer > summary::before { content: "＋ "; color: var(--accent); }
.evidence-drawer[open] > summary::before { content: "－ "; }
.evidence-drawer[open] > summary { border-bottom: 1px solid var(--border); }
.evidence-form { display: grid; gap: .55rem; padding: 1rem; }
.evidence-row { display: flex; gap: .5rem; }
.evidence-kind, .evidence-text, .evidence-note {
  font: inherit; padding: .55rem .7rem; border-radius: var(--radius-sm);
  border: 1px solid var(--border-2); background: var(--bg); color: var(--text); width: 100%;
}
.evidence-caption { padding: 0 1rem 1rem; font-size: var(--t--1); color: var(--muted); font-style: italic; }

/* The two things the parties themselves shaped — the subtraction and the
   agreement — are the only things that earn a real surface. Soft, central,
   roomy; a hairline rather than a hard box. */
.panel { border: 1px solid var(--border); border-radius: var(--radius); padding: 1.6rem 1.7rem; margin: .4rem 0; background: var(--surface); box-shadow: var(--shadow); align-self: stretch; }
.panel-head { display: flex; align-items: center; gap: .5rem; margin-bottom: 1rem; }
.panel-kicker { font-size: var(--t--1); text-transform: uppercase; letter-spacing: .07em; color: var(--muted); font-weight: 700; }
.panel-note { margin-top: 1.1rem; color: var(--text-2); font-size: var(--t--1); line-height: 1.6; }

/* the signature subtraction panel — quiet, central, beautiful */
.panel-residue { background: linear-gradient(180deg, var(--surface), var(--bg-2)); border-color: transparent; padding: 1.9rem 1.8rem; }
.panel-residue .residue-mark { font-family: var(--serif); font-size: var(--t-2); color: var(--accent); line-height: 1; }
.panel-residue .panel-kicker { color: var(--text-2); }
.residue-list { list-style: none; display: grid; gap: .8rem; margin-top: .4rem; }
.residue-list li { font-family: var(--serif); font-size: var(--t-1); line-height: 1.45; color: var(--text); padding-left: 1.2rem; border-left: 2px solid var(--accent); }

/* options: one calm suggestion, the rest one quiet disclosure away */
.room-options { align-self: stretch; margin: .4rem 0; }
.room-opt { padding: .2rem 0 .4rem; }
.room-opt-head { margin-bottom: .35rem; }
.room-opt-head strong { font-family: var(--serif); font-size: var(--t-1); font-weight: 600; }
.room-opt-sum { color: var(--text-2); font-size: var(--t-0); line-height: 1.55; }
.room-accept { margin-top: .9rem; padding: .55rem 1.15rem; border: 1px solid var(--accent); border-radius: var(--radius-sm); background: transparent; color: var(--accent); font: inherit; font-weight: 600; cursor: pointer; transition: background .12s, color .12s; }
.room-accept:hover { background: var(--accent); color: #fff; }
.more-options { margin-top: 1.1rem; }
.more-options > summary { cursor: pointer; color: var(--muted); font-size: var(--t--1); list-style: none; }
.more-options > summary::-webkit-details-marker { display: none; }
.more-options > summary::before { content: "› "; }
.more-options[open] > summary::before { content: "⌄ "; }
.more-options .room-opt { margin-top: 1rem; padding-top: 1rem; border-top: 1px solid var(--border); }
.options-note { margin-top: 1.2rem; color: var(--text-2); font-size: var(--t--1); line-height: 1.6; }

.panel-draft .draft-form { display: grid; gap: .8rem; }
.draft-text { font: inherit; line-height: 1.65; padding: 1rem 1.1rem; border-radius: var(--radius-sm); border: 1px solid var(--border-2); background: var(--bg); color: var(--text); resize: vertical; }
.draft-text:focus { outline: none; border-color: var(--accent); }
.draft-actions { display: flex; gap: .6rem; flex-wrap: wrap; }
.sign-name.signed { color: var(--green); font-weight: 700; }

.panel-landed { background: var(--green-bg); border-color: transparent; text-align: center; padding: 1.9rem 1.7rem; }
.panel-landed .panel-head { justify-content: center; }
.panel-landed .landed-text { font-family: var(--serif); font-size: var(--t-1); line-height: 1.55; color: var(--text); white-space: pre-wrap; }

.session-foot .record-link { color: var(--text-2); text-decoration: underline; text-underline-offset: 2px; }

@media (max-width: 560px) {
  .room .b { max-width: 100%; }
  .room-form { flex-direction: column; align-items: stretch; }
}
"#;

// ───────────────────────────── the audit record ─────────────────────────────

fn audit_record_for(
    d: &LoadedDispute,
    key: &ed25519_dalek::SigningKey,
) -> mediator_audit::MediationRecord {
    let mut events = vec![(
        "mediation_opened".to_string(),
        serde_json::json!({
            "dispute": d.id,
            "title": d.dispute.title,
            "parties": d.dispute.parties.iter().map(|p| &p.id).collect::<Vec<_>>(),
        }),
    )];
    // Record every contested question handed back — the *set* of cruxes (a real
    // dispute can have several), each explicitly NOT decided by the kernel.
    if !d.analysis.cruxes.is_empty() {
        for c in &d.analysis.cruxes {
            events.push((
                "crux_handed_back".to_string(),
                serde_json::json!({
                    "predicate": c.predicate,
                    "question": c.question,
                    "verdict": c.verdict,
                    "decided_by_kernel": false,
                }),
            ));
        }
    } else {
        events.push((
            "crux_handed_back".to_string(),
            serde_json::json!({ "crux": d.analysis.crux, "decided_by_kernel": false }),
        ));
    }
    mediator_audit::build(&d.receipts, &events, key)
}

async fn audit_view(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&id) else {
        return not_found("We don't have a record of that dispute.");
    };
    let record = audit_record_for(d, &state.audit_key);
    let verified = mediator_audit::verify(&record);

    let markup = page(&format!("Audit record — {}", d.dispute.title), html! {
        (brandbar(Some(html! { (&d.dispute.title) })))
        style { (PreEscaped(AUDIT_CSS)) }
        header .interior-head {
            h1 { "The record" }
            p .lede {
                "Every fact this mediation leaned on, in a tamper-evident chain — each "
                "link sealed by the hash of the one before it, the whole thing signed. "
                "Anyone can check it wasn't altered after the fact. This is what lets a "
                "process be trusted without a referee in the room."
            }
        }
        section {
            @match &verified {
                Ok(()) => div .verify-ok { "✓ verified — this record is internally consistent and the signature is valid; it has not been altered since it was sealed." },
                Err(e) => div .verify-bad { "✗ verification failed: " (e.to_string()) },
            }
        }
        section {
            table .audit-table {
                thead { tr { th { "#" } th { "step" } th { "hash" } th { "verdict" } } }
                tbody {
                    @for e in &record.entries {
                        tr {
                            td { (e.seq) }
                            td { (e.kind) }
                            td .mono { (short_hex(&e.hash)) }
                            td { @if let Some(v) = &e.verdict { (v) } @else { "—" } }
                        }
                    }
                }
            }
        }
        section .audit-sig {
            div { span .k { "public key" } span .mono { (short_hex(&record.public_key)) } }
            div { span .k { "signature" } span .mono { (short_hex(&record.signature)) } }
            a .dl href=(format!("/audit/{}/download", d.id)) { "Download the signed record (JSON) →" }
        }
        p .session-foot { "Signature: ed25519 · chain: sha256. Verify it yourself with the downloaded record." }
    });
    (StatusCode::OK, markup).into_response()
}

async fn audit_download(
    Path(id): Path<String>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let Some(d) = state.get(&id) else {
        return (StatusCode::NOT_FOUND, "unknown dispute").into_response();
    };
    let record = audit_record_for(d, &state.audit_key);
    let json = serde_json::to_string_pretty(&record).unwrap_or_default();
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/json"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"mediation-record.json\"",
            ),
        ],
        json,
    )
        .into_response()
}

fn short_hex(s: &str) -> String {
    if s.len() > 20 {
        format!("{}…{}", &s[..10], &s[s.len() - 6..])
    } else {
        s.to_string()
    }
}

const AUDIT_CSS: &str = r#"
.verify-ok { background:#e9f3ec; color:#2f6b46; border:1px solid #cfe6d6; padding:.7rem 1rem; border-radius:12px; }
.verify-bad { background:#fbeaea; color:#8a2b2b; border:1px solid #efcccc; padding:.7rem 1rem; border-radius:12px; }
.audit-table { width:100%; border-collapse:collapse; margin:1rem 0; font-size:.92rem; }
.audit-table th, .audit-table td { text-align:left; padding:.45rem .6rem; border-bottom:1px solid #00000010; }
.audit-table th { opacity:.6; font-weight:500; }
.mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
.audit-sig { margin-top:1rem; display:flex; flex-direction:column; gap:.4rem; }
.audit-sig .k { display:inline-block; width:7rem; opacity:.6; }
.audit-sig .dl { display:inline-block; margin-top:.6rem; }
"#;

// ─────────────────────────────── the gallery ─────────────────────────────────

async fn gallery(State(state): State<SharedState>) -> Markup {
    page("Disputes", html! {
        header .hero {
            div .hero-mark { (WORDMARK) }
            h1 .hero-title { "A calm room for a hard conversation." }
            p .hero-lede {
                "Sit down with a patient, impartial mediator that has no stake in how "
                "this lands. It hears you out, reflects back what matters, clears away "
                "what was never really the fight, and hands the one honest question "
                "back to you. Nothing is decided for you."
            }
            @if !state.disputes.is_empty() || !state.pacts.is_empty() {
                div .hero-cta {
                    style { (PreEscaped(".hero-cta{display:flex;gap:.7rem;flex-wrap:wrap;justify-content:center;margin-top:1.4rem}.hero-cta a{padding:.66rem 1.15rem;border-radius:12px;text-decoration:none;font-weight:600}.cta-primary{background:var(--accent);color:#fff}.cta-primary:hover{background:var(--accent-2)}.cta-secondary{border:1px solid var(--border-2);color:inherit}.cta-secondary:hover{background:var(--bg-2)}")) }
                    @if let Some(first) = state.disputes.first() {
                        @let first_party = first.dispute.parties.first().map(|p| p.id.clone()).unwrap_or_default();
                        a .cta-primary href=(format!("/talk/{}/{}", first.id, first_party)) { "Enter the room →" }
                        a .cta-secondary href=(format!("/session/{}", first.id)) { "Or watch a full mediation" }
                    }
                    @if !state.pacts.is_empty() {
                        // The pact CTA is the primary call when no disputes are loaded.
                        @let cls = if state.disputes.is_empty() { "cta-primary" } else { "cta-secondary" };
                        a .(cls) href="/pacts" { "Settle it in advance" }
                    }
                }
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
                            @let n = crux_count(&d.analysis);
                            @if n == 1 {
                                span .chip .chip-amber { "1 open question" }
                            } @else if n > 1 {
                                span .chip .chip-amber { (n) " open questions" }
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

        // The forward-constitutions invite sits beside the live room regardless of
        // whether any disputes are loaded — it's a door of its own.
        @if !state.pacts.is_empty() {
            section .forward-invite {
                style { (PreEscaped(".forward-invite{margin-top:1.6rem}.forward-invite .fwd-card{display:block;color:inherit;background:var(--bg-2);border:1.5px solid var(--border);border-style:dashed;border-radius:var(--radius);padding:1.4rem 1.5rem;box-shadow:var(--shadow);transition:transform .16s ease,border-color .16s ease,box-shadow .16s ease}.forward-invite .fwd-card:hover{transform:translateY(-2px);border-color:var(--accent);box-shadow:var(--shadow-lg);text-decoration:none}.fwd-eyebrow{font-size:var(--t--1);text-transform:uppercase;letter-spacing:.06em;color:var(--muted);font-weight:700}.fwd-title{font-family:var(--serif);font-size:var(--t-2);font-weight:600;letter-spacing:-.01em;margin-top:.3rem}.fwd-blurb{color:var(--text-2);margin-top:.5rem}.fwd-go{display:inline-block;margin-top:.8rem;color:var(--accent);font-weight:600;font-size:var(--t--1)}")) }
                a .fwd-card href="/pacts" {
                    div .fwd-eyebrow { "before the dispute" }
                    h2 .fwd-title { "Forward constitutions" }
                    p .fwd-blurb {
                        "Or don't wait for the fight at all. Settle, in advance, how every "
                        "situation you can name will be handled — and prove the agreement "
                        "holds before anyone needs it."
                    }
                    span .fwd-go { "see the certified pacts →" }
                }
            }
        }
    })
}

/// How many contested questions a dispute reduces to: the multi-crux set if
/// present, else 1 if a single crux is on record, else 0.
fn crux_count(a: &Analysis) -> usize {
    if !a.cruxes.is_empty() {
        a.cruxes.len()
    } else if a.crux.is_some() {
        1
    } else {
        0
    }
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
            style { (PreEscaped(".talk-invite{margin-top:1.6rem}.talk-invite>p{color:var(--text-2)}.talk-links{display:flex;gap:.6rem;flex-wrap:wrap;margin-top:.65rem}.talk-link{padding:.7rem 1.1rem;border:1.5px solid var(--accent);border-radius:12px;text-decoration:none;color:var(--accent);font-weight:600}.talk-link:hover{background:var(--accent);color:#fff}")) }
            p { strong { "Or step into the room yourself." } " Sit with the mediator, live — be heard, in your own words, with no clock and no judgement:" }
            div .talk-links {
                @for p in &d.dispute.parties {
                    a .talk-link href=(format!("/talk/{}/{}", d.id, p.id)) {
                        "Enter as " (party_first_name(&p.display_name)) " →"
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

        // (3) The open question(s) — the crux(es), phrased kindly. A real dispute
        //     can reduce to several; we show each, handed back, never decided.
        @let crux_qs = crux_questions(&Session::new(d.dispute.clone(), d.analysis.clone(), Vec::new()));
        @if !crux_qs.is_empty() {
            section .reveal .step {
                span .step-n { "3" }
                @if crux_qs.len() == 1 {
                    h2 { "The one question that's really yours" }
                    p .step-lede {
                        "Everything else has been settled or set aside. This is the "
                        "single thing left — and it's not ours to decide. It's a "
                        "judgement only the two of you can make."
                    }
                } @else {
                    h2 { "The questions that are really yours" }
                    p .step-lede {
                        "Everything else has been settled or set aside. These are the "
                        "genuine knots that remain — and they're not ours to decide. "
                        "They're judgements only the two of you can make."
                    }
                }
                @for q in &crux_qs {
                    div .crux-box {
                        p .crux-q { (q) }
                    }
                }
                p .crux-note {
                    @if crux_qs.len() == 1 {
                        "We've confirmed this is the genuine crux: answer it, and the numbers below follow on their own."
                    } @else {
                        "Each of these was confirmed to be a genuine crux: settle them between you, and the numbers below follow on their own."
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
///
/// Decoupled from `LoadedDispute` so both the party view and the room (which
/// holds a `Session`) can reuse it.
fn crux_question_from(dispute: &Dispute, analysis: &Analysis, kernel_crux: &str) -> String {
    if let Some(gloss) = crux_gloss(dispute) {
        let g = gloss.trim().trim_end_matches('.');
        return format!("Is it true that {g}?");
    }
    if let Some(c) = analysis.genuine_conflicts.first() {
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

/// Phrase one specific [`Crux`] (multi-crux case) as a kind question. Prefers the
/// crux predicate's own signature gloss; falls back to the crux's stored
/// `question`, then its predicate symbol prettified.
fn crux_view_question(dispute: &Dispute, crux: &Crux) -> String {
    if let Some(gloss) = predicate_gloss(dispute, &crux.predicate) {
        let g = gloss.trim().trim_end_matches('.');
        return format!("Is it true that {g}?");
    }
    let q = crux.question.trim();
    if !q.is_empty() {
        if let Some(rest) = q.strip_prefix("Whether ") {
            let core = rest.split(" — ").next().unwrap_or(rest).trim();
            return format!("Is it true that {core}?");
        }
        return q.split(" — ").next().unwrap_or(q).trim().to_string();
    }
    format!("Is it true that {}?", prettify_id(&crux.predicate))
}

/// The human gloss for a named predicate symbol, from any party's signature.
fn predicate_gloss(dispute: &Dispute, predicate: &str) -> Option<String> {
    dispute
        .parties
        .iter()
        .flat_map(|p| &p.signature)
        .find(|s| s.name == predicate && !s.gloss.trim().is_empty())
        .map(|s| s.gloss.clone())
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

        section .audit-link {
            style { (PreEscaped(".audit-link{margin:.2rem 0 1rem}.audit-link a{display:inline-block;padding:.55rem .95rem;border:1px solid #d8cbb6;border-radius:10px;text-decoration:none;color:inherit}.audit-link a:hover{background:#fbf6ee}")) }
            a href=(format!("/audit/{}", d.id)) { "🔏 View the signed, tamper-evident record →" }
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
    headers: HeaderMap,
    conn: Option<ConnectInfo<SocketAddr>>,
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

    // Per-IP + global rate limiter (this is a live model call).
    let ip = client_ip(&headers, conn.map(|c| c.0));
    if !state.rate_limiter.allow(&ip) {
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
