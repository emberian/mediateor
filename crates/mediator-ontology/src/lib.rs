//! `mediator-ontology` — vocabulary negotiation: telling a real disagreement
//! apart from two people using different words for the same thing.
//!
//! Each party brings a signature (their vocabulary, a `Vec<Sig>`). Mediation is,
//! in part, the **pushout of the alignment span** between those vocabularies:
//!
//! ```text
//!            shared / synonym edges
//!      Σ_A  ◄───────── Span ──────────►  Σ_B
//!        \                               /
//!         \           pushout           /
//!          ▼                           ▼
//!                 merged vocabulary
//! ```
//!
//! Where the merge identifies two symbols, an apparent "disagreement" was just
//! unshared *words*; where two predicate names land in *distinct* merged classes,
//! the clash is genuine. This is the institution-theory thread from the project's
//! beginning, made concrete.
//!
//! ## Trust boundary
//!
//! The offline core is **deterministic**: type compatibility is structural and
//! gloss similarity is a fixed token/Jaccard heuristic over lowercased glosses
//! minus stopwords. An optional LLM hook ([`SynonymOracle`]) may *re-rank or veto*
//! candidates, but it can never manufacture a shared symbol the host would not
//! certify, and every test here runs with no network and no oracle.
//!
//! ## Structural pushout
//!
//! The merge is the **coequalizer of the coproduct** Σ_A ⊔ Σ_B under the
//! alignment edges, which is exactly *connected components* of the alignment
//! graph. We borrow that primitive directly from `open-hypergraphs`
//! (`array::vec::connected_components`) so the merge is computed by the same
//! data-parallel routine the string-diagram substrate uses for coequalizers —
//! the honest categorical operation, not a hand-rolled lookalike.
//!
//! ## Pushout of *theories*, not just vocabularies (exploratory)
//!
//! Aligning words is only half the institution-theory story. The other half is
//! the **amalgamation**: when both parties' stipulated facts are *translated
//! along the pushout maps* into the merged vocabulary, is the union still
//! consistent? In institution theory this is the pushout-of-theories /
//! amalgamation-property question — the colimit in the category of theory
//! presentations, not of signatures.
//!
//! We make a concrete, honest down-payment on this:
//!
//! * [`rename_formula`] is the **signature morphism applied to sentences** — the
//!   sentence-translation functor `Sen(σ)` of the institution, here a syntactic
//!   substitution of each predicate/function symbol by its merged canonical name.
//! * [`amalgamate`] applies that translation to both parties' stipulated facts,
//!   takes their union over the merged vocabulary, and runs a **sound-but-
//!   incomplete** syntactic consistency check (see [`AmalgamationReport`]).
//!
//! The check is deliberately modest: it finds *manifest* contradictions
//! (`p` and `¬p`, `t = a` and `t = b` for distinct literals, `p` and a stipulated
//! `p ⟶ q` chaining to `¬q`) once both sides speak the merged vocabulary. It does
//! **not** invoke the host prover and makes no completeness claim: a clean report
//! means "no contradiction was *witnessed* here," never "consistent." Real
//! consistency remains the host's job — this layer's value is *surfacing the
//! obligation* (the exact merged-vocabulary sentence set to hand to Isabelle) and
//! catching the cheap clashes before paying for a proof.
//!
//! ## Assessment (the skeptical case, stated fairly)
//!
//! Does the categorical framing earn its keep, or is it ornament over a lookup
//! table? Honestly, both, in different places:
//!
//! * **Earns its keep — the merge as connected components.** The one place the
//!   structure is *load-bearing* is transitive alignment. If A says `tenant`, B
//!   says `renter`, and a third utterance bridges `renter`≈`lessee`, the fact
//!   that `tenant` and `lessee` are now the *same concept* is a connected-
//!   components fact, not a pairwise one. A naive "is this pair a synonym?" table
//!   gets this wrong; the coequalizer gets it right by construction, and gives a
//!   single canonical name to rewrite both theories into. This is where "it's a
//!   pushout" is a real claim and not decoration.
//! * **Earns its keep — the well-typed-alias discipline.** Refusing to align
//!   symbols whose typed interfaces differ ([`types_compatible`]) is the functor
//!   laws doing actual work: it blocks "same word, incompatible arity/sort" false
//!   friends that a pure gloss-similarity heuristic would happily merge. That is a
//!   correctness guard, cheaply justified by the categorical condition.
//! * **Overkill — the binary clash verdict.** For the *immediate* question core
//!   asks ("are these two opposing predicates synonyms or not?"), a plain
//!   `types_compatible(a,b) && gloss_similarity(a,b) >= θ` would give the identical
//!   answer in the 2-symbol case. The span/pushout machinery is genuinely *equal*
//!   to the heuristic there. We keep it because (a) it generalizes to the
//!   transitive case above at no extra cost and (b) it yields the merged theory we
//!   need for amalgamation — but on its own, the verdict does not need a colimit.
//! * **Honest gap — the hard part is still the oracle.** Whether `tenant_caused
//!   _damage` and `renter_at_fault` *mean the same thing* is a natural-language
//!   judgment. The category theory organizes and constrains that judgment (typed,
//!   transitive, canonicalized); it does not make it. Jaccard-over-glosses is a
//!   stand-in for an LLM/human, and the [`SynonymOracle`] seam is where the real
//!   work would plug in. No amount of colimit removes that.
//!
//! Bottom line: the framing pays for itself at the *theory* level (typed
//! transitive merge + the amalgamation obligation it hands the prover) and is
//! break-even ornament at the *single-pair* level. We mark the exploratory
//! amalgamation pieces as such rather than overclaim them.

use mediator_types::{Sig, Sort};
use open_hypergraphs::array::vec::connected_components;
use serde::{Deserialize, Serialize};

/// Default gloss-similarity threshold for flagging a candidate synonym.
///
/// Jaccard overlap of content tokens; tuned so the canonical roommate example
/// ("the tenant caused the damage" vs "the renter is at fault for the damage")
/// clears the bar while unrelated glosses do not.
pub const DEFAULT_SYNONYM_THRESHOLD: f64 = 0.18;

// ───────────────────────────── type compatibility ─────────────────────────────

