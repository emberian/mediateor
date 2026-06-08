# we hate each other but not that badly ☄️

live demo online: <https://mediateor.fg-goose.online>. 

```
if you appreciate the demo please consider venmo @ember_arlynx
for the token fund (i dropped in $50 of seed funding myself..)
```

*A peace technology. An impartial, patient, **disinterested** guide that helps two
people resolve a disagreement they'd both rather not let escalate — and is
trustworthy enough to use with no one in the robe, because the few things that
must not be fudged are checked by a machine that has no stake in the outcome.*

> two worlds, each whole, each sure it sees the whole —
> we don't crown a winner of the two;
> we build the smallest world that holds them both, and true.

Codename **Mediateor** (a meteor ☄️ of a mediator). Built by ember (proof
engineer, trained peer mediator) and Claude, with pug giving it a face; seeded by
[world-model-trajectories](https://emberian.github.io/world-model-trajectories/).
Live and open to all at **mediateor.fg-goose.online**.

---

## Why this matters more than it looks like it should

Justice is the oldest technology for living together despite disagreement — how a
group metabolizes conflict without tearing itself apart. And every fair *process*
humans have built — courts, law, arbitration, mediation — has failed at the same
two points in every society that built it: it is **scarce** (it runs on expensive,
tired, trained humans, so most conflict never reaches it and the more powerful
party wins by default) and it is **capturable** (the humans in the robes have
moods and mortgages and biases). "Equal justice" has never once been kept, because
fair process has never been abundant or incorruptible.

For the first time there's a path to a process that is **abundant** (a machine
doesn't tire or bill by the hour — it can be there for the carpet stain *and* the
estate, at 3am, for free), **structurally disinterested** (no stake in your
outcome), **transparent and auditable** (its reasoning is a signed record you can
check, not a deliberation behind a bench), **consistent** (no hungry-judge effect),
and — the part that turns it from frightening to trustworthy — **humble** (it knows
what it cannot decide, and hands those questions *back*).

The same parts, assembled with three different commitments, build the most
efficient injustice machine ever made. So the design choices that look small are
the whole thing — they are the **constitution** of it (see below). We mean to build
the first one, and the toy scale is exactly how you build a peace technology
without lying about it.

## The one honest idea (and a trained peer mediator already knew it)

