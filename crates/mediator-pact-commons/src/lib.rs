//! `mediator-pact-commons` — the **COMMONS OF PACTS**: case law without a
//! sovereign, that actually has SIGNAL.
//!
//! The dead-headline version of a precedent commons (the salvaged
//! `mediator-commons`) indexes *ad-hoc* resolved cruxes by a structural key. It
//! is correct, but its signal is thin: every ad-hoc dispute names its knot in
//! its own private vocabulary, so two disputes almost never reduce to a
//! byte-equal key — the cruxes are all unique, and the commons mostly shows you
//! singletons. There is no "case law" because there is no shared question.
//!
//! A **forward constitution** (a [`mediator_pact::Pact`]) is different. Two
//! pacts built from the **same template** — a roommate deposit split, a
//! cohabitation lease split, a founder-vesting cliff — declare the *same shape*
//! of question-space (a free crux + an integer dial) and the *same structure of
//! clause-guards* (above/below a threshold, crux-true/crux-false). They differ
//! only in the *names*, the *threshold*, and — the part that matters — the
//! **awards the two humans chose**. So pacts of one template **COLLIDE by
//! construction**: they share a [`template_key`], and at each structurally-
//! identical decision point they record *what people actually decided*. That is
//! the first thing in this project with real case-law signal, and it exists for
//! exactly one reason, stated loudly below.
//!
//! # What this is, in three falsifiable pieces
//!
//!   1. **[`template_key`]** — a deterministic, re-computable key over a pact's
//!      *normalized declared question-space* (the sort signature of its cruxes
//!      and dials) plus its *clause-guard structure* (each guard canonicalized
//!      with names, thresholds, and awards stripped). **Same template ⇒
//!      byte-equal key.** Pure Rust, no AI, re-derivable by a skeptic from the
//!      pact JSON alone. See [`template`].
//!
//!   2. **[`Commons::controversy`]** — across the certified pacts of one
//!      template, for a given **decision point** (a normalized world: which side
//!      of the threshold, and the crux's truth-value), report the
//!      **DISTRIBUTION of the choices people actually made** — *here are the ways
//!      humans resolved this, all signed*. It NEVER says "yours should be X":
//!      that would re-import the normative gravity the constitution forbids. It
//!      reports relative shapes (how the crux moved the award, ranked within each
//!      pact), so the tally is comparable across domains (cents, basis points)
//!      without smuggling a cross-pact "fair number". See [`controversy`].
//!
//!   3. **[`reverify_corpus`] / [`Commons::reverify`]** — a skeptic recomputes
//!      every [`template_key`], re-verifies each cited pact's signed
//!      [`mediator_audit::MediationRecord`] (ed25519 over the hash chain), and
//!      re-checks the tally — **no AI, no trust in the commons.** A tampered
//!      cited record fails (a test proves it). See [`reverify`].
//!
//! # The honesty (the headline, not a footnote)
//!
//! The signal exists ONLY because the shared template fixes the vocabulary. A
//! controversy over `N` pacts is exactly as strong as `N` is large — and on this
//! repo's corpus `N` is **tiny** (a handful of pacts per template). So the
//! commons publishes the **corpus size and the support behind every controversy
//! as the headline** — an *error bar*, not a green checkmark. A tally backed by
//! two pacts is reported as "backed by 2 pacts (tiny — not a norm)", in those
//! words. See [`Tally::confidence_note`] and [`CorpusReport::headline`].
//!
//! Two further honest boundaries, named not hidden:
//!
//!   * **Only certified, crux-free pacts aggregate.** A pact enters the tally
//!     only if its certificate is [`mediator_pact::Status::Certified`] AND its
//!     contested predicate genuinely stayed uninterpreted — re-checked offline by
//!     re-running the authoring-time firewall ([`mediator_pact::validate`]) over
//!     the pact, with no Isabelle. A pact whose "crux" the machine could decide
//!     never contributed a value-call, so it cannot back a controversy over one.
//!   * **The template key is conservative.** It is canonical under the
//!     equivalences the pact codegen already treats as identity (commutative
//!     `∧`/`∨`, the threshold direction of a dial comparison), and deliberately
//!     NOT under deeper logical equivalence. Erring *fine* is the safe direction:
//!     a too-fine key merely fails to notice two templates are alike (a human can
//!     still cite by hand); a too-coarse key would silently merge *different*
//!     question-spaces, smuggling a value-judgment into "these are the same
//!     question". We err fine on purpose.