/// Two sorts are compatible iff they are structurally equal. `Uninterp` sorts
/// compare by their carried name, so two parties naming the same uninterpreted
/// sort (`Uninterp("Stain")`) are compatible even across signatures.
pub fn sorts_compatible(a: &Sort, b: &Sort) -> bool {
    a == b
}

/// Two symbols are *type-compatible* iff they have the same arity, pairwise
/// compatible argument sorts (in order), and a compatible return sort.
///
/// This is the categorical condition for the two symbols to be identifiable in a
/// merged signature: a synonym must have the *same typed interface*, otherwise
/// aligning them would not even typecheck.
pub fn types_compatible(a: &Sig, b: &Sig) -> bool {
    a.arg_sorts.len() == b.arg_sorts.len()
        && a.arg_sorts
            .iter()
            .zip(&b.arg_sorts)
            .all(|(x, y)| sorts_compatible(x, y))
        && sorts_compatible(&a.ret, &b.ret)
}

// ───────────────────────────── gloss similarity ─────────────────────────────

/// English stopwords stripped before measuring gloss overlap. Kept small and
/// closed so the heuristic stays deterministic and easy to reason about.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "of", "for", "to", "in",
    "on", "at", "by", "with", "and", "or", "but", "as", "that", "this", "these", "those", "it",
    "its", "their", "there", "has", "have", "had", "did", "does", "do", "who", "whom", "which",
    "from", "into", "than", "then", "so", "if", "not", "no",
];

fn is_stopword(tok: &str) -> bool {
    STOPWORDS.contains(&tok)
}

/// Tokenize a gloss into a sorted, de-duplicated set of lowercased content
/// words: split on any non-alphanumeric boundary, lowercase, drop stopwords and
/// 1-char tokens.
fn content_tokens(gloss: &str) -> Vec<String> {
    let mut toks: Vec<String> = gloss
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() > 1 && !is_stopword(s))
        .collect();
    toks.sort();
    toks.dedup();
    toks
}

/// Jaccard similarity of the two glosses' content-token sets: |A ∩ B| / |A ∪ B|.
///
/// Deterministic, symmetric, and in `[0, 1]`. Two empty glosses are defined to
/// have similarity `0.0` (no positive evidence of synonymy).
pub fn gloss_similarity(gloss_a: &str, gloss_b: &str) -> f64 {
    let a = content_tokens(gloss_a);
    let b = content_tokens(gloss_b);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut inter = 0usize;
    for t in &a {
        if b.binary_search(t).is_ok() {
            inter += 1;
        }
    }
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

// ───────────────────────────── optional LLM hook ─────────────────────────────

/// Optional, *untrusted* re-ranking hook for synonym candidates. The offline
/// heuristic proposes; an oracle may only **lower** confidence or veto (it can
/// shrink the candidate set, never invent membership the types don't permit).
///
/// Implementations may call a model; the core and all tests run with `None`.
pub trait SynonymOracle {
    /// Given two type-compatible symbols and the heuristic score, return an
    /// adjusted score in `[0, 1]`. Returning `0.0` vetoes the pair.
    fn rescore(&self, a: &Sig, b: &Sig, heuristic: f64) -> f64;
}

// ───────────────────────────── the analysis ─────────────────────────────

/// A symbol present in *both* signatures under the same name and a compatible
/// type — common ground, no negotiation needed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharedSymbol {
    pub name: String,
    /// `true` iff the glosses also agree closely (corroborating the alias).
    pub glosses_agree: bool,
    pub gloss_similarity: f64,
}

/// A candidate synonym pair: different names, compatible type, and close glosses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynonymCandidate {
    pub a_name: String,
    pub b_name: String,
    pub similarity: f64,
}

/// One element of the structural alignment span `Σ_A ← shared → Σ_B`, recording
/// *which* symbol of each party an alignment edge connects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlignmentEdge {
    /// Index into `sig_a`.
    pub a_index: usize,
    /// Index into `sig_b`.
    pub b_index: usize,
    /// Canonical name chosen for the merged class.
    pub merged_name: String,
    /// `"shared"` (same name) or `"synonym"` (heuristic match).
    pub kind: String,
}

/// One class of the merged vocabulary: the pushout's connected component.
/// `members` are `(party, name)` pairs identified by the alignment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MergedSymbol {
    pub canonical_name: String,
    /// `(party, name)`: party is `"A"`, `"B"`, or `"AB"` when both contribute.
    pub members: Vec<(String, String)>,
    /// `true` iff symbols from both parties landed in this class.
    pub bridged: bool,
}

/// The proposed alignment: the span (both vocabularies + the bridging edges) and
/// its pushout (the merged vocabulary).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposedAlignment {
    /// Party A's symbol names, in signature order (the left leg of the span).
    pub vocab_a: Vec<String>,
    /// Party B's symbol names, in signature order (the right leg of the span).
    pub vocab_b: Vec<String>,
    /// The bridging edges (shared + synonym) — the apex maps of the span.
    pub edges: Vec<AlignmentEdge>,
    /// The pushout: merged classes, one per connected component of the apex.
    pub merged: Vec<MergedSymbol>,
}

/// The full result of vocabulary negotiation between two signatures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VocabularyAnalysis {
    /// Symbols shared verbatim (same name, compatible type).
    pub shared: Vec<SharedSymbol>,
    /// Candidate synonyms as `(a_name, b_name, similarity)`.
    pub synonyms: Vec<(String, String, f64)>,
    /// The structural span + pushout.
    pub proposed_alignment: ProposedAlignment,
    /// Human-readable observations for the operator cockpit.
    pub notes: Vec<String>,
}

impl VocabularyAnalysis {
    /// Is `name` (in either party's vocabulary) bridged to the other party in the
    /// merged vocabulary? A bridged class means the two parties have a *shared*
    /// concept for that name.
    pub fn is_bridged(&self, name: &str) -> bool {
        self.proposed_alignment
            .merged
            .iter()
            .any(|m| m.bridged && m.members.iter().any(|(_, n)| n == name))
    }

    /// The merged (canonical) class name for a party symbol, if any.
    pub fn merged_class_of(&self, name: &str) -> Option<&str> {
        self.proposed_alignment
            .merged
            .iter()
            .find(|m| m.members.iter().any(|(_, n)| n == name))
            .map(|m| m.canonical_name.as_str())
    }
}

// ───────────────────────────── the entry point ─────────────────────────────

