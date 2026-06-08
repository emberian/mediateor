//! `mediator-tui` — two-tier, *kind* UX over one `Analysis`.
//!
//! # Two projections of one truth
//!
//! - [`party_view`] — warm, pared down, plain language for a named party.
//!   Shows shared ground first, the one real knot second, dissolved
//!   misunderstandings third, the certified refund range, and the fair
//!   settlement options to accept / reject / counter. Never says a party
//!   is wrong.
//!
//! - [`operator_view`] — the cockpit. Dense, precise, for the lawyer +
//!   engineer pair. Everything: genuine conflicts with claim IDs, crux
//!   status, ledger findings, settlement fairness certificates.
//!
//! - [`run`] — an interactive `ratatui` app (Party / Operator tabs, `q` to
//!   quit). The renderers above are pure string functions re-used by the web
//!   crate and tests.
//!
//! Kindness is a property of the projection, not a softening of the math.

use mediator_types::{Analysis, Dispute, Settlement};

// ─────────────────────────────────────────────────────────────────────────────
// Money helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Format an integer-cents amount as `$X.XX`.
fn fmt_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}${}.{:02}", abs / 100, abs % 100)
}

// ─────────────────────────────────────────────────────────────────────────────
// party_view
// ─────────────────────────────────────────────────────────────────────────────

/// Render the party-facing view for `for_party` — warm, pared down, plain
/// language.  Output is UTF-8 text with no ANSI escapes (safe for web and
/// tests).
///
/// Structure:
/// 1. What you both already agree on
/// 2. Things that turned out to be just different words
/// 3. The one real knot — phrased gently, never as a verdict
/// 4. What the numbers show (ledger, certified)
/// 5. Fair settlement options to accept, reject, or counter
pub fn party_view(analysis: &Analysis, for_party: &str) -> String {
    let mut out = String::with_capacity(1024);

    // Title
    out.push_str("A note for you\n");
    out.push_str("═══════════════════════════════════════════════════════\n\n");

    // ── 1. Shared ground ───────────────────────────────────────────────────
    if !analysis.shared_core.is_empty() {
        out.push_str("What you both already agree on\n");
        out.push_str("──────────────────────────────\n");
        for fact in &analysis.shared_core {
            out.push_str("  • ");
            out.push_str(fact);
            out.push('\n');
        }
        out.push('\n');
    }

    // ── 2. Dissolved misunderstandings ────────────────────────────────────
    if !analysis.dissolved.is_empty() {
        out.push_str("Things that turned out to be just different words\n");
        out.push_str("──────────────────────────────────────────────────\n");
        out.push_str("  These points looked like disagreements but resolved once\n");
        out.push_str("  the language was lined up:\n");
        for d in &analysis.dissolved {
            out.push_str("  • ");
            out.push_str(d);
            out.push('\n');
        }
        out.push('\n');
    }

    // ── 3. The one real knot ───────────────────────────────────────────────
    if let Some(crux) = &analysis.crux {
        out.push_str("The one question that's still open\n");
        out.push_str("──────────────────────────────────\n");
        out.push_str("  After clearing away everything that could be cleared,\n");
        out.push_str("  the entire money question comes down to one point:\n\n");
        out.push_str("  ");
        out.push_str(crux);
        out.push_str("\n\n");
        out.push_str("  Reasonable people can read the same situation differently.\n");
        out.push_str("  The options below are designed so that either resolution\n");
        out.push_str("  of that question still leads to a fair outcome.\n\n");
    } else if !analysis.genuine_conflicts.is_empty() {
        out.push_str("The points that still need to be worked through\n");
        out.push_str("───────────────────────────────────────────────\n");
        for c in &analysis.genuine_conflicts {
            out.push_str("  • ");
            out.push_str(&c.description);
            out.push('\n');
        }
        out.push('\n');
    }

    // ── 4. What the numbers show ──────────────────────────────────────────
    let has_ledger = analysis.ledger_refund_cents.is_some() || !analysis.ledger_findings.is_empty();
    if has_ledger {
        out.push_str("What the numbers show (certified)\n");
        out.push_str("─────────────────────────────────\n");
        if let Some(refund) = analysis.ledger_refund_cents {
            out.push_str(&format!("  Certified refund: {}\n", fmt_cents(refund)));
        }
        for finding in &analysis.ledger_findings {
            out.push_str("  • ");
            out.push_str(finding);
            out.push('\n');
        }
        out.push('\n');
    }

    // ── 5. Settlement options ─────────────────────────────────────────────
    if !analysis.settlements.is_empty() {
        out.push_str("Settlement options — yours to accept, reject, or counter\n");
        out.push_str("─────────────────────────────────────────────────────────\n");
        out.push_str("  These splits are mathematically fair (no one would prefer\n");
        out.push_str("  the other person's share). They're a starting point, not\n");
        out.push_str("  a final say.\n\n");
        for (i, s) in analysis.settlements.iter().enumerate() {
            render_settlement_party(&mut out, s, i + 1, for_party);
        }
    }

    if analysis.shared_core.is_empty()
        && analysis.dissolved.is_empty()
        && analysis.crux.is_none()
        && analysis.genuine_conflicts.is_empty()
        && !has_ledger
        && analysis.settlements.is_empty()
    {
        out.push_str("  No analysis data yet — the mediator is still working.\n");
    }

    out
}