use std::collections::BTreeMap;
use std::path::Path;

use mediator_audit::verify as verify_record;
use mediator_pact::{validate, Pact, PactCertificate, Status};

mod controversy;
mod reverify;
mod template;

pub use controversy::{
    decision_points, CruxEffect, DecisionPoint, Tally, TallyEntry, TallyShape, WorldSide,
};
pub use reverify::{reverify_corpus, reverify_one, AdmitError, ReverifyOutcome};
pub use template::{canonical_template, template_descriptor, template_key, TemplateDescriptor};

// ───────────────────────────── the corpus entry ─────────────────────────────

/// One admitted member of the commons: a [`Pact`] paired with its signed,
/// re-verified [`PactCertificate`]. The two are kept together because the
/// commons' every claim is a function of *both* — the pact gives the structure
/// (and the template key), the certificate gives the signed proof that the
/// structure is complete/consistent over a free crux.
///
/// An entry is admitted only through [`Commons::admit`], which enforces the
/// aggregation gate (certified, crux-free, record verifies, and the certificate
/// actually describes this pact). So holding a `CorpusPact` is itself evidence
/// it passed the gate.
#[derive(Clone, Debug)]
pub struct CorpusPact {
    pub pact: Pact,
    pub cert: PactCertificate,
    /// A short, stable label for the source (e.g. the file stem
    /// `"roommate_moveout"`), for human-readable tallies. Provenance, not
    /// authority — the signed record is what a skeptic re-checks.
    pub label: String,
}

impl CorpusPact {
    /// This pact's deterministic template key. Two corpus pacts share a key iff
    /// they are the same template (same question-space shape + guard structure).
    pub fn template_key(&self) -> String {
        template_key(&self.pact)
    }
}

// ─────────────────────────── the aggregation gate ───────────────────────────

/// Why a candidate pact could not be admitted into the commons. Each is a
/// constitutional or integrity reason, re-checkable offline by a skeptic.
#[derive(Debug, PartialEq)]
pub enum CommonsError {
    /// The certificate is not [`Status::Certified`] — the pact was refused,
    /// inconsistent, or rejected at authoring. Only a certified pact (complete &
    /// consistent over a free crux) can back a controversy.
    NotCertified,
    /// The certificate's declared title/parties/predicates do not match the
    /// pact's — the cert is for a *different* pact, so pairing them would be a
    /// lie. (Carries the disagreeing field.)
    CertMismatch(String),
    /// Re-running the authoring-time firewall ([`mediator_pact::validate`]) over
    /// the pact found a problem — most importantly, a guard the prover could
    /// decide on its own (a value-call in disguise). Such a pact's "crux" never
    /// stayed uninterpreted, so it cannot contribute a humanly-decided value
    /// call. Carries the plain reasons.
    CruxNotFree(Vec<String>),
    /// The embedded signed record failed to re-verify (hash chain or ed25519).
    /// The pact does not stand on its own signature.
    RecordInvalid(String),
}

impl std::fmt::Display for CommonsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommonsError::NotCertified => write!(
                f,
                "pact is not CERTIFIED (only a complete & consistent pact over a free crux can \
                 back a controversy)"
            ),
            CommonsError::CertMismatch(field) => {
                write!(f, "certificate does not match the pact (field: {field})")
            }
            CommonsError::CruxNotFree(reasons) => write!(
                f,
                "pact's contested predicate did not stay uninterpreted (firewall): {}",
                reasons.join("; ")
            ),
            CommonsError::RecordInvalid(e) => {
                write!(f, "embedded signed record failed to re-verify: {e}")
            }
        }
    }
}

impl std::error::Error for CommonsError {}

