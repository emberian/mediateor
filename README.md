# we hate each other but not that badly ☄️

*A trusted mediator. It doesn't judge — it makes the **shape** of a disagreement
legible to the people inside it, and keeps everyone honest about the few things
that must not be fudged.*

> two worlds, each whole, each sure it sees the whole —
> we don't crown a winner of the two;
> we build the smallest world that holds them both, and true.

Codename **Mediateor** (a meteor ☄️ of a mediator). Built by ember (proof
engineer / architect) and Claude, for pug to give a face;
seeded by [world-model-trajectories](https://emberian.github.io/world-model-trajectories/).

---

## The one honest idea

Most of a dispute is **not formalizable** — values that don't share a scale,
recognition, the fight under the fight ("you owe me $400" is usually "you checked
out on me for months"). That part stays the work of the LLM mediator and the
humans. We don't pretend a theorem prover can resolve it. The graveyard of this
field is full of systems that tried to formalize *everything* and died; the
survivors formalize a thin slice. **We formalize the thin slice and refuse the
rest, visibly.**

What the formal core *does* earn its keep on is narrow and real:

- **the ledger** — a calculator that cannot be lied to (not by a motivated
  roommate, not by a hallucinating model);
- **consistency** — whether two claims actually clash, or just use different words;
- **the crux** — collapsing a whole tangled fight onto the *one* contested
  question, and handing exactly that back to the humans;
- **fair division** — provably envy-free, equitable settlements over divisible stakes.

The core's real gift is **subtraction**: clear the parts only *masquerading* as
the dispute, so the real, smaller knot is visible and out in the open.

And the rule that makes it trustworthy: **model proposes, prover disposes.** The
LLM (and the council of diverse models) only ever *proposes* solver-checkable
artifacts; nothing is load-bearing until [Isabelle/HOL](https://isabelle.in.tum.de/)
certifies it. Sympathetic framing can't reach the verdict, because the council
evaluates the *normalized formal* form, never the prose. Every certified fact is
written to an append-only, hash-chained receipt ledger.

## A walk through the demo

`scenarios/roommate.json` — Robin is moving out; Sam holds the $1,200 deposit.
They're fighting over a carpet stain and some shared furniture, and they hate each
other, but not *that* badly. Mediateor:

1. **Certifies the ledger.** Itemized deductions are cleaning ($150) + carpet
   ($300) = **$450**. Sam's verbal claim of "$500" is *refuted by the host* — not
   by anyone's say-so. Refund is **$1,050** if the stain is ordinary wear,
   **$750** if it's chargeable damage.
2. **Isolates the crux.** From the lease, `tenant_owes_carpet ⟷ stain_is_damage`.
   The prover proves the entire carpet question reduces to one predicate — *is the
   stain damage or wear?* — and then **stops**. That's the human question. It is
   not the kernel's to decide, and the kernel says so.
3. **Dissolves a misunderstanding.** The "$500 vs $450" was a rough memory, not a
   lie — surfaced kindly, with a receipt, as a number to correct.
4. **Offers fair settlements.** Adjusted Winner splits the shared belongings into
   an envy-free, equitable allocation to accept, reject, or counter.

Robin and Sam never see a single formula. They see: *here's what you already
agree on (it's more than you think), here's the one real knot, and here are fair
options.* The verdict is never "you're wrong." It's *here is the smallest world
that holds you both.*

## Architecture

The cathedral is **backstage**. Only the LLM walks its halls; the people in the
dispute receive the plain-language mass.

```
   UNTRUSTED                          TRUSTED
   ─────────                          ───────
   mediator-llm  ── proposes ──►   mediator-core ── generates .thy ──►  mediator-prover
   (Bedrock,                       (the reduction,                      (Isabelle/HOL —
    LM Studio,                      receipts, analysis)                  the only authority)
    council)     ◄── verdict ───   ◄──────────────────────────────────  Proved / Unknown / Error
                                          │
                                          ├──►  mediator-fairdiv   (Adjusted Winner + certificates)
                                          └──►  mediator-tui / -web (operator cockpit · kind party view)
```

It's **HOL-shaped, not Hets-shaped**: one host (Isabelle/HOL) with the few logics
we need shallow-embedded ([LogiKEy](https://www.sciencedirect.com/science/article/pii/S0004370219301110)/Benzmüller
style) — the *alive* branch of this field — rather than many provers glued by
morphisms (the dead branch).

| crate | what it is |
|---|---|
| `mediator-types` | the frozen contract — pure data + trait seams |
| `mediator-core` | the brain: Formula→Isabelle codegen, the reduction, the hash-chained receipt ledger |
| `mediator-prover` | the trusted gate: drives `isabelle`, returns per-obligation `Proved`/`Unknown`/`Error` |
| `mediator-fairdiv` | Brams–Taylor Adjusted Winner + envy-free / equitable / Pareto certificates |
| `mediator-llm` | the untrusted operator + council (AWS Bedrock, LM Studio); degrades to a scripted operator offline |
| `mediator-tui` | two projections of one analysis, in the terminal (ratatui) |
| `mediator-web` | the same two faces as a simple, delightful htmx page (axum + maud) |
| `mediator-demo` | the `mediator` binary — wires it all together on a scenario |
| `isabelle/` | `Keystone.thy` (the proven foundation stone) + `lib/` (the deontic + defeasible normative layer) |

Two front doors, both *kind*: a terminal TUI and a minimalist web page. Same
underlying analysis, two altitudes — the **party view** (warm, pared down, never a
morphism, never a verdict-as-judgment) and the **operator cockpit** (the full
graph, prover verdicts, and receipt chain, for the human supervisor + engineer
flying as a pair).

## Run it

Prerequisites: a Rust toolchain and Isabelle2025-2 (the prover path is
auto-located; override with `MEDIATEOR_ISABELLE`/`IsabelleProver::new`).

```sh
# the foundation stone — the thesis, machine-checked
isabelle build -D isabelle               # Keystone + the deontic library

# the kernel and its pieces
cargo test                               # per-crate tests (prover hits real Isabelle)

# the demo, end to end on the roommate dispute
cargo run -p mediator-demo -- scenarios/roommate.json     # CLI + TUI
cargo run -p mediator-web                                 # then open http://127.0.0.1:3000
```

Everything runs **offline and deterministically** by default — the LLM operator
falls back to a scripted formalizer so the demo never depends on a network or a
key. Point it at Bedrock / LM Studio to let live models drive the kernel.

## Why this shape (the honest part)

In the lineage of [WMT](https://emberian.github.io/world-model-trajectories/):
the faithfulness of any formalization is a **seam**, and the only honest move is
to make it cheap and visible, not to hide it.

- The kernel guarantees consistency *of the formalization* and the certified
  ledger/division — **never** that it captured what you meant, and never that
  anyone is morally right. That stays yours.
- The prover may answer `Unknown` on hard goals. It is reported as `Unknown` —
  **never** silently treated as consistent. (The crux comes back `Unknown` *on
  purpose*: refusing to decide it is the honest answer.)
- Where a dispute resists formalization, the system **says so** and routes around
  it. The boundary of what can be formalized is itself a diagnosis: a fight that
  won't formalize is usually about values or recognition, not facts — and naming
  that is more useful than faking an answer.

No overclaim. The formal core is a supporting actor — maybe a fifth of the system
— and that is correct, not a disappointment.

## Lineage

Standing on: AGM belief revision (Alchourrón–Gärdenfors–Makinson), Dung
argumentation, defeasible deontic logic (Governatori), default-logic-for-statutes
([Catala](https://arxiv.org/abs/2103.03198)), shallow semantic embeddings in HOL
([LogiKEy](https://www.sciencedirect.com/science/article/pii/S0004370219301110)),
fair division (Brams–Taylor Adjusted Winner, Family\_Winner), and the
"model proposes, prover disposes" pattern from the LLM-theorem-proving literature
(DSP, Baldur, COPRA). The contribution is the honest *composition*, and the seam
made cheap and visible. (Reading in `pdfs/`; design in `ARCHITECTURE.md`.)

## Status

Early, but real. The keystone builds; the components are implemented and tested;
end-to-end integration of the `mediator` binary and the web page is being wired.
Expect rough edges — this is the CLI/TUI kernel that proves the machinery before
pug gives it a proper face.

## License

MIT OR Apache-2.0.

---

*( the prover's kindest move is knowing where to stop —
it clears the ledger, then it lets the rest be human. )* ☄️
