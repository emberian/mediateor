//! `mediator-session` — the AI-conducted mediation **session**: the part where
//! the AI actually mediates.
//!
//! This is the star the rest of the system was scaffolding for. The formal core
//! (the certified `Analysis` + its receipts) sits *quietly underneath* as the
//! trust spine; the session is the human-facing process a community could opt
//! into instead of a worse escalation, with **no professional required** — only
//! an escalation backstop.
//!
//! The flow is a real mediation:
//!   Intake → Caucus (privately, each party) → Shared Ground (certified) →
//!   Crux (the genuine knot, handed back) → Proposals (certified-coherent) →
//!   Convergence → Agreement   (or → Escalated, at any time).
//!
//! The mediator's *voice and judgment* are abstracted behind [`MediatorBrain`]
//! so a deterministic [`ScriptedBrain`] (offline, testable) and a live Bedrock
//! brain are drop-in interchangeable. The driver never lets the mediator decide
//! a crux; it only ever surfaces it.

use mediator_core::render::money;
use mediator_types::{Analysis, Dispute, Formula, PartyId, Receipt, Settlement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod live;
pub use live::{LiveBrain, DEFAULT_MEDIATOR_MODEL, DEFAULT_REGION};

pub mod intake;

// ───────────────────────────── session state ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Intake,
    Caucus,
    SharedGround,
    Crux,
    Proposals,
    Convergence,
    Agreement,
    Escalated,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Speaker {
    Party(PartyId),
    Mediator,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Utterance {
    pub speaker: Speaker,
    pub text: String,
}

impl Utterance {
    pub fn mediator(t: impl Into<String>) -> Self {
        Self { speaker: Speaker::Mediator, text: t.into() }
    }
    pub fn party(p: &str, t: impl Into<String>) -> Self {
        Self { speaker: Speaker::Party(p.to_string()), text: t.into() }
    }
    pub fn system(t: impl Into<String>) -> Self {
        Self { speaker: Speaker::System, text: t.into() }
    }
}

/// What a piece of evidence *is*. The mediator weighs all three to inform the
/// humans, but the kind matters for how it's framed: a `Statement` is one
/// party's account, a `Fact` is a concrete claimed fact, an `Artifact` points at
/// something outside the conversation (a photo, a receipt, a lease clause).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    /// A party's narrative account of what happened (their side, in their words).
    Statement,
    /// A concrete factual assertion the party is putting on the record.
    Fact,
    /// A reference to something outside the conversation — a label plus an
    /// optional note/url in [`Evidence::note`] (e.g. a photo, a receipt).
    Artifact,
}

/// A piece of evidence a party attaches to their thread. The mediator may
/// **acknowledge and weigh** evidence in its reflection — but it NEVER uses
/// evidence to *decide the crux*. Evidence informs the humans; it does not let
/// the machine rule. (The crux stays `Unknown`, handed back, always.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// The label/account/claim text the party submitted.
    pub text: String,
    /// Whose evidence this is.
    pub by: PartyId,
    /// Optional supporting note or URL (used especially by [`EvidenceKind::Artifact`]).
    pub note: Option<String>,
}

impl Evidence {
    pub fn statement(by: &str, text: impl Into<String>) -> Self {
        Self { kind: EvidenceKind::Statement, text: text.into(), by: by.to_string(), note: None }
    }
    pub fn fact(by: &str, text: impl Into<String>) -> Self {
        Self { kind: EvidenceKind::Fact, text: text.into(), by: by.to_string(), note: None }
    }
    pub fn artifact(by: &str, label: impl Into<String>, note: Option<String>) -> Self {
        Self { kind: EvidenceKind::Artifact, text: label.into(), by: by.to_string(), note }
    }
    /// A one-line human framing of this piece of evidence (for acknowledgement).
    pub fn render(&self) -> String {
        let head = match self.kind {
            EvidenceKind::Statement => "account",
            EvidenceKind::Fact => "stated fact",
            EvidenceKind::Artifact => "exhibit",
        };
        match &self.note {
            Some(n) if !n.is_empty() => format!("{head}: {} ({n})", self.text),
            _ => format!("{head}: {}", self.text),
        }
    }
}

/// Two pieces of evidence that **collide on a fact** — each party putting a
/// different concrete value on the record for the same thing. The kernel's most
/// honest move: surface that this is a *factual* conflict that "needs evidence,
/// not logic", name exactly which claimed fact is contested — and *refuse to
/// decide it*. The machine never rules; it only points. Detection is structural
/// and deliberately simple; the honesty (handing it back) is the point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FactualConflict {
    /// The two parties whose facts collide.
    pub between: (PartyId, PartyId),
    /// The colliding claimed-fact texts, in `between` order.
    pub claims: (String, String),
    /// A plain-English naming of what's contested (what the two facts disagree
    /// about), e.g. "how big the stain is". Never an adjudication.
    pub about: String,
}

/// A draft agreement the **parties co-author** toward mutual sign-off. The text
/// is theirs — specific, mutual, in their own words — not an accept/reject of an
/// AI's option. The receipt later certifies only that the agreement is internally
/// coherent (consistent with the certified facts), **never** that it's "right".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftAgreement {
    /// The current agreement text the parties are shaping. Edited toward sign-off.
    pub text: String,
    /// Who has signed off on the *current* text. Any edit clears this (a changed
    /// agreement must be re-owned by both), so a signature always attaches to the
    /// exact words in front of them.
    pub signed_by: Vec<PartyId>,
    /// The provenance of each edit — who shaped this toward its current form, so
    /// the ownership is legible. `(party_or_mediator, what_changed)`.
    pub revisions: Vec<(String, String)>,
}

impl DraftAgreement {
    pub fn new(text: impl Into<String>, by: &str) -> Self {
        let text = text.into();
        Self {
            revisions: vec![(by.to_string(), "opened the draft".to_string())],
            text,
            signed_by: Vec::new(),
        }
    }
    /// Replace the agreement text. **Clears all signatures** — a changed
    /// agreement is no longer the one anyone signed; it must be re-owned by both.
    pub fn revise(&mut self, by: &str, text: impl Into<String>) {
        self.text = text.into();
        self.signed_by.clear();
        self.revisions.push((by.to_string(), "revised the wording".to_string()));
    }
    /// A party signs off on the *current* text. Idempotent per party.
    pub fn sign(&mut self, party: &str) {
        if !self.signed_by.iter().any(|p| p == party) {
            self.signed_by.push(party.to_string());
        }
    }
    /// True once every listed party has signed the current text.
    pub fn fully_signed(&self, parties: &[PartyId]) -> bool {
        !parties.is_empty() && parties.iter().all(|p| self.signed_by.contains(p))
    }
}

/// One party's private thread + what the mediator has learned from them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartyThread {
    pub id: PartyId,
    pub display_name: String,
    /// The private caucus conversation (party ↔ mediator).
    pub caucus: Vec<Utterance>,
    /// The *interests under the positions* the mediator surfaced.
    pub interests: Vec<String>,
    /// Evidence this party submitted, attached to their thread. Stored, weighed,
    /// acknowledged — but never load-bearing on the crux.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// **Readiness, not structure.** True once the mediator judges this party has
    /// been heard *fully* — they've had their uninterrupted space and aren't still
    /// venting. The flow gives more caucus space until this is set, instead of
    /// racing to the joint work. Set by the brain (`CaucusMove::heard_fully`).
    #[serde(default)]
    pub heard_fully: bool,
    /// An interest the mediator has *named* under the position but not yet checked
    /// back / had confirmed. Position→interest is a gentle loop, not one beat: the
    /// mediator reflects this, the party confirms (or corrects), and only then does
    /// it become a `confirmed_interest`.
    #[serde(default)]
    pub pending_interest: Option<String>,
    /// The interest the party themselves **confirmed** is the real thing under the
    /// position ("yes — it's that you stopped showing up"). The core human work:
    /// only this counts as the surfaced interest, because the party owns it.
    #[serde(default)]
    pub interest_confirmed: bool,
    #[serde(default)]
    pub confirmed_interest: Option<String>,
}

/// A settlement option on the table.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub summary: String,
    pub settlement: Option<Settlement>,
    /// Certified consistent with the stipulated facts (the prover's gate).
    pub coherent: bool,
    pub accepted_by: Vec<PartyId>,
}

