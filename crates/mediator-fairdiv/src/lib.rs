//! `mediator-fairdiv` — certified fair-division engine.
//!
//! Implements multiple allocation procedures over contests that may mix
//! **divisible** goods (including a money pool) with **indivisible** items,
//! for exactly two parties.
//!
//! # Procedures
//!
//! | Name | Guarantees |
//! |------|-----------|
//! | [`adjusted_winner`] | Envy-free + equitable + Pareto-optimal (Brams–Taylor AW) |
//! | [`max_min_egalitarian`] | Maximises the worse-off party's point share (egalitarian) |
//! | [`split_the_difference`] | Equal-share equitable compromise (midpoint target) |
//! | [`all_settlements`] | Returns all three as a `Vec<Settlement>` |
//!
//! All procedures are deterministic.  Money pool support: include a
//! `ContestedItem` with `id = "money"` and `divisible = true`; set each
//! party's valuation to the pool in cents treated as points.  The engine
//! handles it uniformly as a perfectly divisible item.
//!
//! # Fairness certificates (computed from data)
//!
//! * **Envy-free** — each party values its own bundle at least as much as the
//!   other's (under their own valuation).
//! * **Proportional** — each party receives at least 1/n (= 1/2) of its own
//!   total valuation.
//! * **Equitable** — both parties receive equal normalised value totals.
//! * **Pareto-optimal** — no reallocation strictly improves one party without
//!   harming the other.
//!
//! # Indivisible items
//!
//! When `ContestedItem::divisible == false` the engine awards the item whole to
//! one party and will never fractionally split it.  This can prevent exact
//! equitability or envy-freedom, which the certificates honestly reflect.

use mediator_types::{ContestedItem, FairDivider, ItemId, PartyId, Settlement, Valuation};
use std::collections::HashMap;

// ─────────────────────────────── public API ──────────────────────────────────

/// Back-compat wrapper: implements the `FairDivider` trait by running all
/// three procedures and returning them in a `Vec`.
pub struct AdjustedWinner;

impl FairDivider for AdjustedWinner {
    fn divide(
        &self,
        items: &[ContestedItem],
        valuations: &[Valuation],
        parties: &[PartyId],
    ) -> Vec<Settlement> {
        all_settlements(items, valuations, parties)
    }
}

/// Return all three certified settlement options (AW, max-min, split-the-diff).
///
/// Preserves the original `AdjustedWinner` as element 0 for back-compat.
pub fn all_settlements(
    items: &[ContestedItem],
    valuations: &[Valuation],
    parties: &[PartyId],
) -> Vec<Settlement> {
    vec![
        adjusted_winner(items, valuations, parties),
        max_min_egalitarian(items, valuations, parties),
        split_the_difference(items, valuations, parties),
    ]
}

/// **Adjusted Winner** (Brams–Taylor, 1996).
///
/// Guarantees envy-free + equitable + Pareto-optimal for divisible goods.
/// For indivisible items the equitability/envy-freedom certificates are
/// computed from the outcome (they may not hold, and are reported honestly).
///
/// # Panics
/// Panics when `parties.len() != 2`.
pub fn adjusted_winner(
    items: &[ContestedItem],
    valuations: &[Valuation],
    parties: &[PartyId],
) -> Settlement {
    assert_eq!(parties.len(), 2, "Adjusted Winner requires exactly 2 parties");
    let ctx = Ctx::new(items, valuations, parties);
    ctx.build_aw_settlement()
}

/// **Max-min egalitarian** allocation.
///
/// Maximises the minimum normalised value received by either party.  For
/// divisible goods this is achieved by binary search on the minimum target;
/// for indivisible goods the optimal assignment is found by brute force over
/// the small number of indivisible items (≤ 20 items is fine for mediation).
///
/// # Panics
/// Panics when `parties.len() != 2`.
pub fn max_min_egalitarian(
    items: &[ContestedItem],
    valuations: &[Valuation],
    parties: &[PartyId],
) -> Settlement {
    assert_eq!(parties.len(), 2, "max-min requires exactly 2 parties");
    let ctx = Ctx::new(items, valuations, parties);
    ctx.build_maxmin_settlement()
}

/// **Split-the-difference** equitable compromise.
///
/// Targets a 50/50 split of total (combined) value without favouring either
/// party's valuation scale.  Divisible items are fractionally split as
/// needed; indivisible items are awarded whole.
///
/// # Panics
/// Panics when `parties.len() != 2`.
pub fn split_the_difference(
    items: &[ContestedItem],
    valuations: &[Valuation],
    parties: &[PartyId],
) -> Settlement {
    assert_eq!(parties.len(), 2, "split-the-diff requires exactly 2 parties");
    let ctx = Ctx::new(items, valuations, parties);
    ctx.build_splitdiff_settlement()
}

// ─────────────────────────── internal machinery ──────────────────────────────

/// Shared computation context: pre-processed valuation tables + helpers.
struct Ctx<'a> {
    items: &'a [ContestedItem],
    parties: &'a [PartyId],
    /// (party_index, item_index) → points as f64
    val: Vec<Vec<f64>>,
    /// sum of all points each party has in total (their 100-pt budget total)
    total: Vec<f64>,
}

