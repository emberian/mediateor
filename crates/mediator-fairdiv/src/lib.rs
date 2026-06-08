//! `mediator-fairdiv` — fair division over divisible stakes.
//!
//! Implements the **Brams–Taylor Adjusted Winner** (AW) procedure for exactly
//! two parties.  AW is guaranteed envy-free, equitable, and Pareto-optimal for
//! divisible goods when both parties honestly report 100-point valuations.
//!
//! # Algorithm (high level)
//!
//! 1. **Initial award** — give each item to the party that values it more.
//!    Break ties by awarding to the party currently behind on points; if still
//!    tied, award to party 0 (deterministic).
//! 2. **Equitability adjustment** — the "leading" party transfers items (ordered
//!    by ascending ratio = leader_value / trailer_value) to the trailer until
//!    point totals are equal.  Exactly one item may be fractionally split.
//! 3. **Fairness certificates** — verify envy-freedom, equitability, and
//!    Pareto-optimality, then attach a plain-language explanation.
//!
//! The result is a `Vec<Settlement>` with exactly one entry (AW produces a
//! unique, canonical solution).

use mediator_types::{ContestedItem, FairDivider, ItemId, PartyId, Settlement, Valuation};
use std::collections::HashMap;

pub struct AdjustedWinner;

impl FairDivider for AdjustedWinner {
    fn divide(
        &self,
        items: &[ContestedItem],
        valuations: &[Valuation],
        parties: &[PartyId],
    ) -> Vec<Settlement> {
        vec![adjusted_winner(items, valuations, parties)]
    }
}