/// Render one settlement option in the party view (warm, no fairness jargon).
fn render_settlement_party(out: &mut String, s: &Settlement, n: usize, for_party: &str) {
    out.push_str(&format!("  Option {n}: {}\n", s.label));

    // Items going to this party
    let mine: Vec<&str> = s
        .allocations
        .iter()
        .filter(|(_, p)| p == for_party)
        .map(|(item, _)| item.as_str())
        .collect();
    if !mine.is_empty() {
        out.push_str(&format!("    You receive: {}\n", mine.join(", ")));
    }

    let theirs: Vec<&str> = s
        .allocations
        .iter()
        .filter(|(_, p)| p != for_party)
        .map(|(item, _)| item.as_str())
        .collect();
    if !theirs.is_empty() {
        out.push_str(&format!("    They receive: {}\n", theirs.join(", ")));
    }

    for (item, frac) in &s.splits {
        let pct = (frac * 100.0).round() as u32;
        out.push_str(&format!("    {item}: shared {pct}% / {}%\n", 100 - pct));
    }

    if let Some((_, pts)) = s.party_points.iter().find(|(p, _)| p == for_party) {
        out.push_str(&format!("    Your share value: {pts:.1} points\n"));
    }

    if !s.explanation.is_empty() {
        out.push_str(&format!("    Note: {}\n", s.explanation));
    }
    out.push('\n');
}

// ─────────────────────────────────────────────────────────────────────────────
// operator_view
// ─────────────────────────────────────────────────────────────────────────────

