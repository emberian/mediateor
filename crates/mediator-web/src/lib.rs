//! `mediator-web` — a simple, delightful, minimalist **htmx** front end.
//! STUB for the swarm.
//!
//! Server-rendered HTML (maud) + htmx for the little interactions. Two faces of
//! one `Analysis`, same as the TUI: a kind party view and an operator cockpit.
//! No build step, no SPA — just warm pages a frightened roommate could love.

use axum::Router;
use mediator_types::{Analysis, Dispute};

/// What the server renders from. Decoupled from the kernel: the binary computes
/// the `Analysis` (via the real pipeline) and hands it here.
pub struct AppState {
    pub dispute: Dispute,
    pub analysis: Analysis,
}

/// Build the axum router for the given state.
pub fn router(_state: AppState) -> Router {
    todo!("swarm: maud + htmx views over Analysis; vendor htmx into static/")
}
