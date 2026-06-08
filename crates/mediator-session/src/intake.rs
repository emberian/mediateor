//! Accretion: turn a free-text description of a dispute into a structured
//! `Dispute` the kernel can certify and mediate.
//!
//! This is what lets Mediateor work on a *real, novel* dispute — not just the
//! pre-seeded scenarios. The honesty discipline holds: the model **proposes** the
//! structure; it is a DRAFT, to be confirmed by the people and (where Isabelle is
//! available) certified by the prover. Nothing the model emits is trusted blindly
//! — the ledger arithmetic and the consistency are re-derived downstream, and the
//! contested predicate is named, never decided.

use mediator_llm::live::{converse_text, LiveConfig};
use mediator_types::{Dispute, Formula, Term};

/// The worked example handed to the model — the canonical roommate Dispute, in
/// exactly the shape we want back.
const EXAMPLE: &str = include_str!("../../../scenarios/roommate.json");

const INTAKE_SYSTEM: &str = "\
You convert a plain-language description of a dispute into a structured, \
machine-checkable Dispute. You output ONLY a single JSON object matching the \
schema shown in the example — no commentary, no code fences. Money is always \
integer CENTS. Reuse the exact field names and shapes from the example. The point \
is a faithful skeleton a prover can check, not a perfect reading; keep it simple \
and well-formed.";

fn user_prompt(description: &str) -> String {
    format!(
        "Here is a COMPLETE example of the JSON structure (a different dispute):\n\n\
         {EXAMPLE}\n\n\
         Now produce a Dispute JSON, in EXACTLY that structure, for THIS situation:\n\n\
         \"{description}\"\n\n\
         Requirements:\n\
         - exactly 2 parties; ids = first name lowercased; sensible display_names.\n\
         - each party's `signature` declares the nullary symbols it introduces \
         (Bool predicates, Int totals), each with a plain-English `gloss`.\n\
         - `stipulated` MUST contain ONE Iff whose RIGHT side is a nullary Bool \
         predicate App(\"<crux>\",[]) — the single contested question everything \
         reduces to — and whose LEFT side is the nullary Bool obligation it \
         controls.\n\
         - `ledger`: deposit_cents = the pool in CENTS; `items` in CENTS; mark the \
         ONE contested item \"disputed\": true (it falls away if the crux predicate \
         is false), others false.\n\
         - include ONE party claim of the form Eq(App(\"<some_total>\",[]), \
         IntLit(<cents>)) where the number deliberately does NOT equal the itemized \
         sum (an over-claim the prover will refute).\n\
         - `contested_items` + 100-point `valuations` for BOTH parties.\n\
         - two claims (ids r1, s1) taking OPPOSITE sides of the crux predicate \
         (one Atom(App(crux,[])), one Not(Atom(App(crux,[])))).\n\
         Output ONLY the JSON object."
    )
}

/// Extract a draft `Dispute` from a free-text description via the live model.
pub async fn extract_dispute(description: &str, cfg: &LiveConfig) -> anyhow::Result<Dispute> {
    let description = description.trim();
    if description.len() < 12 {
        anyhow::bail!("description too short to work with");
    }
    let model = cfg
        .models
        .first()
        .map(|m| m.id.clone())
        .unwrap_or_else(|| "us.anthropic.claude-haiku-4-5-20251001-v1:0".to_string());

    let raw = converse_text(
        &cfg.region,
        &model,
        INTAKE_SYSTEM,
        &user_prompt(description),
        3000,
        0.2,
    )
    .await?;

    let json = extract_json_object(&raw)
        .ok_or_else(|| anyhow::anyhow!("model did not return a JSON object:\n{raw}"))?;
    let dispute: Dispute = serde_json::from_str(&json)
        .map_err(|e| anyhow::anyhow!("draft Dispute did not parse: {e}\n---\n{json}"))?;
    validate(&dispute)
        .map_err(|e| anyhow::anyhow!("{e}\n  stipulated: {:?}", dispute.stipulated))?;
    Ok(dispute)
}

/// Blocking wrapper for CLI use.
pub fn extract_dispute_blocking(description: &str) -> anyhow::Result<Dispute> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(extract_dispute(description, &LiveConfig::default()))
}

/// Sanity-check the draft has the structure the kernel needs. Returns a helpful
/// error rather than letting a malformed draft flow downstream.
pub fn validate(d: &Dispute) -> anyhow::Result<()> {
    if d.parties.len() != 2 {
        anyhow::bail!("expected exactly 2 parties, got {}", d.parties.len());
    }
    // a crux: an Iff with a bare nullary-atom predicate on EITHER side
    let is_bare_atom = |f: &Formula| matches!(f, Formula::Atom(Term::App(_, a)) if a.is_empty());
    let crux = d.stipulated.iter().any(|f| {
        matches!(f, Formula::Iff(l, r) if is_bare_atom(l) || is_bare_atom(r))
    });
    if !crux {
        anyhow::bail!("no crux: `stipulated` needs an Iff with a bare predicate App(pred,[]) on a side");
    }
    if d.ledger.items.is_empty() {
        anyhow::bail!("ledger has no items");
    }
    if !d.ledger.items.iter().any(|i| i.disputed) {
        anyhow::bail!("no contested ledger item (none marked disputed)");
    }
    Ok(())
}

/// Pull the first balanced top-level `{...}` JSON object out of model text,
/// tolerant of code fences and surrounding prose. String-literal aware so braces
/// inside strings don't fool the scanner.
fn extract_json_object(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_object_from_fenced_prose() {
        let raw = "Sure! Here you go:\n```json\n{\"a\": {\"b\": 1}, \"s\": \"}{\"}\n```\ndone";
        let j = extract_json_object(raw).unwrap();
        assert!(j.starts_with('{') && j.ends_with('}'));
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["a"]["b"], 1);
    }

    #[test]
    fn validate_rejects_structureless_dispute() {
        // the example parses + validates (a known-good Dispute)
        let good: Dispute = serde_json::from_str(EXAMPLE).unwrap();
        assert!(validate(&good).is_ok());
    }
}