/// Analyze two parties' vocabularies: find shared symbols, candidate synonyms,
/// and propose the alignment span + its pushout (the merged vocabulary).
///
/// Fully offline and deterministic at the default threshold.
pub fn analyze(sig_a: &[Sig], sig_b: &[Sig]) -> VocabularyAnalysis {
    analyze_with(sig_a, sig_b, DEFAULT_SYNONYM_THRESHOLD, None)
}

/// As [`analyze`], but with an explicit similarity `threshold` and an optional
/// [`SynonymOracle`] for untrusted re-ranking. The default path passes `None`.
pub fn analyze_with(
    sig_a: &[Sig],
    sig_b: &[Sig],
    threshold: f64,
    oracle: Option<&dyn SynonymOracle>,
) -> VocabularyAnalysis {
    let mut shared = Vec::new();
    let mut synonyms: Vec<(String, String, f64)> = Vec::new();
    let mut edges: Vec<AlignmentEdge> = Vec::new();
    let mut notes = Vec::new();

    // Index B by name for the shared-symbol pass.
    // (Linear scans below are fine: signatures are tens of symbols, not millions.)

    // ── shared symbols: same name, compatible type ──
    for (ai, a) in sig_a.iter().enumerate() {
        if let Some((bi, b)) = sig_b
            .iter()
            .enumerate()
            .find(|(_, b)| b.name == a.name && types_compatible(a, b))
        {
            let sim = gloss_similarity(&a.gloss, &b.gloss);
            shared.push(SharedSymbol {
                name: a.name.clone(),
                glosses_agree: sim >= threshold,
                gloss_similarity: sim,
            });
            edges.push(AlignmentEdge {
                a_index: ai,
                b_index: bi,
                merged_name: a.name.clone(),
                kind: "shared".to_string(),
            });
        } else if sig_b.iter().any(|b| b.name == a.name && !types_compatible(a, b)) {
            // Same word, *incompatible* type — a real false friend; never aligned.
            notes.push(format!(
                "symbol \"{}\" appears in both vocabularies with incompatible types — \
                 NOT aligned (a likely genuine type-level disagreement)",
                a.name
            ));
        }
    }

    let shared_names: Vec<&str> = shared.iter().map(|s| s.name.as_str()).collect();

    // ── candidate synonyms: different names, compatible type, close glosses ──
    for (ai, a) in sig_a.iter().enumerate() {
        if shared_names.contains(&a.name.as_str()) {
            continue;
        }
        for (bi, b) in sig_b.iter().enumerate() {
            if a.name == b.name || shared_names.contains(&b.name.as_str()) {
                continue;
            }
            if !types_compatible(a, b) {
                continue;
            }
            let mut sim = gloss_similarity(&a.gloss, &b.gloss);
            if let Some(o) = oracle {
                sim = o.rescore(a, b, sim).clamp(0.0, 1.0);
            }
            if sim >= threshold {
                synonyms.push((a.name.clone(), b.name.clone(), sim));
                edges.push(AlignmentEdge {
                    a_index: ai,
                    b_index: bi,
                    // canonical name: lexicographically smaller, for determinism
                    merged_name: if a.name <= b.name {
                        a.name.clone()
                    } else {
                        b.name.clone()
                    },
                    kind: "synonym".to_string(),
                });
            }
        }
    }

    // Deterministic ordering of synonym candidates: by descending similarity,
    // then by names, so a symbol's best partner sorts first.
    synonyms.sort_by(|x, y| {
        y.2.partial_cmp(&x.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.0.cmp(&y.0))
            .then_with(|| x.1.cmp(&y.1))
    });

    let proposed_alignment = build_pushout(sig_a, sig_b, &edges);

    if shared.is_empty() && synonyms.is_empty() {
        notes.push(
            "no shared symbols and no candidate synonyms — the two vocabularies are disjoint; \
             any apparent clash is genuine, not a wording mismatch"
                .to_string(),
        );
    } else {
        notes.push(format!(
            "{} shared symbol(s), {} candidate synonym(s); merged vocabulary has {} class(es)",
            shared.len(),
            synonyms.len(),
            proposed_alignment.merged.len()
        ));
    }

    VocabularyAnalysis {
        shared,
        synonyms,
        proposed_alignment,
        notes,
    }
}

/// Build the pushout (merged vocabulary) as the coequalizer of the coproduct
/// `Σ_A ⊔ Σ_B` under the alignment edges — i.e. the connected components of the
/// alignment graph, computed by `open_hypergraphs::array::vec::connected_components`.
///
/// Node indexing of the coproduct: `0..|A|` are A's symbols, `|A|..|A|+|B|` are
/// B's symbols. Each alignment edge `(ai, bi)` becomes the undirected graph edge
/// `ai → |A| + bi`. Components with members from both halves are the *bridged*
/// (shared/synonym) classes; singletons are vocabulary unique to one party.
fn build_pushout(sig_a: &[Sig], sig_b: &[Sig], edges: &[AlignmentEdge]) -> ProposedAlignment {
    let na = sig_a.len();
    let nb = sig_b.len();
    let n = na + nb;

    let vocab_a: Vec<String> = sig_a.iter().map(|s| s.name.clone()).collect();
    let vocab_b: Vec<String> = sig_b.iter().map(|s| s.name.clone()).collect();

    if n == 0 {
        return ProposedAlignment {
            vocab_a,
            vocab_b,
            edges: edges.to_vec(),
            merged: Vec::new(),
        };
    }

    let sources: Vec<usize> = edges.iter().map(|e| e.a_index).collect();
    let targets: Vec<usize> = edges.iter().map(|e| na + e.b_index).collect();

    // The categorical coequalizer of the coproduct under the apex relation.
    let (node_to_component, num_components) = connected_components(&sources, &targets, n);

    // Gather members per component, preserving node order for determinism.
    let mut comp_members: Vec<Vec<(String, String)>> = vec![Vec::new(); num_components];
    for node in 0..n {
        let comp = node_to_component[node];
        let (party, name) = if node < na {
            ("A".to_string(), vocab_a[node].clone())
        } else {
            ("B".to_string(), vocab_b[node - na].clone())
        };
        comp_members[comp].push((party, name));
    }

    let mut merged: Vec<MergedSymbol> = comp_members
        .into_iter()
        .filter(|m| !m.is_empty())
        .map(|members| {
            let has_a = members.iter().any(|(p, _)| p == "A");
            let has_b = members.iter().any(|(p, _)| p == "B");
            let bridged = has_a && has_b;
            // Canonical name: lexicographically smallest member name (stable).
            let canonical_name = members
                .iter()
                .map(|(_, n)| n.clone())
                .min()
                .unwrap_or_default();
            // Collapse a member appearing in both parties under the same name to "AB".
            MergedSymbol {
                canonical_name,
                members,
                bridged,
            }
        })
        .collect();

    // Stable output order: bridged classes first, then by canonical name.
    merged.sort_by(|x, y| {
        y.bridged
            .cmp(&x.bridged)
            .then_with(|| x.canonical_name.cmp(&y.canonical_name))
    });

    ProposedAlignment {
        vocab_a,
        vocab_b,
        edges: edges.to_vec(),
        merged,
    }
}