/// Render the operator cockpit — everything, dense, for the lawyer + engineer.
/// Output is UTF-8 text, no ANSI escapes.
///
/// Sections:
/// 1. Shared core (stipulated facts)
/// 2. Genuine conflicts (with claim IDs)
/// 3. Dissolved (vocabulary mismatch, receipted)
/// 4. Crux status
/// 5. Ledger findings
/// 6. Settlement certificates
pub fn operator_view(analysis: &Analysis) -> String {
    let mut out = String::with_capacity(2048);

    out.push_str("OPERATOR COCKPIT\n");
    out.push_str("═══════════════════════════════════════════════════════\n\n");

    // ── 1. Shared core ────────────────────────────────────────────────────
    out.push_str("SHARED CORE\n");
    out.push_str("───────────\n");
    if analysis.shared_core.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for fact in &analysis.shared_core {
            out.push_str("  • ");
            out.push_str(fact);
            out.push('\n');
        }
    }
    out.push('\n');

    // ── 2. Genuine conflicts ──────────────────────────────────────────────
    out.push_str("GENUINE CONFLICTS\n");
    out.push_str("─────────────────\n");
    if analysis.genuine_conflicts.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for c in &analysis.genuine_conflicts {
            out.push_str(&format!("  Parties: {}\n", c.parties.join(", ")));
            out.push_str(&format!("  Claims:  {}\n", c.claim_ids.join(", ")));
            out.push_str(&format!("  Desc:    {}\n", c.description));
            out.push('\n');
        }
    }

    // ── 3. Dissolved ──────────────────────────────────────────────────────
    out.push_str("DISSOLVED (VOCABULARY MISMATCH)\n");
    out.push_str("───────────────────────────────\n");
    if analysis.dissolved.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for d in &analysis.dissolved {
            out.push_str("  • ");
            out.push_str(d);
            out.push('\n');
        }
    }
    out.push('\n');

    // ── 4. Crux ───────────────────────────────────────────────────────────
    out.push_str("CRUX\n");
    out.push_str("────\n");
    match &analysis.crux {
        Some(c) => {
            out.push_str("  STATUS: isolated — the whole obligation reduces to one predicate\n");
            out.push_str(&format!("  PREDICATE: {c}\n"));
            out.push_str("  VERDICT: Unknown (human question; handed back, not decided)\n");
        }
        None => {
            out.push_str("  STATUS: not isolated\n");
        }
    }
    out.push('\n');

    // ── 5. Ledger findings ────────────────────────────────────────────────
    out.push_str("LEDGER FINDINGS (CERTIFIED)\n");
    out.push_str("───────────────────────────\n");
    if let Some(refund) = analysis.ledger_refund_cents {
        out.push_str(&format!("  Certified refund: {} ({}¢)\n", fmt_cents(refund), refund));
    } else {
        out.push_str("  Refund: Unknown / not certified\n");
    }
    if analysis.ledger_findings.is_empty() {
        out.push_str("  (no further findings)\n");
    } else {
        for f in &analysis.ledger_findings {
            out.push_str("  • ");
            out.push_str(f);
            out.push('\n');
        }
    }
    out.push('\n');

    // ── 6. Settlements ────────────────────────────────────────────────────
    out.push_str("SETTLEMENTS\n");
    out.push_str("───────────\n");
    if analysis.settlements.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for (i, s) in analysis.settlements.iter().enumerate() {
            render_settlement_operator(&mut out, s, i + 1);
        }
    }

    out
}

/// Render one settlement in the operator view (dense, with fairness flags).
fn render_settlement_operator(out: &mut String, s: &Settlement, n: usize) {
    out.push_str(&format!("  [{n}] {}\n", s.label));

    for (item, party) in &s.allocations {
        out.push_str(&format!("      alloc  {item} → {party}\n"));
    }
    for (item, frac) in &s.splits {
        out.push_str(&format!("      split  {item}  {:.1}% / {:.1}%\n", frac * 100.0, (1.0 - frac) * 100.0));
    }
    for (party, pts) in &s.party_points {
        out.push_str(&format!("      pts    {party}: {pts:.2}\n"));
    }

    let ef = flag(s.envy_free);
    let eq = flag(s.equitable);
    let po = flag(s.pareto_optimal);
    out.push_str(&format!("      certs  envy-free={ef}  equitable={eq}  pareto={po}\n"));

    if !s.explanation.is_empty() {
        out.push_str(&format!("      note   {}\n", s.explanation));
    }
    out.push('\n');
}

fn flag(b: bool) -> &'static str {
    if b { "✓" } else { "✗" }
}

// ─────────────────────────────────────────────────────────────────────────────
// Interactive TUI — ratatui + crossterm
// ─────────────────────────────────────────────────────────────────────────────