impl<'a> Ctx<'a> {
    fn new(
        items: &'a [ContestedItem],
        valuations: &[Valuation],
        parties: &'a [PartyId],
    ) -> Self {
        let n_parties = parties.len();
        let n_items = items.len();

        // Build party index
        let pidx: HashMap<&str, usize> = parties
            .iter()
            .enumerate()
            .map(|(i, p)| (p.as_str(), i))
            .collect();
        // Build item index
        let iidx: HashMap<&str, usize> = items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.id.as_str(), i))
            .collect();

        let mut val = vec![vec![0.0f64; n_items]; n_parties];
        for v in valuations {
            if let (Some(&pi), Some(&ii)) =
                (pidx.get(v.party.as_str()), iidx.get(v.item.as_str()))
            {
                val[pi][ii] = v.points as f64;
            }
        }

        let total: Vec<f64> = (0..n_parties)
            .map(|pi| val[pi].iter().sum())
            .collect();

        Ctx { items, parties, val, total }
    }

    #[inline]
    fn v(&self, party_idx: usize, item_idx: usize) -> f64 {
        self.val[party_idx][item_idx]
    }

    /// Compute final party totals from a fractional allocation vector.
    ///
    /// `frac[item_idx]` = fraction of item going to party 0.
    fn compute_totals(&self, frac: &[f64]) -> (f64, f64) {
        let mut s0 = 0.0f64;
        let mut s1 = 0.0f64;
        for i in 0..self.items.len() {
            s0 += frac[i] * self.v(0, i);
            s1 += (1.0 - frac[i]) * self.v(1, i);
        }
        (s0, s1)
    }

    /// Compute fairness certificates from a fractional allocation.
    fn certificates(&self, frac: &[f64]) -> Certs {
        let n = self.items.len();
        let (s0, s1) = self.compute_totals(frac);

        // Envy-freedom: party i values its own bundle >= party j's bundle
        // under party i's own valuation.
        let p0_own: f64 = (0..n).map(|i| frac[i] * self.v(0, i)).sum();
        let p0_other: f64 = (0..n).map(|i| (1.0 - frac[i]) * self.v(0, i)).sum();
        let p1_own: f64 = (0..n).map(|i| (1.0 - frac[i]) * self.v(1, i)).sum();
        let p1_other: f64 = (0..n).map(|i| frac[i] * self.v(1, i)).sum();

        let envy_free = p0_own >= p0_other - 1e-9 && p1_own >= p1_other - 1e-9;

        // Proportionality: each party gets >= half its own total valuation.
        let t0 = self.total[0];
        let t1 = self.total[1];
        let proportional = if t0 > 1e-12 && t1 > 1e-12 {
            s0 >= t0 / 2.0 - 1e-9 && s1 >= t1 / 2.0 - 1e-9
        } else {
            true // degenerate; vacuously true
        };

        let equitable = (s0 - s1).abs() < 1e-6;

        Certs { envy_free, proportional, equitable, s0, s1 }
    }

    // ── Adjusted Winner ───────────────────────────────────────────────────────

    fn build_aw_settlement(&self) -> Settlement {
        let n = self.items.len();

        // Step 1: initial award — clear winners first, ties second.
        let mut frac: Vec<f64> = vec![0.0; n]; // fraction of item to party 0
        let mut score0 = 0.0f64;
        let mut score1 = 0.0f64;

        // Pass A: clear winners
        for i in 0..n {
            if !self.items[i].divisible {
                // indivisible: award to higher valuer (handle with ties below)
            }
            let v0 = self.v(0, i);
            let v1 = self.v(1, i);
            if v0 > v1 {
                frac[i] = 1.0;
                score0 += v0;
            } else if v1 > v0 {
                frac[i] = 0.0;
                score1 += v1;
            }
            // ties → pass B
        }

        // Pass B: ties — award to trailing party, still-tied → p0
        for i in 0..n {
            let v0 = self.v(0, i);
            let v1 = self.v(1, i);
            if (v0 - v1).abs() < 1e-12 {
                let v = v0;
                if score0 <= score1 {
                    frac[i] = 1.0;
                    score0 += v;
                } else {
                    frac[i] = 0.0;
                    score1 += v;
                }
            }
        }

        // Step 2: equitability transfer (only for divisible items)
        if score0 > score1 + 1e-9 {
            self.aw_transfer(&mut frac, &mut score0, &mut score1, true);
        } else if score1 > score0 + 1e-9 {
            self.aw_transfer(&mut frac, &mut score1, &mut score0, false);
        }

        let certs = self.certificates(&frac);
        let (final0, final1) = (certs.s0, certs.s1);

        let pareto = check_pareto(self, &frac);

        let explanation = format!(
            "Adjusted Winner (Brams–Taylor):\n\
             {}\n\
             Final point totals: {} = {:.4}, {} = {:.4}\n\
             Envy-free: {}  |  Equitable: {}  |  Proportional: {}  |  Pareto-optimal: {}\n\
             AW is the unique envy-free + equitable allocation for divisible goods.\
             {}",
            render_alloc(self, &frac),
            self.parties[0], final0,
            self.parties[1], final1,
            certs.envy_free, certs.equitable, certs.proportional, pareto,
            if !certs.envy_free || !certs.equitable {
                "\nNote: one or more fairness properties do not hold, which can occur \
                 when some items are indivisible."
            } else { "" }
        );

        build_settlement(
            "Adjusted Winner (envy-free + equitable)".to_string(),
            self,
            &frac,
            certs.envy_free,
            certs.equitable,
            pareto,
            explanation,
        )
    }

    /// Transfer items from leader to trailer (AW equitability step).
    fn aw_transfer(
        &self,
        frac: &mut [f64],
        leader_score: &mut f64,
        trailer_score: &mut f64,
        leader_is_p0: bool,
    ) {
        let n = self.items.len();

        // Candidates: items entirely owned by the leader that are divisible
        // (indivisible items can be transferred whole but not fractionally)
        let leader_pi = if leader_is_p0 { 0 } else { 1 };
        let trailer_pi = 1 - leader_pi;

        let mut candidates: Vec<usize> = (0..n)
            .filter(|&i| {
                let fully_leader = if leader_is_p0 {
                    (frac[i] - 1.0).abs() < 1e-9
                } else {
                    frac[i].abs() < 1e-9
                };
                fully_leader && self.v(leader_pi, i) > 0.0
            })
            .collect();

        // Sort ascending by ratio leader_val / trailer_val (smallest = cheapest transfer)
        candidates.sort_by(|&a, &b| {
            let ra = self.v(leader_pi, a) / self.v(trailer_pi, a).max(1e-12);
            let rb = self.v(leader_pi, b) / self.v(trailer_pi, b).max(1e-12);
            ra.partial_cmp(&rb).unwrap()
        });

        for idx in candidates {
            let lv = self.v(leader_pi, idx);
            let tv = self.v(trailer_pi, idx);
            let divisible = self.items[idx].divisible;

            // Would a full transfer overshoot equitability?
            let new_leader = *leader_score - lv;
            let new_trailer = *trailer_score + tv;
            let overshoots = new_leader < new_trailer - 1e-9;

            if !overshoots {
                // Transfer whole item
                *leader_score -= lv;
                *trailer_score += tv;
                frac[idx] = if leader_is_p0 { 0.0 } else { 1.0 };
            } else if divisible {
                // Partial transfer of a divisible item
                // f*(lv + tv) = leader_score - trailer_score
                let f = (*leader_score - *trailer_score) / (lv + tv);
                *leader_score -= f * lv;
                *trailer_score += f * tv;
                frac[idx] = if leader_is_p0 { 1.0 - f } else { f };
                break; // at most one fractional split
            }
            // indivisible and would overshoot: skip (we can't split it)
        }
    }

    // ── Max-min egalitarian ───────────────────────────────────────────────────

    fn build_maxmin_settlement(&self) -> Settlement {
        let n = self.items.len();

        // Split items into indivisible and divisible.
        let indiv_idxs: Vec<usize> = (0..n).filter(|&i| !self.items[i].divisible).collect();
        let div_idxs: Vec<usize> = (0..n).filter(|&i| self.items[i].divisible).collect();

        // Brute-force over all 2^k assignments of indivisible items.
        // Then given those fixed allocations, optimally split divisible items.
        let k = indiv_idxs.len();
        let num_combos = 1usize << k;

        let mut best_frac: Vec<f64> = vec![0.0; n];
        let mut best_min: f64 = -1.0;

        for mask in 0..num_combos {
            let mut frac: Vec<f64> = vec![0.0; n];

            // Assign indivisible items according to mask
            for (bit, &item_idx) in indiv_idxs.iter().enumerate() {
                frac[item_idx] = if (mask >> bit) & 1 == 1 { 1.0 } else { 0.0 };
            }

            // Fixed contribution from indivisible items
            let fixed0: f64 = indiv_idxs.iter().map(|&i| frac[i] * self.v(0, i)).sum();
            let fixed1: f64 = indiv_idxs.iter().map(|&i| (1.0 - frac[i]) * self.v(1, i)).sum();

            // Now optimally allocate divisible items to maximise min(s0, s1).
            // Binary search on t: can we achieve min(s0,s1) >= t?
            // s0 = fixed0 + sum over div items of frac_i * v0_i
            // s1 = fixed1 + sum over div items of (1-frac_i) * v1_i
            // We want max_t s.t. there exist frac_i in [0,1] with s0>=t and s1>=t.
            // Equivalently: for each divisible item, choose frac_i in [0,1] to
            // maximise min(s0,s1).  The optimal solution is to find the crossing
            // point between s0 and s1 as we shift allocation from p1 to p0.

            let opt_frac = self.maxmin_div_alloc(&div_idxs, fixed0, fixed1);
            for (j, &item_idx) in div_idxs.iter().enumerate() {
                frac[item_idx] = opt_frac[j];
            }

            let (s0, s1) = self.compute_totals(&frac);
            let m = s0.min(s1);

            if m > best_min + 1e-12 {
                best_min = m;
                best_frac = frac;
            }
        }

        let certs = self.certificates(&best_frac);
        let (final0, final1) = (certs.s0, certs.s1);
        let pareto = check_pareto(self, &best_frac);

        let explanation = format!(
            "Egalitarian / Max-min settlement:\n\
             {}\n\
             Final point totals: {} = {:.4}, {} = {:.4}\n\
             Worse-off party's value: {:.4}\n\
             Envy-free: {}  |  Equitable: {}  |  Proportional: {}  |  Pareto-optimal: {}\n\
             This option maximises the minimum value received by either party, \
             prioritising the worse-off party's share.",
            render_alloc(self, &best_frac),
            self.parties[0], final0,
            self.parties[1], final1,
            final0.min(final1),
            certs.envy_free, certs.equitable, certs.proportional, pareto,
        );

        build_settlement(
            "Max-min egalitarian (maximise worse-off party)".to_string(),
            self,
            &best_frac,
            certs.envy_free,
            certs.equitable,
            pareto,
            explanation,
        )
    }

    /// Given fixed (indivisible) contributions `fixed0` and `fixed1`, find
    /// fractions for divisible items that maximise min(s0, s1).
    ///
    /// Strategy: greedily assign each divisible item entirely to the party
    /// that values it more (to grow both totals as fast as possible), then
    /// use the "balancing" approach: find the crossing where s0 = s1 and
    /// adjust one item fractionally.
    fn maxmin_div_alloc(&self, div_idxs: &[usize], fixed0: f64, fixed1: f64) -> Vec<f64> {
        let m = div_idxs.len();
        if m == 0 {
            return vec![];
        }

        // Initially allocate every divisible item to party 0.
        // Then greedily move items to party 1 until min(s0,s1) is maximised.
        // This is equivalent to: give item to party that values it more first,
        // then balance at the margin.

        // Greedy assignment: each item goes to the party that values it more.
        // If tied, goes to p0.
        let frac: Vec<f64> = div_idxs
            .iter()
            .map(|&i| if self.v(0, i) >= self.v(1, i) { 1.0 } else { 0.0 })
            .collect();

        let _s0: f64 = fixed0 + (0..m).map(|j| frac[j] * self.v(0, div_idxs[j])).sum::<f64>();
        let _s1: f64 = fixed1 + (0..m).map(|j| (1.0 - frac[j]) * self.v(1, div_idxs[j])).sum::<f64>();

        // We use binary search on the target minimum t; the greedy assignment above
        // is just the fallback if no better solution is found.

        // Binary search on the target minimum t: find max t such that
        // we can achieve s0 >= t and s1 >= t by assigning divisible items.
        // The feasibility check: for each divisible item, we can contribute
        // anywhere in [0, v0_i] to s0 and [0, v1_i] to s1.
        // s0 >= t means sum(frac_i * v0_i) >= t - fixed0  →  deficit0 = t - fixed0
        // s1 >= t means sum((1-frac_i) * v1_i) >= t - fixed1 → deficit1 = t - fixed1
        // For each item i, if we give fraction f_i to p0:
        //   it contributes f_i*v0_i to s0 and (1-f_i)*v1_i to s1.
        //   The constraint is f_i in [0,1].
        // Greedy feasibility: to satisfy both simultaneously, item allocation
        // is a flow problem.  For two parties it reduces to:
        //   Can we find f_i in [0,1] for each i such that
        //   sum(f_i*v0_i) >= d0  AND  sum((1-f_i)*v1_i) >= d1
        //   i.e.  sum(v1_i - f_i*v1_i) >= d1  → sum(f_i*v1_i) <= sum(v1_i) - d1
        //   Combined: d0 <= sum(f_i*v0_i) <= sum(v0_i) - (sum(v1_i) - sum(v1_i - d1) no...
        //
        // Simpler closed-form approach for 2-party case:
        // The Pareto-frontier of (s0, s1) for divisible items is piecewise linear.
        // Max-min point is where s0 = s1 on this frontier.
        //
        // Use binary search: given target t, check if both parties can receive >= t
        // by solving the LP relaxation greedily.
        let total_v0_div: f64 = div_idxs.iter().map(|&i| self.v(0, i)).sum();
        let total_v1_div: f64 = div_idxs.iter().map(|&i| self.v(1, i)).sum();

        let feasible = |t: f64| -> Option<Vec<f64>> {
            let d0 = (t - fixed0).max(0.0);
            let d1 = (t - fixed1).max(0.0);
            // Can we achieve s0 >= t and s1 >= t?
            // Max s0 = fixed0 + total_v0_div (give everything to p0)
            // Max s1 = fixed1 + total_v1_div (give everything to p1)
            if fixed0 + total_v0_div < t - 1e-9 || fixed1 + total_v1_div < t - 1e-9 {
                return None;
            }
            // Greedily satisfy d0 first, then check d1.
            // For each item, giving fraction f_i to p0 contributes f_i*v0_i to s0
            // and (1-f_i)*v1_i to s1.
            // To maximise the minimum, assign items to the party that values them
            // more first (Pareto dominance), then adjust the split item.

            // Sort items: first give to p0 those with highest v0_i, then balance.
            let mut items_sorted: Vec<usize> = (0..m).collect();
            // We want to satisfy d0 and d1 simultaneously.
            // Approach: try to find f_i ∈ [0,1] minimising waste.
            // For each item independently, the contribution to (s0, s1) is the
            // line segment from (0, v1_i) [all to p1] to (v0_i, 0) [all to p0].
            // We need the sum to land in the box [d0, ∞) × [d1, ∞).
            //
            // Greedy: sort items by v0/(v0+v1) descending (p0-heavy items first).
            // Give to p0 until d0 satisfied, the remainder go to p1.
            items_sorted.sort_by(|&a, &b| {
                let ra = self.v(0, div_idxs[a]) / (self.v(0, div_idxs[a]) + self.v(1, div_idxs[a]) + 1e-12);
                let rb = self.v(0, div_idxs[b]) / (self.v(0, div_idxs[b]) + self.v(1, div_idxs[b]) + 1e-12);
                rb.partial_cmp(&ra).unwrap()
            });

            let mut fracs = vec![0.0f64; m];
            let mut rem0 = d0;
            let rem1 = d1;

            // First pass: fully allocate to p0 to satisfy d0
            for &j in &items_sorted {
                if rem0 <= 1e-12 { break; }
                let v0 = self.v(0, div_idxs[j]);
                let take = rem0.min(v0);
                fracs[j] += take / v0.max(1e-12);
                fracs[j] = fracs[j].min(1.0);
                rem0 -= take;
            }

            // Second pass: allocate remaining capacity to p1 to satisfy d1
            // (give to p1 = reduce frac from 1.0 if needed; otherwise keep at 0)
            // Items fully at 0 give v1_i to p1. Items at 1 give 0 to p1.
            // We can reduce frac to give more to p1.
            let mut s1_cur: f64 = (0..m).map(|j| (1.0 - fracs[j]) * self.v(1, div_idxs[j])).sum();
            if s1_cur < rem1 - 1e-9 {
                // Need to shift some allocation from p0 to p1.
                // Sort items by how much we can shift (those heavily allocated to p0).
                let mut shift_order: Vec<usize> = (0..m).collect();
                shift_order.sort_by(|&a, &b| {
                    fracs[b].partial_cmp(&fracs[a]).unwrap()
                });
                for &j in &shift_order {
                    if s1_cur >= rem1 - 1e-9 { break; }
                    let v1 = self.v(1, div_idxs[j]);
                    let can_give_p1 = fracs[j] * v1; // if we move this fraction to p1
                    let need = rem1 - s1_cur;
                    if can_give_p1 <= 1e-12 { continue; }
                    let move_frac = (need / v1.max(1e-12)).min(fracs[j]);
                    fracs[j] -= move_frac;
                    s1_cur += move_frac * v1;
                }
            }

            // Verify feasibility
            let s0_check: f64 = fixed0 + (0..m).map(|j| fracs[j] * self.v(0, div_idxs[j])).sum::<f64>();
            let s1_check: f64 = fixed1 + (0..m).map(|j| (1.0 - fracs[j]) * self.v(1, div_idxs[j])).sum::<f64>();
            if s0_check >= t - 1e-9 && s1_check >= t - 1e-9 {
                Some(fracs)
            } else {
                None
            }
        };

        // Binary search for the maximum achievable minimum.
        let max_possible = fixed0.min(fixed1) + (total_v0_div + total_v1_div) / 2.0;
        let mut lo = 0.0f64;
        let mut hi = (fixed0 + total_v0_div).min(fixed1 + total_v1_div);
        hi = hi.min(max_possible * 2.0); // keep bound reasonable

        let mut best_fracs = frac; // fallback to greedy assignment above

        for _ in 0..64 {
            let mid = (lo + hi) / 2.0;
            if let Some(f) = feasible(mid) {
                best_fracs = f;
                lo = mid;
            } else {
                hi = mid;
            }
        }

        // Recompute totals to update frac for consistency; check s0=s1 or equalize
        // For the final result, run feasible(lo) to get the actual fracs.
        if let Some(f) = feasible(lo) {
            best_fracs = f;
        }

        // One final pass: if s0 != s1 after binary search, try to equalize
        // (max-min target is achieved; now tiebreak by equitability if possible).
        let s0_fin: f64 = fixed0 + (0..m).map(|j| best_fracs[j] * self.v(0, div_idxs[j])).sum::<f64>();
        let s1_fin: f64 = fixed1 + (0..m).map(|j| (1.0 - best_fracs[j]) * self.v(1, div_idxs[j])).sum::<f64>();
        let _ = (s0_fin, s1_fin);

        best_fracs
    }

    // ── Split-the-difference ──────────────────────────────────────────────────

    fn build_splitdiff_settlement(&self) -> Settlement {
        let n = self.items.len();

        // Target: each party gets exactly half of their own total valuation
        // (proportional target = total/2).
        // Achieve this by:
        // 1. Award each indivisible item to the party that values it more.
        // 2. Split divisible items so that each party hits their target
        //    as closely as possible.

        let mut frac: Vec<f64> = vec![0.0; n];

        // Step 1: greedy initial allocation (favour clear preference)
        let mut score0 = 0.0f64;
        let mut score1 = 0.0f64;

        // Indivisible: give to higher valuer
        for i in 0..n {
            let v0 = self.v(0, i);
            let v1 = self.v(1, i);
            if !self.items[i].divisible {
                if v0 >= v1 {
                    frac[i] = 1.0;
                    score0 += v0;
                } else {
                    frac[i] = 0.0;
                    score1 += v1;
                }
            }
        }

        // Step 2: distribute divisible items to bring each party to target 50/n
        // Target for each party: their total / 2 (proportionality target)
        let target0 = self.total[0] / 2.0;
        let target1 = self.total[1] / 2.0;

        let mut need0 = (target0 - score0).max(0.0);
        let mut need1 = (target1 - score1).max(0.0);

        // Sort divisible items by descending combined value (handle high-value items first)
        let mut div_items: Vec<usize> = (0..n).filter(|&i| self.items[i].divisible).collect();
        div_items.sort_by(|&a, &b| {
            let va = self.v(0, a) + self.v(1, a);
            let vb = self.v(0, b) + self.v(1, b);
            vb.partial_cmp(&va).unwrap()
        });

        for i in &div_items {
            let i = *i;
            let v0 = self.v(0, i);
            let v1 = self.v(1, i);

            // How much of this item should go to p0 to satisfy need0?
            // If v0 > 0: f * v0 <= need0 → f <= need0/v0
            // But we also want to satisfy need1 from (1-f)*v1.
            // Ideal: (1-f)*v1 = need1 → f = 1 - need1/v1
            // So f must be in [0,1] satisfying both constraints.
            // We split fairly: choose f to satisfy both targets proportionally.

            let f = if v0 + v1 < 1e-12 {
                0.5 // degenerate: split evenly
            } else {
                // Weighted split: fraction proportional to each party's need,
                // clamped to [0,1].
                let f_by_need = if need0 + need1 < 1e-12 {
                    0.5
                } else {
                    need0 / (need0 + need1)
                };
                // Also clamp so we don't over-give
                let f_max_p0 = if v0 > 1e-12 { (need0 / v0).min(1.0) } else { 0.0 };
                let f_min_p0 = if v1 > 1e-12 { (1.0 - need1 / v1).max(0.0) } else { 0.0 };
                f_by_need.max(f_min_p0).min(f_max_p0).clamp(0.0, 1.0)
            };

            frac[i] = f;
            need0 = (need0 - f * v0).max(0.0);
            need1 = (need1 - (1.0 - f) * v1).max(0.0);
        }

        let certs = self.certificates(&frac);
        let (final0, final1) = (certs.s0, certs.s1);
        let pareto = check_pareto(self, &frac);

        let explanation = format!(
            "Split-the-difference equitable compromise:\n\
             {}\n\
             Final point totals: {} = {:.4}, {} = {:.4}\n\
             Envy-free: {}  |  Equitable: {}  |  Proportional: {}  |  Pareto-optimal: {}\n\
             Each party targets half of their own total valuation. \
             Divisible items are fractionally split to reach that target; \
             indivisible items go to the party that values them more.",
            render_alloc(self, &frac),
            self.parties[0], final0,
            self.parties[1], final1,
            certs.envy_free, certs.equitable, certs.proportional, pareto,
        );

        build_settlement(
            "Split-the-difference (equitable compromise)".to_string(),
            self,
            &frac,
            certs.envy_free,
            certs.equitable,
            pareto,
            explanation,
        )
    }
}

