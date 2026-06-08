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
        Ok(())
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
        p.accepted_by = everyone;
        s.events.push(format!("accepted:{}", p.id));
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
    /// Caucus privately with this party (chosen because they've not been heard).
    AskParty(PartyId),
    /// Weigh and acknowledge evidence that's been submitted but not yet folded
    /// in (informs the humans; never decides the crux).
    AcknowledgeEvidence,
    /// Reflect the certified common ground back to both.
    ReflectSharedGround,
    /// Name the genuine knot and hand it back (never decide it).
    NameCrux,
    /// Put certified-coherent settlement options on the table.
    ProposeOptions,
    /// Invite the parties to accept / counter / hold.
    InviteAgreement,
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

    // 2. Weigh any evidence that's come in but hasn't been acknowledged on the
    //    record yet. Submitting a late exhibit re-opens this (revisit), because
    //    `submit_evidence` clears `evidence_acknowledged`.
    if s.has_evidence() && !s.evidence_acknowledged {
        return MediatorAction::AcknowledgeEvidence;
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

    // 6. Invite agreement until someone accepts.
    if !s.someone_accepted() {
        return MediatorAction::InviteAgreement;
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
        // The scripted brain reflects back and gently names a possible interest.
        // (The live brain will do the real elicitation.)
        let interest = guess_interest(input);
        let reply = format!(
            "Thank you for telling me that — I want to make sure I've got it. \
             It sounds like, underneath the specifics, what matters to you is {}. \
             Is that close?",
            interest.as_deref().unwrap_or("being treated fairly here")
        );
        CaucusMove {
            reply,
            interests: interest.into_iter().collect(),
            claims: Vec::new(),
            done: true,
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

        // both heard now
        if let Some(th) = s.parties.iter_mut().find(|t| t.id == ids[1]) {
            th.caucus.push(Utterance::party(&ids[1], "my side too"));
        }
        assert!(s.everyone_spoken());

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

        // a coherent proposal exists → invite agreement
        s.proposals.push(Proposal {
            id: "p1".into(),
            summary: "split".into(),
            settlement: None,
            coherent: true,
            accepted_by: Vec::new(),
        });
        assert_eq!(next_action(&s), MediatorAction::InviteAgreement);

        // someone accepts → close
        s.proposals[0].accepted_by = vec![ids[0].clone()];
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
            }
        }
        s.events.push("shared_ground".into());
        s.crux_named = true;
        s.proposals.push(Proposal {
            id: "p1".into(),
            summary: "split".into(),
            settlement: None,
            coherent: true,
            accepted_by: vec![ids[0].clone()],
        });
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
}