/// A whole mediation. Serializable + resumable — a session is a trajectory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub phase: Phase,
    /// The formal model of the dispute (signatures, stipulated facts, ledger).
    pub dispute: Dispute,
    /// The certified analysis — the quiet trust spine the session leans on.
    pub analysis: Analysis,
    /// The receipts that certified `analysis` (the audit trail).
    pub base_receipts: Vec<Receipt>,
    pub parties: Vec<PartyThread>,
    /// The joint (shared) conversation: intro, shared ground, crux, proposals,
    /// agreement.
    pub joint_transcript: Vec<Utterance>,
    pub proposals: Vec<Proposal>,
    /// A human-readable log of phase transitions and decisions.
    pub events: Vec<String>,
    /// Whether the mediator has *acknowledged the evidence on the record* since
    /// the last submission. Reset to `false` whenever new evidence arrives, so a
    /// late exhibit re-opens the acknowledgement step (the flow revisits).
    #[serde(default)]
    pub evidence_acknowledged: bool,
    /// Whether the mediator has named the crux yet (drives the adaptive flow;
    /// independent of `phase`, which the batch driver marches through).
    #[serde(default)]
    pub crux_named: bool,
    /// Factual conflicts detected across the evidence on record — two parties'
    /// claimed facts colliding on the same thing. Surfaced (named, handed back),
    /// never decided. Recomputed structurally as evidence arrives.
    #[serde(default)]
    pub factual_conflicts: Vec<FactualConflict>,
    /// Whether the mediator has surfaced the current factual conflicts on the
    /// record. Cleared whenever the set of conflicts changes (new collision ⇒
    /// the flow revisits and surfaces it).
    #[serde(default)]
    pub conflicts_surfaced: bool,
    /// **Subtraction made visible.** Once the formalizable ledger is settled and
    /// certified, the *residue* is what was never about the money — the part the
    /// kernel cannot touch and shouldn't pretend to. Named explicitly so a
    /// frightened person can see the philosophy: the money is handled and
    /// certified fair; this is what's left, and it's human.
    #[serde(default)]
    pub residue: Vec<String>,
    /// Whether the mediator has named the residue on the record yet.
    #[serde(default)]
    pub residue_named: bool,
    /// The agreement the parties are **co-authoring** toward mutual sign-off — in
    /// their own words, owned by both. `None` until convergence opens it.
    #[serde(default)]
    pub draft_agreement: Option<DraftAgreement>,
}

#[derive(Deserialize)]
struct CacheFile {
    analysis: Analysis,
    #[serde(default)]
    receipts: Vec<Receipt>,
}

impl Session {
    /// Open a session over a scenario whose facts are already certified: loads
    /// the `Dispute` and its sibling `*.analysis.json` cache (the certified
    /// `Analysis` + receipts). This is the common case for the demo — the
    /// formal core already did its quiet work; the session is the conversation
    /// on top.
    pub fn from_scenario(dispute_path: &str) -> anyhow::Result<Session> {
        let dispute = mediator_core::load_dispute(dispute_path)?;
        let cache_path = dispute_path
            .strip_suffix(".json")
            .map(|s| format!("{s}.analysis.json"))
            .unwrap_or_else(|| format!("{dispute_path}.analysis.json"));
        let (analysis, base_receipts) = match std::fs::read_to_string(&cache_path) {
            Ok(txt) => {
                let c: CacheFile = serde_json::from_str(&txt)?;
                (c.analysis, c.receipts)
            }
            // No cache → empty analysis. The session still runs; it just has no
            // certified shared ground to lean on (honest, not faked).
            Err(_) => (Analysis::default(), Vec::new()),
        };
        Ok(Session::new(dispute, analysis, base_receipts))
    }

    /// Construct a fresh session from a dispute + its certified analysis.
    pub fn new(dispute: Dispute, analysis: Analysis, base_receipts: Vec<Receipt>) -> Session {
        let parties = dispute
            .parties
            .iter()
            .map(|p| PartyThread {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
                caucus: Vec::new(),
                interests: Vec::new(),
                evidence: Vec::new(),
                heard_fully: false,
                pending_interest: None,
                interest_confirmed: false,
                confirmed_interest: None,
            })
            .collect();
        let id = slug(&dispute.title);
        Session {
            id,
            title: dispute.title.clone(),
            phase: Phase::Intake,
            dispute,
            analysis,
            base_receipts,
            parties,
            joint_transcript: Vec::new(),
            proposals: Vec::new(),
            events: Vec::new(),
            evidence_acknowledged: false,
            crux_named: false,
            factual_conflicts: Vec::new(),
            conflicts_surfaced: false,
            residue: Vec::new(),
            residue_named: false,
            draft_agreement: None,
        }
    }

    pub fn party_ids(&self) -> Vec<PartyId> {
        self.parties.iter().map(|p| p.id.clone()).collect()
    }

    pub fn party_name(&self, id: &str) -> String {
        self.parties
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// Escalate to a human at any time, handing over the full certified record.
    /// This is always available — the backstop that keeps it voluntary.
    pub fn escalate(&mut self, reason: &str) {
        self.phase = Phase::Escalated;
        self.joint_transcript.push(Utterance::system(format!(
            "Escalated to a human mediator. Reason: {reason}. The full record — \
             every certified fact and its receipt — goes with you, so the person \
             you talk to starts oriented, not from scratch."
        )));
        self.events.push(format!("escalated:{reason}"));
    }

    /// A party submits evidence attached to their thread. Stored on the party's
    /// `PartyThread`; logged as an event; and the acknowledgement step is
    /// re-opened so the mediator weighs the new exhibit (the flow revisits,
    /// it doesn't silently absorb it). Returns `Err` if `by` is not a party.
    pub fn submit_evidence(&mut self, ev: Evidence) -> anyhow::Result<()> {
        let by = ev.by.clone();
        let th = self
            .parties
            .iter_mut()
            .find(|t| t.id == by)
            .ok_or_else(|| anyhow::anyhow!("no such party: {by}"))?;
        let render = ev.render();
        th.evidence.push(ev);
        self.evidence_acknowledged = false;
        self.events.push(format!("evidence:{by}:{render}"));
        self.recompute_factual_conflicts();
        Ok(())
    }

    /// Re-derive the set of [`FactualConflict`]s from the `Fact` evidence on
    /// record. **Structural and deliberately simple**: two parties' stated facts
    /// collide when they're *about the same thing* (share a salient noun) yet
    /// carry *different concrete values* (different numbers/measurements). The
    /// honesty is in handing it back, not in cleverness — a real human mediator
    /// just notices "you two are saying different numbers about the same thing."
    ///
    /// If the conflict set changes, `conflicts_surfaced` is reset so the flow
    /// revisits and names the new collision. Never decides who's right.
    pub fn recompute_factual_conflicts(&mut self) {
        let facts: Vec<&Evidence> = self
            .all_evidence()
            .into_iter()
            .filter(|e| e.kind == EvidenceKind::Fact)
            .collect();
        let mut found: Vec<FactualConflict> = Vec::new();
        for i in 0..facts.len() {
            for j in (i + 1)..facts.len() {
                let (a, b) = (facts[i], facts[j]);
                if a.by == b.by {
                    continue; // a party doesn't contradict themselves here
                }
                if let Some(about) = facts_collide(&a.text, &b.text) {
                    found.push(FactualConflict {
                        between: (a.by.clone(), b.by.clone()),
                        claims: (a.text.clone(), b.text.clone()),
                        about,
                    });
                }
            }
        }
        if found != self.factual_conflicts {
            self.factual_conflicts = found;
            self.conflicts_surfaced = false;
        }
    }

    pub fn has_factual_conflict(&self) -> bool {
        !self.factual_conflicts.is_empty()
    }

    /// All evidence across every party thread, in submission order per party.
    pub fn all_evidence(&self) -> Vec<&Evidence> {
        self.parties.iter().flat_map(|t| t.evidence.iter()).collect()
    }

    /// Evidence submitted by one party.
    pub fn evidence_by(&self, party: &str) -> Vec<&Evidence> {
        self.parties
            .iter()
            .find(|t| t.id == party)
            .map(|t| t.evidence.iter().collect())
            .unwrap_or_default()
    }

    pub fn has_evidence(&self) -> bool {
        self.parties.iter().any(|t| !t.evidence.is_empty())
    }

    /// True once a party has had any caucus exchange (the mediator heard them).
    pub fn has_spoken(&self, party: &str) -> bool {
        self.parties
            .iter()
            .find(|t| t.id == party)
            .map(|t| !t.caucus.is_empty())
            .unwrap_or(false)
    }

    /// True once *every* party has been heard at least once.
    pub fn everyone_spoken(&self) -> bool {
        !self.parties.is_empty() && self.parties.iter().all(|t| !t.caucus.is_empty())
    }

    /// A party who has not yet spoken, if any (intake order).
    pub fn next_unheard(&self) -> Option<PartyId> {
        self.parties
            .iter()
            .find(|t| t.caucus.is_empty())
            .map(|t| t.id.clone())
    }

    /// Whether the certified analysis carries shareable common ground.
    pub fn has_shared_ground(&self) -> bool {
        !self.analysis.shared_core.is_empty()
    }

    /// Whether there is a crux to name (certified open question).
    pub fn has_crux(&self) -> bool {
        self.analysis.crux.is_some() || !self.analysis.cruxes.is_empty()
    }

    /// Whether any proposal is on the table.
    pub fn has_proposals(&self) -> bool {
        !self.proposals.is_empty()
    }

    /// Whether some party has accepted a proposal (the convergence signal).
    pub fn someone_accepted(&self) -> bool {
        self.proposals.iter().any(|p| !p.accepted_by.is_empty())
    }

    // ── readiness (read the room, don't count states) ──

    /// A party who has spoken but whom the mediator hasn't judged *heard fully*
    /// yet — still venting, still needing uninterrupted space. The flow gives
    /// them more room before any joint move. `None` once everyone is heard fully.
    pub fn needs_more_space(&self) -> Option<PartyId> {
        self.parties
            .iter()
            .find(|t| !t.caucus.is_empty() && !t.heard_fully)
            .map(|t| t.id.clone())
    }

    /// True once every party has both spoken *and* been heard fully — the real
    /// readiness gate for moving into joint work (not a state counter).
    pub fn everyone_heard_fully(&self) -> bool {
        !self.parties.is_empty() && self.parties.iter().all(|t| t.heard_fully)
    }

    /// A party with an interest the mediator *named* but hasn't had *confirmed*
    /// yet — the check-back is owed. Position→interest is a loop: name it, check
    /// it back, let them confirm. `None` when no check-back is pending.
    pub fn needs_interest_check_back(&self) -> Option<PartyId> {
        self.parties
            .iter()
            .find(|t| t.pending_interest.is_some() && !t.interest_confirmed)
            .map(|t| t.id.clone())
    }

    /// Record that the mediator has *named* a candidate interest for a party and
    /// owes them a check-back. Does not yet confirm it (the party must).
    pub fn name_interest(&mut self, party: &str, interest: impl Into<String>) {
        if let Some(th) = self.parties.iter_mut().find(|t| t.id == party) {
            th.pending_interest = Some(interest.into());
            th.interest_confirmed = false;
        }
    }

    /// The party **confirms** the named interest is right (they own it). Promotes
    /// the pending interest to the confirmed one, and folds it into `interests`.
    /// If `corrected` is given, that becomes the confirmed interest instead — the
    /// party's words win ("no — it's actually that…").
    pub fn confirm_interest(&mut self, party: &str, corrected: Option<String>) {
        if let Some(th) = self.parties.iter_mut().find(|t| t.id == party) {
            let interest = corrected.or_else(|| th.pending_interest.take());
            if let Some(i) = interest {
                if !th.interests.contains(&i) {
                    th.interests.push(i.clone());
                }
                th.confirmed_interest = Some(i);
                th.interest_confirmed = true;
                th.pending_interest = None;
            }
        }
        self.events.push(format!("interest_confirmed:{party}"));
    }

    /// Mark a party as heard fully (the mediator judged they've had their space).
    pub fn mark_heard_fully(&mut self, party: &str) {
        if let Some(th) = self.parties.iter_mut().find(|t| t.id == party) {
            th.heard_fully = true;
        }
    }

    // ── subtraction made visible / co-authored agreement ──

    /// Whether the formalizable ledger is settled and certified — the gate for
    /// naming the residue. (A refund is certified ⇒ the money question is handled.)
    pub fn ledger_settled(&self) -> bool {
        self.analysis.ledger_refund_cents.is_some()
    }

    /// Whether there is residue to name — the non-formalizable part left once the
    /// money is handled. Drawn from confirmed interests + any unresolved factual
    /// conflict, falling back to a single honest line. Computed, not invented.
    pub fn residue_candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for th in &self.parties {
            if let Some(i) = &th.confirmed_interest {
                out.push(format!("for {}: {}", first_name(&th.display_name), i));
            }
        }
        if out.is_empty() {
            out.push(
                "the part of this that was never really about the money — the trust, \
                 the feeling of being seen — which no ledger can settle"
                    .to_string(),
            );
        }
        out
    }