// ───────────────────────────── clash classifier ─────────────────────────────

/// The verdict for a pair of opposing predicate names, given the vocabulary
/// analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClashKind {
    /// The two predicates are the *same word* (or already-shared symbol): if the
    /// parties disagree, they disagree about that one predicate's truth — genuine.
    GenuinePredicate,
    /// The two predicates are different words the analysis bridges as synonyms:
    /// the apparent clash is a **vocabulary mismatch** — align and dissolve.
    VocabularyMismatch { similarity: f64 },
    /// The two predicates are distinct, unbridged concepts: a **genuine**
    /// disagreement between different things.
    GenuineDistinctConcepts,
    /// One or both predicate names are absent from the analyzed vocabularies;
    /// the classifier abstains rather than guess.
    Indeterminate,
}

impl ClashKind {
    /// `true` iff the clash is (likely) only a wording difference that aligning
    /// the vocabularies would dissolve.
    pub fn is_vocabulary_mismatch(&self) -> bool {
        matches!(self, ClashKind::VocabularyMismatch { .. })
    }
}

/// Classify whether an apparent clash between two opposing claims — identified by
/// their top predicate names — is a likely **vocabulary mismatch** (synonyms →
/// align, dissolve) or a **genuine** disagreement about distinct concepts.
///
/// `pred_a` should name a symbol in party A's vocabulary and `pred_b` one in
/// party B's, but the function is robust to either ordering.
pub fn classify_clash(pred_a: &str, pred_b: &str, analysis: &VocabularyAnalysis) -> ClashKind {
    let known_a = analysis.proposed_alignment.vocab_a.iter().any(|n| n == pred_a)
        || analysis.proposed_alignment.vocab_b.iter().any(|n| n == pred_a);
    let known_b = analysis.proposed_alignment.vocab_a.iter().any(|n| n == pred_b)
        || analysis.proposed_alignment.vocab_b.iter().any(|n| n == pred_b);
    if !known_a || !known_b {
        return ClashKind::Indeterminate;
    }

    // Same word: the disagreement is over that predicate's truth value.
    if pred_a == pred_b {
        return ClashKind::GenuinePredicate;
    }

    // Different words flagged as synonyms → vocabulary mismatch.
    if let Some((_, _, sim)) = analysis.synonyms.iter().find(|(a, b, _)| {
        (a == pred_a && b == pred_b) || (a == pred_b && b == pred_a)
    }) {
        return ClashKind::VocabularyMismatch { similarity: *sim };
    }

    // Both landed in the same bridged merged class (e.g. transitively aligned).
    let same_class = match (
        analysis.merged_class_of(pred_a),
        analysis.merged_class_of(pred_b),
    ) {
        (Some(ca), Some(cb)) => ca == cb,
        _ => false,
    };
    if same_class {
        let sim = analysis
            .synonyms
            .iter()
            .find(|(a, b, _)| {
                (a == pred_a && b == pred_b) || (a == pred_b && b == pred_a)
            })
            .map(|(_, _, s)| *s)
            .unwrap_or(1.0);
        return ClashKind::VocabularyMismatch { similarity: sim };
    }

    ClashKind::GenuineDistinctConcepts
}

/// A proposed alignment between two clashing predicate names: the canonical
/// merged name both should be rewritten to, plus the gloss-similarity evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClashAlignment {
    pub a_name: String,
    pub b_name: String,
    /// The canonical name of the merged class the two predicates share.
    pub merged_name: String,
    pub similarity: f64,
}

/// The full verdict for a clash between two opposing claims, identified by their
/// top predicate names. This is the shape `mediator-core` consumes to enrich its
/// `dissolved` category: `Vocabulary` clashes can be *dissolved* (align + drop
/// the false disagreement), `Genuine` clashes are handed back UNDECIDED.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClashVerdict {
    pub kind: ClashKind,
    /// Confidence in `[0, 1]` that the verdict's *direction* is right.
    ///
    /// For a `VocabularyMismatch` this is the (oracle-adjusted) gloss similarity —
    /// how strongly the words look like synonyms. For a genuine clash it is how
    /// strongly we believe the concepts are *distinct* (1 − best cross-similarity
    /// for distinct concepts; a fixed high constant for a same-predicate clash,
    /// whose genuineness is structural, not heuristic). `Indeterminate` is `0`.
    pub confidence: f64,
    /// Present iff `kind` is a `VocabularyMismatch`: how to dissolve the clash.
    pub alignment: Option<ClashAlignment>,
}

impl ClashVerdict {
    /// `true` iff this clash is a wording difference that aligning the
    /// vocabularies would dissolve — the cue for core's `dissolved` category.
    pub fn is_vocabulary(&self) -> bool {
        self.kind.is_vocabulary_mismatch()
    }
    /// `true` iff this is a genuine disagreement to hand back UNDECIDED.
    pub fn is_genuine(&self) -> bool {
        matches!(
            self.kind,
            ClashKind::GenuinePredicate | ClashKind::GenuineDistinctConcepts
        )
    }
}

