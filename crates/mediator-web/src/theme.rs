//! The single stylesheet. Inline, no framework, no build step.
//!
//! A warm-neutral palette with one cool accent, a real type scale, generous
//! whitespace, subtle motion, mobile-friendly, and a cheap dark mode via
//! `prefers-color-scheme`.

pub const CSS: &str = r#"
:root {
  /* warm neutrals + one accent */
  --bg:        #f7f4ef;
  --bg-2:      #f1ece4;
  --surface:   #fffdfa;
  --border:    #e7e0d6;
  --border-2:  #d9d0c3;
  --text:      #2b2722;
  --text-2:    #5a5249;
  --muted:     #8d8478;
  --accent:    #b5552f;   /* warm terracotta */
  --accent-2:  #9a4526;
  --green:     #4a7c59;
  --green-bg:  #e3efe6;
  --amber:     #9a7223;
  --amber-bg:  #f4ecd8;
  --blue:      #3f6f93;
  --blue-bg:   #e0e9f1;
  --red:       #9a3a2c;
  --red-bg:    #f3e1dd;
  --grey-bg:   #ece7df;

  --radius:    14px;
  --radius-sm: 9px;
  --shadow:    0 1px 2px rgba(60,45,30,.05), 0 8px 24px -16px rgba(60,45,30,.20);
  --shadow-lg: 0 2px 4px rgba(60,45,30,.06), 0 18px 50px -24px rgba(60,45,30,.28);

  /* type scale (1.250 major third) */
  --t--1: .8125rem;
  --t-0:  1rem;
  --t-1:  1.25rem;
  --t-2:  1.563rem;
  --t-3:  1.953rem;
  --t-4:  2.441rem;
  --serif: "Iowan Old Style", "Palatino Linotype", Palatino, "Book Antiqua", Georgia, serif;
  --sans:  -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  --mono:  "SFMono-Regular", "JetBrains Mono", "Menlo", "Consolas", monospace;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg:        #1c1916;
    --bg-2:      #211d19;
    --surface:   #262119;
    --border:    #3a332a;
    --border-2:  #463d31;
    --text:      #efe8dd;
    --text-2:    #c5bcae;
    --muted:     #998f80;
    --accent:    #e08a5f;
    --accent-2:  #ef9d73;
    --green:     #8fc79f;
    --green-bg:  #25342a;
    --amber:     #d6b25f;
    --amber-bg:  #383019;
    --blue:      #8bb6d8;
    --blue-bg:   #20303c;
    --red:       #e09384;
    --red-bg:    #3a261f;
    --grey-bg:   #2e2920;
    --shadow:    0 1px 2px rgba(0,0,0,.3), 0 8px 24px -16px rgba(0,0,0,.6);
    --shadow-lg: 0 2px 4px rgba(0,0,0,.35), 0 18px 50px -24px rgba(0,0,0,.7);
  }
}

* { box-sizing: border-box; margin: 0; padding: 0; }
html { -webkit-text-size-adjust: 100%; }
body {
  font-family: var(--sans);
  background:
    radial-gradient(1200px 600px at 80% -10%, var(--bg-2), transparent 60%),
    var(--bg);
  color: var(--text);
  line-height: 1.65;
  font-size: var(--t-0);
  -webkit-font-smoothing: antialiased;
}
.page { max-width: 760px; margin: 0 auto; padding: 1.5rem 1.25rem 5rem; }

a { color: var(--accent); text-decoration: none; }
a:hover { color: var(--accent-2); text-decoration: underline; text-underline-offset: 2px; }

.mono { font-family: var(--mono); }
.muted { color: var(--muted); }
.small { font-size: var(--t--1); }

/* ── brand bar (interior pages) ──────────────────────────────────────── */
.brandbar {
  display: flex; align-items: baseline; gap: .55rem; flex-wrap: wrap;
  padding-bottom: 1.4rem; margin-bottom: 1.8rem;
  border-bottom: 1px solid var(--border);
  font-size: var(--t--1);
}
.wordmark { font-weight: 700; letter-spacing: -.01em; color: var(--text); }
.wordmark:hover { color: var(--accent); text-decoration: none; }
.crumb-sep { color: var(--muted); }
.crumb { color: var(--text-2); }
.crumb a { color: var(--text-2); }

/* ── hero (gallery) ──────────────────────────────────────────────────── */
.hero { padding: 2.6rem 0 2.2rem; text-align: center; }
.hero-mark {
  display: inline-block; font-weight: 700; letter-spacing: .02em;
  color: var(--accent); font-size: var(--t-0);
  padding: .3rem .8rem; border: 1px solid var(--border-2); border-radius: 99px;
  background: var(--surface); box-shadow: var(--shadow);
}
.hero-title {
  font-family: var(--serif);
  font-size: var(--t-4); line-height: 1.12; font-weight: 600;
  letter-spacing: -.015em; margin: 1.3rem auto .9rem; max-width: 14ch;
}
.hero-lede { color: var(--text-2); max-width: 54ch; margin: 0 auto; font-size: var(--t-1); line-height: 1.55; }

