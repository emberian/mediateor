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
//! - [`receipts_view`] — pure text rendering of the hash-chained receipt
//!   ledger (seq, op, short hash, verdict) plus a chain-validity line.
//!
//! - [`run`] — an interactive `ratatui` app: one tab per party, plus
//!   Operator and Receipts tabs. `Tab`/`←`/`→` to switch, `q` to quit.
//!   The renderers above are pure string functions re-used by the web
//!   crate and tests.
//!
//! Kindness is a property of the projection, not a softening of the math.

use mediator_types::{Analysis, Dispute, Receipt, Settlement, Verdict};

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
/// 3. Dissolved (vocabulary mismatch)
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
// receipts_view — pure text renderer for the hash-chained ledger
// ─────────────────────────────────────────────────────────────────────────────

/// Render the receipt ledger as a verifiable table. Output is UTF-8, no ANSI.
///
/// Each row: `#seq  op_name  short_hash…  verdict`
/// Final line: `✓ chain verified (N links)` or `✗ chain broken at entry I`.
///
/// `verify_fn` accepts the receipts slice and returns `Ok(())` or `Err(idx)` —
/// pass `mediator_core::receipts::verify_chain` in production; inject a stub
/// in tests.
pub fn receipts_view(receipts: &[Receipt], verify_fn: fn(&[Receipt]) -> Result<(), usize>) -> String {
    let mut out = String::with_capacity(512);

    out.push_str("RECEIPT LEDGER  (append-only, hash-chained)\n");
    out.push_str("════════════════════════════════════════════════════════\n\n");

    if receipts.is_empty() {
        out.push_str("  (no receipts yet)\n\n");
    } else {
        out.push_str("  #    operation            hash (first 12)  verdict\n");
        out.push_str("  ─────────────────────────────────────────────────────\n");
        for r in receipts {
            let short_hash = if r.hash.len() >= 12 { &r.hash[..12] } else { &r.hash };
            let verdict_str = match &r.verdict {
                Some(Verdict::Proved) => "Proved",
                Some(Verdict::Refuted) => "Refuted",
                Some(Verdict::Unknown) => "Unknown",
                Some(Verdict::Error(e)) => {
                    // Truncate long errors for display
                    let _ = e; // used via the format below
                    "Error"
                }
                None => "—",
            };
            out.push_str(&format!(
                "  {:<4} {:<20} {}…  {}\n",
                format!("#{}", r.seq),
                r.op,
                short_hash,
                verdict_str,
            ));
        }
        out.push('\n');

        // Prev-hash chain detail (one indent-level deeper)
        out.push_str("  prev-hash links\n");
        out.push_str("  ───────────────\n");
        for r in receipts {
            let prev_short = if r.prev_hash.len() >= 8 { &r.prev_hash[..8] } else { &r.prev_hash };
            let hash_short = if r.hash.len() >= 8 { &r.hash[..8] } else { &r.hash };
            out.push_str(&format!("  #{}: {}… → {}…\n", r.seq, prev_short, hash_short));
        }
        out.push('\n');
    }

    // Chain validity summary
    match verify_fn(receipts) {
        Ok(()) => out.push_str(&format!(
            "  ✓  hash chain verified ({} link{})\n",
            receipts.len(),
            if receipts.len() == 1 { "" } else { "s" },
        )),
        Err(i) => out.push_str(&format!("  ✗  hash chain broken at entry #{i}\n")),
    }

    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Interactive TUI — ratatui + crossterm
// ─────────────────────────────────────────────────────────────────────────────

/// Tab identity — one per party, plus the two fixed tabs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Party(usize),
    Operator,
    Receipts,
}

/// Run an interactive ratatui application:
/// - One tab per party (their kind party view, by name)
/// - **Operator** tab — the cockpit
/// - **Receipts** tab — the hash-chained ledger, with chain-validity line
///
/// Keys: `Tab` / `→` to advance, `←` to go back, `q` / `Esc` to quit,
///       `↑` / `↓` / `PgUp` / `PgDn` to scroll.
///
/// The string renderers are called once at startup (pure, no TTY needed) and
/// the results are stored; the event loop just re-renders on state change.
pub fn run(analysis: Analysis, dispute: Dispute) -> std::io::Result<()> {
    run_with_verify(analysis, dispute, |_| Ok(()))
}

