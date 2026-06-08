//! `LiveBrain` — the mediator's voice, conducted by a live Bedrock model.
//!
//! The model writes the *human* part: how it greets, reflects, names the knot,
//! and frames the options. It is given the **certified facts** (the shared
//! ground, the dissolved misunderstandings, the one open question) as
//! established truth and told never to invent facts, numbers, or settlement
//! terms — those stay load-bearing from the prover and the fair-division math.
//!
//! Every call falls back to [`ScriptedBrain`] on any error, so a flaky model or
//! missing credentials degrades the *voice*, never the session. This is the
//! "AI does the work; the formalism is the quiet trust spine" split, made real.

use crate::{
    CaucusMove, MediatorBrain, Proposal, ProposalDraft, ScriptedBrain, Session, Speaker,
};
use mediator_types::PartyId;

/// The default mediator model — one consistent, warm voice (Claude Haiku 4.5),
/// distinct from the multi-model council used for *formalizing* claims.
pub const DEFAULT_MEDIATOR_MODEL: &str = "us.anthropic.claude-haiku-4-5-20251001-v1:0";
pub const DEFAULT_REGION: &str = "us-east-1";

pub struct LiveBrain {
    rt: tokio::runtime::Runtime,
    region: String,
    model_id: String,
    temperature: f32,
    max_tokens: i32,
    fallback: ScriptedBrain,
}

impl LiveBrain {
    /// Build a live mediator brain on the default model/region. Returns an error
    /// only if a Tokio runtime can't be created; it does NOT check credentials
    /// (each call falls back to the scripted voice if Bedrock is unreachable).
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            rt: tokio::runtime::Runtime::new()?,
            region: DEFAULT_REGION.to_string(),
            model_id: DEFAULT_MEDIATOR_MODEL.to_string(),
            temperature: 0.5,
            max_tokens: 400,
            fallback: ScriptedBrain,
        })
    }

    pub fn with_model(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// One model call, returning `Some(text)` on success and `None` on any
    /// failure (the caller then uses the scripted fallback).
    fn ask(&self, user: &str) -> Option<String> {
        let fut = mediator_llm::live::converse_text(
            &self.region,
            &self.model_id,
            MEDIATOR_SYSTEM,
            user,
            self.max_tokens,
            self.temperature,
        );
        match self.rt.block_on(fut) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("⚠ live mediator falling back to scripted voice: {e}");
                None
            }
        }
    }
}

impl MediatorBrain for LiveBrain {
    fn intro(&self, s: &Session) -> String {
        let names = party_names(s).join(" and ");
        let user = format!(
            "Open the joint session. The two people are {names}. What they're sorting \
             out: \"{}\". Greet them warmly and briefly, and set expectations: you'll \
             speak with each of them privately first, then find what they already agree \
             on, then look honestly at the one thing they don't; nothing is decided for \
             them; either can ask for a human or stop at any time.",
            s.title
        );
        self.ask(&user).unwrap_or_else(|| self.fallback.intro(s))
    }

    fn caucus(&self, s: &Session, party: &PartyId, input: &str) -> CaucusMove {
        let name = s.party_name(party);
        let prior = caucus_context(s, party);
        let user = format!(
            "You are in a PRIVATE caucus with {name} (the other person cannot see this). \
             {prior}They just said:\n\n\"{input}\"\n\nRespond as the mediator: reflect what \
             you heard, and gently surface the interest *underneath* their position. Warm, \
             a few sentences, ending with a soft check-in. Then, on a FINAL separate line, \
             write exactly:\nINTEREST: <a short phrase naming the underlying interest>"
        );
        match self.ask(&user) {
            Some(text) => {
                let (reply, interest) = split_interest(&text);
                CaucusMove {
                    reply,
                    interests: interest.into_iter().collect(),
                    claims: Vec::new(),
                    done: true,
                }
            }
            None => self.fallback.caucus(s, party, input),
        }
    }

    fn shared_ground(&self, s: &Session) -> String {
        if s.analysis.shared_core.is_empty() {
            return self.fallback.shared_ground(s);
        }
        let mut facts = String::new();
        for f in &s.analysis.shared_core {
            facts.push_str(&format!("- {f}\n"));
        }
        let dissolved = if s.analysis.dissolved.is_empty() {
            String::new()
        } else {
            let mut d = String::from(
                "\nThings that looked like disagreements but were really crossed wires:\n",
            );
            for x in &s.analysis.dissolved {
                d.push_str(&format!("- {x}\n"));
            }
            d
        };
        let user = format!(
            "Move into the joint session and reflect the common ground back to both of \
             them. The certified facts they share:\n{facts}{dissolved}\nSay this back \
             warmly in a few sentences — make the point that they agree on more than the \
             fight made it feel. Speak it like a person; don't just list it.",
        );
        self.ask(&user).unwrap_or_else(|| self.fallback.shared_ground(s))
    }

