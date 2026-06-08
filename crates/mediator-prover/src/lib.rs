//! `mediator-prover` — the Isabelle/HOL backend. THE TRUSTED GATE.
//!
//! Drives the `isabelle` binary to check generated theory source and reports
//! which named lemmas were discharged. Knows nothing about disputes — it runs
//! `.thy` text and parses the result.
//!
//! For each [`Obligation`] we assemble a *complete, self-contained* theory by
//! splicing the obligation's lemma onto the shared `preamble`, write it plus a
//! minimal session `ROOT` into a fresh temp directory, and run
//! `isabelle build -D <dir>`. Each obligation is checked in **isolation** so one
//! malformed lemma can never poison another's verdict, and so a failed proof
//! method on lemma A doesn't abort the build before lemma B is reached.
//!
//! Outcome mapping (keyed on Isabelle's own phrasing, read from stdout+stderr):
//!   * build succeeds                                      -> [`Verdict::Proved`]
//!   * theory is well-formed but the proof method can't
//!     close the goal ("Failed to finish proof", "Failed
//!     to apply initial proof method", "Step error", …)    -> [`Verdict::Unknown`]
//!   * genuine syntax/type error, or the binary itself
//!     could not be run / timed out                        -> [`Verdict::Error`]

use mediator_types::{Obligation, Prover, Verdict};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default location of the Isabelle launcher on this machine.
pub const DEFAULT_ISABELLE_BIN: &str = "/Users/ember/isabelle/Isabelle2025-2.app/bin/isabelle";

/// Per-obligation wall-clock budget. A pathological proof method (e.g. a
/// diverging `auto`) must not be able to wedge the whole suite, so we kill the
/// child after this long and report `Error`. The HOL image is prebuilt, so a
/// healthy run finishes in a few seconds.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Verifier backed by the local Isabelle install.
pub struct IsabelleProver {
    /// Absolute path to the `isabelle` executable.
    pub isabelle_bin: String,
    /// Per-obligation timeout.
    pub timeout: Duration,
    /// Whether to run obligations concurrently (one thread each). Correctness is
    /// identical either way since runs are independent; this only trades CPU for
    /// latency.
    pub parallel: bool,
}

impl IsabelleProver {
    pub fn new(isabelle_bin: impl Into<String>) -> Self {
        Self {
            isabelle_bin: isabelle_bin.into(),
            timeout: DEFAULT_TIMEOUT,
            parallel: true,
        }
    }

    /// Use [`DEFAULT_ISABELLE_BIN`] if it exists on disk, else fall back to the
    /// bare name `isabelle` and hope it is on `PATH`.
    pub fn locate() -> Self {
        if Path::new(DEFAULT_ISABELLE_BIN).exists() {
            Self::new(DEFAULT_ISABELLE_BIN)
        } else {
            Self::new("isabelle")
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_parallel(mut self, parallel: bool) -> Self {
        self.parallel = parallel;
        self
    }

    /// Run a single obligation end to end and classify the result.
    fn run_one(&self, preamble: &str, ob: &Obligation) -> Verdict {
        let theory_name = match theory_name_of(preamble) {
            Some(n) => n,
            None => {
                return Verdict::Error(
                    "preamble does not start with `theory <Name> imports …`".into(),
                )
            }
        };

        let work = match TempDir::fresh() {
            Ok(d) => d,
            Err(e) => return Verdict::Error(format!("could not create temp dir: {e}")),
        };

        // Assemble the complete theory. The preamble runs from
        // `theory <Name> imports Main begin` through every declaration with NO
        // trailing `end`; we add the lemma and the closing `end` ourselves.
        let thy = format!(
            "{preamble}\n\nlemma {name}: \"{goal}\"\n  {proof}\n\nend\n",
            preamble = preamble.trim_end(),
            name = ob.name,
            goal = ob.goal,
            proof = ob.proof,
        );

        let thy_path = work.path().join(format!("{theory_name}.thy"));
        let root_path = work.path().join("ROOT");
        // A unique session name avoids any chance of heap collisions if the user
        // shares an Isabelle home across parallel runs.
        let session = format!("Gate_{}", unique_token());
        let root = format!(
            "session \"{session}\" = \"HOL\" +\n  theories\n    {theory_name}\n",
        );

        if let Err(e) = std::fs::write(&thy_path, &thy) {
            return Verdict::Error(format!("could not write theory: {e}"));
        }
        if let Err(e) = std::fs::write(&root_path, root) {
            return Verdict::Error(format!("could not write ROOT: {e}"));
        }

        match self.invoke_build(work.path()) {
            Ok(out) => classify(&out),
            Err(e) => Verdict::Error(e),
        }
    }

    /// Spawn `isabelle build -D <dir>`, enforce the timeout, and return the
    /// combined stdout+stderr along with success/failure folded into the text
    /// classification (we key on phrasing, not just exit code, because Isabelle
    /// exits non-zero for both "proof failed" and "syntax error").
    fn invoke_build(&self, dir: &Path) -> Result<BuildOutput, String> {
        let mut child = Command::new(&self.isabelle_bin)
            .arg("build")
            .arg("-D")
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Pin Isabelle's user output/heaps inside our temp dir so concurrent
            // runs don't fight over the shared `~/.isabelle` lock files.
            .env("ISABELLE_OUTPUT", dir.join("output"))
            .spawn()
            .map_err(|e| format!("failed to spawn `{}`: {e}", self.isabelle_bin))?;

        // Drain pipes on threads so a full pipe buffer can't deadlock the child.
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let out_handle = thread::spawn(move || drain(&mut stdout_pipe));
        let err_handle = thread::spawn(move || drain(&mut stderr_pipe));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!(
                            "isabelle build exceeded {}s timeout",
                            self.timeout.as_secs()
                        ));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(format!("error waiting on isabelle: {e}")),
            }
        };

