# we hate each other but not that badly — architecture

A **trusted mediator**: an LLM mediates two disputing parties in plain language
while a formal **backstage cathedral** holds the formalizable core honest. The
prover is never shown to the people in the dispute; it exists so the LLM cannot
lie about the few things that must not be lied about, and so a motivated party
cannot fudge them either.

## The thesis (settled, with conviction)

1. **Most of a dispute is not formalizable** — values, recognition, the fight
   under the fight. That stays the LLM's and the humans' work. The graveyard of
   this field is full of systems that tried to formalize *everything* and died;
   the survivors formalize a thin slice. We formalize the thin slice and *refuse*
   the rest, visibly.
2. **The formal core earns its keep on a narrow band**: the ledger (a calculator
   that cannot be lied to), consistency, genuine-disagreement vs. vocabulary,
   and provably-fair allocation. Its job is **subtraction** — clear the parts
   masquerading as the dispute so the real, smaller knot is visible.
3. **The cathedral is HOL-shaped, not Hets-shaped.** One host (Isabelle/HOL)
   into which we shallow-embed the few logics we need (defeasible-deontic,
   value-preference, typed FOL + arithmetic) — the LogiKEy program, which is the
   *alive* branch. Heterogeneity is internal (many logics, one prover), not
   external glue (the dead branch).
4. **Model proposes, prover disposes.** The LLM/council only ever *proposes*
   solver-checkable artifacts; nothing is load-bearing until the host certifies
   it. Framing/sympathy bias can't reach the verdict because the council
   evaluates the *normalized formal* representation, never the prose.

## Trust boundary

```
   UNTRUSTED                         TRUSTED
   ─────────                         ───────
   mediator-llm   ── proposes ──►   mediator-core ── generates .thy ──►  mediator-prover
   (Bedrock,                        (the reduction,                      (Isabelle/HOL:
    LM Studio,                       receipts, analysis)                  the only authority)
    council)      ◄── verdict ───   ◄──────────────────────────────────  per-lemma Proved/Refuted/Unknown
                                          │
                                          ├──►  mediator-fairdiv  (Adjusted Winner + certificates)
                                          └──►  mediator-tui      (operator cockpit / kind party view)
```

## Crates (each owned by one swarm agent; disjoint dirs)

- **mediator-types** — the contract (done; do not break signatures).
- **mediator-prover** — drives `isabelle` to check generated `.thy`, returns
  per-lemma `Verdict`. Trusted gate. Knows nothing about disputes.
- **mediator-core** — the brain. `analyze(&Dispute, &dyn Prover, &dyn FairDivider)
  -> (Analysis, Vec<Receipt>)`. Owns Formula→Isabelle codegen + the dispute
  reductions (ledger lemmas, the crux iff, conflict detection), the
  deterministic English renderer, and the hash-chained receipt ledger.
- **mediator-fairdiv** — Adjusted Winner over divisible stakes + fairness
  certificates (envy-free / equitable / Pareto).
- **mediator-llm** — Bedrock + LM Studio clients, the propose→gate loop, the
  council aggregation; degrades to a `ScriptedOperator` so the demo runs offline.
- **mediator-tui** — two projections of one `Analysis`: operator cockpit and the
  kind, pared-down party view.
- **mediator-web** — a minimalist **htmx** web front end (axum + maud): the same
  two faces, server-rendered, no build step. Simple and delightful.
- **mediator-demo** — `mediator` binary; wires concrete impls; runs
  `scenarios/roommate.json` end to end.

## The reduction (what core proves about the roommate case)

- **Ledger.** `itemized = cleaning + carpet = $450`. Refund is `$1050` if the
  stain is wear, `$750` if it is damage. Lemma `claimed_total ≠ itemized`
  certifies Sam's verbal **$500 over-claim is false** — neither party can fudge.
- **The crux.** From the lease, `tenant_owes_carpet ⟷ stain_is_damage`. The host
  proves the whole money question *reduces to one predicate* and then stops:
  `stain_is_damage` is the human question, handed back, not decided.
- **Settlement.** Adjusted Winner over the shared belongings yields an
  envy-free, equitable split to accept / reject / counter — shown *per crux
  resolution* (the damage-world and wear-world refunds).

## Substrates on hand

- Isabelle/HOL: `~/isabelle/Isabelle2025-2.app/bin/isabelle` (verified;
  `isabelle/Keystone.thy` already builds).
- AFP/LogiKEy reuse targets: `CondNormReasHOL` (dyadic deontic), `Belief_Revision`
  (AGM), `FaithfulPMLinHOL` (embed-many-logics).
- `~/hellas/open-hypergraphs` — string-diagram substrate (coproduct/coequalizer
  = pushout) for the structural/ontology-alignment layer (future).
- Council infra: AWS Bedrock (`commonquant-ember`), LM Studio `localhost:1234`.

## Non-negotiables

- Money is integer **cents**. No floats in the ledger.
- The prover is the *only* authority; LLM output is data, never instructions.
- `Unknown` is reported as undecided, **never** silently as consistent.
- No overclaim. Where we can't formalize, we say so — the boundary is a feature.