/// **The classifier `mediator-core` calls.** Given two parties' signatures and a
/// pair of opposing predicate names, decide whether the apparent clash is a
/// vocabulary mismatch (synonyms → align & dissolve) or genuine, with a
/// confidence and (when dissolvable) a concrete alignment to apply.
///
/// Runs the full vocabulary [`analyze`] internally, so callers need only the raw
/// signatures. Fully offline and deterministic at the default threshold; pass an
/// oracle via [`classify_clash_sigs_with`] to let a model re-rank.
pub fn classify_clash_sigs(
    sig_a: &[Sig],
    sig_b: &[Sig],
    pred_a: &str,
    pred_b: &str,
) -> ClashVerdict {
    classify_clash_sigs_with(sig_a, sig_b, pred_a, pred_b, DEFAULT_SYNONYM_THRESHOLD, None)
}

/// As [`classify_clash_sigs`], with an explicit threshold and optional oracle.
pub fn classify_clash_sigs_with(
    sig_a: &[Sig],
    sig_b: &[Sig],
    pred_a: &str,
    pred_b: &str,
    threshold: f64,
    oracle: Option<&dyn SynonymOracle>,
) -> ClashVerdict {
    let analysis = analyze_with(sig_a, sig_b, threshold, oracle);
    let kind = classify_clash(pred_a, pred_b, &analysis);

    let (confidence, alignment) = match &kind {
        ClashKind::VocabularyMismatch { similarity } => {
            let merged_name = analysis
                .merged_class_of(pred_a)
                .or_else(|| analysis.merged_class_of(pred_b))
                .unwrap_or(pred_a)
                .to_string();
            (
                *similarity,
                Some(ClashAlignment {
                    a_name: pred_a.to_string(),
                    b_name: pred_b.to_string(),
                    merged_name,
                    similarity: *similarity,
                }),
            )
        }
        // A same-word clash is genuine for *structural* reasons (one predicate,
        // two truth-value claims), so confidence is high and not heuristic.
        ClashKind::GenuinePredicate => (0.95, None),
        // Distinct concepts: our confidence that they're distinct is the
        // complement of how synonym-like the best gloss overlap looked.
        ClashKind::GenuineDistinctConcepts => {
            let best = best_cross_similarity(sig_a, sig_b, pred_a, pred_b);
            ((1.0 - best).clamp(0.0, 1.0), None)
        }
        ClashKind::Indeterminate => (0.0, None),
    };

    ClashVerdict {
        kind,
        confidence,
        alignment,
    }
}

/// Gloss similarity between the two named symbols (wherever each lives), used to
/// gauge how *close to* synonymy a "distinct concepts" verdict actually was.
/// Returns `0.0` if either symbol is missing.
fn best_cross_similarity(sig_a: &[Sig], sig_b: &[Sig], pred_a: &str, pred_b: &str) -> f64 {
    let find = |name: &str| -> Option<&Sig> {
        sig_a.iter().chain(sig_b.iter()).find(|s| s.name == name)
    };
    match (find(pred_a), find(pred_b)) {
        (Some(a), Some(b)) if types_compatible(a, b) => gloss_similarity(&a.gloss, &b.gloss),
        _ => 0.0,
    }
}

// ─────────────────────── pushout of theories (amalgamation) ───────────────────────

use mediator_types::{Formula, Term};

/// A name-rewriting map from party symbol names to merged canonical names,
/// derived from a [`VocabularyAnalysis`]. Applying it to a [`Formula`] is the
/// institution's sentence-translation functor `Sen(σ)` for the pushout map σ.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Renaming {
    /// `original name -> merged canonical name` (only entries that actually
    /// change are stored; missing keys map to themselves).
    pub map: std::collections::BTreeMap<String, String>,
}

impl Renaming {
    /// Build the renaming that sends every party symbol to its merged class's
    /// canonical name. Identity on symbols that are alone in their class.
    pub fn from_analysis(analysis: &VocabularyAnalysis) -> Renaming {
        let mut map = std::collections::BTreeMap::new();
        for class in &analysis.proposed_alignment.merged {
            for (_party, name) in &class.members {
                if *name != class.canonical_name {
                    map.insert(name.clone(), class.canonical_name.clone());
                }
            }
        }
        Renaming { map }
    }

    fn rename_name<'a>(&'a self, name: &'a str) -> &'a str {
        self.map.get(name).map(String::as_str).unwrap_or(name)
    }

    /// Apply the renaming to a term (recursively rewriting function/predicate
    /// heads; variables and literals are untouched).
    pub fn apply_term(&self, t: &Term) -> Term {
        match t {
            Term::Var(v) => Term::Var(v.clone()),
            Term::IntLit(n) => Term::IntLit(*n),
            Term::App(head, args) => Term::App(
                self.rename_name(head).to_string(),
                args.iter().map(|a| self.apply_term(a)).collect(),
            ),
        }
    }

    /// Apply the renaming to a formula: `Sen(σ)` over the connectives.
    pub fn apply_formula(&self, f: &Formula) -> Formula {
        rename_formula(f, self)
    }
}

/// The sentence-translation functor `Sen(σ)`: rewrite every symbol in `f` along
/// the renaming `r`, recursing through all connectives and quantifiers. Pure,
/// structural, and total.
pub fn rename_formula(f: &Formula, r: &Renaming) -> Formula {
    match f {
        Formula::Atom(t) => Formula::Atom(r.apply_term(t)),
        Formula::Eq(a, b) => Formula::Eq(r.apply_term(a), r.apply_term(b)),
        Formula::Le(a, b) => Formula::Le(r.apply_term(a), r.apply_term(b)),
        Formula::Lt(a, b) => Formula::Lt(r.apply_term(a), r.apply_term(b)),
        Formula::Not(x) => Formula::Not(Box::new(rename_formula(x, r))),
        Formula::And(xs) => Formula::And(xs.iter().map(|x| rename_formula(x, r)).collect()),
        Formula::Or(xs) => Formula::Or(xs.iter().map(|x| rename_formula(x, r)).collect()),
        Formula::Implies(a, b) => {
            Formula::Implies(Box::new(rename_formula(a, r)), Box::new(rename_formula(b, r)))
        }
        Formula::Iff(a, b) => {
            Formula::Iff(Box::new(rename_formula(a, r)), Box::new(rename_formula(b, r)))
        }
        Formula::Forall(v, s, body) => {
            Formula::Forall(v.clone(), s.clone(), Box::new(rename_formula(body, r)))
        }
        Formula::Exists(v, s, body) => {
            Formula::Exists(v.clone(), s.clone(), Box::new(rename_formula(body, r)))
        }
        Formula::Obligation(x) => Formula::Obligation(Box::new(rename_formula(x, r))),
        Formula::Permission(x) => Formula::Permission(Box::new(rename_formula(x, r))),
    }
}