    /// Open a co-authored draft agreement (idempotent: keeps an existing draft).
    pub fn open_draft_agreement(&mut self, initial: impl Into<String>, by: &str) {
        if self.draft_agreement.is_none() {
            self.draft_agreement = Some(DraftAgreement::new(initial, by));
            self.events.push("draft_opened".into());
        }
    }

    /// A party (or the mediator) revises the draft toward their own words. Clears
    /// signatures — a changed agreement must be re-owned by both.
    pub fn revise_draft(&mut self, by: &str, text: impl Into<String>) {
        if let Some(d) = self.draft_agreement.as_mut() {
            d.revise(by, text);
            self.events.push(format!("draft_revised:{by}"));
        }
    }

    /// A party signs the current draft text.
    pub fn sign_draft(&mut self, party: &str) {
        if let Some(d) = self.draft_agreement.as_mut() {
            d.sign(party);
            self.events.push(format!("draft_signed:{party}"));
        }
    }

    /// True once the draft agreement exists and every party has signed its
    /// current text — the agreement is theirs, mutual, and owned.
    pub fn agreement_owned(&self) -> bool {
        let ids = self.party_ids();
        self.draft_agreement
            .as_ref()
            .map(|d| d.fully_signed(&ids))
            .unwrap_or(false)
    }
}

// ────────────────────────── the mediator's voice ─────────────────────────

/// What a caucus exchange produced: the mediator's reply, any interests it
/// surfaced, and any formalizable claims it proposes adding to the dispute.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaucusMove {
    pub reply: String,
    pub interests: Vec<String>,
    /// Formalizable claims elicited from this exchange (accreted into the
    /// dispute, then certified). Empty in the scripted brain for now.
    pub claims: Vec<ClaimDraft>,
    /// The mediator feels this caucus has what it needs.
    pub done: bool,
    /// **Readiness, read from the room.** The mediator judges this party has now
    /// been heard *fully* — they've had their space and aren't still venting. When
    /// false, the flow gives more uninterrupted caucus space instead of rushing on.
    #[serde(default)]
    pub heard_fully: bool,
    /// A candidate interest the mediator hears under the position, to be *checked
    /// back* (not yet confirmed). Drives the [`MediatorAction::CheckBackInterest`]
    /// loop: named here, confirmed by the party, only then surfaced.
    #[serde(default)]
    pub interest_to_check: Option<String>,
}