/// Run an interactive ratatui application with two tabs:
/// - **Party** tab — rendered with [`party_view`] for each party; cycle with
///   left/right arrows, or `1`/`2` to jump.
/// - **Operator** tab — the cockpit, rendered with [`operator_view`].
///
/// Keys: `Tab` / `←` / `→` to switch tabs, `q` / `Esc` to quit.
///
/// The string renderers are called fresh on each redraw, keeping rendering
/// logic cleanly separated from the event loop.
pub fn run(analysis: Analysis, dispute: Dispute) -> std::io::Result<()> {
    use crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{
        backend::CrosstermBackend,
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Paragraph, Tabs, Wrap},
        Terminal,
    };
    use std::io;

    // ── Terminal setup ────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stderr = io::stderr();
    execute!(stderr, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend)?;

    // ── App state ─────────────────────────────────────────────────────────
    // Tab 0 = Party view; tabs 1..n = one per party; last tab = Operator
    let party_ids: Vec<String> = dispute.parties.iter().map(|p| p.id.clone()).collect();
    let party_names: Vec<String> = dispute.parties.iter().map(|p| p.display_name.clone()).collect();
    let n_party_tabs = party_ids.len().max(1);
    let operator_tab_idx = n_party_tabs;
    let total_tabs = n_party_tabs + 1;

    // Pre-render strings so we don't re-allocate every frame unless we want to.
    let party_texts: Vec<String> = party_ids
        .iter()
        .map(|id| party_view(&analysis, id))
        .collect();
    // If no parties, show a generic view
    let generic_party = if party_ids.is_empty() {
        party_view(&analysis, "")
    } else {
        String::new()
    };
    let op_text = operator_view(&analysis);

    let mut selected_tab: usize = 0;
    let mut scroll_offset: u16 = 0;

    // ── Render closure ────────────────────────────────────────────────────
    let render = |terminal: &mut Terminal<CrosstermBackend<io::Stderr>>,
                  selected_tab: usize,
                  scroll_offset: u16,
                  party_texts: &[String],
                  generic_party: &str,
                  op_text: &str,
                  party_names: &[String],
                  operator_tab_idx: usize|
     -> io::Result<()> {
        terminal.draw(|f| {
            let size = f.area();

            // Top/body split
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)])
                .split(size);

            // ── Tab bar ────────────────────────────────────────────────
            let tab_labels: Vec<Line> = {
                let mut labels: Vec<Line> = party_names
                    .iter()
                    .map(|n| Line::from(Span::raw(n.as_str())))
                    .collect();
                if labels.is_empty() {
                    labels.push(Line::from("Party"));
                }
                labels.push(Line::from("Operator"));
                labels
            };

            let tabs_widget = Tabs::new(tab_labels)
                .block(Block::default().borders(Borders::ALL).title(" Mediator "))
                .select(selected_tab)
                .style(Style::default().fg(Color::White))
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_widget(tabs_widget, chunks[0]);

            // ── Body ───────────────────────────────────────────────────
            let content: &str = if selected_tab == operator_tab_idx {
                op_text
            } else if party_texts.is_empty() {
                generic_party
            } else {
                party_texts
                    .get(selected_tab)
                    .map(|s| s.as_str())
                    .unwrap_or(generic_party)
            };

            let (border_color, title) = if selected_tab == operator_tab_idx {
                (Color::Yellow, " Operator Cockpit ")
            } else {
                (Color::Cyan, " Party View ")
            };

            let para = Paragraph::new(content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(Style::default().fg(border_color)),
                )
                .wrap(Wrap { trim: false })
                .scroll((scroll_offset, 0));
            f.render_widget(para, chunks[1]);

            // ── Help line overlay ──────────────────────────────────────
            let help = Paragraph::new(
                " Tab/←/→: switch  q/Esc: quit  ↑↓: scroll ",
            )
            .style(Style::default().fg(Color::DarkGray));
            // Render in bottom-right corner of the body block
            let help_area = ratatui::layout::Rect {
                x: chunks[1].x + 1,
                y: chunks[1].y + chunks[1].height.saturating_sub(1),
                width: chunks[1].width.saturating_sub(2),
                height: 1,
            };
            f.render_widget(help, help_area);
        })?;
        Ok(())
    };

    // Initial draw
    render(
        &mut terminal,
        selected_tab,
        scroll_offset,
        &party_texts,
        &generic_party,
        &op_text,
        &party_names,
        operator_tab_idx,
    )?;

    // ── Event loop ────────────────────────────────────────────────────────
    loop {
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                // Only handle press events (not release/repeat on some backends)
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab | KeyCode::Right => {
                            selected_tab = (selected_tab + 1) % total_tabs;
                            scroll_offset = 0;
                        }
                        KeyCode::Left => {
                            selected_tab = if selected_tab == 0 {
                                total_tabs - 1
                            } else {
                                selected_tab - 1
                            };
                            scroll_offset = 0;
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let n = c as usize - '0' as usize;
                            if n > 0 && n <= total_tabs {
                                selected_tab = n - 1;
                                scroll_offset = 0;
                            }
                        }
                        KeyCode::Down => {
                            scroll_offset = scroll_offset.saturating_add(1);
                        }
                        KeyCode::Up => {
                            scroll_offset = scroll_offset.saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            scroll_offset = scroll_offset.saturating_add(20);
                        }
                        KeyCode::PageUp => {
                            scroll_offset = scroll_offset.saturating_sub(20);
                        }
                        _ => {}
                    }

                    render(
                        &mut terminal,
                        selected_tab,
                        scroll_offset,
                        &party_texts,
                        &generic_party,
                        &op_text,
                        &party_names,
                        operator_tab_idx,
                    )?;
                }
            }
        }
    }

    // ── Teardown ──────────────────────────────────────────────────────────
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::{Analysis, Conflict, Settlement};

    /// Build a representative `Analysis` covering all the interesting cases:
    /// shared facts, one Conflict with claim IDs, a crux, ledger findings,
    /// and one certified-fair settlement.
    fn sample_analysis() -> Analysis {
        Analysis {
            shared_core: vec![
                "Both parties agree the deposit was $1,200.00.".to_string(),
                "Professional cleaning ($150.00) is undisputed.".to_string(),
            ],
            genuine_conflicts: vec![Conflict {
                description: "Whether the carpet stain is chargeable damage.".to_string(),
                parties: vec!["robin".to_string(), "sam".to_string()],
                claim_ids: vec!["r1".to_string(), "s1".to_string()],
            }],
            dissolved: vec![
                "\"Carpet repair\" vs. \"stain remediation\" — same item, different words."
                    .to_string(),
            ],
            ledger_refund_cents: Some(105000),
            ledger_findings: vec![
                "Claimed total $500.00 refuted; itemized total = $450.00.".to_string(),
                "Refund range: $750.00 (damage) – $1,050.00 (wear).".to_string(),
            ],
            crux: Some(
                "stain_is_damage — is the carpet stain chargeable damage (vs. ordinary wear)?"
                    .to_string(),
            ),
            settlements: vec![Settlement {
                label: "Adjusted Winner (wear scenario)".to_string(),
                allocations: vec![
                    ("couch".to_string(), "robin".to_string()),
                    ("kitchenware".to_string(), "sam".to_string()),
                ],
                splits: vec![("standing_desk".to_string(), 0.6)],
                party_points: vec![
                    ("robin".to_string(), 52.0),
                    ("sam".to_string(), 52.0),
                ],
                envy_free: true,
                equitable: true,
                pareto_optimal: true,
                explanation: "Both parties receive equal value; neither prefers the other's share."
                    .to_string(),
            }],
        }
    }

    // ── party_view tests ────────────────────────────────────────────────────

    #[test]
    fn party_view_contains_shared_agreement() {
        let analysis = sample_analysis();
        let view = party_view(&analysis, "robin");
        assert!(
            view.contains("Both parties agree the deposit was $1,200.00."),
            "party_view must mention shared agreement facts\ngot:\n{view}"
        );
    }

    #[test]
    fn party_view_contains_gentle_crux() {
        let analysis = sample_analysis();
        let view = party_view(&analysis, "robin");
        assert!(
            view.contains("stain_is_damage"),
            "party_view must mention the crux predicate\ngot:\n{view}"
        );
    }

    #[test]
    fn party_view_contains_dissolved() {
        let analysis = sample_analysis();
        let view = party_view(&analysis, "robin");
        assert!(
            view.contains("different words"),
            "party_view must mention dissolved misunderstandings\ngot:\n{view}"
        );
    }

    #[test]
    fn party_view_never_says_wrong() {
        let analysis = sample_analysis();
        for party in &["robin", "sam"] {
            let view = party_view(&analysis, party);
            assert!(
                !view.contains("wrong"),
                "party_view must never use the word 'wrong' (party={party})\ngot:\n{view}"
            );
        }
    }

    #[test]
    fn party_view_shows_refund() {
        let analysis = sample_analysis();
        let view = party_view(&analysis, "robin");
        // $1050.00 from ledger_refund_cents = 105000
        assert!(
            view.contains("$1050.00") || view.contains("1,050"),
            "party_view must show the certified refund\ngot:\n{view}"
        );
    }

    #[test]
    fn party_view_shows_settlement() {
        let analysis = sample_analysis();
        let view = party_view(&analysis, "robin");
        assert!(
            view.contains("Adjusted Winner"),
            "party_view must list settlement options\ngot:\n{view}"
        );
    }

    #[test]
    fn party_view_shows_items_for_party() {
        let analysis = sample_analysis();
        let robin_view = party_view(&analysis, "robin");
        // Robin gets the couch in our sample settlement
        assert!(
            robin_view.contains("couch"),
            "party_view for robin must mention 'couch'\ngot:\n{robin_view}"
        );
    }

    // ── operator_view tests ─────────────────────────────────────────────────

    #[test]
    fn operator_view_contains_conflict_claim_ids() {
        let analysis = sample_analysis();
        let view = operator_view(&analysis);
        assert!(
            view.contains("r1"),
            "operator_view must contain claim id 'r1'\ngot:\n{view}"
        );
        assert!(
            view.contains("s1"),
            "operator_view must contain claim id 's1'\ngot:\n{view}"
        );
    }

    #[test]
    fn operator_view_contains_crux_status() {
        let analysis = sample_analysis();
        let view = operator_view(&analysis);
        assert!(
            view.contains("stain_is_damage"),
            "operator_view must include the crux predicate\ngot:\n{view}"
        );
        assert!(
            view.contains("Unknown"),
            "operator_view must report crux verdict as Unknown\ngot:\n{view}"
        );
    }

    #[test]
    fn operator_view_contains_ledger_findings() {
        let analysis = sample_analysis();
        let view = operator_view(&analysis);
        assert!(
            view.contains("refuted"),
            "operator_view must include the over-claim refutation\ngot:\n{view}"
        );
    }

    #[test]
    fn operator_view_contains_fairness_certs() {
        let analysis = sample_analysis();
        let view = operator_view(&analysis);
        // envy_free / equitable / pareto flags
        assert!(
            view.contains("envy-free"),
            "operator_view must show envy-free certificate\ngot:\n{view}"
        );
        assert!(
            view.contains("pareto"),
            "operator_view must show pareto certificate\ngot:\n{view}"
        );
    }

    #[test]
    fn operator_view_shows_dissolved_section() {
        let analysis = sample_analysis();
        let view = operator_view(&analysis);
        assert!(
            view.contains("DISSOLVED"),
            "operator_view must have a DISSOLVED section\ngot:\n{view}"
        );
    }

    // ── fmt_cents ───────────────────────────────────────────────────────────

    #[test]
    fn fmt_cents_zero() {
        assert_eq!(fmt_cents(0), "$0.00");
    }

    #[test]
    fn fmt_cents_positive() {
        assert_eq!(fmt_cents(105000), "$1050.00");
        assert_eq!(fmt_cents(75099), "$750.99");
    }

    #[test]
    fn fmt_cents_negative() {
        assert_eq!(fmt_cents(-500), "-$5.00");
    }

    // ── empty analysis ──────────────────────────────────────────────────────

    #[test]
    fn empty_analysis_does_not_panic() {
        let a = Analysis::default();
        let pv = party_view(&a, "alice");
        let ov = operator_view(&a);
        assert!(pv.contains("mediator is still working"));
        assert!(ov.contains("OPERATOR COCKPIT"));
    }
}