/// The result of amalgamating two theories over the merged vocabulary.
///
/// **Exploratory & honestly incomplete.** `consistent_so_far == true` means *no
/// contradiction was witnessed* by the cheap syntactic check below — it is **not**
/// a consistency proof. The valuable artifact is `merged_facts`: the exact
/// sentence set, all over the canonical vocabulary, to hand to the host prover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmalgamationReport {
    /// Both parties' stipulated facts, translated into the merged vocabulary and
    /// unioned (de-duplicated). The obligation to discharge with the host.
    pub merged_facts: Vec<Formula>,
    /// `false` iff the syntactic check *witnessed* a contradiction. `true` is
    /// "nothing cheap found" — never a proof of consistency.
    pub consistent_so_far: bool,
    /// Human-readable description of any witnessed clash (empty when clean).
    pub witnessed_conflicts: Vec<String>,
    /// The renaming applied (the pushout map on sentences), for the receipt.
    pub renaming: Renaming,
}

/// Amalgamate two parties' stipulated facts over the **merged vocabulary**: apply
/// the pushout's sentence-translation to each side, union them, and run a
/// sound-but-incomplete syntactic consistency check.
///
/// This is the *pushout of theories* down-payment (see module docs). It does not
/// call the prover; `consistent_so_far` is a witness-search, not a proof.
pub fn amalgamate(
    analysis: &VocabularyAnalysis,
    stipulated_a: &[Formula],
    stipulated_b: &[Formula],
) -> AmalgamationReport {
    let renaming = Renaming::from_analysis(analysis);

    let mut merged_facts: Vec<Formula> = Vec::new();
    for f in stipulated_a.iter().chain(stipulated_b.iter()) {
        let t = rename_formula(f, &renaming);
        if !merged_facts.contains(&t) {
            merged_facts.push(t);
        }
    }

    let witnessed_conflicts = witness_contradictions(&merged_facts);
    let consistent_so_far = witnessed_conflicts.is_empty();

    AmalgamationReport {
        merged_facts,
        consistent_so_far,
        witnessed_conflicts,
        renaming,
    }
}

