//! `session` — run a scripted mediation session over a scenario and print the
//! transcript. The deterministic (offline) preview of the AI-conducted session;
//! the live Bedrock brain replaces `ScriptedBrain` to make it real.
//!
//!     cargo run -p mediator-session --bin session -- scenarios/roommate.json

use mediator_session::{conduct, render_transcript, ScriptedBrain, ScriptedInputs, Session};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "scenarios/roommate.json".to_string());
    let session = Session::from_scenario(&path)?;

    // Scripted caucus lines. Rich, hand-written ones for the roommate demo;
    // otherwise a couple of generic openers per party so any scenario runs.
    let inputs = if path.contains("roommate") {
        ScriptedInputs::new()
            .with(
                "robin",
                &[
                    "Honestly I just don't think I should pay for that stain — it was wear and tear.",
                    "And look, I wasn't around for the deep clean, but I pulled my weight the whole lease.",
                ],
            )
            .with(
                "sam",
                &["The carpet is real damage and it's only fair that Robin covers it. I just want this to be fair."],
            )
    } else {
        let mut inp = ScriptedInputs::new();
        for p in &session.dispute.parties {
            inp = inp.with(
                &p.id,
                &[
                    "Here's how I see it, in my own words.",
                    "I want an outcome that's actually fair to both of us.",
                ],
            );
        }
        inp
    };

    let out = conduct(session, &ScriptedBrain, &inputs);
    println!("{}", render_transcript(&out));
    Ok(())
}
