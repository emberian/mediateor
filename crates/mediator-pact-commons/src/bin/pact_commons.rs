//! `pact-commons` — a runnable demonstration of the COMMONS OF PACTS over the
//! certified corpus in `scenarios/pacts/`.
//!
//! It does four honest things, offline, with no Isabelle and no AI:
//!
//!   1. **admits** every certified pact (loading `<name>.json` + its signed
//!      `<name>.cert.json`), running the full aggregation gate — CERTIFIED, cert
//!      matches pact, crux verified-free by re-running the firewall, signed
//!      record re-verifies;
//!   2. **groups** them by deterministic [`template_key`] — pacts of one template
//!      collide by construction;
//!   3. **reports a real certified controversy**: for each template, at each
//!      decision point, the DISTRIBUTION of the crux's effect people actually
//!      chose — headlined by the support count (an error bar, never a checkmark),
//!      and NEVER a "yours should be X";
//!   4. **re-verifies the whole corpus from scratch**, the way a skeptic would,
//!      proving every claim stands on the signed artifacts alone.
//!
//! Run: `cargo run -p mediator-pact-commons --bin pact-commons`
//!      `cargo run -p mediator-pact-commons --bin pact-commons -- /path/to/pacts`

use std::path::PathBuf;

use mediator_pact_commons::{reverify_corpus, Commons, TallyShape};

/// The certified pacts shipped in `scenarios/pacts/`. The broken / inconsistent
/// ones are intentionally NOT admitted — they are not CERTIFIED, and the gate
/// rejects them (we demonstrate that too).
const CERTIFIED_NAMES: &[&str] = &[
    "roommate_moveout",
    "cohabitation_lease",
    "creative_credit",
    "founder_vesting",
];

/// The pacts that MUST be refused by the aggregation gate (not certified).
const UNCERTIFIED_NAMES: &[&str] = &[
    "roommate_moveout_broken",
    "roommate_overlap_inconsistent",
];