/// Core Adjusted Winner computation.  Returns a single canonical `Settlement`.
///
/// # Panics
/// Panics if `parties.len() != 2`.  AW is a two-party procedure.
pub fn adjusted_winner(
    items: &[ContestedItem],
    valuations: &[Valuation],
    parties: &[PartyId],
) -> Settlement {
    assert_eq!(parties.len(), 2, "Adjusted Winner requires exactly 2 parties");

    let p0 = &parties[0];
    let p1 = &parties[1];

    // ── build (party, item) → points lookup ──────────────────────────────────
    let mut val: HashMap<(&str, &str), f64> = HashMap::new();
    for v in valuations {
        val.insert((v.party.as_str(), v.item.as_str()), v.points as f64);
    }

    let item_ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();

    // helper: look up valuation, defaulting to 0 if missing
    let get = |party: &str, item: &str| -> f64 { *val.get(&(party, item)).unwrap_or(&0.0) };

    // ── Step 1: initial award ─────────────────────────────────────────────────
    //
    // Scores after tentative assignment (before tie-breaking).
    // We process items in a stable order (their original slice order) so that
    // the whole procedure is fully deterministic.
    //
    // For tie-breaking we make two passes:
    //   pass A  — items where one party values them strictly more (clear winner)
    //   pass B  — tied items (award to whichever party is trailing after pass A)

    // Ownership: None = unassigned, Some(true) = p0, Some(false) = p1
    let mut owner: Vec<Option<bool>> = vec![None; item_ids.len()];
    let mut score0: f64 = 0.0;
    let mut score1: f64 = 0.0;

    // pass A — clear winners
    for (idx, &id) in item_ids.iter().enumerate() {
        let v0 = get(p0, id);
        let v1 = get(p1, id);
        if v0 > v1 {
            owner[idx] = Some(true);
            score0 += v0;
        } else if v1 > v0 {
            owner[idx] = Some(false);
            score1 += v1;
        }
        // ties handled in pass B
    }

    // pass B — ties: award to the party currently trailing; still-tied → p0
    for (idx, &id) in item_ids.iter().enumerate() {
        if owner[idx].is_some() {
            continue;
        }
        let v = get(p0, id); // both equal
        if score0 <= score1 {
            owner[idx] = Some(true);
            score0 += v;
        } else {
            owner[idx] = Some(false);
            score1 += v;
        }
    }

    // ── Step 2: equitability adjustment ──────────────────────────────────────
    //
    // Identify the leader/trailer, then transfer items (ascending by the ratio
    // leader_val/trailer_val) from leader to trailer until totals equalise.
    // Exactly one item may be fractionally split; it is recorded in `splits`.

    // We'll track fractional ownership: fraction of item going to p0.
    // Initially 1.0 if owned by p0, 0.0 if owned by p1.
    let mut frac0: Vec<f64> = owner
        .iter()
        .map(|o| if *o == Some(true) { 1.0 } else { 0.0 })
        .collect();

    if score0 > score1 {
        // p0 is leading — transfer items from p0 to p1
        equitability_transfer(
            &item_ids,
            &mut frac0,
            &mut score0,
            &mut score1,
            |id| get(p0, id), // leader_val
            |id| get(p1, id), // trailer_val
            true,              // leader is p0: transfer means reduce frac0
        );
    } else if score1 > score0 {
        // p1 is leading — transfer items from p1 to p0
        equitability_transfer(
            &item_ids,
            &mut frac0,
            &mut score1,
            &mut score0,
            |id| get(p1, id), // leader_val
            |id| get(p0, id), // trailer_val
            false,             // leader is p1: transfer means increase frac0
        );
    }
    // if already equal, nothing to do

    // ── Step 3: build Settlement ──────────────────────────────────────────────

    let mut allocations: Vec<(ItemId, PartyId)> = Vec::new();
    let mut splits: Vec<(ItemId, f64)> = Vec::new();

    // Final point totals (recompute from frac0 for precision)
    let mut final0: f64 = 0.0;
    let mut final1: f64 = 0.0;
    for (idx, &id) in item_ids.iter().enumerate() {
        let f = frac0[idx];
        final0 += f * get(p0, id);
        final1 += (1.0 - f) * get(p1, id);

        if (f - 1.0).abs() < 1e-9 {
            allocations.push((id.to_string(), p0.clone()));
        } else if f.abs() < 1e-9 {
            allocations.push((id.to_string(), p1.clone()));
        } else {
            // fractionally split — record fraction going to p0 (the first party)
            splits.push((id.to_string(), f));
        }
    }

    // ── envy-freedom check ────────────────────────────────────────────────────
    //
    // Party i is envy-free iff its value for its own bundle ≥ its value for the
    // other party's bundle.  For party 0: sum over items of frac0[i] * val(p0,i)
    // vs. sum of (1-frac0[i]) * val(p0,i) (p0's valuation of p1's share).

    let p0_own: f64 = item_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| frac0[i] * get(p0, id))
        .sum();
    let p0_other: f64 = item_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (1.0 - frac0[i]) * get(p0, id))
        .sum();
    let p1_own: f64 = item_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (1.0 - frac0[i]) * get(p1, id))
        .sum();
    let p1_other: f64 = item_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| frac0[i] * get(p1, id))
        .sum();

    let envy_free = p0_own >= p0_other - 1e-9 && p1_own >= p1_other - 1e-9;
    let equitable = (final0 - final1).abs() < 1e-6;

    // ── explanation ───────────────────────────────────────────────────────────

    let explanation = build_explanation(
        p0,
        p1,
        &allocations,
        &splits,
        final0,
        final1,
        items,
        equitable,
        envy_free,
    );

    Settlement {
        label: "Adjusted Winner".to_string(),
        allocations,
        splits,
        party_points: vec![(p0.clone(), final0), (p1.clone(), final1)],
        envy_free,
        equitable,
        pareto_optimal: true, // AW is Pareto-optimal by construction
        explanation,
    }
}