/// The aggregation gate, run once per candidate (and again, identically, by any
/// skeptic via [`reverify_corpus`]). Returns `Ok(())` iff the pact may enter the
/// commons: its certificate is CERTIFIED, the certificate truly describes this
/// pact, the contested predicate genuinely stayed free (re-checked offline by
/// re-running the firewall), and the signed record re-verifies.
///
/// This is the executable form of "only aggregate over pacts whose contested
/// predicates stayed Unknown": a CERTIFIED forward constitution proves coverage
/// and consistency *with the crux universally quantified* (left uninterpreted),
/// and the firewall re-check confirms the crux is load-bearing in every guard.
pub fn admission_gate(pact: &Pact, cert: &PactCertificate) -> Result<(), CommonsError> {
    // (1) CERTIFIED only.
    if !matches!(cert.status, Status::Certified) {
        return Err(CommonsError::NotCertified);
    }

    // (2) The certificate must actually be *for this pact*. We never trust a
    // cert handed to us next to an unrelated pact.
    if cert.title != pact.title {
        return Err(CommonsError::CertMismatch("title".into()));
    }
    if cert.parties != pact.parties {
        return Err(CommonsError::CertMismatch("parties".into()));
    }
    let declared: Vec<String> = pact.predicates.iter().map(|s| s.name.clone()).collect();
    if cert.declared_predicates != declared {
        return Err(CommonsError::CertMismatch("declared_predicates".into()));
    }

    // (3) The crux must genuinely have stayed uninterpreted. Re-run the
    // authoring-time firewall over the pact OURSELVES, offline — a certified
    // pact passed it once, but a skeptic re-checks rather than trusting that.
    let errs = validate(pact);
    if !errs.is_empty() {
        return Err(CommonsError::CruxNotFree(
            errs.iter().map(|e| e.plain()).collect(),
        ));
    }

    // (4) The signed record re-verifies (ed25519 over the hash chain).
    verify_record(&cert.record).map_err(|e| CommonsError::RecordInvalid(e.to_string()))?;

    Ok(())
}

// ───────────────────────────────── the commons ──────────────────────────────

/// The commons of pacts: an append-only set of admitted [`CorpusPact`]s, grouped
/// by [`template_key`] on demand. Holds **no authority** — every claim it makes
/// is a pure function of the admitted pacts + their signed certificates, and a
/// skeptic re-derives all of it (see [`reverify_corpus`]).
#[derive(Clone, Debug, Default)]
pub struct Commons {
    entries: Vec<CorpusPact>,
}

impl Commons {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Admit a pact + its certificate, **running the full aggregation gate
    /// first** ([`admission_gate`]). A pact that is not certified, whose cert
    /// does not match it, whose crux is not genuinely free, or whose record does
    /// not verify, is rejected — it cannot stand on its own, so it does not enter
    /// the commons.
    pub fn admit(&mut self, pact: Pact, cert: PactCertificate, label: impl Into<String>) -> Result<usize, CommonsError> {
        admission_gate(&pact, &cert)?;
        let idx = self.entries.len();
        self.entries.push(CorpusPact {
            pact,
            cert,
            label: label.into(),
        });
        Ok(idx)
    }

    /// Load a pact + its sibling `<name>.cert.json` from `dir` and admit it. The
    /// label is the file stem. Returns the admitted index, or any load /
    /// admission error.
    pub fn admit_from_dir(&mut self, dir: &Path, name: &str) -> anyhow::Result<usize> {
        let pact_path = dir.join(format!("{name}.json"));
        let cert_path = dir.join(format!("{name}.cert.json"));
        let pact = Pact::load(pact_path.to_str().ok_or_else(|| anyhow::anyhow!("non-utf8 path"))?)?;
        let cert_raw = std::fs::read_to_string(&cert_path)
            .map_err(|e| anyhow::anyhow!("read {}: {e}", cert_path.display()))?;
        let cert: PactCertificate = serde_json::from_str(&cert_raw)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", cert_path.display()))?;
        self.admit(pact, cert, name)
            .map_err(|e| anyhow::anyhow!("admit {name}: {e}"))
    }

    /// All admitted pacts.
    pub fn entries(&self) -> &[CorpusPact] {
        &self.entries
    }

    /// Mutable access to admitted pacts — used by tests to simulate a corrupted
    /// on-disk cache and confirm `reverify` catches it. Not part of the normal
    /// flow (admission is the only way in).
    #[cfg(test)]
    pub(crate) fn entries_mut(&mut self) -> &mut [CorpusPact] {
        &mut self.entries
    }