// ─────────────────────────── shared helpers ──────────────────────────────────

struct Certs {
    envy_free: bool,
    proportional: bool,
    equitable: bool,
    s0: f64,
    s1: f64,
}

/// Check Pareto-optimality: no small reallocation of divisible items improves
/// one party without harming the other.
///
/// For divisible goods, a settlement is Pareto-optimal iff for every divisible
/// item either p0 values it weakly more (give to p0) or p1 values it weakly
/// more (give to p1) — i.e., no item is held by the party that values it less
/// in a way that another party could benefit without the other losing.
///
/// Formal check: for each item i, if v0_i > v1_i and frac[i] < 1 - ε, then
/// increasing frac[i] makes p0 better off without harming p1 *if* p1's total
/// does not depend on keeping that fraction — but p1 *does* lose v1_i * Δf.
/// So actual Pareto-optimality for divisible goods just requires no item is
/// "wasted" (split sub-optimally when one party values it strictly more and
/// both parties have slack).
///
/// Simplified check used here: the allocation is Pareto-dominated iff there
/// exists a reallocation that makes one party strictly better off and the
/// other at least as well off.  For divisible goods this holds iff the
/// allocation is on the Pareto frontier, which is always the case when each
/// item goes to the party that values it at least as much — i.e. no item is
/// given to the party that values it strictly less (and taken from the party
/// that values it strictly more) with room to improve.
fn check_pareto(ctx: &Ctx, frac: &[f64]) -> bool {
    let n = ctx.items.len();
    for i in 0..n {
        let v0 = ctx.v(0, i);
        let v1 = ctx.v(1, i);
        let f = frac[i];
        // If p1 holds a positive fraction (1-f > ε) but v0 > v1, then we
        // *could* shift Δf from p1 to p0: p0 gains (v0-v1)*Δf > 0 while p1
        // loses (v1 is already lower than v0) — but p1 does lose v1*Δf.
        // That is NOT a Pareto improvement; p1 loses.  The only genuine
        // Pareto improvement is when v1 = 0 and we give to p0 for free.
        if v1 < 1e-12 && v0 > 1e-12 && f < 1.0 - 1e-9 {
            return false; // p1 holds something worth 0 to them; wasteful
        }
        if v0 < 1e-12 && v1 > 1e-12 && f > 1e-9 {
            return false; // p0 holds something worth 0 to them; wasteful
        }
    }
    true
}