/* ── gallery cards ───────────────────────────────────────────────────── */
.gallery { display: grid; gap: 1.1rem; margin-top: 1rem; }
.case-card {
  display: block; color: inherit;
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius); padding: 1.4rem 1.5rem;
  box-shadow: var(--shadow);
  transition: transform .18s ease, box-shadow .18s ease, border-color .18s ease;
}
.case-card:hover {
  transform: translateY(-3px); box-shadow: var(--shadow-lg);
  border-color: var(--border-2); text-decoration: none;
}
.case-eyebrow {
  display: flex; align-items: center; gap: .5rem; flex-wrap: wrap;
  font-size: var(--t--1); color: var(--muted); margin-bottom: .5rem;
  text-transform: uppercase; letter-spacing: .06em;
}
.case-party { color: var(--text-2); font-weight: 600; }
.vs { color: var(--muted); font-weight: 400; opacity: .8; }
.case-title { font-family: var(--serif); font-size: var(--t-2); font-weight: 600; letter-spacing: -.01em; line-height: 1.2; }
.case-blurb { color: var(--text-2); margin-top: .55rem; font-size: var(--t-0); }
.case-foot { display: flex; align-items: center; gap: .45rem; flex-wrap: wrap; margin-top: 1rem; }
.case-go { margin-left: auto; color: var(--accent); font-weight: 600; font-size: var(--t--1); }

/* ── chips & pills ───────────────────────────────────────────────────── */
.chip, .pill {
  display: inline-block; padding: .16rem .55rem; border-radius: 99px;
  font-size: var(--t--1); font-weight: 600; line-height: 1.4; white-space: nowrap;
}
.pill { font-weight: 500; }
.chip-green, .pill-green { background: var(--green-bg); color: var(--green); }
.chip-amber, .pill-amber { background: var(--amber-bg); color: var(--amber); }
.chip-blue,  .pill-blue  { background: var(--blue-bg);  color: var(--blue);  }
.pill-red                { background: var(--red-bg);   color: var(--red);   }
.pill-grey               { background: var(--grey-bg);  color: var(--muted); }

/* ── interior heads ──────────────────────────────────────────────────── */
.interior-head { margin-bottom: 2rem; }
.interior-head h1 { font-family: var(--serif); font-size: var(--t-3); font-weight: 600; letter-spacing: -.015em; line-height: 1.15; }
.eyebrow { font-size: var(--t--1); text-transform: uppercase; letter-spacing: .08em; color: var(--accent); font-weight: 700; margin-bottom: .4rem; }
.lede { color: var(--text-2); margin-top: .7rem; font-size: var(--t-1); line-height: 1.5; max-width: 56ch; }

/* ── seat picker ─────────────────────────────────────────────────────── */
.seats { display: grid; gap: 1rem; }
.seat {
  display: block; color: inherit;
  background: var(--surface); border: 1.5px solid var(--border);
  border-radius: var(--radius); padding: 1.3rem 1.4rem; box-shadow: var(--shadow);
  transition: transform .16s ease, border-color .16s ease, box-shadow .16s ease;
}
.seat:hover { transform: translateY(-2px); border-color: var(--accent); box-shadow: var(--shadow-lg); text-decoration: none; }
.seat-i { display: block; font-family: var(--serif); font-size: var(--t-2); font-weight: 600; letter-spacing: -.01em; }
.seat-sub { display: block; font-size: var(--t--1); color: var(--muted); text-transform: uppercase; letter-spacing: .05em; margin-top: .15rem; }
.seat-hint { display: block; color: var(--text-2); font-size: var(--t-0); margin-top: .55rem; }
.seat-operator { background: var(--bg-2); border-style: dashed; }
.seat-operator:hover { border-color: var(--blue); }

/* ── cards (generic) ─────────────────────────────────────────────────── */
.card {
  background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius); padding: 1.2rem 1.4rem;
  box-shadow: var(--shadow); margin-bottom: .9rem;
}
.card.soft { background: var(--bg-2); box-shadow: none; }
.card.empty { text-align: center; }

ul.plain { list-style: none; }
ul.plain li { padding: .3rem 0 .3rem 1.3rem; position: relative; }
ul.plain li::before { content: ""; position: absolute; left: .2rem; top: .95em; width: .35rem; height: .35rem; border-radius: 99px; background: var(--accent); opacity: .55; }

hr.divider { border: none; border-top: 1px solid var(--border); margin: 1rem 0; }