/// Like [`run`] but accepts any chain-verify function (used for testing the
/// receipts rendering path without Isabelle receipts).
pub fn run_with_receipts(
    analysis: Analysis,
    dispute: Dispute,
    receipts: Vec<Receipt>,
    verify_fn: fn(&[Receipt]) -> Result<(), usize>,
) -> std::io::Result<()> {
    run_impl(analysis, dispute, receipts, verify_fn)
}

fn run_with_verify(
    analysis: Analysis,
    dispute: Dispute,
    verify_fn: fn(&[Receipt]) -> Result<(), usize>,
) -> std::io::Result<()> {
    run_impl(analysis, dispute, vec![], verify_fn)
}

fn run_impl(
    analysis: Analysis,
    dispute: Dispute,
    receipts: Vec<Receipt>,
    verify_fn: fn(&[Receipt]) -> Result<(), usize>,
) -> std::io::Result<()> {
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

    // ── Pre-render all tab content (pure; no TTY dependency) ──────────────
    let party_ids: Vec<String> = dispute.parties.iter().map(|p| p.id.clone()).collect();
    let party_names: Vec<String> = dispute.parties.iter().map(|p| p.display_name.clone()).collect();

    let party_texts: Vec<String> = party_ids.iter().map(|id| party_view(&analysis, id)).collect();
    let op_text = operator_view(&analysis);
    let rec_text = receipts_view(&receipts, verify_fn);

    // Tab order: party_0, party_1, …, Operator, Receipts
    let n_parties = party_ids.len().max(1);
    let tab_count = n_parties + 2; // +Operator +Receipts

    let tab_at = |idx: usize| -> Tab {
        if idx < n_parties { Tab::Party(idx) }
        else if idx == n_parties { Tab::Operator }
        else { Tab::Receipts }
    };
    let idx_of = |tab: Tab| -> usize {
        match tab {
            Tab::Party(i) => i,
            Tab::Operator => n_parties,
            Tab::Receipts => n_parties + 1,
        }
    };

    let mut selected_tab: Tab = Tab::Party(0);
    let mut scroll_offset: u16 = 0;

    // ── Colors ────────────────────────────────────────────────────────────
    // Warm amber for the header wordmark; teal/cyan for party tabs;
    // yellow for operator; magenta for receipts.
    let accent_for = |tab: Tab| -> Color {
        match tab {
            Tab::Party(_) => Color::Cyan,
            Tab::Operator => Color::Yellow,
            Tab::Receipts => Color::Magenta,
        }
    };

    // ── Render closure ────────────────────────────────────────────────────
    let draw = |terminal: &mut Terminal<CrosstermBackend<io::Stderr>>,
                selected_tab: Tab,
                scroll_offset: u16| -> io::Result<()> {
        terminal.draw(|f| {
            let size = f.area();

            // Layout: header (1 line) | tab-bar (3 lines) | body | hint (1 line)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // wordmark header
                    Constraint::Length(3), // tab bar
                    Constraint::Min(0),    // content
                    Constraint::Length(1), // key hints
                ])
                .split(size);

            // ── Wordmark header ────────────────────────────────────────────
            // "  Mediateor ☄   ·   <dispute title>"
            let header_line = Line::from(vec![
                Span::styled(
                    "  Mediateor ☄  ",
                    Style::default()
                        .fg(Color::Rgb(255, 160, 60)) // warm amber
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "·  ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    dispute.title.as_str(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]);
            f.render_widget(
                Paragraph::new(header_line)
                    .style(Style::default().bg(Color::Rgb(20, 20, 28))),
                chunks[0],
            );

            // ── Tab bar ────────────────────────────────────────────────────
            let tab_labels: Vec<Line> = {
                let mut labels: Vec<Line> = if party_names.is_empty() {
                    vec![Line::from("Party")]
                } else {
                    party_names.iter().map(|n| Line::from(n.as_str())).collect()
                };
                labels.push(Line::from("Operator"));
                labels.push(Line::from("Receipts"));
                labels
            };

            let selected_idx = idx_of(selected_tab);
            let tabs_widget = Tabs::new(tab_labels)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .select(selected_idx)
                .style(Style::default().fg(Color::DarkGray))
                .highlight_style(
                    Style::default()
                        .fg(accent_for(selected_tab))
                        .add_modifier(Modifier::BOLD),
                );
            f.render_widget(tabs_widget, chunks[1]);

            // ── Body ───────────────────────────────────────────────────────
            let content: &str = match selected_tab {
                Tab::Party(i) => party_texts
                    .get(i)
                    .map(|s| s.as_str())
                    .unwrap_or("(no party data)"),
                Tab::Operator => &op_text,
                Tab::Receipts => &rec_text,
            };

            let title = match selected_tab {
                Tab::Party(i) => {
                    let name = party_names.get(i).map(|s| s.as_str()).unwrap_or("Party");
                    format!(" {name} ")
                }
                Tab::Operator => " Operator Cockpit ".to_string(),
                Tab::Receipts => " Receipt Ledger ".to_string(),
            };

            let accent = accent_for(selected_tab);

            let para = Paragraph::new(content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Span::styled(
                            title.as_str(),
                            Style::default()
                                .fg(accent)
                                .add_modifier(Modifier::BOLD),
                        ))
                        .border_style(Style::default().fg(accent)),
                )
                .wrap(Wrap { trim: false })
                .scroll((scroll_offset, 0));
            f.render_widget(para, chunks[2]);

            // ── Key hint bar ───────────────────────────────────────────────
            let hint = Line::from(vec![
                Span::styled(" Tab", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("/", Style::default().fg(Color::DarkGray)),
                Span::styled("←→", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" switch tab   ", Style::default().fg(Color::DarkGray)),
                Span::styled("↑↓ PgUp PgDn", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" scroll   ", Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Rgb(255, 100, 100)).add_modifier(Modifier::BOLD)),
                Span::styled(" quit", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "   ☄  mediateor",
                    Style::default().fg(Color::Rgb(255, 160, 60)),
                ),
            ]);
            f.render_widget(
                Paragraph::new(hint).style(Style::default().bg(Color::Rgb(20, 20, 28))),
                chunks[3],
            );
        })?;
        Ok(())
    };

    // Initial draw
    draw(&mut terminal, selected_tab, scroll_offset)?;

    // ── Event loop ────────────────────────────────────────────────────────
    loop {
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let mut changed = true;
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,

                        KeyCode::Tab | KeyCode::Right => {
                            let next = (idx_of(selected_tab) + 1) % tab_count;
                            selected_tab = tab_at(next);
                            scroll_offset = 0;
                        }
                        KeyCode::Left => {
                            let cur = idx_of(selected_tab);
                            let prev = if cur == 0 { tab_count - 1 } else { cur - 1 };
                            selected_tab = tab_at(prev);
                            scroll_offset = 0;
                        }

                        // Jump to party tabs by number (1-based); O = Operator; R = Receipts
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let n = (c as usize).saturating_sub('0' as usize);
                            if n > 0 && n <= n_parties {
                                selected_tab = Tab::Party(n - 1);
                                scroll_offset = 0;
                            }
                        }
                        KeyCode::Char('o') | KeyCode::Char('O') => {
                            selected_tab = Tab::Operator;
                            scroll_offset = 0;
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            selected_tab = Tab::Receipts;
                            scroll_offset = 0;
                        }

                        KeyCode::Down => scroll_offset = scroll_offset.saturating_add(1),
                        KeyCode::Up => scroll_offset = scroll_offset.saturating_sub(1),
                        KeyCode::PageDown => scroll_offset = scroll_offset.saturating_add(20),
                        KeyCode::PageUp => scroll_offset = scroll_offset.saturating_sub(20),

                        _ => { changed = false; }
                    }

                    if changed {
                        draw(&mut terminal, selected_tab, scroll_offset)?;
                    }
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
    use mediator_types::{Analysis, Conflict, Receipt, Settlement};
    use serde_json::json;

    /// Build a representative `Analysis` covering all the interesting cases.
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

    /// Build a small valid receipt chain.
    fn sample_receipts() -> Vec<Receipt> {
        let genesis = "0000000000000000000000000000000000000000000000000000000000000000";
        // Compute hashes properly so verify_chain passes
        use sha2::{Digest, Sha256};
        let chain_hash = |prev: &str, op: &str, detail: &serde_json::Value| -> String {
            let mut h = Sha256::new();
            h.update(prev.as_bytes());
            h.update(op.as_bytes());
            h.update(serde_json::to_vec(detail).unwrap_or_default());
            h.finalize().iter().map(|b| format!("{b:02x}")).collect()
        };

        let d0 = json!({"items": 2});
        let h0 = chain_hash(genesis, "verify_ledger", &d0);
        let d1 = json!({"verdict": "Proved"});
        let h1 = chain_hash(&h0, "isolate_crux", &d1);

        vec![
            Receipt {
                seq: 0,
                prev_hash: genesis.to_string(),
                hash: h0.clone(),
                op: "verify_ledger".to_string(),
                detail: d0,
                verdict: Some(Verdict::Proved),
            },
            Receipt {
                seq: 1,
                prev_hash: h0,
                hash: h1,
                op: "isolate_crux".to_string(),
                detail: d1,
                verdict: Some(Verdict::Unknown),
            },
        ]
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

    // ── receipts_view tests ─────────────────────────────────────────────────

    fn always_ok(_: &[Receipt]) -> Result<(), usize> { Ok(()) }
    fn always_err(_: &[Receipt]) -> Result<(), usize> { Err(0) }

    #[test]
    fn receipts_view_empty_shows_no_receipts() {
        let view = receipts_view(&[], always_ok);
        assert!(view.contains("no receipts yet"), "got:\n{view}");
    }

    #[test]
    fn receipts_view_shows_chain_verified() {
        let receipts = sample_receipts();
        // Use the always_ok stub so we don't need the sha2 dep to verify
        let view = receipts_view(&receipts, always_ok);
        assert!(view.contains("✓"), "should show chain-valid checkmark\ngot:\n{view}");
        assert!(view.contains("chain verified"), "should mention chain verified\ngot:\n{view}");
    }

    #[test]
    fn receipts_view_shows_broken_chain() {
        let receipts = sample_receipts();
        let view = receipts_view(&receipts, always_err);
        assert!(view.contains("✗"), "should show broken-chain marker\ngot:\n{view}");
        assert!(view.contains("broken"), "should mention broken chain\ngot:\n{view}");
    }

    #[test]
    fn receipts_view_lists_ops() {
        let receipts = sample_receipts();
        let view = receipts_view(&receipts, always_ok);
        assert!(view.contains("verify_ledger"), "should list op names\ngot:\n{view}");
        assert!(view.contains("isolate_crux"), "should list op names\ngot:\n{view}");
    }

    #[test]
    fn receipts_view_lists_verdicts() {
        let receipts = sample_receipts();
        let view = receipts_view(&receipts, always_ok);
        assert!(view.contains("Proved"), "should show Proved verdict\ngot:\n{view}");
        assert!(view.contains("Unknown"), "should show Unknown verdict\ngot:\n{view}");
    }

    #[test]
    fn receipts_view_shows_short_hashes() {
        let receipts = sample_receipts();
        let view = receipts_view(&receipts, always_ok);
        // Each row should have a truncated hash followed by "…"
        assert!(view.contains('…'), "should show truncated hashes with ellipsis\ngot:\n{view}");
    }

    #[test]
    fn receipts_view_shows_prev_links() {
        let receipts = sample_receipts();
        let view = receipts_view(&receipts, always_ok);
        assert!(view.contains("prev-hash links"), "should show prev-hash link section\ngot:\n{view}");
        assert!(view.contains("→"), "should show link arrows\ngot:\n{view}");
    }

    #[test]
    fn receipts_view_link_count() {
        let receipts = sample_receipts();
        let view = receipts_view(&receipts, always_ok);
        assert!(
            view.contains("2 links"),
            "should report correct link count\ngot:\n{view}"
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