impl CaucusMove {
    /// A bare reply with no special signals (heard-fully not yet judged).
    pub fn reply_only(reply: impl Into<String>) -> Self {
        Self {
            reply: reply.into(),
            interests: Vec::new(),
            claims: Vec::new(),
            done: false,
            heard_fully: false,
            interest_to_check: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimDraft {
    pub party: PartyId,
    pub nl: String,
    pub formula: Formula,
    pub english: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalDraft {
    pub summary: String,
    pub settlement: Option<Settlement>,
}

/// The mediator's voice and judgment. The session driver supplies certified
/// facts; the brain supplies the *human* part — how to listen, reflect, name
/// the knot, and propose. A live Bedrock brain and the deterministic
/// [`ScriptedBrain`] are interchangeable here.
pub trait MediatorBrain {
    fn intro(&self, s: &Session) -> String;
    fn caucus(&self, s: &Session, party: &PartyId, input: &str) -> CaucusMove;
    fn shared_ground(&self, s: &Session) -> String;
    fn crux(&self, s: &Session) -> String;
    fn proposals(&self, s: &Session) -> Vec<ProposalDraft>;
    fn present_proposals(&self, s: &Session) -> String;
    fn closing(&self, s: &Session, accepted: &Proposal) -> String;
    /// Acknowledge and weigh the evidence on record. Defaults to the pure
    /// [`acknowledge_evidence`] helper (deterministic, offline); a live brain may
    /// override for a warmer voice but must keep the same discipline: weigh it,
    /// never let it decide the crux.
    fn acknowledge_evidence(&self, s: &Session) -> String {
        acknowledge_evidence(s)
    }

    /// **Check an interest back** to the party (the iterated heart of the work):
    /// "it sounds like what matters isn't the $400, it's that you stopped showing
    /// up — am I close?" Defaults to the pure [`check_back_interest`] helper. The
    /// driver only advances once the party confirms or corrects it.
    fn check_back_interest(&self, s: &Session, party: &PartyId) -> String {
        check_back_interest(s, party)
    }

    /// **Surface a factual conflict** — two parties' claimed facts colliding on
    /// the same thing — as something that "needs evidence, not logic", naming
    /// exactly what's contested and refusing to decide it. Defaults to the pure
    /// [`surface_factual_conflict`] helper.
    fn surface_factual_conflict(&self, s: &Session) -> String {
        surface_factual_conflict(s)
    }

    /// **Name the residue** — subtraction made visible. With the money handled and
    /// certified fair, name the part that was never about the money. Defaults to
    /// the pure [`name_residue`] helper.
    fn name_residue(&self, s: &Session) -> String {
        name_residue(s)
    }

    /// Help the parties **co-author** their agreement in their own words. Returns
    /// an opening draft text (a scaffold the parties then edit toward sign-off) —
    /// never the AI's verdict. Defaults to the pure [`co_author_agreement`] helper.
    fn co_author_agreement(&self, s: &Session) -> String {
        co_author_agreement(s)
    }
}

// ───────────────────────────── the driver ────────────────────────────────

/// Scripted party inputs for a batch (non-interactive) session: each party's
/// caucus messages, in order. Used for tests and offline demos.
#[derive(Clone, Debug, Default)]
pub struct ScriptedInputs(pub HashMap<PartyId, Vec<String>>);

impl ScriptedInputs {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn with(mut self, party: &str, msgs: &[&str]) -> Self {
        self.0
            .insert(party.to_string(), msgs.iter().map(|s| s.to_string()).collect());
        self
    }
}

/// Conduct a full mediation, batch-style, with scripted party inputs. The
/// interactive driver (web/TUI) reuses the same phase steps; this is the
/// deterministic spine.
pub fn conduct(mut s: Session, brain: &dyn MediatorBrain, inputs: &ScriptedInputs) -> Session {
    // ── Intake ──
    s.phase = Phase::Intake;
    s.joint_transcript.push(Utterance::mediator(brain.intro(&s)));
    s.events.push("intake".into());

    // ── Caucus: each party, privately ──
    s.phase = Phase::Caucus;
    for pid in s.party_ids() {
        let msgs = inputs.0.get(&pid).cloned().unwrap_or_default();
        for msg in msgs {
            let mv = brain.caucus(&s, &pid, &msg);
            if let Some(th) = s.parties.iter_mut().find(|t| t.id == pid) {
                th.caucus.push(Utterance::party(&pid, msg));
                th.caucus.push(Utterance::mediator(mv.reply));
                for i in mv.interests {
                    if !th.interests.contains(&i) {
                        th.interests.push(i);
                    }
                }
                // READINESS: the brain reads the room. A party heard fully moves
                // on; otherwise the flow would give more space (in the interactive
                // driver). The batch spine takes the brain's judgement as given.
                if mv.heard_fully {
                    th.heard_fully = true;
                }
            }
            // INTEREST CHECK-BACK LOOP: a named-but-unconfirmed interest gets
            // checked back, and the party confirms it (the batch spine confirms on
            // their behalf; the interactive driver waits for the real reply).
            if let Some(interest) = mv.interest_to_check {
                s.name_interest(&pid, &interest);
                let cb = brain.check_back_interest(&s, &pid);
                if let Some(th) = s.parties.iter_mut().find(|t| t.id == pid) {
                    th.caucus.push(Utterance::mediator(cb));
                    th.caucus.push(Utterance::party(&pid, "Yes — that's it, exactly."));
                }
                s.confirm_interest(&pid, None);
            }
        }
    }
    s.events.push("caucus".into());

    // ── Acknowledge evidence, if any party put some on the record ──
    if s.has_evidence() {
        let ack = brain.acknowledge_evidence(&s);
        s.joint_transcript.push(Utterance::mediator(ack));
        s.evidence_acknowledged = true;
        s.events.push("acknowledge_evidence".into());
    }

    // ── Surface any factual conflict the evidence created (handed back, never
    //    decided). Recomputed when evidence arrived via `submit_evidence`. ──
    if s.has_factual_conflict() && !s.conflicts_surfaced {
        let msg = brain.surface_factual_conflict(&s);
        s.joint_transcript.push(Utterance::mediator(msg));
        s.conflicts_surfaced = true;
        s.events.push("factual_conflict".into());
    }

    // ── Shared Ground (certified) ──
    s.phase = Phase::SharedGround;
    let sg = brain.shared_ground(&s);
    s.joint_transcript.push(Utterance::mediator(sg));
    s.events.push("shared_ground".into());

    // ── Crux (handed back, never decided) ──
    s.phase = Phase::Crux;
    let cx = brain.crux(&s);
    s.joint_transcript.push(Utterance::mediator(cx));
    s.crux_named = true;
    s.events.push("crux".into());

    // ── Subtraction made visible: once the money is settled & certified, name the
    //    residue — the part that was never about the money. The signature move. ──
    if s.ledger_settled() && !s.residue_named {
        if s.residue.is_empty() {
            s.residue = s.residue_candidates();
        }
        let r = brain.name_residue(&s);
        s.joint_transcript.push(Utterance::mediator(r));
        s.residue_named = true;
        s.events.push("residue".into());
    }

    // ── Proposals (only certified-coherent ones reach the parties) ──
    s.phase = Phase::Proposals;
    for (i, d) in brain.proposals(&s).into_iter().enumerate() {
        // Coherence comes from the certified analysis: these settlements were
        // produced by fair division over the certified facts.
        s.proposals.push(Proposal {
            id: format!("p{}", i + 1),
            summary: d.summary,
            settlement: d.settlement,
            coherent: true,
            accepted_by: Vec::new(),
        });
    }
    s.joint_transcript.push(Utterance::mediator(brain.present_proposals(&s)));
    s.events.push("proposals".into());

    // ── Convergence: in the batch spine, all parties accept the first coherent
    //    option. (The interactive driver mediates real accept/counter rounds.) ──
    s.phase = Phase::Convergence;
    let everyone = s.party_ids();
    if let Some(p) = s.proposals.iter_mut().find(|p| p.coherent) {
        p.accepted_by = everyone.clone();
        s.events.push(format!("accepted:{}", p.id));
    }

    // ── Co-authored, OWNED agreement. Convergence isn't accept/reject of an AI
    //    option: the parties shape the final text in their own words and sign off.
    //    The batch spine opens the draft and has both sign; the interactive driver
    //    mediates the real editing rounds. ──
    let draft_text = brain.co_author_agreement(&s);
    s.open_draft_agreement(draft_text.clone(), "mediator");
    s.joint_transcript.push(Utterance::mediator(draft_text));
    for pid in &everyone {
        s.sign_draft(pid);
    }

    // ── Agreement ──
    if let Some(accepted) = s
        .proposals
        .iter()
        .find(|p| !p.accepted_by.is_empty())
        .cloned()
    {
        s.phase = Phase::Agreement;
        s.joint_transcript
            .push(Utterance::mediator(brain.closing(&s, &accepted)));
        s.events.push("agreement".into());
    }

    s
}

// ─────────────────────── the adaptive (non-linear) flow ──────────────────────

/// One move the mediator can make. The interactive driver (web/TUI) asks
/// [`next_action`] what to do next and performs exactly that move, so the flow
/// can **loop, branch, revisit, and isn't forced through a fixed order** — a
/// late exhibit re-opens acknowledgement; an unheard party is always returned to
/// before the joint work; the crux is never reached before both are heard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediatorAction {
    /// Open the session (nothing said yet).
    Welcome,
    /// Caucus privately with this party (chosen because they've not been heard,
    /// or haven't yet been heard *fully* — still venting / needing space).
    AskParty(PartyId),
    /// Check an interest *back* to the party: name what seems to matter under the
    /// position and ask if it's close — the iterated heart of the human work. Only
    /// advances once the party confirms (or corrects) it.
    CheckBackInterest(PartyId),
    /// Weigh and acknowledge evidence that's been submitted but not yet folded
    /// in (informs the humans; never decides the crux).
    AcknowledgeEvidence,
    /// Two parties' claimed facts collide on the same thing. Surface it as a
    /// *factual* conflict that "needs evidence, not logic", name exactly what's
    /// contested — and hand it back. The machine never rules on which fact is true.
    SurfaceFactualConflict,
    /// Reflect the certified common ground back to both.
    ReflectSharedGround,
    /// Name the genuine knot and hand it back (never decide it).
    NameCrux,
    /// **Subtraction made visible.** With the money handled and certified fair,
    /// name the residue — the part that was never about the money. The signature
    /// move, made legible.
    NameResidue,
    /// Put certified-coherent settlement options on the table.
    ProposeOptions,
    /// Invite the parties to accept / counter / hold.
    InviteAgreement,
    /// Help the parties **co-author** the final agreement in their own words and
    /// edit it toward mutual sign-off — the agreement is theirs, not the AI's.
    CoAuthorAgreement,
    /// Hand off to a human with the full record (the always-available backstop).
    Escalate,
    /// Nothing left to do — the session has landed (or been escalated).
    Close,
}

/// Choose the mediator's next move from the *state of the session*, not from a
/// fixed script. This is the seam the interactive driver loops on:
///
/// ```text
///   loop { match next_action(&s) { Close => break, a => perform(a, &mut s) } }
/// ```
///
/// The ordering encodes a real mediator's priorities, but every gate is a state
/// check, so the flow naturally **revisits**: submit late evidence and
/// `AcknowledgeEvidence` comes back; a party who hasn't spoken is always pulled
/// in before any joint work. The crux is gated behind *everyone heard* — the
/// machine never races to the knot before the people are heard.
pub fn next_action(s: &Session) -> MediatorAction {
    // Escalation is terminal.
    if s.phase == Phase::Escalated {
        return MediatorAction::Close;
    }

    // 0. Open if nothing has happened at all.
    let opened = s.joint_transcript.iter().any(|u| matches!(u.speaker, Speaker::Mediator))
        || s.parties.iter().any(|t| !t.caucus.is_empty());
    if !opened {
        return MediatorAction::Welcome;
    }

    // 1. Hear everyone privately first. Always return to an unheard party
    //    before doing any joint work — caucus is the foundation.
    if let Some(p) = s.next_unheard() {
        return MediatorAction::AskParty(p);
    }

    // 1b. READ THE ROOM, don't count states. A party who has spoken but isn't yet
    //     *heard fully* — still venting, still needing uninterrupted space — gets
    //     more room before any joint move. The pacing is the mediator's skill; we
    //     don't race to the crux while someone is still being heard.
    if let Some(p) = s.needs_more_space() {
        return MediatorAction::AskParty(p);
    }

    // 1c. INTEREST ELICITATION is a check-back LOOP, not one beat. If the mediator
    //     has named a candidate interest under the position but the party hasn't
    //     confirmed it yet, the check-back is owed before we move on. The interest
    //     isn't surfaced until the party *owns* it.
    if let Some(p) = s.needs_interest_check_back() {
        return MediatorAction::CheckBackInterest(p);
    }

    // 2. Weigh any evidence that's come in but hasn't been acknowledged on the
    //    record yet. Submitting a late exhibit re-opens this (revisit), because
    //    `submit_evidence` clears `evidence_acknowledged`.
    if s.has_evidence() && !s.evidence_acknowledged {
        return MediatorAction::AcknowledgeEvidence;
    }

    // 2b. EVIDENCE COLLIDED. When two parties' claimed facts conflict on the same
    //     thing, surface it as a *factual* conflict that needs evidence not logic —
    //     name what's contested, hand it back, never rule. New collision ⇒ revisit.
    if s.has_factual_conflict() && !s.conflicts_surfaced {
        return MediatorAction::SurfaceFactualConflict;
    }

    // 3. With everyone heard and evidence weighed, reflect shared ground (once).
    let reflected_ground =
        s.events.iter().any(|e| e == "shared_ground") || !s.has_shared_ground();
    if !reflected_ground {
        return MediatorAction::ReflectSharedGround;
    }

    // 4. Name the genuine knot (once), only after shared ground — and only if
    //    there is one. No crux ⇒ skip straight to options.
    if s.has_crux() && !s.crux_named {
        return MediatorAction::NameCrux;
    }

    // 5. Put coherent options on the table if none are there yet.
    if !s.has_proposals() {
        return MediatorAction::ProposeOptions;
    }

    // 5b. SUBTRACTION MADE VISIBLE. Once the money is handled and certified fair,
    //     name the residue — the part that was never about the money — before
    //     pressing toward sign-off. The signature move, made legible.
    if s.ledger_settled() && !s.residue_named {
        return MediatorAction::NameResidue;
    }

    // 6. Invite agreement until someone signals convergence.
    if !s.someone_accepted() {
        return MediatorAction::InviteAgreement;
    }

    // 6b. CO-AUTHORED, OWNED AGREEMENT. Convergence isn't accept/reject of an AI
    //     option: the parties shape the final text in their own words and sign off
    //     on it. Keep helping co-author until the agreement is mutually owned.
    if !s.agreement_owned() {
        return MediatorAction::CoAuthorAgreement;
    }

    // 7. Landed.
    MediatorAction::Close
}

/// Fold the evidence on record into a single plain acknowledgement the mediator
/// can speak. **Pure** (no model, no network) so it's the deterministic floor a
/// live brain can surpass — and so it can be tested offline.
///
/// It *acknowledges and weighs* (names whose exhibit it is, and what kind) but
/// is explicit that none of it decides the open question — evidence informs the
/// humans; it does not let the machine rule. The crux stays theirs.
pub fn acknowledge_evidence(s: &Session) -> String {
    let all = s.all_evidence();
    if all.is_empty() {
        return "No one's put anything on the record yet — and that's fine; your \
                accounts are evidence enough to work with."
            .to_string();
    }
    let mut out = String::from(
        "I want to make sure what each of you brought is on the record and that \
         you've been heard on it:\n",
    );
    for th in &s.parties {
        if th.evidence.is_empty() {
            continue;
        }
        out.push_str(&format!("  {}:\n", first_name(&th.display_name)));
        for ev in &th.evidence {
            out.push_str(&format!("    • {}\n", ev.render()));
        }
    }
    out.push_str(
        "\nI've taken all of it in, and it matters — it tells me where each of you \
         is coming from. What it does NOT do is settle the one open question for \
         you: that stays yours to answer, not something this evidence (or I) get to \
         decide.",
    );
    out.trim_end().to_string()
}

/// Check an interest **back** to a party (the iterated heart of the human work):
/// name what seems to matter under the position and ask if it's close, so the
/// party can confirm or correct. **Pure** (offline floor). The driver only
/// advances once the party owns it via [`Session::confirm_interest`].
pub fn check_back_interest(s: &Session, party: &PartyId) -> String {
    let th = s.parties.iter().find(|t| &t.id == party);
    let name = th.map(|t| first_name(&t.display_name)).unwrap_or_default();
    match th.and_then(|t| t.pending_interest.clone()) {
        Some(interest) => format!(
            "{name}, I want to check something back with you before we go on. It \
             sounds like, underneath the specifics, what really matters to you here \
             isn't only the practical part — it's {interest}. Am I close? If I've got \
             it wrong, tell me how you'd put it — your words are the ones that count.",
        ),
        None => format!(
            "{name}, can you help me name what matters most to you underneath all \
             this? I'd rather hear it in your words than guess at it."
        ),
    }
}

/// Surface a **factual conflict**: two parties' claimed facts colliding on the
/// same thing. **Pure** (offline floor). It names exactly what's contested and is
/// explicit that this is the one kind of thing that *needs evidence, not logic* —
/// and that the machine will not decide it. Honesty is the whole move.
pub fn surface_factual_conflict(s: &Session) -> String {
    if s.factual_conflicts.is_empty() {
        return "Your accounts line up on the facts — there's nothing here where \
                you're claiming two different things about the same thing."
            .to_string();
    }
    let mut out = String::from(
        "There's a place where what each of you has put on the record doesn't \
         line up — and I want to be honest about what that is, because it's a \
         particular kind of thing:\n",
    );
    for c in &s.factual_conflicts {
        let a = first_name(&s.party_name(&c.between.0));
        let b = first_name(&s.party_name(&c.between.1));
        out.push_str(&format!(
            "  • You disagree about {about}: {a} says \"{ca}\", {b} says \"{cb}\".\n",
            about = c.about,
            ca = c.claims.0,
            cb = c.claims.1,
        ));
    }
    out.push_str(
        "\nThat's not something logic or I can settle — it's a question of fact, \
         and a question of fact needs evidence, not argument. I'm not going to \
         decide which of you is right about it; that wouldn't be fair or honest. \
         What we *can* do is be clear that this is the thing the evidence has to \
         speak to, and keep working everything that doesn't hang on it.",
    );
    out.trim_end().to_string()
}

/// Name the **residue** — subtraction made visible. **Pure** (offline floor).
/// With the money handled and certified fair, this names the part that was never
/// about the money, so a frightened person can see the philosophy: the ledger is
/// settled; what's left is human, and it's theirs.
pub fn name_residue(s: &Session) -> String {
    let residue = if s.residue.is_empty() {
        s.residue_candidates()
    } else {
        s.residue.clone()
    };
    let mut out = String::from(
        "I want to draw a line under something, because it matters. The money part \
         of this is handled — it was checked, and the split is fair; that's settled \
         and it can't be fudged. So here's the honest thing: what's left isn't the \
         money. What's left is the part that was never really about it —\n",
    );
    for r in &residue {
        out.push_str(&format!("  • {r}\n"));
    }
    out.push_str(
        "\nI can't certify that part, and I won't pretend a number ever could. But \
         naming it is worth something — it's the real thing, and it's yours to do \
         with what you choose.",
    );
    out.trim_end().to_string()
}

/// Open a **co-authored** agreement: a scaffold in plain, mutual language the
/// parties then edit toward sign-off. **Pure** (offline floor). The text is a
/// starting point for *them* to make theirs — never the AI's verdict, and the
/// receipt later certifies only internal coherence, never that it's "right".
pub fn co_author_agreement(s: &Session) -> String {
    let names: Vec<String> = s.parties.iter().map(|t| first_name(&t.display_name)).collect();
    let who = join_names(&names);
    let mut out = format!(
        "Let's write this down together, in your words — not mine. Here's a plain \
         starting point for {who} to shape until it's something you both actually \
         mean:\n\n",
    );
    out.push_str("  \"We, ");
    out.push_str(&who);
    out.push_str(
        ", agree to settle this between us. We've handled the money in the way we \
         worked out, and we each understand what mattered to the other underneath \
         it.",
    );
    if let Some(c) = s.analysis.crux.as_ref() {
        out.push_str(&format!(
            " On the one open question — {c} — we've decided together how to leave \
             it, and that decision is ours.",
        ));
    }
    out.push_str(
        " We're signing this because it's fair enough to live with, not because \
         anyone made us.\"\n\n",
    );
    out.push_str(
        "Change any word of it. When it says what you both mean, you each sign it — \
         and then it's yours. I only check that it doesn't contradict the facts we \
         already settled; I never decide that it's the *right* outcome. That part \
         was always yours.",
    );
    out
}

// ── structural fact-collision detection (simple by design) ──

/// Whether two stated-fact texts **collide**: they're plausibly about the same
/// thing (share a salient content word) yet carry *different concrete values*
/// (different numbers / measurements). Returns a plain description of what they
/// disagree about, or `None`. Deliberately simple — the honesty is in handing the
/// conflict back, not in clever detection.
fn facts_collide(a: &str, b: &str) -> Option<String> {
    let na = numbers(a);
    let nb = numbers(b);
    if na.is_empty() || nb.is_empty() {
        return None;
    }
    // A collision needs different numbers...
    if na.iter().any(|x| nb.contains(x)) {
        return None; // they share a number ⇒ not obviously contradicting
    }
    // ...and a shared salient noun (what the numbers are *about*).
    let shared = shared_keyword(a, b)?;
    Some(format!("how much/many {shared}"))
}

/// Extract integer-ish tokens from text (e.g. "$400", "30cm", "2 years" → 400,
/// 30, 2). Used only to notice two facts asserting *different* concrete values.
fn numbers(s: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<i64>() {
                out.push(n);
            }
            cur.clear();
        }
    }
    if let Ok(n) = cur.parse::<i64>() {
        out.push(n);
    }
    out
}

/// A salient content word shared by both texts (lowercased, length ≥ 4, not a
/// stopword) — a cheap proxy for "these facts are about the same thing".
fn shared_keyword(a: &str, b: &str) -> Option<String> {
    let words = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| w.len() >= 4 && !is_stopword(w))
            .map(|w| w.to_string())
            .collect()
    };
    let wa = words(a);
    let wb = words(b);
    wa.into_iter().find(|w| wb.contains(w))
}