/* ── scrollytelling steps ────────────────────────────────────────────── */
.party-head .scroll-cue { margin-top: 1.6rem; color: var(--muted); font-size: var(--t--1); letter-spacing: .12em; text-transform: uppercase; animation: bob 2.2s ease-in-out infinite; }
@keyframes bob { 0%,100%{ transform: translateY(0);} 50%{ transform: translateY(5px);} }

.step { position: relative; padding: 2.4rem 0 1.4rem; border-top: 1px solid var(--border); }
.step:first-of-type { border-top: none; }
.step-n {
  display: inline-flex; align-items: center; justify-content: center;
  width: 2rem; height: 2rem; border-radius: 99px;
  background: var(--accent); color: #fff; font-weight: 700; font-size: var(--t--1);
  margin-bottom: .7rem; box-shadow: var(--shadow);
}
.step h2 { font-family: var(--serif); font-size: var(--t-2); font-weight: 600; letter-spacing: -.01em; line-height: 1.2; }
.step-lede { color: var(--text-2); margin: .5rem 0 1rem; font-size: var(--t-0); max-width: 56ch; }

.reveal { opacity: 0; transform: translateY(18px); transition: opacity .6s ease, transform .6s ease; }
.reveal.in { opacity: 1; transform: none; }
@media (prefers-reduced-motion: reduce) {
  .reveal { opacity: 1; transform: none; transition: none; }
  .party-head .scroll-cue { animation: none; }
}

/* ── dissolved & figure & crux ───────────────────────────────────────── */
.card.dissolved { display: flex; gap: .7rem; align-items: flex-start; background: var(--green-bg); border-color: transparent; }
.dissolved-mark { color: var(--green); font-weight: 800; flex: none; }

.card.figure { text-align: center; padding: 1.8rem 1.4rem; }
.figure-amount { font-family: var(--serif); font-size: var(--t-4); font-weight: 700; letter-spacing: -.02em; color: var(--text); }
.figure-note { color: var(--muted); font-size: var(--t--1); margin-top: .35rem; max-width: 40ch; margin-inline: auto; }

.crux-box { border: 1.5px solid var(--amber); border-radius: var(--radius); padding: 1.4rem 1.5rem; background: var(--amber-bg); }
.crux-q { font-family: var(--serif); font-size: var(--t-2); font-weight: 600; line-height: 1.25; color: var(--text); }
.crux-note { margin-top: .8rem; color: var(--text-2); font-size: var(--t-0); }

/* ── settlement cards ────────────────────────────────────────────────── */
.settlement-head { display: flex; align-items: center; gap: .5rem; flex-wrap: wrap; margin-bottom: .5rem; }
.settlement-text { color: var(--text-2); }
ul.alloc { list-style: none; margin: .8rem 0 .2rem; display: grid; gap: .35rem; }
ul.alloc li { display: flex; align-items: center; gap: .5rem; font-size: var(--t--1); }
.alloc-item { color: var(--text); font-weight: 600; }
.alloc-arrow { color: var(--muted); }
.alloc-who { color: var(--text-2); }