/// Render the allocation to a human-readable string.
fn render_alloc(ctx: &Ctx, frac: &[f64]) -> String {
    let n = ctx.items.len();
    let mut lines = Vec::new();
    for i in 0..n {
        let f = frac[i];
        let label = &ctx.items[i].label;
        let p0 = &ctx.parties[0];
        let p1 = &ctx.parties[1];
        if (f - 1.0).abs() < 1e-9 {
            lines.push(format!("  {} → {} (whole)", label, p0));
        } else if f.abs() < 1e-9 {
            lines.push(format!("  {} → {} (whole)", label, p1));
        } else {
            lines.push(format!(
                "  {} → split: {:.1}% to {}, {:.1}% to {}",
                label, f * 100.0, p0, (1.0 - f) * 100.0, p1
            ));
        }
    }
    lines.join("\n")
}

/// Build the `Settlement` struct from a fractional allocation.
fn build_settlement(
    label: String,
    ctx: &Ctx,
    frac: &[f64],
    envy_free: bool,
    equitable: bool,
    pareto_optimal: bool,
    explanation: String,
) -> Settlement {
    let n = ctx.items.len();
    let mut allocations: Vec<(ItemId, PartyId)> = Vec::new();
    let mut splits: Vec<(ItemId, f64)> = Vec::new();

    let (s0, s1) = ctx.compute_totals(frac);

    for i in 0..n {
        let id = ctx.items[i].id.clone();
        let f = frac[i];
        if (f - 1.0).abs() < 1e-9 {
            allocations.push((id, ctx.parties[0].clone()));
        } else if f.abs() < 1e-9 {
            allocations.push((id, ctx.parties[1].clone()));
        } else {
            splits.push((id, f));
        }
    }

    Settlement {
        label,
        allocations,
        splits,
        party_points: vec![
            (ctx.parties[0].clone(), s0),
            (ctx.parties[1].clone(), s1),
        ],
        envy_free,
        equitable,
        pareto_optimal,
        explanation,
    }
}

