# Mediateor ☄️ — 60-second guided demo

*A trusted mediator. It doesn't judge — it makes the shape of a disagreement
legible, and keeps everyone honest about the few things that must not be fudged.*

---

## The one-liner

```sh
./run.sh
```

That's it. The script builds the workspace, runs the Isabelle/HOL pipeline on
the roommate scenario (30–60 s, first run only), and opens the web interface
at **http://127.0.0.1:3000**.

---

## What you'll see in the terminal

```
☄  Roommate security-deposit dispute — Robin moves out
   analyzing — driving Isabelle/HOL, a few seconds…

  ✓ cache fresh: roommate.analysis.json
☄  starting mediator-web on http://127.0.0.1:3000
   operator cockpit → http://127.0.0.1:3000/operator
   Robin's view     → http://127.0.0.1:3000/party/robin
   Sam's view       → http://127.0.0.1:3000/party/sam
```

The browser opens automatically on macOS.

---

## The scenario: Robin moves out, Sam holds the deposit

Robin and Sam shared a flat. Robin is leaving; Sam holds the $1,200 deposit.
They're fighting over a carpet stain and some shared furniture. They hate each
other — but not *that* badly.

---

## Three things to show (and why each is impressive)

### 1. The ledger that cannot lie

Sam told Robin the deductions would "come to about $500." The system opens with:

> **Sam's claimed total of $500.00 is REFUTED.**
> Itemized total is $450.00 (cleaning $150 + carpet repair $300).

This is not anyone's opinion. It is a theorem discharged by Isabelle/HOL and
written to an append-only, hash-chained receipt ledger. Neither party can argue
with it; it is certified arithmetic. The refund floor — assuming the stain is
just wear — is **$1,050**.

*Why it matters:* the "lie" was probably a rough memory, not bad faith. Surfacing
it as a number to correct (with a receipt) dissolves a grievance that was never
really a disagreement.

### 2. The crux — and the honest stop

The system reduces the entire money question to a single predicate:

> **stain_is_damage** — is the carpet stain chargeable damage,
> or ordinary wear and tear?

The prover proves this reduction is complete and then **stops**. It does not
decide the predicate. It hands it back to the humans, cleanly labelled.

*Why it matters:* this is the refusal that earns trust. A system that pretended
to answer the human question would be lying. The kernel says: *I have cleared
everything I can certify. This one thing is yours. Here is exactly what it costs
each of you.*

### 3. Fair settlement options

While the crux stays human, the furniture question is fully mechanical.
Adjusted Winner (Brams–Taylor) produces envy-free, equitable splits:

| World | Cash refund | Couch | Standing desk | Kitchenware | Bookshelf |
|---|---|---|---|---|---|
| Wear (stain = ordinary) | $1,050 to Robin | Robin | Robin | Sam | Sam |
| Damage (stain = Robin's) | $750 to Robin | Robin | Robin | Sam | Sam |

Both outcomes are certified envy-free and Pareto-optimal. Robin and Sam can
**accept, reject, or counter** — but they're doing it on a level table, not a
tilted one.

---

## The web pages

| Page | What it shows |
|---|---|
| `/` | Landing — the dispute in plain language |
| `/party/robin` | Robin's view: what's settled, what's still open, fair options |
| `/party/sam` | Sam's view: the same, from Sam's side |
| `/operator` | Full cockpit: prover verdicts, receipt chain, every formula |

Robin and Sam never see a formula. The operator sees everything.

---

## CLI alternative (no browser)

```sh
just demo          # full CLI output: party views + operator cockpit + receipt chain
just tui           # interactive terminal app (arrow keys to navigate)
```

---

## Re-running is instant

The pipeline caches its output beside the scenario file
(`scenarios/roommate.analysis.json`). On re-run, `run.sh` skips any cache
that is already newer than its scenario — so the web server starts in seconds.
To force a fresh Isabelle run, delete the cache file first.

---

## The honest part

The kernel does **not** solve the dispute. It subtracts everything that was
only *masquerading* as the dispute — the bad arithmetic, the vocabulary
mismatch, the things the prover can certify — so the real, smaller knot is
visible. The crux coming back `Unknown` is not a failure. It is the point.

> *the prover's kindest move is knowing where to stop —*
> *it clears the ledger, then it lets the rest be human.* ☄️