.settle-actions { display: flex; align-items: center; gap: .6rem; flex-wrap: wrap; margin-top: 1rem; }
.btn { border: 1px solid transparent; border-radius: var(--radius-sm); padding: .5rem 1.05rem; font-size: var(--t-0); font-weight: 600; cursor: pointer; font-family: inherit; transition: transform .1s ease, background .12s ease, border-color .12s; }
.btn:active { transform: translateY(1px); }
.btn-accept { background: var(--accent); color: #fff; }
.btn-accept:hover { background: var(--accent-2); }
.btn-counter { background: transparent; color: var(--text-2); border-color: var(--border-2); }
.btn-counter:hover { border-color: var(--accent); color: var(--accent); }
.settle-done { margin-top: .9rem; color: var(--green); font-weight: 700; }
.settle-waiting { margin-top: .9rem; color: var(--accent); font-weight: 600; }
.settle-counter { margin-top: .6rem; color: var(--text-2); }
.htmx-indicator { color: var(--muted); font-size: var(--t--1); opacity: 0; transition: opacity .1s; }
.htmx-request .htmx-indicator { opacity: 1; }

.closing { padding: 2.6rem 0 1rem; border-top: 1px solid var(--border); text-align: center; color: var(--text-2); }
.closing p { max-width: 50ch; margin: 0 auto 1.2rem; font-size: var(--t-1); }
.ghost-link { font-size: var(--t--1); color: var(--muted); }

/* ── operator cockpit ────────────────────────────────────────────────── */
.op-h { font-size: var(--t--1); text-transform: uppercase; letter-spacing: .08em; color: var(--muted); font-weight: 700; margin: 1.8rem 0 .7rem; }
section:first-of-type .op-h { margin-top: 0; }
.op-row { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; }
.op-meta { display: flex; gap: .4rem; align-items: center; flex-wrap: wrap; margin-top: .4rem; font-size: var(--t--1); color: var(--muted); }
.op-crux { margin-top: .6rem; }
.op-claims .mono { color: var(--text-2); }
.op-claim-nl { margin-top: .5rem; }
.ledger-line { display: flex; justify-content: space-between; align-items: center; gap: 1rem; padding: .35rem 0; border-bottom: 1px solid var(--border); }
.ledger-line:last-child { border-bottom: none; }
.ledger-line .pill { margin-left: .5rem; }

.tablewrap { overflow-x: auto; }
.op-table { width: 100%; border-collapse: collapse; font-size: var(--t--1); }
.op-table th { text-align: left; color: var(--muted); font-weight: 600; text-transform: uppercase; letter-spacing: .04em; font-size: .72rem; padding: .4rem .55rem; border-bottom: 1px solid var(--border-2); }
.op-table td { padding: .5rem .55rem; border-bottom: 1px solid var(--border); vertical-align: middle; }
.op-table tr:last-child td { border-bottom: none; }
.op-table .num, th.num { text-align: right; font-variant-numeric: tabular-nums; }
.op-table.receipts .hash { color: var(--muted); }
.yes { color: var(--green); font-weight: 700; }
.no { color: var(--muted); }
.chain-note { margin-top: .8rem; }

/* ── footer ──────────────────────────────────────────────────────────── */
.site-footer { margin-top: 4rem; padding-top: 1.4rem; border-top: 1px solid var(--border); color: var(--muted); font-size: var(--t--1); display: flex; gap: .6rem; align-items: center; flex-wrap: wrap; justify-content: center; text-align: center; }
.site-footer .dot { opacity: .5; }
.site-footer span:last-child { font-style: italic; }

/* ── live formalization panel ────────────────────────────────────────── */
.formalize-card { border-left: 3px solid var(--blue); background: var(--surface); }
.formalize-input-row { display: flex; gap: .6rem; align-items: center; flex-wrap: wrap; margin-bottom: .5rem; }
.formalize-input {
  flex: 1; min-width: 0;
  font-family: var(--sans); font-size: var(--t-0); color: var(--text);
  background: var(--bg); border: 1px solid var(--border-2);
  border-radius: var(--radius-sm); padding: .45rem .75rem;
  transition: border-color .15s;
}
.formalize-input:focus { outline: none; border-color: var(--blue); }
.formalize-spinner { font-size: var(--t--1); color: var(--muted); }
.formalize-hint { font-size: var(--t--1); color: var(--muted); margin-top: .25rem; }
.formalize-caption { font-size: var(--t--1); color: var(--muted); font-style: italic; margin-top: .9rem; padding-top: .6rem; border-top: 1px solid var(--border); }
.formalize-result { margin-top: .8rem; }
.formalize-off, .formalize-error { color: var(--muted); font-size: var(--t--1); margin-top: .5rem; }
.formalize-error { color: var(--red); }

/* reading cards (per-model) */
.council-result { display: grid; gap: .75rem; }
.reading-card { border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 1rem 1.2rem; background: var(--bg-2); }
.reading-head { display: flex; gap: .5rem; align-items: center; margin-bottom: .5rem; flex-wrap: wrap; }
.reading-model { color: var(--text-2); font-size: var(--t--1); }
.reading-english { font-size: var(--t-1); font-family: var(--serif); color: var(--text); line-height: 1.3; margin: .4rem 0; }
.reading-issues { list-style: none; margin: .3rem 0; }
.reading-issues li { font-size: var(--t--1); color: var(--amber); padding-left: 1rem; position: relative; }
.reading-issues li::before { content: "⚠"; position: absolute; left: 0; }
.reading-details { margin-top: .5rem; font-size: var(--t--1); }
.reading-details summary { color: var(--muted); cursor: pointer; }
.reading-details summary:hover { color: var(--text-2); }
.reading-ir, .reading-raw { margin-top: .35rem; font-family: var(--mono); font-size: .72rem; color: var(--text-2); white-space: pre-wrap; word-break: break-all; background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: .4rem .6rem; }
.consensus-line { margin-top: .5rem; font-size: var(--t--1); color: var(--text-2); display: flex; align-items: center; gap: .4rem; flex-wrap: wrap; }

/* ── responsive ──────────────────────────────────────────────────────── */
@media (max-width: 560px) {
  .page { padding: 1rem 1rem 4rem; }
  .hero-title { font-size: var(--t-3); }
  .hero-lede { font-size: var(--t-0); }
  .case-go { margin-left: 0; }
}
"#;