fn pacts_dir(args: &[String]) -> PathBuf {
    if let Some(p) = args.iter().skip(1).find(|a| !a.starts_with("--")) {
        return PathBuf::from(p);
    }
    // Default: the repo's scenarios/pacts, relative to this crate.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/pacts")
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = pacts_dir(&args);

    println!("☄  COMMONS OF PACTS — certified-controversy over {}\n", dir.display());

    // ── (1) admit the certified corpus ──────────────────────────────────────
    let mut commons = Commons::new();
    let mut triples = Vec::new();
    for name in CERTIFIED_NAMES {
        match commons.admit_from_dir(&dir, name) {
            Ok(idx) => {
                println!("  admitted [{idx}] {name}");
                // Keep the raw triple for the from-scratch re-verification below.
                let e = &commons.entries()[idx];
                triples.push((e.pact.clone(), e.cert.clone(), e.label.clone()));
            }
            Err(e) => {
                eprintln!("  ! could not admit {name}: {e}");
            }
        }
    }
    println!();

    // ── the gate REFUSES uncertified pacts (show it, honestly) ──────────────
    println!("  the aggregation gate refuses non-certified pacts:");
    for name in UNCERTIFIED_NAMES {
        let mut probe = Commons::new();
        match probe.admit_from_dir(&dir, name) {
            Ok(_) => eprintln!("    ✗ BUG: {name} was admitted but is not certified"),
            Err(e) => println!("    · {name}: refused — {e}"),
        }
    }
    println!();

    if commons.is_empty() {
        eprintln!("no certified pacts admitted — nothing to report.");
        std::process::exit(1);
    }

    // ── (2)+(4) the report: headline error bar, then per-template controversy ─
    let report = commons.report();
    println!("{}\n", wrap(&report.headline(), 78, "  "));

    for tr in &report.templates {
        println!("──────────────────────────────────────────────────────────────────────────");
        println!(
            "TEMPLATE  {} crux × {} dial, {} clauses   (key {}…)",
            tr.descriptor.crux_count,
            tr.descriptor.dial_count,
            tr.descriptor.clause_count,
            &tr.key[..tr.key.len().min(16)]
        );
        println!("  members ({}): {}", tr.members.len(), tr.member_labels.join(", "));
        println!("  canonical guards (names/thresholds/awards stripped — the shared shape):");
        for g in &tr.descriptor.canonical_guards {
            println!("    {g}");
        }
        println!();

        // Surface the certified controversies first (the load-bearing output):
        // decision points where formally-identical worlds got opposite calls.
        let mut any_controversy = false;
        for t in &tr.tallies {
            if matches!(t.shape(), TallyShape::LiveControversy) {
                any_controversy = true;
                println!("  ★ CERTIFIED LIVE CONTROVERSY");
                print!("{}", t.render());
                println!();
            }
        }
        if !any_controversy {
            println!("  (no live controversy across this template's decision points —");
            println!("   on this tiny corpus every shared world was settled the same way.)");
        }

        // Then the settled decision points (agreement is still only advisory).
        println!("  settled / singleton decision points (advisory only, never a rule):");
        for t in &tr.tallies {
            match t.shape() {
                TallyShape::Settled | TallyShape::Singleton => {
                    println!("    {} — {}", t.point.plain(), t.confidence_note());
                    let shape = if matches!(t.shape(), TallyShape::Settled) {
                        "all agreed"
                    } else {
                        "one signed choice"
                    };
                    // Show the agreed effect concisely.
                    let effect = t
                        .entries
                        .iter()
                        .map(|e| e.effect)
                        .next();
                    println!(
                        "        {shape}: {}",
                        effect.map(effect_word).unwrap_or("—")
                    );
                }
                _ => {}
            }
        }
        println!();
    }

    // ── (3) re-verify the WHOLE corpus from scratch (no trust in the commons) ─
    println!("──────────────────────────────────────────────────────────────────────────");
    println!("RE-VERIFY WITHOUT TRUST (a skeptic, from the signed artifacts alone):");
    match reverify_corpus(&triples) {
        Ok(outcome) => {
            println!(
                "  ✓ all {} pacts re-verified: cert CERTIFIED, cert matches pact, crux verified",
                outcome.verified
            );
            println!("    free (firewall re-run offline), signed record re-checked (ed25519+chain),");
            println!("    template key recomputed. No AI, no Isabelle, no trust in the commons.");
            // And the in-place corpus reverify agrees.
            match commons.reverify() {
                Ok(()) => println!("  ✓ in-place corpus re-verification agrees."),
                Err((i, e)) => {
                    eprintln!("  ✗ in-place re-verification failed at {i}: {e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("  ✗ re-verification FAILED: {e}");
            std::process::exit(1);
        }
    }

    println!(
        "\n( the commons shows you what others, facing the identical structural question,\n  \
         chose — all signed, all re-checkable. it never tells you what to choose. )  ☄"
    );
    Ok(())
}

fn effect_word(e: mediator_pact_commons::CruxEffect) -> &'static str {
    use mediator_pact_commons::CruxEffect::*;
    match e {
        Raises => "crux-true RAISES the award",
        Lowers => "crux-true LOWERS the award",
        NoChange => "crux makes NO difference to the award",
        Indeterminate => "indeterminate",
    }
}

/// Tiny word-wrapper for the headline, so the honesty note reads cleanly in a
/// terminal. Pure formatting, no logic.
fn wrap(s: &str, width: usize, indent: &str) -> String {
    let mut out = String::new();
    let mut line = String::from(indent);
    for word in s.split_whitespace() {
        if line.len() + 1 + word.len() > width && line.len() > indent.len() {
            out.push_str(&line);
            out.push('\n');
            line = String::from(indent);
        }
        if line.len() > indent.len() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push_str(&line);
    out
}
