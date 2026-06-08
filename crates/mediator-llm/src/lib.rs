//! `mediator-llm` — the untrusted operator + council. STUB for the swarm.
//!
//! Clients for AWS Bedrock (the `commonquant-ember` account) and a local
//! LM Studio endpoint (OpenAI-compatible, `http://localhost:1234`). Provides
//! the agent loop that proposes formalizations and the council aggregation
//! (diverse models, framing-bias-resistant: evaluate the *normalized formal*
//! representation, never the prose). MUST degrade gracefully: a
//! `ScriptedOperator` returns pre-baked formalizations so the demo runs with
//! no network and no credentials.

use mediator_types::{Formula, LlmOperator, Sig};

/// Offline fallback so the demo always runs. Returns pre-baked formalizations.
pub struct ScriptedOperator;

impl LlmOperator for ScriptedOperator {
    fn formalize(&self, _nl: &str, _sig: &[Sig]) -> Result<Formula, String> {
        todo!("swarm: pre-baked formalizations for the roommate scenario")
    }
    fn render_english(&self, _f: &Formula) -> String {
        todo!("swarm")
    }
}