        let stdout = out_handle.join().unwrap_or_default();
        let stderr = err_handle.join().unwrap_or_default();
        Ok(BuildOutput {
            success: status.success(),
            combined: format!("{stdout}\n{stderr}"),
        })
    }
}

impl Prover for IsabelleProver {
    fn check(&self, preamble: &str, obligations: &[Obligation]) -> HashMap<String, Verdict> {
        if self.parallel {
            // One thread per obligation. Each run is fully self-contained (own
            // temp dir, own session name) so there is no shared mutable state.
            thread::scope(|scope| {
                let handles: Vec<_> = obligations
                    .iter()
                    .map(|ob| scope.spawn(move || (ob.name.clone(), self.run_one(preamble, ob))))
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("prover thread panicked"))
                    .collect()
            })
        } else {
            obligations
                .iter()
                .map(|ob| (ob.name.clone(), self.run_one(preamble, ob)))
                .collect()
        }
    }
}

struct BuildOutput {
    success: bool,
    combined: String,
}

/// Map Isabelle's build transcript to a [`Verdict`].
///
/// The central distinction is *well-formed-but-unproved* (Unknown) versus
/// *malformed* (Error). When a proof method fails to discharge a real goal,
/// Isabelle has already parsed and type-checked the theory — it emits a
/// recognizable family of "could not close the goal" messages. Anything else
/// that fails is a genuine syntax/type error or a tooling failure.
fn classify(out: &BuildOutput) -> Verdict {
    let text = &out.combined;

    if out.success {
        // Defensive: a healthy build prints "Finished <Session>".
        return Verdict::Proved;
    }

    // Phrases that mean "the theory is fine, the proof method just didn't close
    // the goal." These are the honest `Unknown` cases.
    const UNFINISHED: &[&str] = &[
        "Failed to finish proof",
        "Failed to apply initial proof method",
        "Step error",
        "Local statement fails to refine",
        "Failed to refine any pending goal",
    ];
    if UNFINISHED.iter().any(|p| text.contains(p)) {
        return Verdict::Unknown;
    }

    // Everything else that failed is a real defect in the theory text (or the
    // tooling). Surface the first Isabelle error line so the caller can see why.
    Verdict::Error(extract_error(text))
}

/// Pull the most informative `*** …` error line(s) out of Isabelle's output,
/// falling back to a trimmed tail if none are present.
fn extract_error(text: &str) -> String {
    let stars: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| l.trim_start().starts_with("***"))
        .take(4)
        .collect();
    if !stars.is_empty() {
        return stars.join(" ").replace("***", "").split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let tail: String = text.trim().lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
    if tail.is_empty() {
        "isabelle build failed with no diagnostic output".into()
    } else {
        tail
    }
}

/// Extract the theory name from the first non-blank line of the preamble, which
/// the contract guarantees has the form `theory <Name> imports …`.
fn theory_name_of(preamble: &str) -> Option<String> {
    let first = preamble.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut toks = first.split_whitespace();
    if toks.next()? != "theory" {
        return None;
    }
    let name = toks.next()?;
    if name.is_empty() || name == "imports" {
        None
    } else {
        Some(name.to_string())
    }
}

fn drain(pipe: &mut Option<impl Read>) -> String {
    let mut buf = String::new();
    if let Some(p) = pipe.as_mut() {
        let _ = p.read_to_string(&mut buf);
    }
    buf
}