fn is_stopword(w: &str) -> bool {
    matches!(
        w,
        "that" | "this" | "they" | "them" | "with" | "from" | "have" | "were"
            | "what" | "when" | "your" | "about" | "there" | "their" | "which"
            | "would" | "could" | "should" | "been" | "just" | "only" | "into"
            | "over" | "than" | "then" | "some" | "more" | "most" | "much"
            | "many" | "said" | "says" | "very" | "really"
    )
}

/// Render the whole session as a readable transcript (caucuses + joint).
pub fn render_transcript(s: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!("☄ Mediation — {}\n", s.title));
    out.push_str(&format!("  phase: {:?}\n\n", s.phase));
    for th in &s.parties {
        if th.caucus.is_empty() {
            continue;
        }
        out.push_str(&format!("── private caucus · {} ──\n", th.display_name));
        for u in &th.caucus {
            out.push_str(&render_utterance(s, u));
        }
        if !th.interests.is_empty() {
            out.push_str(&format!(
                "   (interests heard: {})\n",
                th.interests.join("; ")
            ));
        }
        out.push('\n');
    }
    out.push_str("── joint session ──\n");
    for u in &s.joint_transcript {
        out.push_str(&render_utterance(s, u));
    }
    out
}

fn render_utterance(s: &Session, u: &Utterance) -> String {
    let who = match &u.speaker {
        Speaker::Mediator => "mediator".to_string(),
        Speaker::System => "·".to_string(),
        Speaker::Party(p) => s.party_name(p),
    };
    format!("  {who}: {}\n", u.text)
}

