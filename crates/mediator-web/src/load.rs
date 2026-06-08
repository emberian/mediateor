//! Discovering and loading disputes from `scenarios/`.
//!
//! The app prefers the precomputed `<name>.analysis.json` cache (instant, no
//! Isabelle needed). If a cache is missing it *may* compute one by driving the
//! kernel — but a dispute with neither cache nor a working prover is skipped
//! gracefully rather than crashing the gallery.

use std::path::{Path, PathBuf};

use mediator_types::{Analysis, Dispute, Receipt};
use serde::Deserialize;

/// A fully loaded dispute: the source `Dispute` plus the certified `Analysis`
/// and receipt chain.
pub struct DisputeRecord {
    pub id: String,
    pub dispute: Dispute,
    pub analysis: Analysis,
    pub receipts: Vec<Receipt>,
}

/// The on-disk cache shape written by `mediator-demo … --write-cache`.
#[derive(Deserialize)]
struct CacheFile {
    analysis: Analysis,
    #[serde(default)]
    receipts: Vec<Receipt>,
}

/// Locate the `scenarios/` directory. Prefers `MEDIATEOR_SCENARIOS`, then a
/// path relative to the crate (compile-time `CARGO_MANIFEST_DIR`), then cwd.
pub fn scenarios_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MEDIATEOR_SCENARIOS") {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            return pb;
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest.join("../../scenarios");
    if candidate.is_dir() {
        return candidate;
    }
    PathBuf::from("scenarios")
}

/// Discover every `scenarios/*.json` (excluding the `.analysis.json` sidecars)
/// and load each into a [`DisputeRecord`]. Disputes that can't be loaded (bad
/// JSON, or no cache and no way to compute one) are skipped with a warning so
/// one broken file never takes the gallery down.
pub fn discover_disputes(dir: &Path) -> Vec<DisputeRecord> {
    let mut records = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("⚠  scenarios dir not found: {}", dir.display());
        return records;
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".json") && !name.ends_with(".analysis.json")
        })
        .collect();
    paths.sort();

    for path in paths {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("dispute")
            .to_string();
        match load_record(&id, &path) {
            Ok(rec) => records.push(rec),
            Err(e) => eprintln!("⚠  skipping scenario {id}: {e}"),
        }
    }
    records
}

/// Load one dispute by id from its scenario path, preferring the sibling cache.
pub fn load_record(id: &str, scenario_path: &Path) -> anyhow::Result<DisputeRecord> {
    let raw = std::fs::read_to_string(scenario_path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", scenario_path.display()))?;
    let dispute: Dispute = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", scenario_path.display()))?;

    let cache_path = sibling_cache(scenario_path);
    if cache_path.exists() {
        let craw = std::fs::read_to_string(&cache_path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", cache_path.display()))?;
        let cache: CacheFile = serde_json::from_str(&craw)
            .map_err(|e| anyhow::anyhow!("parsing {}: {e}", cache_path.display()))?;
        return Ok(DisputeRecord {
            id: id.to_string(),
            dispute,
            analysis: cache.analysis,
            receipts: cache.receipts,
        });
    }

    // No cache. Compute one if the prover is available, else surface a clear
    // error so the caller can skip this dispute gracefully.
    compute_record(id, dispute)
}

/// Compute an analysis by driving the kernel (Isabelle). Slow; the cache is
/// strongly preferred. Used only when a scenario ships without its sidecar.
fn compute_record(id: &str, dispute: Dispute) -> anyhow::Result<DisputeRecord> {
    eprintln!("ℹ  no cache for {id}; computing via Isabelle (this is slow)…");
    let prover = mediator_prover::IsabelleProver::locate();
    let fair = mediator_fairdiv::AdjustedWinner;
    let (analysis, receipts) = mediator_core::analyze(&dispute, &prover, &fair);
    Ok(DisputeRecord {
        id: id.to_string(),
        dispute,
        analysis,
        receipts,
    })
}

/// `scenarios/roommate.json` → `scenarios/roommate.analysis.json`.
fn sibling_cache(scenario_path: &Path) -> PathBuf {
    let stem = scenario_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dispute");
    scenario_path.with_file_name(format!("{stem}.analysis.json"))
}