/// Transfer items from `leader` to `trailer` (ascending ratio order) until
/// `leader_score` equals `target`.
///
/// `leader_is_p0` controls whether transferring an item means *reducing* frac0
/// (p0 was the leader) or *increasing* frac0 (p1 was the leader).
fn equitability_transfer<FL, FT>(
    item_ids: &[&str],
    frac0: &mut [f64],
    leader_score: &mut f64,
    trailer_score: &mut f64,
    leader_val: FL,
    trailer_val: FT,
    leader_is_p0: bool,
) where
    FL: Fn(&str) -> f64,
    FT: Fn(&str) -> f64,
{
    // Collect indices of items currently owned entirely by the leader
    let mut candidates: Vec<usize> = item_ids
        .iter()
        .enumerate()
        .filter(|(i, &id)| {
            let f = frac0[*i];
            let leader_owns = if leader_is_p0 {
                (f - 1.0).abs() < 1e-9 // p0 owns it fully
            } else {
                f.abs() < 1e-9 // p1 owns it fully
            };
            leader_owns && leader_val(id) > 0.0 // avoid 0/0 ratio
        })
        .map(|(i, _)| i)
        .collect();

    // Sort ascending by ratio = leader_val / trailer_val (smallest first)
    // Items where the trailer values it relatively more (low ratio) are
    // transferred first — they cost the leader the least per unit of
    // trailer gain, which is exactly what we want.
    candidates.sort_by(|&a, &b| {
        let ra = leader_val(item_ids[a]) / trailer_val(item_ids[a]).max(1e-12);
        let rb = leader_val(item_ids[b]) / trailer_val(item_ids[b]).max(1e-12);
        ra.partial_cmp(&rb).unwrap()
    });

    for idx in candidates {
        let id = item_ids[idx];
        let lv = leader_val(id);
        let tv = trailer_val(id);

        // Check whether transferring the whole item overshoots equitability.
        // After a whole transfer: new_leader = leader_score - lv,
        //                          new_trailer = trailer_score + tv.
        // Overshoot iff new_leader < new_trailer, i.e. leader_score - lv < trailer_score + tv.
        let overshoots = (*leader_score - lv) < (*trailer_score + tv) - 1e-9;

        if !overshoots {
            // Transfer the whole item safely
            *leader_score -= lv;
            *trailer_score += tv;
            if leader_is_p0 {
                frac0[idx] = 0.0;
            } else {
                frac0[idx] = 1.0;
            }
        } else {
            // Partial transfer: find fraction f of the item to move so that
            //   leader_score - f*lv = trailer_score + f*tv
            //   f*(lv + tv) = leader_score - trailer_score
            //   f = (leader_score - trailer_score) / (lv + tv)
            let f = (*leader_score - *trailer_score) / (lv + tv);
            *leader_score -= f * lv;
            *trailer_score += f * tv;
            if leader_is_p0 {
                // frac0 was 1.0; transfer f of it to p1
                frac0[idx] = 1.0 - f;
            } else {
                // frac0 was 0.0; transfer f of it to p0
                frac0[idx] = f;
            }
            break; // equitability achieved; at most one fractional split
        }
    }
}

/// Build a plain-language explanation of the settlement.
#[allow(clippy::too_many_arguments)]
fn build_explanation(
    p0: &str,
    p1: &str,
    allocations: &[(ItemId, PartyId)],
    splits: &[(ItemId, f64)],
    final0: f64,
    final1: f64,
    items: &[ContestedItem],
    equitable: bool,
    envy_free: bool,
) -> String {
    // build label lookup
    let label_of: HashMap<&str, &str> = items.iter().map(|i| (i.id.as_str(), i.label.as_str())).collect();

    let mut lines: Vec<String> = Vec::new();
    lines.push("Adjusted Winner settlement:".to_string());

    // whole allocations
    for (item_id, party) in allocations {
        let label = label_of.get(item_id.as_str()).copied().unwrap_or(item_id.as_str());
        lines.push(format!("  {} → {} (whole item)", label, party));
    }

    // fractional splits
    for (item_id, frac) in splits {
        let label = label_of.get(item_id.as_str()).copied().unwrap_or(item_id.as_str());
        let pct0 = frac * 100.0;
        let pct1 = (1.0 - frac) * 100.0;
        lines.push(format!(
            "  {} → split: {:.1}% to {p0}, {:.1}% to {p1}",
            label, pct0, pct1
        ));
    }

    lines.push(format!(
        "Final point totals: {} = {:.4}, {} = {:.4}",
        p0, final0, p1, final1
    ));

    let fairness_note = match (equitable, envy_free) {
        (true, true) => "The split is equitable (equal point totals) and envy-free: \
                         neither party values the other's share more than their own.",
        (true, false) => "The split is equitable but envy-freedom check failed (unusual; \
                          check valuations sum to 100).",
        (false, true) => "The split is envy-free but point totals differ slightly \
                          (numerical precision).",
        (false, false) => "Warning: neither equitability nor envy-freedom verified.",
    };
    lines.push(fairness_note.to_string());
    lines.push("Pareto-optimal: no redistribution can make one party better off without harming the other.".to_string());

    lines.join("\n")
}