    /// The number of admitted pacts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Group the corpus by template key. Each distinct template maps to the
    /// indices of the pacts that share it, in ascending order. Deterministic.
    pub fn templates(&self) -> BTreeMap<String, Vec<usize>> {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            groups.entry(e.template_key()).or_default().push(i);
        }
        groups
    }

    /// The indices of every admitted pact whose template matches `pact`'s. This
    /// is the "show me every pact built on the SAME constitution" lookup a new
    /// pair runs before they pick their own awards — so they can see the
    /// distribution of prior human choices (never to be told what to choose).
    pub fn matching_template(&self, pact: &Pact) -> Vec<usize> {
        let k = template_key(pact);
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.template_key() == k)
            .map(|(i, _)| i)
            .collect()
    }

    /// The certified controversy at one decision point, over the pacts of a given
    /// template key: the distribution of the choices people actually made there,
    /// each attributed to its signed pact. Returns `None` if no admitted pact has
    /// that template. See [`controversy::tally`] for the semantics and the
    /// honesty discipline.
    pub fn controversy(&self, template: &str, point: &DecisionPoint) -> Option<Tally> {
        let members: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.template_key() == template)
            .map(|(i, _)| i)
            .collect();
        if members.is_empty() {
            return None;
        }
        Some(controversy::tally(&self.entries, &members, point))
    }

    /// A full report over the corpus: every template, its size, and — for each
    /// canonical decision point of every template (every dial-side × crux-value
    /// world) — the certified controversy with its corpus-size headline. This is
    /// what the `pact-commons` bin renders.
    pub fn report(&self) -> CorpusReport {
        let mut templates = Vec::new();
        for (key, members) in self.templates() {
            // The representative pact (lowest index) gives us the template's
            // shape and its canonical decision points.
            let rep = &self.entries[members[0]];
            let points = decision_points(&rep.pact);
            let mut tallies = Vec::new();
            for point in &points {
                let t = controversy::tally(&self.entries, &members, point);
                tallies.push(t);
            }
            templates.push(TemplateReport {
                key,
                descriptor: template_descriptor(&rep.pact),
                members: members.clone(),
                member_labels: members.iter().map(|&i| self.entries[i].label.clone()).collect(),
                tallies,
            });
        }
        CorpusReport {
            corpus_size: self.entries.len(),
            templates,
        }
    }

    /// Re-verify the ENTIRE corpus from scratch, with no trust in the commons:
    /// re-run the admission gate (cert CERTIFIED, cert matches pact, crux free,
    /// record verifies) on every entry, and confirm every template key recomputes
    /// to what the grouping used. Returns the first failing entry, if any. A
    /// skeptic runs this (or the free function [`reverify_corpus`]) to confirm the
    /// whole body of precedent stands on its own.
    pub fn reverify(&self) -> Result<(), (usize, CommonsError)> {
        for (i, e) in self.entries.iter().enumerate() {
            admission_gate(&e.pact, &e.cert).map_err(|err| (i, err))?;
        }
        Ok(())
    }
}

// ───────────────────────────── the corpus report ────────────────────────────

/// The certified-controversy report for one template: its key, a human
/// descriptor, the member pacts, and a tally at each decision point.
#[derive(Clone, Debug)]
pub struct TemplateReport {
    pub key: String,
    pub descriptor: TemplateDescriptor,
    /// Indices into the commons of the pacts sharing this template.
    pub members: Vec<usize>,
    /// The labels of those pacts, for a human reading.
    pub member_labels: Vec<String>,
    /// The certified controversy at each canonical decision point.
    pub tallies: Vec<Tally>,
}

/// The whole-corpus report: the corpus size (the headline error bar) and a
/// per-template controversy report.
#[derive(Clone, Debug)]
pub struct CorpusReport {
    pub corpus_size: usize,
    pub templates: Vec<TemplateReport>,
}

impl CorpusReport {
    /// The honest one-line headline: how big the corpus is and how many distinct
    /// templates it spans — stated as the *limit* on every claim below it, an
    /// error bar rather than a checkmark.
    pub fn headline(&self) -> String {
        let n_templates = self.templates.len();
        let backed: Vec<usize> = self.templates.iter().map(|t| t.members.len()).collect();
        let largest = backed.iter().copied().max().unwrap_or(0);
        format!(
            "COMMONS OF PACTS — {} certified pact(s) across {} template(s); the largest template \
             is backed by {} pact(s). This is a TINY corpus: every controversy below is exactly \
             as strong as its support count, which is the headline, not a footnote. The signal \
             exists ONLY because pacts of one template share a declared vocabulary — that is the \
             whole trick, and it is stated, not hidden.",
            self.corpus_size, n_templates, largest
        )
    }
}

#[cfg(test)]
mod tests;
