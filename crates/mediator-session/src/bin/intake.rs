//! `intake` — turn a free-text dispute description into a structured Dispute
//! the kernel can certify and mediate. The first step of accretion: Mediateor on
//! a *real, novel* dispute, not just the pre-seeded scenarios.
//!
//!     cargo run -p mediator-session --bin intake -- "Two roommates, Jess and \
//!       Kai, are splitting a $1500 deposit. Kai says Jess broke the window; Jess \
//!       says it was cracked when they moved in. Cleaning was $200."

use mediator_session::intake::extract_dispute_blocking;

fn main() -> anyhow::Result<()> {
    let desc = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if desc.trim().is_empty() {
        eprintln!("usage: intake \"<describe the dispute in plain language>\"");
        std::process::exit(2);
    }
    eprintln!("☄ reading the situation and drafting a structured dispute…\n");
    let dispute = extract_dispute_blocking(&desc)?;
    println!("{}", serde_json::to_string_pretty(&dispute)?);
    eprintln!(
        "\n✓ draft parsed + validated — \"{}\": {} parties, {} ledger items ({} contested), {} items to divide.\n  \
         This is a DRAFT: the people confirm it, and the prover certifies the numbers and the crux.",
        dispute.title,
        dispute.parties.len(),
        dispute.ledger.items.len(),
        dispute.ledger.items.iter().filter(|i| i.disputed).count(),
        dispute.contested_items.len(),
    );
    Ok(())
}