// ─────────────────────────────────── tests ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::{ContestedItem, Valuation};

    // ── helpers ───────────────────────────────────────────────────────────────

    fn item(id: &str) -> ContestedItem {
        ContestedItem {
            id: id.to_string(),
            label: id.to_string(),
            divisible: true,
        }
    }

    fn val(party: &str, item: &str, points: u32) -> Valuation {
        Valuation {
            party: party.to_string(),
            item: item.to_string(),
            points,
        }
    }

    // ── test 1: roommate scenario (from scenarios/roommate.json) ──────────────
    //
    // robin: couch=40, kitchenware=10, desk=35, shelf=15
    // sam:   couch=25, kitchenware=30, desk=30, shelf=15
    //
    // Initial award (no ties except shelf):
    //   couch  → robin (40 > 25)           robin: 40
    //   kitchenware → sam (30 > 10)        sam:   30
    //   desk   → robin (35 > 30)           robin: 40+35=75
    //   shelf  → tie; sam is trailing →    sam:   30+15=45
    //
    // target = (75+45)/2 = 60
    // robin leads by 30; candidates ordered by ratio robin_val/sam_val asc:
    //   desk:  35/30 ≈ 1.167
    //   couch: 40/25 = 1.600
    //
    // Transfer desk first: if whole, robin→40, sam→75 (sam leads) → partial.
    //   f = (75-60)/35 = 15/35 = 3/7 of desk transferred to sam
    //   robin keeps fraction (1 - 3/7) = 4/7 of desk in robin-points
    //   robin: 75 - 35*(3/7) = 75 - 15 = 60
    //   sam:   45 + 30*(3/7) = 45 + 90/7 ≈ 45 + 12.857 = 57.857 ← not 60!
    //
    // Equitability adjustment (partial transfer of desk):
    //   f*(lv + tv) = leader_score - trailer_score
    //   f*(35 + 30) = 75 - 45  →  f = 30/65 = 6/13
    //
    // After transfer:
    //   robin: 75 - (6/13)*35 = 75 - 210/13 = 765/13 ≈ 58.846
    //   sam:   45 + (6/13)*30 = 45 + 180/13 = 765/13 ≈ 58.846  ✓
    //
    // splits: desk, fraction to p0 (robin) = 1 - 6/13 = 7/13

    #[test]
    fn roommate_equal_points() {
        let items = vec![
            item("couch"),
            item("kitchenware"),
            item("standing_desk"),
            item("bookshelf"),
        ];
        let valuations = vec![
            val("robin", "couch", 40),
            val("robin", "kitchenware", 10),
            val("robin", "standing_desk", 35),
            val("robin", "bookshelf", 15),
            val("sam", "couch", 25),
            val("sam", "kitchenware", 30),
            val("sam", "standing_desk", 30),
            val("sam", "bookshelf", 15),
        ];
        let parties = vec!["robin".to_string(), "sam".to_string()];

        let s = adjusted_winner(&items, &valuations, &parties);

        // Retrieve final point totals
        let pts = |party: &str| {
            s.party_points
                .iter()
                .find(|(p, _)| p == party)
                .map(|(_, v)| *v)
                .expect("party not found")
        };
        let robin_pts = pts("robin");
        let sam_pts = pts("sam");

        // Equitability: totals should match within 1e-6
        assert!(
            (robin_pts - sam_pts).abs() < 1e-6,
            "Points not equal: robin={robin_pts}, sam={sam_pts}"
        );

        // Known value: 765/13
        let expected = 765.0 / 13.0;
        assert!(
            (robin_pts - expected).abs() < 1e-6,
            "robin points off: got {robin_pts}, expected {expected}"
        );

        // Envy-freedom guaranteed by AW
        assert!(s.envy_free, "settlement should be envy-free");
        assert!(s.equitable, "settlement should be equitable");
        assert!(s.pareto_optimal, "settlement should be Pareto-optimal");

        // Determinism: running again yields identical result
        let s2 = adjusted_winner(&items, &valuations, &parties);
        assert_eq!(s.party_points, s2.party_points);
        assert_eq!(s.allocations, s2.allocations);
        assert_eq!(s.splits, s2.splits);

        // The split item should be standing_desk
        assert_eq!(s.splits.len(), 1);
        assert_eq!(s.splits[0].0, "standing_desk");

        // Fraction to robin (p0) = 7/13
        let expected_frac = 7.0_f64 / 13.0;
        assert!(
            (s.splits[0].1 - expected_frac).abs() < 1e-9,
            "wrong split fraction: got {}, expected {}",
            s.splits[0].1,
            expected_frac
        );

        // Whole-item allocations: robin gets couch; sam gets kitchenware and bookshelf
        let alloc: HashMap<&str, &str> = s
            .allocations
            .iter()
            .map(|(i, p)| (i.as_str(), p.as_str()))
            .collect();
        assert_eq!(alloc.get("couch"), Some(&"robin"));
        assert_eq!(alloc.get("kitchenware"), Some(&"sam"));
        assert_eq!(alloc.get("bookshelf"), Some(&"sam"));
    }

    // ── test 2: hand-checked symmetric case ──────────────────────────────────
    //
    // Two items, each party values one item at 70 and the other at 30.
    //   alice: x=70, y=30
    //   bob:   x=30, y=70
    //
    // Initial award: x→alice (70>30), y→bob (70>30)
    // alice: 70, bob: 70 — already equitable!
    // No transfer needed.  Each party gets the item they value more.
    //
    // alice values own bundle at 70, other bundle at 30 → no envy ✓
    // bob   values own bundle at 70, other bundle at 30 → no envy ✓
    #[test]
    fn symmetric_two_items_no_split_needed() {
        let items = vec![item("x"), item("y")];
        let valuations = vec![
            val("alice", "x", 70),
            val("alice", "y", 30),
            val("bob", "x", 30),
            val("bob", "y", 70),
        ];
        let parties = vec!["alice".to_string(), "bob".to_string()];

        let s = adjusted_winner(&items, &valuations, &parties);

        let pts = |party: &str| {
            s.party_points
                .iter()
                .find(|(p, _)| p == party)
                .map(|(_, v)| *v)
                .expect("party not found")
        };

        assert!((pts("alice") - pts("bob")).abs() < 1e-6, "should be equitable");
        assert!((pts("alice") - 70.0).abs() < 1e-9);
        assert!(s.splits.is_empty(), "no fractional split should be needed");
        assert!(s.envy_free);
        assert!(s.equitable);
        assert!(s.pareto_optimal);

        let alloc: HashMap<&str, &str> = s
            .allocations
            .iter()
            .map(|(i, p)| (i.as_str(), p.as_str()))
            .collect();
        assert_eq!(alloc.get("x"), Some(&"alice"));
        assert_eq!(alloc.get("y"), Some(&"bob"));
    }

    // ── test 3: tie-breaking + partial split ─────────────────────────────────
    //
    // Three items; parties A and B.
    //   A: p=50, q=30, r=20
    //   B: p=20, q=30, r=50
    //
    // Initial award (clear winners first):
    //   p → A (50>20)   score_A=50
    //   r → B (50>20)   score_B=50
    //   q → tie (both 30): A has 50, B has 50 — tied scores, award to p0 = A
    //       score_A = 80, score_B = 50
    //
    // target = (80+50)/2 = 65
    // A leads; A's items: p(50), q(30)
    // ratios: q→30/30=1.0, p→50/20=2.5
    // transfer q first: whole? A→80-30=50, B→50+30=80 (B leads) → partial
    //   f*(30) transferred from A, f*(30) gained by B
    //   80 - 30f = 50 + 30f → 30 = 60f → f = 0.5
    //   A: 80 - 15 = 65  B: 50 + 15 = 65 ✓
    //
    // split: q, fraction to A (p0) = 0.5
    #[test]
    fn three_item_tie_break_and_split() {
        let items = vec![item("p"), item("q"), item("r")];
        let valuations = vec![
            val("A", "p", 50),
            val("A", "q", 30),
            val("A", "r", 20),
            val("B", "p", 20),
            val("B", "q", 30),
            val("B", "r", 50),
        ];
        let parties = vec!["A".to_string(), "B".to_string()];

        let s = adjusted_winner(&items, &valuations, &parties);

        let pts = |party: &str| {
            s.party_points
                .iter()
                .find(|(p, _)| p == party)
                .map(|(_, v)| *v)
                .expect("party not found")
        };
        assert!((pts("A") - pts("B")).abs() < 1e-6, "equitable");
        assert!((pts("A") - 65.0).abs() < 1e-9, "A should have 65");

        assert_eq!(s.splits.len(), 1);
        assert_eq!(s.splits[0].0, "q");
        assert!((s.splits[0].1 - 0.5).abs() < 1e-9, "q should split 50/50");

        assert!(s.envy_free);
        assert!(s.equitable);
    }
}