/// A process-wide-unique token built from pid, a monotonic counter, and the
/// clock — enough to keep concurrent temp dirs and session names distinct.
fn unique_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}_{}_{}", std::process::id(), n, nanos)
}

/// Minimal RAII temp directory (avoids pulling in `tempfile`).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn fresh() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("mediator-prover-{}", unique_token()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prover() -> IsabelleProver {
        // Keep the suite snappy: a real proof here is a couple of seconds.
        IsabelleProver::locate().with_timeout(Duration::from_secs(90))
    }

    const PREAMBLE: &str =
        "theory T imports Main begin\ndefinition x :: int where \"x = 45000\"";

    /// The headline test from the spec: three obligations against one preamble,
    /// hitting REAL Isabelle, exercising each verdict branch.
    #[test]
    fn proved_unknown_and_error() {
        let p = prover();
        let obs = vec![
            Obligation {
                name: "good".into(),
                goal: "x = 45000".into(),
                proof: "by (simp add: x_def)".into(),
            },
            Obligation {
                name: "bad".into(),
                goal: "x = 1".into(),
                proof: "by (simp add: x_def)".into(),
            },
            Obligation {
                name: "oops_lemma".into(),
                // Malformed proposition: a syntax error, not a failed proof.
                goal: "x =".into(),
                proof: "by simp".into(),
            },
        ];

        let verdicts = p.check(PREAMBLE, &obs);

        assert_eq!(verdicts.get("good"), Some(&Verdict::Proved));
        assert_eq!(verdicts.get("bad"), Some(&Verdict::Unknown));
        match verdicts.get("oops_lemma") {
            Some(Verdict::Error(_)) => {}
            other => panic!("expected Error for malformed goal, got {other:?}"),
        }
        assert_eq!(verdicts.len(), 3);
    }

    /// The Keystone ledger reduction itself, checked through the gate — proves
    /// the prover handles multi-definition preambles and a realistic lemma.
    #[test]
    fn keystone_ledger_balances() {
        let p = prover();
        let preamble = "theory Ledger imports Main begin\n\
            definition deposit :: int where \"deposit = 120000\"\n\
            definition cleaning :: int where \"cleaning = 15000\"\n\
            definition carpet :: int where \"carpet = 30000\"\n\
            definition itemized :: int where \"itemized = cleaning + carpet\"\n\
            definition refund :: int where \"refund = deposit - itemized\"";
        let obs = vec![Obligation {
            name: "ledger_balances".into(),
            goal: "refund = 75000".into(),
            proof: "by (simp add: refund_def itemized_def deposit_def cleaning_def carpet_def)"
                .into(),
        }];
        let verdicts = p.check(preamble, &obs);
        assert_eq!(verdicts.get("ledger_balances"), Some(&Verdict::Proved));
    }

    /// Sequential mode must agree with parallel mode (correctness is independent
    /// of the execution strategy).
    #[test]
    fn sequential_matches() {
        let p = prover().with_parallel(false);
        let obs = vec![Obligation {
            name: "good".into(),
            goal: "x = 45000".into(),
            proof: "by (simp add: x_def)".into(),
        }];
        let verdicts = p.check(PREAMBLE, &obs);
        assert_eq!(verdicts.get("good"), Some(&Verdict::Proved));
    }

    #[test]
    fn theory_name_parsing() {
        assert_eq!(
            theory_name_of("theory Keystone imports Main begin\nstuff").as_deref(),
            Some("Keystone")
        );
        assert_eq!(theory_name_of("   \n\ntheory T imports Main").as_deref(), Some("T"));
        assert_eq!(theory_name_of("not a theory"), None);
    }

    #[test]
    fn classify_phrases() {
        let unknown = BuildOutput {
            success: false,
            combined: "*** Failed to finish proof (line 3)\n*** goal (1 subgoal):".into(),
        };
        assert_eq!(classify(&unknown), Verdict::Unknown);

        let unknown2 = BuildOutput {
            success: false,
            combined: "*** Failed to apply initial proof method (line 3):".into(),
        };
        assert_eq!(classify(&unknown2), Verdict::Unknown);

        let err = BuildOutput {
            success: false,
            combined: "*** Inner syntax error (line 3)\n*** Failed to parse prop".into(),
        };
        match classify(&err) {
            Verdict::Error(m) => assert!(m.contains("syntax error"), "msg was {m:?}"),
            other => panic!("expected Error, got {other:?}"),
        }

        let ok = BuildOutput { success: true, combined: "Finished Probe".into() };
        assert_eq!(classify(&ok), Verdict::Proved);
    }
}