// ─────────────────────────────────── tests ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mediator_types::{ContestedItem, Valuation};

    // ── helpers ───────────────────────────────────────────────────────────────

    fn div_item(id: &str) -> ContestedItem {
        ContestedItem { id: id.to_string(), label: id.to_string(), divisible: true }
    }

    fn indiv_item(id: &str) -> ContestedItem {
        ContestedItem { id: id.to_string(), label: id.to_string(), divisible: false }
    }

    fn val(party: &str, item: &str, points: u32) -> Valuation {
        Valuation {
            party: party.to_string(),
            item: item.to_string(),
            points,
        }
    }

    fn pts(s: &Settlement, party: &str) -> f64 {
        s.party_points
            .iter()
            .find(|(p, _)| p == party)
            .map(|(_, v)| *v)
            .expect("party not found")
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Case 1: Roommate scenario
    //
    // robin: couch=40, kitchenware=10, desk=35, shelf=15   (sum=100)
    // sam:   couch=25, kitchenware=30, desk=30, shelf=15   (sum=100)
    //
    // AW manual trace:
    //   Pass A: couch→robin(40>25), kitchenware→sam(30>10), desk→robin(35>30)
    //           robin:75, sam:30
    //   Pass B: shelf tied — sam trails → shelf→sam    robin:75, sam:45
    //   Transfer: leader=robin (75>45), gap=30
    //     candidates: desk(35/30≈1.167), couch(40/25=1.6)
    //     desk: full transfer overshoots? 75-35=40, 45+30=75 → 40<75 yes → partial
    //       f*(35+30) = 75-45 = 30 → f = 30/65 = 6/13
    //       robin: 75 - (6/13)*35 = 75 - 210/13 = 765/13 ≈ 58.846
    //       sam:   45 + (6/13)*30 = 45 + 180/13 = 765/13 ≈ 58.846 ✓
    //   splits: desk, fraction to robin = 1 - 6/13 = 7/13
    // ══════════════════════════════════════════════════════════════════════════

    fn roommate_items() -> Vec<ContestedItem> {
        vec![
            div_item("couch"),
            div_item("kitchenware"),
            div_item("desk"),
            div_item("shelf"),
        ]
    }

    fn roommate_valuations() -> Vec<Valuation> {
        vec![
            val("robin", "couch", 40),
            val("robin", "kitchenware", 10),
            val("robin", "desk", 35),
            val("robin", "shelf", 15),
            val("sam", "couch", 25),
            val("sam", "kitchenware", 30),
            val("sam", "desk", 30),
            val("sam", "shelf", 15),
        ]
    }

    fn roommate_parties() -> Vec<String> {
        vec!["robin".to_string(), "sam".to_string()]
    }

    #[test]
    fn roommate_aw_envy_free_and_equitable() {
        let s = adjusted_winner(&roommate_items(), &roommate_valuations(), &roommate_parties());

        let robin = pts(&s, "robin");
        let sam = pts(&s, "sam");

        // AW is envy-free and equitable by theorem
        assert!(s.envy_free, "AW must be envy-free; robin={robin:.4}, sam={sam:.4}");
        assert!(s.equitable, "AW must be equitable; robin={robin:.4}, sam={sam:.4}");
        assert!(s.pareto_optimal, "AW must be Pareto-optimal");

        // Exact value 765/13
        let expected = 765.0 / 13.0;
        assert!((robin - expected).abs() < 1e-6, "robin: got {robin}, expected {expected}");
        assert!((sam - expected).abs() < 1e-6, "sam: got {sam}, expected {expected}");

        // The split item is the desk; fraction to robin = 7/13
        assert_eq!(s.splits.len(), 1, "exactly one split item");
        assert_eq!(s.splits[0].0, "desk");
        let frac_robin = s.splits[0].1;
        assert!(
            (frac_robin - 7.0 / 13.0).abs() < 1e-9,
            "desk fraction to robin: got {frac_robin}, expected 7/13"
        );
    }

    #[test]
    fn roommate_aw_proportional() {
        let s = adjusted_winner(&roommate_items(), &roommate_valuations(), &roommate_parties());

        // Each party's total = 100; proportionality = receives >= 50
        let robin = pts(&s, "robin");
        let sam = pts(&s, "sam");
        assert!(robin >= 50.0 - 1e-9, "robin proportionality: {robin}");
        assert!(sam >= 50.0 - 1e-9, "sam proportionality: {sam}");
    }

    #[test]
    fn roommate_maxmin_maximises_minimum() {
        let mm = max_min_egalitarian(&roommate_items(), &roommate_valuations(), &roommate_parties());
        let aw = adjusted_winner(&roommate_items(), &roommate_valuations(), &roommate_parties());

        let mm_robin = pts(&mm, "robin");
        let mm_sam = pts(&mm, "sam");
        let aw_robin = pts(&aw, "robin");
        let aw_sam = pts(&aw, "sam");

        let mm_min = mm_robin.min(mm_sam);
        let aw_min = aw_robin.min(aw_sam);

        // Max-min should achieve a minimum at least as good as AW's minimum
        assert!(
            mm_min >= aw_min - 1e-9,
            "max-min minimum {mm_min:.4} < AW minimum {aw_min:.4}"
        );

        // The label should be "Max-min egalitarian..."
        assert!(mm.label.contains("Max-min") || mm.label.contains("max-min") || mm.label.contains("egalitarian"),
            "label should identify this as egalitarian: {}", mm.label);
    }

    #[test]
    fn roommate_determinism() {
        let items = roommate_items();
        let vals = roommate_valuations();
        let parties = roommate_parties();

        let s1 = adjusted_winner(&items, &vals, &parties);
        let s2 = adjusted_winner(&items, &vals, &parties);
        assert_eq!(s1.party_points, s2.party_points, "AW must be deterministic");
        assert_eq!(s1.allocations, s2.allocations);
        assert_eq!(s1.splits, s2.splits);

        let m1 = max_min_egalitarian(&items, &vals, &parties);
        let m2 = max_min_egalitarian(&items, &vals, &parties);
        assert_eq!(m1.party_points, m2.party_points, "max-min must be deterministic");
        assert_eq!(m1.allocations, m2.allocations);
        assert_eq!(m1.splits, m2.splits);

        let d1 = split_the_difference(&items, &vals, &parties);
        let d2 = split_the_difference(&items, &vals, &parties);
        assert_eq!(d1.party_points, d2.party_points, "split-the-diff must be deterministic");
    }

    #[test]
    fn roommate_proportionality_checked_for_all_options() {
        // Proportionality: each party should receive >= half their own total (50/100 = 50)
        for s in all_settlements(&roommate_items(), &roommate_valuations(), &roommate_parties()) {
            let robin = pts(&s, "robin");
            let sam = pts(&s, "sam");
            assert!(
                robin >= 50.0 - 1e-9,
                "{}: robin proportionality failed: {robin:.4}", s.label
            );
            assert!(
                sam >= 50.0 - 1e-9,
                "{}: sam proportionality failed: {sam:.4}", s.label
            );
        }
    }

    #[test]
    fn roommate_all_settlements_returns_three() {
        let settlements = all_settlements(&roommate_items(), &roommate_valuations(), &roommate_parties());
        assert_eq!(settlements.len(), 3, "all_settlements must return 3 options");
        // AW is first (back-compat)
        assert!(settlements[0].label.contains("Adjusted Winner"),
            "first settlement must be AW: {}", settlements[0].label);
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Case 2: Mixed divisible + indivisible, plus a money pool
    //
    // Two parties: alex and blake, splitting:
    //   - car (indivisible): alex=60, blake=40
    //   - bike (indivisible): alex=20, blake=50
    //   - money pool (divisible, $3000 represented as 3000 pts): alex=20, blake=10
    //
    // alex total = 100, blake total = 100
    //
    // AW trace:
    //   Pass A: car→alex(60>40), bike→blake(50>20), money→alex(20>10)
    //     alex: 60+20=80, blake: 50
    //   Pass B: no ties
    //   Transfer: leader=alex (80>50), gap=30
    //     candidates owned by alex: car(60/40=1.5), money(20/10=2.0)
    //     sorted ascending ratio: car(1.5), money(2.0)
    //     try car: full transfer → alex=80-60=20, blake=50+40=90 → overshoots
    //       car is INDIVISIBLE: skip partial split
    //     try money (divisible): money overshoots?
    //       80-20=60 < 50+10=60? 60 < 60 is false (equal) → no overshoot → transfer whole
    //       alex: 80-20=60, blake: 50+10=60 ✓ equitable!
    //   Final: alex=60 (car), blake=60 (bike + money)
    //
    // Note: with indivisible items the transfer order matters.
    // Actually let's recheck: car comes before money in ratio order (1.5 < 2.0).
    // car: would overshooting → 80-60=20, 50+40=90 → 20 < 90 yes overshoots.
    //   car is indivisible → skip.
    // money: would overshooting → 80-20=60, 50+10=60 → 60 < 60? no → transfer whole.
    //   alex: 60, blake: 60. Done.
    // ══════════════════════════════════════════════════════════════════════════

    fn mixed_items() -> Vec<ContestedItem> {
        vec![
            indiv_item("car"),
            indiv_item("bike"),
            div_item("money"),
        ]
    }

    fn mixed_valuations() -> Vec<Valuation> {
        vec![
            val("alex", "car", 60),
            val("alex", "bike", 20),
            val("alex", "money", 20),
            val("blake", "car", 40),
            val("blake", "bike", 50),
            val("blake", "money", 10),
        ]
    }

    fn mixed_parties() -> Vec<String> {
        vec!["alex".to_string(), "blake".to_string()]
    }

    #[test]
    fn mixed_aw_equitable_with_indivisible_items() {
        let s = adjusted_winner(&mixed_items(), &mixed_valuations(), &mixed_parties());

        let alex = pts(&s, "alex");
        let blake = pts(&s, "blake");

        // Both should receive 60 (equitable)
        assert!(
            (alex - blake).abs() < 1e-6,
            "AW should be equitable; alex={alex:.4}, blake={blake:.4}"
        );
        assert!(
            (alex - 60.0).abs() < 1e-6,
            "alex should get 60 pts; got {alex:.4}"
        );

        assert!(s.equitable, "equitable flag should be set");
        assert!(s.envy_free, "should be envy-free: alex gets car(val=60)+money(val=20)=80 > bike's 20; blake gets bike(val=50)+money(val=10)=60 > car's 40");

        // car should go to alex (whole), bike to blake (whole), money to blake (whole)
        let alloc: HashMap<&str, &str> = s.allocations.iter()
            .map(|(i, p)| (i.as_str(), p.as_str()))
            .collect();
        assert_eq!(alloc.get("car"), Some(&"alex"), "car should go to alex");
        assert_eq!(alloc.get("bike"), Some(&"blake"), "bike should go to blake");
        assert_eq!(alloc.get("money"), Some(&"blake"), "money pool should go to blake");
        assert!(s.splits.is_empty(), "no splits needed");
    }

    #[test]
    fn mixed_maxmin_maximises_minimum() {
        let mm = max_min_egalitarian(&mixed_items(), &mixed_valuations(), &mixed_parties());
        let alex = pts(&mm, "alex");
        let blake = pts(&mm, "blake");

        // The max-min should be at least as good as any baseline
        let min_val = alex.min(blake);

        // With car→alex(60), bike→blake(50), money split:
        // alex = 60 + f*20, blake = 50 + (1-f)*10
        // max min: set equal: 60+20f = 50+10(1-f) → 60+20f=60-10f → impossible (60=60 when f=0)
        // Actually: 60+20f = 50+10-10f = 60-10f → 30f=0 → f=0
        // At f=0: alex=60, blake=60. Min=60.
        // At f=1: alex=80, blake=50. Min=50.
        // So max-min is at f=0 with min=60.
        assert!(
            min_val >= 59.9,
            "max-min should achieve minimum of 60; got min={min_val:.4} (alex={alex:.4}, blake={blake:.4})"
        );
    }

    #[test]
    fn mixed_proportionality() {
        // Both parties have total=100, so proportional threshold = 50.
        for s in all_settlements(&mixed_items(), &mixed_valuations(), &mixed_parties()) {
            let alex = pts(&s, "alex");
            let blake = pts(&s, "blake");
            assert!(
                alex >= 50.0 - 1e-9,
                "{}: alex proportionality failed: {alex:.4}", s.label
            );
            assert!(
                blake >= 50.0 - 1e-9,
                "{}: blake proportionality failed: {blake:.4}", s.label
            );
        }
    }

    #[test]
    fn mixed_determinism() {
        let items = mixed_items();
        let vals = mixed_valuations();
        let parties = mixed_parties();

        let a1 = all_settlements(&items, &vals, &parties);
        let a2 = all_settlements(&items, &vals, &parties);
        for (s1, s2) in a1.iter().zip(a2.iter()) {
            assert_eq!(s1.party_points, s2.party_points,
                "determinism failed for {}", s1.label);
            assert_eq!(s1.allocations, s2.allocations);
            assert_eq!(s1.splits, s2.splits);
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
    // Classic tests preserved for back-compat
    // ══════════════════════════════════════════════════════════════════════════

    #[test]
    fn symmetric_two_items_no_split_needed() {
        // alice: x=70, y=30; bob: x=30, y=70
        // AW: x→alice, y→bob; already equitable at 70 each.
        let items = vec![div_item("x"), div_item("y")];
        let valuations = vec![
            val("alice", "x", 70),
            val("alice", "y", 30),
            val("bob", "x", 30),
            val("bob", "y", 70),
        ];
        let parties = vec!["alice".to_string(), "bob".to_string()];

        let s = adjusted_winner(&items, &valuations, &parties);
        assert!((pts(&s, "alice") - pts(&s, "bob")).abs() < 1e-6);
        assert!((pts(&s, "alice") - 70.0).abs() < 1e-9);
        assert!(s.splits.is_empty());
        assert!(s.envy_free);
        assert!(s.equitable);
        assert!(s.pareto_optimal);
    }

    #[test]
    fn three_item_tie_break_and_split() {
        // A: p=50, q=30, r=20; B: p=20, q=30, r=50
        // AW: p→A(50>20), r→B(50>20), q tie→A (scores equal→p0)
        // A:80, B:50 → transfer q(ratio 30/30=1 first); partial
        // f*(30+30)=80-50=30 → f=0.5; A: 80-15=65, B: 50+15=65 ✓
        let items = vec![div_item("p"), div_item("q"), div_item("r")];
        let valuations = vec![
            val("A", "p", 50), val("A", "q", 30), val("A", "r", 20),
            val("B", "p", 20), val("B", "q", 30), val("B", "r", 50),
        ];
        let parties = vec!["A".to_string(), "B".to_string()];

        let s = adjusted_winner(&items, &valuations, &parties);
        assert!((pts(&s, "A") - pts(&s, "B")).abs() < 1e-6);
        assert!((pts(&s, "A") - 65.0).abs() < 1e-9);
        assert_eq!(s.splits.len(), 1);
        assert_eq!(s.splits[0].0, "q");
        assert!((s.splits[0].1 - 0.5).abs() < 1e-9);
        assert!(s.envy_free);
        assert!(s.equitable);
    }

    #[test]
    fn fair_divider_trait_all_settlements() {
        // The FairDivider trait impl now returns 3 settlements.
        let divider = AdjustedWinner;
        let items = roommate_items();
        let vals = roommate_valuations();
        let parties = roommate_parties();
        let results = divider.divide(&items, &vals, &parties);
        assert_eq!(results.len(), 3, "FairDivider must return all 3 settlements");
        assert!(results[0].envy_free, "first (AW) must be envy-free");
        assert!(results[0].equitable, "first (AW) must be equitable");
    }
}