    fn crux(&self, s: &Session) -> String {
        let Some(crux) = &s.analysis.crux else {
            return self.fallback.crux(s);
        };
        let user = format!(
            "Now name the single genuine disagreement. The certified open question is:\n\
             \"{crux}\"\n\nName it gently. Make it unmistakable that you will NOT decide it \
             — it's theirs to answer, not yours to rule on — and note how small it looks now \
             that everything else is settled. A few sentences.",
        );
        self.ask(&user).unwrap_or_else(|| self.fallback.crux(s))
    }

    fn proposals(&self, s: &Session) -> Vec<ProposalDraft> {
        // The *content* of settlements is certified fair-division, never model-
        // invented — so this delegates to the deterministic brain.
        self.fallback.proposals(s)
    }

    fn present_proposals(&self, s: &Session) -> String {
        if s.proposals.is_empty() {
            return self.fallback.present_proposals(s);
        }
        let mut opts = String::new();
        for (i, p) in s.proposals.iter().enumerate() {
            opts.push_str(&format!("{}. {}\n", i + 1, p.summary));
        }
        let user = format!(
            "Introduce these settlement options to both people. Each was produced by a fair-\
             division procedure (envy-free and equitable) over the certified facts — present \
             them as fair starting points, not a verdict, and remind them they can take one, \
             change it, or hold out. Do not invent or alter any option. The options:\n{opts}",
        );
        self.ask(&user)
            .unwrap_or_else(|| self.fallback.present_proposals(s))
    }

    fn closing(&self, s: &Session, accepted: &Proposal) -> String {
        let user = format!(
            "They've agreed to: \"{}\". Give a warm, brief closing — acknowledge that the \
             hard part was staying at the table, note that everything they agreed to rests \
             on facts that were checked and there's a plain record if either ever wants it, \
             and wish them well. A few sentences.",
            accepted.summary
        );
        self.ask(&user)
            .unwrap_or_else(|| self.fallback.closing(s, accepted))
    }
}

// ───────────────────────────── the soul ──────────────────────────────────

const MEDIATOR_SYSTEM: &str = "\
You are the mediator in a community dispute-resolution session — a warm, impartial, \
unhurried guide helping two people resolve something they would both rather not let \
escalate into something worse. You have no stake in the outcome and no relationship to \
either person. You are not a judge.

How you work:
- You listen completely, and make each person feel genuinely heard before anything else.
- You look past positions to the interests underneath — not \"I want the $400\" but the \
need beneath it: to be respected, to have their effort seen, to be treated as having acted \
in good faith.
- You reflect back the common ground, which is almost always larger than the fight makes it feel.
- You name the genuine disagreement plainly and gently — and you HAND IT BACK. You do not \
decide it; it is theirs, and you say so.
- You keep it kind, plain, and human. No legalese, no therapy jargon, no lectures, no bullet \
points. A few sentences, the way a real person who is good at this would actually speak.

You will be given CERTIFIED FACTS that a trusted checker has already verified — sums, what \
both sides agree on, the single open question. Treat them as established: rely on them, never \
contradict them, and never invent new facts, numbers, or settlement terms.

Either person can pause or ask for a human at any moment, and that is always okay.";

// ──────────────────────────── small helpers ──────────────────────────────

fn party_names(s: &Session) -> Vec<String> {
    s.parties.iter().map(|p| p.display_name.clone()).collect()
}

/// A compact recap of a party's caucus so far, for continuity.
fn caucus_context(s: &Session, party: &PartyId) -> String {
    let Some(th) = s.parties.iter().find(|t| &t.id == party) else {
        return String::new();
    };
    if th.caucus.is_empty() {
        return String::new();
    }
    let mut ctx = String::from("So far in this caucus:\n");
    for u in &th.caucus {
        let who = match &u.speaker {
            Speaker::Mediator => "you".to_string(),
            _ => s.party_name(party),
        };
        ctx.push_str(&format!("  {who}: {}\n", u.text));
    }
    ctx.push('\n');
    ctx
}

/// Split a model reply into (reply_without_interest_line, optional_interest).
fn split_interest(text: &str) -> (String, Option<String>) {
    let mut interest = None;
    let mut kept = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t
            .strip_prefix("INTEREST:")
            .or_else(|| t.strip_prefix("Interest:"))
        {
            let v = rest.trim();
            if !v.is_empty() {
                interest = Some(v.to_string());
            }
        } else {
            kept.push(line);
        }
    }
    (kept.join("\n").trim().to_string(), interest)
}