Most of a dispute is **not formalizable** — values that don't share a scale,
recognition, the fight under the fight ("you owe me $400" is usually "you checked
out on me for months"). A theorem prover can't touch that, and we don't pretend it
can. The graveyard of this field is full of systems that tried to formalize
*everything* and died.

What a dispute *is*, instead, is the thing a trained peer mediator runs: make it
safe and voluntary; give each person **uninterrupted time** to be heard fully;
reflect back what you heard; surface the interest under the position; find the
common ground (it's bigger than the fight makes it feel); generate options *without
judgment*; reach an agreement that's specific, mutual, and **owned by both**. The
mediator never decides — they *facilitate*; the parties decide. **That basic human
process is the product.** The AI does it, with infinite patience and no stake.

So why any formalism at all? Because the reason you'd never trust an unsupervised
AI with your dispute is that it could hallucinate the numbers, paper over a real
contradiction, or be steered by whoever wrote the more sympathetic prompt. **The
formal core kills exactly those failure modes and nothing else** — the ledger can't
be faked, a "resolution" can't be internally incoherent, "you actually agree about
X" is *certified* rather than vibed, and every load-bearing claim leaves a receipt.
That thin, boring guarantee is **the trust substitute for the professional who
isn't in the room** — which is the only way this works at grassroots scale. The
prover refusing to decide the crux is just the machine learning the humility a
16-year-old in a peer-mediation vest already has.

The rule: **model proposes, prover disposes.** The LLMs (a council of diverse
models) only ever *propose* solver-checkable artifacts; nothing is load-bearing
until [Isabelle/HOL](https://isabelle.in.tum.de/) certifies it. The formal core is
maybe a fifth of the system — and that is correct, not a disappointment.

## What it actually does

Talk to it in plain words — *"Maria lent her cousin $2,000 for a car; he says
$800 was a gift, she says it was all a loan…"* — and it:

- **builds the case from what you said** (accretion): a model drafts the structure,
  the prover certifies the numbers and isolates the contested question;
- **conducts a real mediation** (live, on the box): private caucuses where it
  reflects back the interest under each position, then reflects the common ground,
  then names the one genuine knot and **hands it back undecided**, then offers
  certified-fair settlement options to accept, reject, or counter;
- **takes evidence** — each person's account, claimed facts, exhibits — and *weighs
  and acknowledges* it so they feel heard, but **never lets it decide the crux**:
  evidence informs the humans; it does not let the machine rule;
- **leaves a signed, tamper-evident record** anyone can re-verify — every certified
  fact and every binding step, hash-chained and ed25519-signed.

Nobody in the dispute ever sees a formula. They see: *here's what you already agree
on (more than you thought); here's the one real question, which is yours; here are
fair options.* The verdict is never "you're wrong." It's *here is the smallest world
that holds you both.*

## Forward constitutions (prevention, not just cure)

The same humility runs the other direction in time. Most disputes were *foreseeable* —
the roommates knew a deposit fight could come; the co-founders knew a departure might be
contested. So instead of waiting for the breakup, **certify the agreement before anyone's
angry.** A *pact* names its own question-space up front — the value-laden questions the
two of them agree to leave open, and the measured dials they agree on — plus a set of
if-this-then-that clauses. The prover then certifies, over **every world the pact named**,
that it is **complete** (every situation fires some clause — no gap it's silent on) and
**non-contradictory** (no situation fires two clauses that disagree), while the human
value-predicates stay **uninterpreted** — still handed back `Unknown` at dispute time.
*Don't mediate the breakup; prove the relationship resolves every breakup it named.*

It's a real trichotomy, and it refuses to overclaim:

- **`Certified`** — "this agreement handles every situation you named, and no two rules collide."
- **`Inconsistent`** — *here is exactly when two rules collide*: a concrete world (these
  facts, this dial value) where two clauses demand different amounts.
- **`Refused`** — *here is a situation you left unhandled*: the open subgoal **is** the
  diagnosis, in plain words ("a cliff value falls through the clauses").

The ceiling, stated everywhere it shows: complete and consistent **relative to the
questions the pact named** — never that it named every question. A predicate nobody thought
to name can't be detected; that residue is the faithfulness seam, pushed to design time,
and it stays the humans'.

And because pacts of one *template* share a declared vocabulary, they **collide by
construction** — so a **commons of pacts** carries signal where a commons of ad-hoc disputes
(whose cruxes never collide) cannot. It reports *certified controversy* — "here are the ways
people resolved this clause, each signed and re-verifiable" — never "yours should be X," and
publishes its corpus size as the headline (a handful of signed choices is *not* a settled
rule, and it says so). Live at **[/pacts](https://mediateor.fg-goose.online/pacts)** (the
shipped templates: roommate move-out, founder vesting, creative-credit, cohabitation).

## The constitution (the line between the two futures)

These are not features. They are the difference between a peace technology and
automated domination in a velvet glove:

- **Voluntary, never coercive**, with an **exit and a human always one button
  away.** The instant it's the *only* door, it stops being justice.
- **It refuses the questions of value.** The crux comes back `Unknown` *on purpose* —
  the genuinely human/moral question is handed to the humans, every time.
- **Auditable by anyone.** Trust comes from the *verifiable record*, not the
  arbiter's virtue. (`quis custodiet ipsos custodes`, answered structurally: the
  judge can't lie about the facts, can't hide its reasoning, can't overstep.)
- **No proprietary power enthroned.** The council is now *fully open-weights* — the
  mediator's voice is **Qwen3-VL 235B**, the deliberating panel is **Mistral Large 3 +
  DeepSeek V3.2 + Qwen**, and the neutrality judge is independent of the voice — spanning
  makers so no single one's bias is the law. Diverse minds, none of them the throne.

## Architecture

The cathedral is **backstage** — only the AI walks its halls; the people receive a
plain, kind conversation.

```
   UNTRUSTED  (proposes)                 TRUSTED  (disposes)
   ──────────────────────                ───────────────────
   mediator-llm   ── proposes IR ──►  mediator-core ── .thy ──►  mediator-prover
   (a council of open models,         (the reduction, the          (Isabelle/HOL —
    Bedrock; a neutrality judge        cruxes, the receipts)         the only authority)
    independent of the voice)   ◄── verdict ──  ◄────────────────  Proved / Unknown / Error
                                             │
              mediator-session  ◄────────────┤   the AI actually mediating (the point)
              mediator-fairdiv  ◄────────────┤   certified-fair options (3 procedures)
              mediator-audit    ◄────────────┘   signed, verifiable record of it all

   · · · · · · · · · · · · · · · · · · · · ·   (exploratory sidecar — NOT load-bearing)
     mediator-ontology   synonym-merge / well-typed aliasing. Honest: no dispute we've
                         put through it yet actually NEEDS the pushout — a plain heuristic
                         would do. It's here to find the limit of the categorical idea,
                         not because anything load-bearing depends on it.
```

| crate | what it is |
|---|---|
| `mediator-types` | the frozen contract — pure data + trait seams |
| `mediator-core` | the reduction: `Dispute` → Isabelle; isolates the *set* of cruxes; the receipt ledger |
| `mediator-prover` | the trusted gate: drives `isabelle`, returns per-obligation `Proved`/`Unknown`/`Error` |
| `mediator-fairdiv` | three certified procedures — Adjusted Winner, max-min egalitarian, split-the-difference — with envy-free / proportional / Pareto certificates |
| `mediator-llm` | the untrusted council (diverse + moving to fully-open) + an independent neutrality judge; offline scripted fallback |
| `mediator-session` | **the AI actually mediating** — the session, evidence, the adaptive (non-rigid) flow, and the mediator's voice |
| `mediator-audit` | a signed, tamper-evident `MediationRecord` (ed25519 over a sha256 chain) anyone can `verify()` |
| `mediator-pact` | **forward constitutions**: certify a two-party agreement *complete + non-contradictory* over its declared question-space (value-predicates left free), else refuse it with the concrete gap/clash; signed cert |
| `mediator-pact-commons` | a *commons of pacts* — same-template pacts collide by construction, so it reports certified *controversy* (signed, re-verifiable), the corpus size as the headline |
| `mediator-ontology` | vocabulary negotiation — dissolves "disagreements" that are two words for one thing (the pushout of the alignment span; *honestly labeled exploratory*) |
| `mediator-tui` / `mediator-web` | two kind front doors over one analysis — a terminal app and a warm htmx page |
| `isabelle/` | `Keystone.thy` (the proven foundation stone), `Pact.thy` (the forward-constitution proof shape) + `lib/` (the deontic + defeasible layer, incl. dyadic contrary-to-duty) |

It's **HOL-shaped, not Hets-shaped**: one host (Isabelle/HOL) with the few logics we
need shallow-embedded ([LogiKEy](https://www.sciencedirect.com/science/article/pii/S0004370219301110)/Benzmüller
style) — the *alive* branch of this field.

## Run it

Prerequisites: a Rust toolchain and Isabelle2025-2 (auto-located at
`~/isabelle/Isabelle2025-2.app`).

```sh
./run.sh                       # build → cache → serve → open the browser
cargo run -p mediator-session --bin intake -- "describe a real dispute in plain words"
cargo run -p mediator-pact --bin pact -- scenarios/pacts/roommate_moveout.json   # certify a pact
cargo run -p mediator-pact-commons                                               # the commons of pacts
just demo | just tui | just web | just verify
```

Everything runs **offline and deterministically** by default — the live models
fall back to a scripted voice so the demo never *needs* a network or a key. The
public box drives live open models on Bedrock, behind per-request rate limits and a
hard **$50 spend cap** (an AWS budget action cuts model access cold at the line).

## Disputes

Five worked disputes ship in `scenarios/`, each certified through the real Isabelle
gate, plus `twocrux.json` (a dispute reducing to *two* contested questions at once).

| Scenario | The story | The open question(s) it's handed back |
|---|---|---|
| `roommate` | Robin moves out; Sam holds the $1,200 deposit; a carpet stain. | Is the stain chargeable damage, or ordinary wear? |
| `freelance` | A $5,000 website: missing features, or delivered to spec? | Did the work meet the agreed spec? |
| `partnership` | Co-founders wind down; an equity cliff and a contested departure date. | Did the departure breach the vesting agreement? |
| `separation` | A shared-apartment deposit; wall scuffs and a cracked tile. | Is the damage attributable to one partner, or wear? |
| `siblings` | Two siblings divide an estate; an $8,000 gift years ago. | Was that money an advance, or a gift? |

## Why this shape (the honest part)

The faithfulness of any formalization is a **seam**, and the only honest move is to
make it cheap and visible, not to hide it.

- The kernel guarantees consistency *of the formalization* and the certified
  ledger/division — **never** that it captured what you meant, never that anyone is
  morally right. That stays yours.
- The prover may answer `Unknown`. It is reported as `Unknown`, never silently as
  consistent. The crux comes back `Unknown` *on purpose*.
- Where a dispute resists formalization, the system **says so**. The boundary is
  itself a diagnosis: a fight that won't formalize is usually about values or
  recognition, and naming that beats faking an answer.
- The **categorical / ontology** layer (mediation as the pushout of two
  vocabularies) is genuinely exploratory — it earns its keep for transitive synonym
  merges and well-typed aliasing, and is honest in its own docs about where a plain
  heuristic would do as well. We're pushing it to find its limit, not pretending
  we've found a theorem.

No overclaim.

## Lineage & status

Standing on: AGM belief revision, Dung argumentation, defeasible deontic logic
(Governatori), default-logic-for-statutes ([Catala](https://arxiv.org/abs/2103.03198)),
shallow semantic embeddings in HOL ([LogiKEy](https://www.sciencedirect.com/science/article/pii/S0004370219301110)),
fair division (Brams–Taylor), and "model proposes, prover disposes" from the
LLM-theorem-proving literature (DSP, Baldur, COPRA). The contribution is the honest
*composition*, and the peer-mediation discipline. (Design in `ARCHITECTURE.md`;
reading in `pdfs/`.)

**Status:** real and live. Free-text disputes get certified through Isabelle; the
AI conducts interactive, live mediations end to end; **forward constitutions certify an
agreement complete + non-contradictory before any dispute** (the live
[/pacts](https://mediateor.fg-goose.online/pacts) gallery); the record is signed and
verifiable; the council is **fully open**. Young, honest about its edges, and — if the
frame above is even half right — worth building carefully and in the open.

Sharing this with the cyborgists, who will see further than two of us can. If
you're reading it there: the seams are marked, the skepticism is invited, and the
constitution is the part that matters. Pull on any of it.

## License

MIT OR Apache-2.0.

---

*( the prover's kindest move is knowing where to stop —
it clears the ledger, then it lets the rest be human. )* ☄️