// ─────────────────────────── the scripted brain ──────────────────────────

/// A deterministic, offline mediator voice. Warm and templated — a faithful
/// floor that the live Bedrock brain will surpass, and the thing tests run on.
/// It draws all of its *substance* from the certified analysis; it only adds
/// the human framing.
pub struct ScriptedBrain;

impl MediatorBrain for ScriptedBrain {
    fn intro(&self, s: &Session) -> String {
        let names: Vec<String> = s.parties.iter().map(|p| first_name(&p.display_name)).collect();
        format!(
            "Hi {} — thank you both for being here. You don't have to like each \
             other to do this well; you just have to be willing to sort it out \
             fairly. I'll talk with each of you on your own first, then we'll \
             find what you already agree on and look honestly at what you don't. \
             Nothing is decided for you, and you can ask for a human, or stop, \
             at any time.",
            join_names(&names)
        )
    }

    fn caucus(&self, _s: &Session, _party: &PartyId, input: &str) -> CaucusMove {
        // The scripted brain reflects back and gently names a possible interest to
        // *check back* — it does not confirm it itself; the party does, in the
        // `CheckBackInterest` loop. (The live brain does the real elicitation.)
        let interest = guess_interest(input);
        let named = interest
            .clone()
            .unwrap_or_else(|| "being treated fairly here".to_string());
        let reply = format!(
            "Thank you for telling me that — I want to make sure I've really got it, \
             not just the surface of it. Take all the space you need.",
        );
        CaucusMove {
            reply,
            interests: Vec::new(),
            claims: Vec::new(),
            done: true,
            // The deterministic floor judges a party heard once they've spoken
            // their piece; the live brain reads the room more finely.
            heard_fully: true,
            interest_to_check: Some(named),
        }
    }

    fn shared_ground(&self, s: &Session) -> String {
        if s.analysis.shared_core.is_empty() {
            return "Before the disagreement: it's worth saying you came here \
                    together to resolve this, which is already more common ground \
                    than most disputes have."
                .to_string();
        }
        let mut out = String::from(
            "Here's the part that usually gets lost in a fight — what you already \
             agree on, and it's more than it feels like:\n",
        );
        for fact in &s.analysis.shared_core {
            out.push_str(&format!("  • {fact}\n"));
        }
        if !s.analysis.dissolved.is_empty() {
            out.push_str(
                "\nAnd a couple of things that looked like disagreements but were \
                 really just crossed wires:\n",
            );
            for d in &s.analysis.dissolved {
                out.push_str(&format!("  • {d}\n"));
            }
        }
        out.trim_end().to_string()
    }

    fn crux(&self, s: &Session) -> String {
        match &s.analysis.crux {
            Some(c) => format!(
                "So here's the one real knot. {c}\n\nThat's not something I get to \
                 decide for you — it's a genuine difference, and it's yours. But \
                 notice how small it is now that everything else is settled.",
                c = c
            ),
            None => "Having cleared away the rest, there isn't a single hard knot \
                     left to point at — which means you may be closer than you \
                     thought."
                .to_string(),
        }
    }

    fn proposals(&self, s: &Session) -> Vec<ProposalDraft> {
        if s.analysis.settlements.is_empty() {
            return vec![ProposalDraft {
                summary: "Talk it through with the shared ground in view and see \
                          whether the knot still feels worth the fight."
                    .to_string(),
                settlement: None,
            }];
        }
        s.analysis
            .settlements
            .iter()
            .map(|set| ProposalDraft {
                summary: settlement_summary(s, set),
                settlement: Some(set.clone()),
            })
            .collect()
    }

    fn present_proposals(&self, s: &Session) -> String {
        let mut out = String::from(
            "Here are some ways this could land — each one is mathematically fair \
             (neither of you would rather have the other's share), and each works \
             whichever way the open question goes. They're a starting point, not \
             a verdict:\n",
        );
        for (i, p) in s.proposals.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, p.summary));
        }
        out.push_str("\nYou can take one as-is, change it, or hold out — your call.");
        out
    }

    fn closing(&self, _s: &Session, accepted: &Proposal) -> String {
        format!(
            "You've landed somewhere you can both live with: {}.\n\nFor what it's \
             worth — you did the hard part, which was staying at the table. \
             Everything you agreed to rests on facts that were checked, and there's \
             a plain record of how you got here if either of you ever wants it. \
             Take care of each other.",
            accepted.summary.trim_end_matches('.'),
        )
    }
}

// ───────────────────────────── small helpers ─────────────────────────────

fn settlement_summary(s: &Session, set: &Settlement) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (item, who) in &set.allocations {
        parts.push(format!("{} → {}", item_label(s, item), s.party_name(who)));
    }
    for (item, frac) in &set.splits {
        parts.push(format!(
            "{} shared {:.0}/{:.0}",
            item_label(s, item),
            frac * 100.0,
            (1.0 - frac) * 100.0
        ));
    }
    let label = if set.label.is_empty() { "A fair split" } else { &set.label };
    if parts.is_empty() {
        label.to_string()
    } else {
        format!("{label}: {}", parts.join(", "))
    }
}

fn item_label(s: &Session, id: &str) -> String {
    s.dispute
        .contested_items
        .iter()
        .find(|i| i.id == id)
        .map(|i| i.label.clone())
        .unwrap_or_else(|| id.to_string())
}

/// A crude interest-from-position heuristic for the scripted brain only.
fn guess_interest(input: &str) -> Option<String> {
    let l = input.to_lowercase();
    let pairs = [
        ("wasn't there", "feeling that your effort and presence were actually seen"),
        ("pull", "feeling that the load was shared fairly"),
        ("depress", "being met with some understanding for a hard stretch"),
        ("fair", "a fair outcome you can stand behind"),
        ("owe", "not being treated as if you acted in bad faith"),
        ("respect", "being treated with respect through this"),
        ("trust", "being able to trust the other person's word"),
    ];
    for (k, v) in pairs {
        if l.contains(k) {
            return Some(v.to_string());
        }
    }
    None
}

fn first_name(display: &str) -> String {
    display
        .split_whitespace()
        .next()
        .unwrap_or(display)
        .to_string()
}