/// Sound-but-incomplete contradiction witness over a flat fact set, all assumed
/// to be over one vocabulary. Catches the cheap, manifest clashes:
///
/// 1. a literal `p` together with its direct negation `¬p`;
/// 2. `t = a` and `t = b` for two *distinct* integer literals `a ≠ b`;
/// 3. one modus-ponens step: `p` and `p ⟶ ¬q` with `q` also asserted (and the
///    mirror `q` and `q ⟶ ¬p` with `p`).
///
/// Anything subtler is deliberately left to the host prover; we never report a
/// clean result as "consistent," only as "no contradiction witnessed."
fn witness_contradictions(facts: &[Formula]) -> Vec<String> {
    let mut out = Vec::new();

    // Index positive atoms and the bodies of negations for O(n²) but tiny sets.
    let asserted = |g: &Formula| facts.iter().any(|f| f == g);

    for f in facts {
        // (1) p and ¬p
        if let Formula::Not(inner) = f {
            if asserted(inner) {
                out.push(format!(
                    "manifest contradiction: both `{}` and its negation are stipulated",
                    render_dbg(inner)
                ));
            }
        }
        // (3) p and (p ⟶ ¬q) with q asserted
        if let Formula::Implies(ante, cons) = f {
            if asserted(ante) {
                if let Formula::Not(q) = cons.as_ref() {
                    if asserted(q) {
                        out.push(format!(
                            "modus-ponens contradiction: `{}` and `{} ⟶ ¬{}` clash with stipulated `{}`",
                            render_dbg(ante),
                            render_dbg(ante),
                            render_dbg(q),
                            render_dbg(q)
                        ));
                    }
                }
            }
        }
    }

    // (2) t = a and t = b, a ≠ b distinct int literals.
    for (i, fi) in facts.iter().enumerate() {
        if let Formula::Eq(lhs, Term::IntLit(a)) = fi {
            for fj in facts.iter().skip(i + 1) {
                if let Formula::Eq(lhs2, Term::IntLit(b)) = fj {
                    if lhs == lhs2 && a != b {
                        out.push(format!(
                            "arithmetic contradiction: `{}` is stipulated equal to both {} and {}",
                            render_dbg(lhs),
                            a,
                            b
                        ));
                    }
                }
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Tiny deterministic term/formula pretty-printer for witness messages (debug
/// quality is fine — these are operator notes, not the trusted renderer).
fn render_dbg<T: std::fmt::Debug>(t: &T) -> String {
    format!("{t:?}")
}

// ───────────────────────────────── tests ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::{Sig, Sort};

    fn pred(name: &str, gloss: &str) -> Sig {
        Sig {
            name: name.to_string(),
            arg_sorts: vec![],
            ret: Sort::Bool,
            gloss: gloss.to_string(),
        }
    }

    fn pred1(name: &str, arg: Sort, gloss: &str) -> Sig {
        Sig {
            name: name.to_string(),
            arg_sorts: vec![arg],
            ret: Sort::Bool,
            gloss: gloss.to_string(),
        }
    }

    #[test]
    fn identical_symbols_are_shared() {
        let a = vec![pred("stain_is_damage", "the stain is damage to the carpet")];
        let b = vec![pred("stain_is_damage", "the stain is damage to the carpet")];
        let r = analyze(&a, &b);
        assert_eq!(r.shared.len(), 1);
        assert_eq!(r.shared[0].name, "stain_is_damage");
        assert!(r.shared[0].glosses_agree);
        assert!(r.synonyms.is_empty());
        // The merged class is bridged (both parties contribute).
        assert!(r.is_bridged("stain_is_damage"));
    }

    #[test]
    fn canonical_synonyms_are_flagged() {
        // The headline example from the task.
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze(&a, &b);
        assert!(
            r.synonyms
                .iter()
                .any(|(x, y, _)| x == "tenant_caused_damage" && y == "renter_at_fault"),
            "expected synonym candidate, got {:?} (notes: {:?})",
            r.synonyms,
            r.notes
        );
        assert!(r.shared.is_empty());
        // and they merge into one bridged class.
        assert!(r.is_bridged("tenant_caused_damage"));
        assert_eq!(
            r.merged_class_of("tenant_caused_damage"),
            r.merged_class_of("renter_at_fault")
        );
    }

    #[test]
    fn unrelated_glosses_are_neither() {
        let a = vec![pred("dog_barks", "the dog barks loudly every morning")];
        let b = vec![pred("invoice_paid", "the invoice was settled on time")];
        let r = analyze(&a, &b);
        assert!(r.shared.is_empty());
        assert!(
            r.synonyms.is_empty(),
            "unrelated glosses should not be synonyms, got {:?}",
            r.synonyms
        );
        // Two singleton classes in the merge, neither bridged.
        assert_eq!(r.proposed_alignment.merged.len(), 2);
        assert!(r.proposed_alignment.merged.iter().all(|m| !m.bridged));
    }

    #[test]
    fn incompatible_types_block_alias_even_with_same_name() {
        // Same word, different arity → false friend, never aligned.
        let a = vec![pred("owes", "party owes money")];
        let b = vec![pred1("owes", Sort::Int, "the amount a party owes")];
        let r = analyze(&a, &b);
        assert!(r.shared.is_empty());
        assert!(r.notes.iter().any(|n| n.contains("incompatible types")));
    }

    #[test]
    fn type_compatibility_respects_uninterp_sorts() {
        let a = pred1("a", Sort::Uninterp("Stain".into()), "g");
        let b = pred1("b", Sort::Uninterp("Stain".into()), "g");
        let c = pred1("c", Sort::Uninterp("Person".into()), "g");
        assert!(types_compatible(&a, &b));
        assert!(!types_compatible(&a, &c));
    }

    #[test]
    fn gloss_similarity_is_symmetric_and_bounded() {
        let s1 = gloss_similarity("the tenant caused the damage", "the renter is at fault for the damage");
        let s2 = gloss_similarity("the renter is at fault for the damage", "the tenant caused the damage");
        assert!((s1 - s2).abs() < 1e-12);
        assert!((0.0..=1.0).contains(&s1));
        assert!(s1 > 0.0, "shared content token 'damage' should give positive overlap");
        assert_eq!(gloss_similarity("", "anything"), 0.0);
        assert_eq!(gloss_similarity("identical words here", "identical words here"), 1.0);
    }

    #[test]
    fn classify_same_predicate_is_genuine() {
        let a = vec![pred("stain_is_damage", "the stain is damage")];
        let b = vec![pred("stain_is_damage", "the stain is damage")];
        let r = analyze(&a, &b);
        assert_eq!(
            classify_clash("stain_is_damage", "stain_is_damage", &r),
            ClashKind::GenuinePredicate
        );
    }

    #[test]
    fn classify_synonyms_is_vocabulary_mismatch() {
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze(&a, &b);
        let k = classify_clash("tenant_caused_damage", "renter_at_fault", &r);
        assert!(k.is_vocabulary_mismatch(), "got {:?}", k);
        // order independence
        assert!(classify_clash("renter_at_fault", "tenant_caused_damage", &r).is_vocabulary_mismatch());
    }

    #[test]
    fn classify_distinct_concepts_is_genuine() {
        let a = vec![
            pred("dog_barks", "the dog barks loudly"),
            pred("rent_unpaid", "the rent was not paid"),
        ];
        let b = vec![
            pred("noise_complaint", "a noise complaint was filed"),
            pred("rent_unpaid", "the rent was not paid"),
        ];
        let r = analyze(&a, &b);
        // dog_barks vs noise_complaint: distinct concepts (low gloss overlap).
        assert_eq!(
            classify_clash("dog_barks", "noise_complaint", &r),
            ClashKind::GenuineDistinctConcepts
        );
    }

    #[test]
    fn classify_unknown_predicate_is_indeterminate() {
        let a = vec![pred("x", "alpha")];
        let b = vec![pred("y", "beta")];
        let r = analyze(&a, &b);
        assert_eq!(
            classify_clash("x", "nonexistent", &r),
            ClashKind::Indeterminate
        );
    }

    #[test]
    fn pushout_merges_via_connected_components() {
        // Transitive bridging: a~b (synonym), and b shared with itself across.
        let a = vec![
            pred("tenant_caused_damage", "the tenant caused the damage"),
            pred("deposit_withheld", "the deposit was withheld"),
        ];
        let b = vec![
            pred("renter_at_fault", "the renter is at fault for the damage"),
            pred("deposit_withheld", "the deposit was withheld"),
        ];
        let r = analyze(&a, &b);
        // 4 symbols, 2 alignment classes bridged → 2 merged classes total.
        assert_eq!(r.proposed_alignment.merged.len(), 2);
        assert!(r.proposed_alignment.merged.iter().all(|m| m.bridged));
        // Bridged classes sort first.
        assert!(r.proposed_alignment.merged[0].bridged);
    }

    #[test]
    fn disjoint_vocab_note_is_emitted() {
        let a = vec![pred("apple", "a red fruit grown on trees")];
        let b = vec![pred("contract", "a binding legal agreement signed")];
        let r = analyze(&a, &b);
        assert!(r.notes.iter().any(|n| n.contains("disjoint")));
    }

    #[test]
    fn oracle_can_veto_a_candidate() {
        struct Veto;
        impl SynonymOracle for Veto {
            fn rescore(&self, _a: &Sig, _b: &Sig, _h: f64) -> f64 {
                0.0
            }
        }
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze_with(&a, &b, DEFAULT_SYNONYM_THRESHOLD, Some(&Veto));
        assert!(r.synonyms.is_empty(), "oracle veto should suppress the candidate");
    }

    #[test]
    fn serde_roundtrips_the_analysis() {
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze(&a, &b);
        let json = serde_json::to_string(&r).unwrap();
        let back: VocabularyAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    // ─────────────── ClashVerdict (the API core consumes) ───────────────

    #[test]
    fn verdict_synonym_clash_is_vocabulary_with_alignment() {
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let v = classify_clash_sigs(&a, &b, "tenant_caused_damage", "renter_at_fault");
        assert!(v.is_vocabulary(), "got {v:?}");
        assert!(!v.is_genuine());
        let al = v.alignment.expect("vocabulary clash must propose an alignment");
        // both predicates rewrite to the same canonical merged name
        assert_eq!(al.merged_name, "renter_at_fault".min("tenant_caused_damage"));
        assert!(v.confidence > 0.0 && v.confidence <= 1.0);
        // order independence of the whole verdict's kind
        let v2 = classify_clash_sigs(&a, &b, "renter_at_fault", "tenant_caused_damage");
        assert!(v2.is_vocabulary());
    }

    #[test]
    fn verdict_distinct_concepts_is_genuine_high_confidence() {
        let a = vec![pred("dog_barks", "the dog barks loudly at night")];
        let b = vec![pred("rent_overdue", "the monthly rent payment is overdue")];
        let v = classify_clash_sigs(&a, &b, "dog_barks", "rent_overdue");
        assert_eq!(v.kind, ClashKind::GenuineDistinctConcepts);
        assert!(v.is_genuine());
        assert!(v.alignment.is_none());
        // distinct concepts with ~no gloss overlap → high confidence in distinctness
        assert!(v.confidence > 0.8, "confidence was {}", v.confidence);
    }

    #[test]
    fn verdict_same_predicate_is_genuine_structural() {
        let a = vec![pred("stain_is_damage", "the stain is damage")];
        let b = vec![pred("stain_is_damage", "the stain is damage")];
        let v = classify_clash_sigs(&a, &b, "stain_is_damage", "stain_is_damage");
        assert_eq!(v.kind, ClashKind::GenuinePredicate);
        assert!(v.is_genuine());
        assert_eq!(v.confidence, 0.95);
    }

    #[test]
    fn verdict_unknown_predicate_is_indeterminate_zero_confidence() {
        let a = vec![pred("x", "alpha")];
        let b = vec![pred("y", "beta")];
        let v = classify_clash_sigs(&a, &b, "x", "ghost");
        assert_eq!(v.kind, ClashKind::Indeterminate);
        assert_eq!(v.confidence, 0.0);
        assert!(v.alignment.is_none());
    }

    #[test]
    fn verdict_oracle_veto_turns_vocabulary_into_genuine() {
        struct Veto;
        impl SynonymOracle for Veto {
            fn rescore(&self, _a: &Sig, _b: &Sig, _h: f64) -> f64 {
                0.0
            }
        }
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let v = classify_clash_sigs_with(
            &a,
            &b,
            "tenant_caused_damage",
            "renter_at_fault",
            DEFAULT_SYNONYM_THRESHOLD,
            Some(&Veto),
        );
        // with the synonym vetoed, the two are now distinct concepts
        assert_eq!(v.kind, ClashKind::GenuineDistinctConcepts);
    }

    // ─────────────── pushout of theories / amalgamation ───────────────

    fn atom(name: &str) -> Formula {
        Formula::Atom(Term::App(name.to_string(), vec![]))
    }

    #[test]
    fn rename_formula_applies_sentence_translation() {
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze(&a, &b);
        let renaming = Renaming::from_analysis(&r);
        // the canonical name is the lexicographically smaller one
        let canon = "renter_at_fault".min("tenant_caused_damage").to_string();
        let f = Formula::Implies(Box::new(atom("tenant_caused_damage")), Box::new(atom("x")));
        let rf = rename_formula(&f, &renaming);
        let expected =
            Formula::Implies(Box::new(atom(&canon)), Box::new(atom("x")));
        assert_eq!(rf, expected);
    }

    #[test]
    fn amalgamation_dissolves_synonym_clash_into_one_fact() {
        // A stipulates the synonym predicate true; B stipulates *its* synonym
        // predicate true. Over the merged vocab these are the SAME sentence, so
        // the amalgamation is a single fact, no contradiction.
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze(&a, &b);
        let rep = amalgamate(&r, &[atom("tenant_caused_damage")], &[atom("renter_at_fault")]);
        assert_eq!(rep.merged_facts.len(), 1, "synonyms collapse to one fact");
        assert!(rep.consistent_so_far);
        assert!(rep.witnessed_conflicts.is_empty());
    }

    #[test]
    fn amalgamation_witnesses_p_and_not_p_after_alignment() {
        // A asserts `tenant_caused_damage`; B asserts ¬`renter_at_fault`. Aligned,
        // these become p and ¬p — a manifest contradiction the cheap check finds.
        let a = vec![pred("tenant_caused_damage", "the tenant caused the damage")];
        let b = vec![pred("renter_at_fault", "the renter is at fault for the damage")];
        let r = analyze(&a, &b);
        let rep = amalgamate(
            &r,
            &[atom("tenant_caused_damage")],
            &[Formula::Not(Box::new(atom("renter_at_fault")))],
        );
        assert!(!rep.consistent_so_far, "should witness p ∧ ¬p, got {rep:?}");
        assert!(rep.witnessed_conflicts.iter().any(|c| c.contains("contradiction")));
    }

    #[test]
    fn amalgamation_witnesses_distinct_int_literals() {
        // No alignment needed: refund_due = 75000 and refund_due = 105000.
        let r = analyze(&[] as &[Sig], &[] as &[Sig]);
        let lhs = Term::App("refund_due".into(), vec![]);
        let rep = amalgamate(
            &r,
            &[Formula::Eq(lhs.clone(), Term::IntLit(75000))],
            &[Formula::Eq(lhs, Term::IntLit(105000))],
        );
        assert!(!rep.consistent_so_far);
        assert!(rep.witnessed_conflicts.iter().any(|c| c.contains("arithmetic")));
    }

    #[test]
    fn amalgamation_clean_is_not_a_consistency_proof_but_collects_facts() {
        // Two unrelated, satisfiable facts: clean witness search, both kept.
        let r = analyze(&[] as &[Sig], &[] as &[Sig]);
        let rep = amalgamate(&r, &[atom("rent_paid")], &[atom("keys_returned")]);
        assert!(rep.consistent_so_far);
        assert_eq!(rep.merged_facts.len(), 2);
    }

    #[test]
    fn amalgamation_report_serde_roundtrips() {
        let r = analyze(&[] as &[Sig], &[] as &[Sig]);
        let rep = amalgamate(&r, &[atom("rent_paid")], &[atom("keys_returned")]);
        let json = serde_json::to_string(&rep).unwrap();
        let back: AmalgamationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(rep, back);
    }
}