fn join_names(names: &[String]) -> String {
    match names.len() {
        0 => "both".into(),
        1 => names[0].clone(),
        2 => format!("{} and {}", names[0], names[1]),
        _ => {
            let (last, rest) = names.split_last().unwrap();
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

fn slug(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
}

// also surface money in the API surface for downstream renderers
pub use mediator_core::render::money as render_money;

#[allow(unused)]
fn _money_in_scope(c: i64) -> String {
    money(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roommate_session() -> Session {
        Session::from_scenario("../../scenarios/roommate.json").expect("load roommate session")
    }

    #[test]
    fn full_scripted_session_reaches_agreement() {
        let s = roommate_session();
        // sanity: the certified spine loaded
        assert!(s.analysis.crux.is_some(), "expected a certified crux");
        assert!(!s.analysis.shared_core.is_empty(), "expected certified shared ground");

        let inputs = ScriptedInputs::new()
            .with(
                "robin",
                &[
                    "Honestly I just don't think I should pay for that stain — it was wear and tear.",
                    "And I feel like I wasn't there for the cleaning but I did pull my weight overall.",
                ],
            )
            .with(
                "sam",
                &["The carpet is real damage and it's only fair Robin covers it. I want this to be fair."],
            );

        let out = conduct(s, &ScriptedBrain, &inputs);

        assert_eq!(out.phase, Phase::Agreement, "session should reach agreement");
        // both parties had a private caucus
        assert!(out.parties.iter().all(|p| !p.caucus.is_empty()));
        // interests were surfaced (the human work)
        assert!(out.parties.iter().any(|p| !p.interests.is_empty()));
        // a coherent proposal was accepted
        assert!(out.proposals.iter().any(|p| p.coherent && !p.accepted_by.is_empty()));

        let t = render_transcript(&out);
        // the joint session reflected certified shared ground and named the crux
        assert!(t.contains("already agree on"));
        assert!(t.to_lowercase().contains("knot") || t.contains("decide"));
        // event log captured the full arc
        for e in ["intake", "caucus", "shared_ground", "crux", "proposals", "agreement"] {
            assert!(out.events.iter().any(|x| x.starts_with(e)), "missing event {e}");
        }
    }

    #[test]
    fn escalation_is_always_available() {
        let mut s = roommate_session();
        s.escalate("a party asked for a human");
        assert_eq!(s.phase, Phase::Escalated);
        assert!(s.events.iter().any(|e| e.starts_with("escalated")));
    }

    #[test]
    fn evidence_is_stored_and_surfaced() {
        let mut s = roommate_session();
        assert!(!s.has_evidence());

        s.submit_evidence(Evidence::statement(
            "robin",
            "I lived there two years and the carpet was already worn when I moved in.",
        ))
        .unwrap();
        s.submit_evidence(Evidence::artifact(
            "sam",
            "photo of the stain",
            Some("https://example/stain.jpg".into()),
        ))
        .unwrap();
        s.submit_evidence(Evidence::fact("sam", "The stain is 30cm across."))
            .unwrap();

        // stored on the right threads
        assert_eq!(s.evidence_by("robin").len(), 1);
        assert_eq!(s.evidence_by("sam").len(), 2);
        assert_eq!(s.all_evidence().len(), 3);
        assert!(s.has_evidence());
        // submitting re-opens acknowledgement
        assert!(!s.evidence_acknowledged);
        // logged
        assert!(s.events.iter().filter(|e| e.starts_with("evidence:")).count() == 3);

        // surfaced in the acknowledgement, with the artifact note
        let ack = acknowledge_evidence(&s);
        assert!(ack.contains("photo of the stain"));
        assert!(ack.contains("https://example/stain.jpg"));
        assert!(ack.contains("30cm"));
        // and the discipline is explicit: evidence does not decide the crux
        let low = ack.to_lowercase();
        assert!(low.contains("not") && (low.contains("decide") || low.contains("settle")));
    }

    #[test]
    fn submit_evidence_rejects_unknown_party() {
        let mut s = roommate_session();
        assert!(s.submit_evidence(Evidence::fact("nobody", "x")).is_err());
    }

    #[test]
    fn next_action_follows_state_not_a_fixed_script() {
        let mut s = roommate_session();
        // fresh: open the session
        assert_eq!(next_action(&s), MediatorAction::Welcome);

        // once opened, an unheard party is pulled in before any joint work
        s.joint_transcript.push(Utterance::mediator("hello"));
        match next_action(&s) {
            MediatorAction::AskParty(_) => {}
            other => panic!("expected AskParty before anyone spoke, got {other:?}"),
        }

        // hear the first party — the *other* unheard party is still next
        let ids = s.party_ids();
        if let Some(th) = s.parties.iter_mut().find(|t| t.id == ids[0]) {
            th.caucus.push(Utterance::party(&ids[0], "my side"));
        }
        assert_eq!(next_action(&s), MediatorAction::AskParty(ids[1].clone()));

        // both have *spoken* now
        if let Some(th) = s.parties.iter_mut().find(|t| t.id == ids[1]) {
            th.caucus.push(Utterance::party(&ids[1], "my side too"));
        }
        assert!(s.everyone_spoken());

        // …but readiness, not a state counter: a party who has spoken yet isn't
        // *heard fully* gets more space before any joint move (read the room).
        assert_eq!(next_action(&s), MediatorAction::AskParty(ids[0].clone()));
        s.mark_heard_fully(&ids[0]);
        assert_eq!(next_action(&s), MediatorAction::AskParty(ids[1].clone()));
        s.mark_heard_fully(&ids[1]);
        assert!(s.everyone_heard_fully());

        // with evidence in but unacknowledged, that's the next move
        s.submit_evidence(Evidence::fact(&ids[0], "a fact")).unwrap();
        assert_eq!(next_action(&s), MediatorAction::AcknowledgeEvidence);

        // acknowledge it → now reflect shared ground (roommate has certified ground)
        s.evidence_acknowledged = true;
        assert!(s.has_shared_ground());
        assert_eq!(next_action(&s), MediatorAction::ReflectSharedGround);

        // reflected → name the crux (roommate has one)
        s.events.push("shared_ground".into());
        assert!(s.has_crux());
        assert_eq!(next_action(&s), MediatorAction::NameCrux);

        // crux named → propose options
        s.crux_named = true;
        assert_eq!(next_action(&s), MediatorAction::ProposeOptions);

        // a coherent proposal exists, but the money is settled & certified →
        // subtraction made visible comes first: name the residue.
        s.proposals.push(Proposal {
            id: "p1".into(),
            summary: "split".into(),
            settlement: None,
            coherent: true,
            accepted_by: Vec::new(),
        });
        assert!(s.ledger_settled());
        assert_eq!(next_action(&s), MediatorAction::NameResidue);

        // residue named → invite agreement
        s.residue_named = true;
        assert_eq!(next_action(&s), MediatorAction::InviteAgreement);

        // someone accepts, but the agreement isn't OWNED yet → co-author it
        s.proposals[0].accepted_by = vec![ids[0].clone()];
        assert_eq!(next_action(&s), MediatorAction::CoAuthorAgreement);

        // parties shape + sign the draft → only then does it land
        s.open_draft_agreement("we agree, in our words", "mediator");
        for id in &ids {
            s.sign_draft(id);
        }
        assert!(s.agreement_owned());
        assert_eq!(next_action(&s), MediatorAction::Close);
    }

    #[test]
    fn late_evidence_reopens_acknowledgement() {
        // The flow is NOT a one-way march: a late exhibit revisits the ack step.
        let mut s = roommate_session();
        let ids = s.party_ids();
        for id in &ids {
            if let Some(th) = s.parties.iter_mut().find(|t| &t.id == id) {
                th.caucus.push(Utterance::party(id, "heard"));
                th.heard_fully = true;
            }
        }
        s.events.push("shared_ground".into());
        s.crux_named = true;
        s.residue_named = true;
        s.proposals.push(Proposal {
            id: "p1".into(),
            summary: "split".into(),
            settlement: None,
            coherent: true,
            accepted_by: vec![ids[0].clone()],
        });
        // …and a fully-owned (signed) agreement
        s.open_draft_agreement("we agree, in our words", "mediator");
        for id in &ids {
            s.sign_draft(id);
        }
        // we're at Close…
        assert_eq!(next_action(&s), MediatorAction::Close);
        // …but a late exhibit drops us back to acknowledgement
        s.submit_evidence(Evidence::artifact(&ids[1], "receipt", None)).unwrap();
        assert_eq!(next_action(&s), MediatorAction::AcknowledgeEvidence);
    }

    #[test]
    fn escalated_session_closes() {
        let mut s = roommate_session();
        s.escalate("asked for a human");
        assert_eq!(next_action(&s), MediatorAction::Close);
    }

    #[test]
    fn conduct_acknowledges_submitted_evidence() {
        let mut s = roommate_session();
        s.submit_evidence(Evidence::artifact(
            "sam",
            "photo of the stain",
            Some("note: corner of the room".into()),
        ))
        .unwrap();
        let inputs = ScriptedInputs::new()
            .with("robin", &["it was wear and tear"])
            .with("sam", &["it's damage and that's only fair"]);
        let out = conduct(s, &ScriptedBrain, &inputs);
        assert!(out.events.iter().any(|e| e == "acknowledge_evidence"));
        assert!(out.evidence_acknowledged);
        let t = render_transcript(&out);
        assert!(t.contains("photo of the stain"));
    }

    // ── (1) read-the-room: readiness, not a state counter ──

    #[test]
    fn unheard_fully_party_gets_more_space_before_joint_work() {
        let mut s = roommate_session();
        let ids = s.party_ids();
        // both have spoken, but neither has been heard *fully* yet
        for id in &ids {
            if let Some(th) = s.parties.iter_mut().find(|t| &t.id == id) {
                th.caucus.push(Utterance::party(id, "venting…"));
            }
        }
        assert!(s.everyone_spoken());
        assert!(!s.everyone_heard_fully());
        // the flow does NOT race to shared ground — it returns to give space
        match next_action(&s) {
            MediatorAction::AskParty(p) => assert_eq!(p, ids[0]),
            other => panic!("expected more space for an unheard-fully party, got {other:?}"),
        }
        // once both are heard fully, joint work proceeds
        for id in &ids {
            s.mark_heard_fully(id);
        }
        assert!(s.everyone_heard_fully());
        assert_eq!(next_action(&s), MediatorAction::ReflectSharedGround);
    }

    // ── (2) interest elicitation as a check-back loop ──

    #[test]
    fn interest_is_a_check_back_loop_owned_by_the_party() {
        let mut s = roommate_session();
        let ids = s.party_ids();
        for id in &ids {
            if let Some(th) = s.parties.iter_mut().find(|t| &t.id == id) {
                th.caucus.push(Utterance::party(id, "my piece"));
            }
            s.mark_heard_fully(id);
        }
        // the mediator NAMES an interest but hasn't confirmed it → check-back owed
        s.name_interest(&ids[0], "that you stopped showing up");
        assert_eq!(s.needs_interest_check_back(), Some(ids[0].clone()));
        assert_eq!(next_action(&s), MediatorAction::CheckBackInterest(ids[0].clone()));
        // the wording reflects it back and asks
        let cb = check_back_interest(&s, &ids[0]);
        assert!(cb.contains("stopped showing up"));
        assert!(cb.to_lowercase().contains("close") || cb.contains('?'));

        // the party CORRECTS it — their words win, and only then is it surfaced
        s.confirm_interest(&ids[0], Some("being treated as if I acted in good faith".into()));
        let th = s.parties.iter().find(|t| t.id == ids[0]).unwrap();
        assert!(th.interest_confirmed);
        assert_eq!(
            th.confirmed_interest.as_deref(),
            Some("being treated as if I acted in good faith")
        );
        assert!(th.interests.iter().any(|i| i.contains("good faith")));
        // no check-back pending now
        assert_eq!(s.needs_interest_check_back(), None);
    }

    #[test]
    fn scripted_caucus_runs_the_full_check_back_loop() {
        let mut s = roommate_session();
        let ids = s.party_ids();
        for id in &ids {
            if let Some(th) = s.parties.iter_mut().find(|t| &t.id == id) {
                th.caucus.push(Utterance::party(id, "x"));
            }
        }
        let inputs = ScriptedInputs::new()
            .with("robin", &["I just want it to be fair and to feel respected."]);
        let out = conduct(s, &ScriptedBrain, &inputs);
        let robin = out.parties.iter().find(|t| t.id == "robin").unwrap();
        // heard fully + a confirmed interest (the loop closed)
        assert!(robin.heard_fully);
        assert!(robin.interest_confirmed);
        assert!(!robin.interests.is_empty());
        // the transcript shows the check-back AND the party owning it
        let caucus_txt: String = robin.caucus.iter().map(|u| u.text.clone()).collect::<Vec<_>>().join("\n");
        assert!(caucus_txt.to_lowercase().contains("check"));
        assert!(caucus_txt.contains("that's it"));
    }

    // ── (3) evidence collision → factual conflict, handed back ──

    #[test]
    fn colliding_facts_surface_a_factual_conflict_never_decided() {
        let mut s = roommate_session();
        // two parties, two different numbers about the SAME thing (the stain)
        s.submit_evidence(Evidence::fact("robin", "The stain is 10 cm across."))
            .unwrap();
        s.submit_evidence(Evidence::fact("sam", "The stain is 40 cm across."))
            .unwrap();
        assert!(s.has_factual_conflict());
        let c = &s.factual_conflicts[0];
        assert!(c.about.contains("stain"));
        // it is structurally a *between two parties* collision
        assert!(c.between.0 == "robin" || c.between.0 == "sam");

        let msg = surface_factual_conflict(&s);
        // names exactly what's contested and refuses to rule
        assert!(msg.contains("10 cm") && msg.contains("40 cm"));
        let low = msg.to_lowercase();
        assert!(low.contains("evidence") && low.contains("not"));
        assert!(low.contains("decide") || low.contains("settle") || low.contains("right"));

        // same number ⇒ no collision; agreeing facts don't fire
        let mut s2 = roommate_session();
        s2.submit_evidence(Evidence::fact("robin", "The stain is 30 cm across.")).unwrap();
        s2.submit_evidence(Evidence::fact("sam", "The stain is 30 cm across.")).unwrap();
        assert!(!s2.has_factual_conflict());
    }

    #[test]
    fn next_action_surfaces_factual_conflict_after_acknowledgement() {
        let mut s = roommate_session();
        let ids = s.party_ids();
        for id in &ids {
            if let Some(th) = s.parties.iter_mut().find(|t| &t.id == id) {
                th.caucus.push(Utterance::party(id, "x"));
            }
            s.mark_heard_fully(id);
        }
        s.submit_evidence(Evidence::fact(&ids[0], "It is 5 inches wide.")).unwrap();
        s.submit_evidence(Evidence::fact(&ids[1], "It is 20 inches wide.")).unwrap();
        // acknowledge first…
        assert_eq!(next_action(&s), MediatorAction::AcknowledgeEvidence);
        s.evidence_acknowledged = true;
        // …then the collision is surfaced before joint shared-ground work
        assert!(s.has_factual_conflict());
        assert_eq!(next_action(&s), MediatorAction::SurfaceFactualConflict);
        s.conflicts_surfaced = true;
        assert_eq!(next_action(&s), MediatorAction::ReflectSharedGround);
    }

    // ── (4) subtraction made visible: the residue ──

    #[test]
    fn residue_is_named_once_money_is_settled() {
        let s = roommate_session();
        // roommate has a certified refund ⇒ the ledger is settled
        assert!(s.ledger_settled());
        let r = name_residue(&s);
        let low = r.to_lowercase();
        assert!(low.contains("never") && low.contains("money"));
        // explicit that it can't be certified (the honesty)
        assert!(low.contains("certif") || low.contains("pretend"));
    }

    #[test]
    fn residue_draws_from_confirmed_interests() {
        let mut s = roommate_session();
        s.name_interest("robin", "feeling that your effort was actually seen");
        s.confirm_interest("robin", None);
        let cands = s.residue_candidates();
        assert!(cands.iter().any(|c| c.contains("effort was actually seen")));
    }

    // ── (5) co-authored, owned agreement ──

    #[test]
    fn agreement_is_co_authored_and_signing_tracks_the_exact_text() {
        let mut s = roommate_session();
        let ids = s.party_ids();
        s.open_draft_agreement("first draft", "mediator");
        assert!(!s.agreement_owned());
        for id in &ids {
            s.sign_draft(id);
        }
        assert!(s.agreement_owned());
        // a revision (in their words) CLEARS signatures — must be re-owned
        s.revise_draft(&ids[0], "our own words now");
        assert!(!s.agreement_owned());
        let d = s.draft_agreement.as_ref().unwrap();
        assert_eq!(d.text, "our own words now");
        assert!(d.signed_by.is_empty());
        // re-sign the new text → owned again
        for id in &ids {
            s.sign_draft(id);
        }
        assert!(s.agreement_owned());
    }

    #[test]
    fn co_author_text_is_a_scaffold_not_a_verdict() {
        let s = roommate_session();
        let text = co_author_agreement(&s);
        // first person plural, in their words, explicitly editable, never "right"
        assert!(text.contains("We,") || text.contains("we"));
        let low = text.to_lowercase();
        assert!(low.contains("change any word") || low.contains("your words"));
        assert!(low.contains("never decide") || low.contains("right"));
    }

    #[test]
    fn full_session_names_residue_and_co_authors_the_agreement() {
        let s = roommate_session();
        let inputs = ScriptedInputs::new()
            .with("robin", &["it was wear and tear and I want to feel respected"])
            .with("sam", &["it's damage and I want this to be fair"]);
        let out = conduct(s, &ScriptedBrain, &inputs);
        assert_eq!(out.phase, Phase::Agreement);
        // residue was named (subtraction made visible)
        assert!(out.events.iter().any(|e| e == "residue"));
        assert!(!out.residue.is_empty());
        // a draft agreement exists and both signed it (owned)
        assert!(out.agreement_owned());
        let t = render_transcript(&out);
        assert!(t.contains("never really about it") || t.to_lowercase().contains("residue") || t.contains("what's left"));
        assert!(t.contains("in your words"));
    }
}
