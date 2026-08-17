//! Per-session planning: sides, budget walk, dedup, spanned boundaries.

use super::*;

/// Which side(s) of a turn a selection takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelSides {
    Both,
    UserOnly,
    AssistantOnly,
}

// ─────────────────────────────────────────────────────────────────────────────
// Budget allocation (2-phase)
// ─────────────────────────────────────────────────────────────────────────────

/// A selection decision for one turn (which sides were chosen).
#[derive(Debug, Clone)]
pub(crate) struct Selected {
    pub(crate) turn_index: usize,
    pub(crate) sides: SelSides,
}

/// The per-session plan: the selected turns (already sorted ascending for render) +
/// accounting for the header.
#[derive(Debug)]
pub(crate) struct SessionPlan {
    pub(crate) selected: Vec<Selected>,
    /// The dedup-flagged + max-compaction-filtered turns the plan selected FROM. The
    /// renderer reads units (incl. the `also_in_summary` flag) from HERE — never from the
    /// un-flagged `ScanResult.turns` — so the dedup demote-flag reaches the output.
    pub(crate) turns: Vec<TurnSlice>,
    pub(crate) spanned_boundaries: usize,
    pub(crate) rendered_chars: usize,
    /// The newest summary line (if any) — for the dedup-note + banners.
    pub(crate) newest_summary_line: Option<usize>,
    pub(crate) dedup_demoted: usize,
}

/// Plan one session: dedup-flag turns against the newest summary, then run the 2-phase
/// recency-first budget allocation, then sort ascending for render.
pub(crate) fn plan_session(
    sr: &ScanResult,
    budget: usize,
    rt_fraction: f64,
    max_compactions: usize,
    cfg: &RichnessCfg,
) -> SessionPlan {
    // ── Dedup-flag (mutate a working copy of the turns) ──
    let newest = sr.summaries.iter().max_by_key(|s| s.line_no);
    let newest_summary_line = newest.map(|s| s.line_no);
    let mut turns: Vec<TurnSlice> = sr.turns.clone();
    let mut dedup_demoted = 0usize;
    if let Some(summary) = newest {
        for t in &mut turns {
            // Dedup is keyed on the live (compactions_before == 0) region primarily —
            // turns predating an OLDER boundary are genuinely gone from context and are
            // pure restoration, never deduped.
            if t.compactions_before != 0 {
                continue;
            }
            // Dedup keys on the SAME two anchors as before the expansion: the user
            // opener and the EOT (last) agent message. Middle agent messages are not
            // deduped (a summary never quotes them verbatim), so demote-flag scope is
            // unchanged. Borrow each anchor separately (the accessor borrows the slice).
            if let Some(u) = t.user.as_mut() {
                if unit_matches_summary(u, &summary.fingerprints) {
                    u.also_in_summary = true;
                    dedup_demoted += 1;
                }
            }
            if let Some(a) = t.assistant_eot_mut() {
                if unit_matches_summary(a, &summary.fingerprints) {
                    a.also_in_summary = true;
                    dedup_demoted += 1;
                }
            }
        }
    }

    // ── Apply --max-compactions: drop turns beyond the cap (0 = unlimited) ──
    if max_compactions > 0 {
        turns.retain(|t| t.compactions_before <= max_compactions);
    }

    // ── Reserve the document HEADER BLOCK up front (a fixed framing the render always
    // emits to stdout) so the chars left for the reconstruction body — boundary banners +
    // selected units — fit `available`. The header block is bounded by a provable
    // worst-case (§ doc_header_block_max_chars); the BANNERS are NOT pre-reserved in bulk
    // (that wasted ~½ a small budget on summary-heavy sessions) but charged INCREMENTALLY
    // as selection deepens the spanned count, so the banner budget is exact. Invariant
    // held below: `doc_header(real ≤ reservation) + banners(spanned) + units ≤ budget`. ──
    let doc_header_reservation = doc_header_block_max_chars(sr, budget);
    let available = budget.saturating_sub(doc_header_reservation);

    // Recency-first order = descending line_no of the turn's latest unit. Ties broken
    // by descending turn_index for determinism.
    let mut order: Vec<usize> = (0..turns.len()).collect();
    order.sort_by(|&a, &b| {
        turn_latest_line(&turns[b])
            .cmp(&turn_latest_line(&turns[a]))
            .then(turns[b].turn_index.cmp(&turns[a].turn_index))
    });

    // The round-trip reservation bounds UNIT chars (the back-and-forth content), computed
    // off `available` (post-header-block); banners are charged on top via `committed`.
    let rt_budget = ((available as f64) * rt_fraction).round() as usize;
    // `spent_units` = unit chars only; `spanned_depth` = max compactions_before among
    // chosen turns (drives the banner charge). The total committed against `available` is
    // `spent_units + cumulative_banner_cost(summaries, spanned_depth)`.
    let mut spent_units = 0usize;
    let mut spanned_depth = 0usize;
    let mut chosen: Vec<Option<SelSides>> = vec![None; turns.len()];

    // ── Phase 1: ROUND-TRIP GUARANTEE — spend rt_budget (unit chars) on complete pairs,
    // while the banner charge for the deepened span still keeps the WHOLE doc ≤ budget. ──
    // Non-dup complete turns first, then dup complete turns (demote, don't drop).
    for dedup_pass in [false, true] {
        for &ti in &order {
            if chosen[ti].is_some() {
                continue;
            }
            let t = &turns[ti];
            // The HARD FLOOR reserves its lane for HUMAN round-trips only — a machine
            // automation pulse is left for Phase-2 fill, so the protected budget the help
            // documents for "user → … → assistant EOT" is never silently spent on a
            // pulse→ack pair (which the header already reports as an automation trigger).
            if !t.is_human_round_trip() {
                continue;
            }
            if turn_is_dup(t) != dedup_pass {
                continue;
            }
            let c = turn_cost(t, SelSides::Both, cfg);
            let new_depth = spanned_depth.max(t.compactions_before);
            let banners = cumulative_banner_cost(&sr.summaries, new_depth);
            let unit_fits_rt = spent_units + c <= rt_budget;
            let doc_fits = spent_units + c + banners <= available;
            if unit_fits_rt && doc_fits {
                chosen[ti] = Some(SelSides::Both);
                spent_units += c;
                spanned_depth = new_depth;
            } else if spent_units == 0 && !dedup_pass && doc_fits {
                // The first (most-recent, non-dup) complete turn exceeds the round-trip
                // reservation but the WHOLE document (its cost + its banners + header
                // block) still fits `budget`: include it anyway — the most-recent exchange
                // is load-bearing and already ellipsis-capped by the role caps. Stop
                // Phase 1; Phase 2 fills the remainder. A round-trip that does NOT satisfy
                // `doc_fits` is left for Phase 2 to take a cheaper single side, so the
                // ≤-budget guarantee is never broken.
                chosen[ti] = Some(SelSides::Both);
                spent_units += c;
                spanned_depth = new_depth;
                break;
            }
            // else: skip (leave for Phase 2 to maybe pick a cheaper single side).
        }
    }

    // ── Phase 2: FILL — spend the rest of `available` (incl. unused rt reservation). ──
    for dedup_pass in [false, true] {
        for &ti in &order {
            if chosen[ti].is_some() {
                continue;
            }
            let t = &turns[ti];
            if turn_is_dup(t) != dedup_pass {
                continue;
            }
            // Prefer a complete turn if it fits; else the user side first (scarcer,
            // higher-signal loss), then the assistant side.
            let candidates: &[SelSides] = if t.is_round_trip() {
                &[SelSides::Both, SelSides::UserOnly, SelSides::AssistantOnly]
            } else if t.user.is_some() {
                &[SelSides::UserOnly]
            } else if !t.agents.is_empty() {
                &[SelSides::AssistantOnly]
            } else {
                &[]
            };
            for &sides in candidates {
                let c = turn_cost(t, sides, cfg);
                let new_depth = spanned_depth.max(t.compactions_before);
                let banners = cumulative_banner_cost(&sr.summaries, new_depth);
                if spent_units + c + banners <= available {
                    chosen[ti] = Some(sides);
                    spent_units += c;
                    spanned_depth = new_depth;
                    break;
                }
            }
        }
    }

    // ── Assemble selected set, ascending for render ──
    let mut selected: Vec<Selected> = Vec::new();
    for (ti, sel) in chosen.iter().enumerate() {
        if let Some(sides) = sel {
            selected.push(Selected {
                turn_index: turns[ti].turn_index,
                sides: *sides,
            });
        }
    }
    selected.sort_by_key(|s| s.turn_index);

    // Boundaries spanned = the GREATEST `compactions_before` among selected turns: the
    // oldest selected turn sits behind that many summaries, so the ascending render
    // crosses exactly that many compaction boundaries on the way to EOF.
    let spanned = spanned_boundary_count(&turns, &selected);

    // Report the conservative upper bound the selection enforced:
    //   doc_header_reservation (≥ the real header block) + banners(spanned) + unit chars.
    // Because selection guaranteed `spent_units + banners(spanned) <= available =
    // budget - doc_header_reservation`, this figure is ≤ budget by construction, and it is
    // ≥ the true emitted length (the real header block ≤ its reservation), so the header
    // line never UNDER-states the cost. `spanned == spanned_depth` here. An EMPTY selection
    // emits NO document at all (the renderer skips empty sessions), so it reports 0 — the
    // header-block reservation is only "spent" when a document is actually written.
    let rendered_chars = if selected.is_empty() {
        0
    } else {
        doc_header_reservation + cumulative_banner_cost(&sr.summaries, spanned) + spent_units
    };

    SessionPlan {
        selected,
        turns,
        spanned_boundaries: spanned,
        rendered_chars,
        newest_summary_line,
        dedup_demoted,
    }
}

/// The latest jsonl line a turn touches (for recency ordering): the max of its user
/// opener line and its LATEST agent message line (the EOT anchor == `agents.last()`,
/// which is the highest agent line by construction), 0 if neither.
pub(crate) fn turn_latest_line(t: &TurnSlice) -> usize {
    // A pending elicitation-sidecar unit (§3.10) has no physical line (line_no 0) yet IS the
    // latest activity — what the session is currently blocked on — so it ranks as most-recent
    // (usize::MAX) for recency-first selection rather than sorting as the oldest.
    if t.user.as_ref().is_some_and(|u| u.from_sidecar) {
        return usize::MAX;
    }
    let u = t.user.as_ref().map(|x| x.line_no).unwrap_or(0);
    let a = t.assistant_eot().map(|x| x.line_no).unwrap_or(0);
    u.max(a)
}

/// True when EITHER dedup anchor of a turn is flagged (the user opener or the EOT agent
/// message — the only two sides dedup ever marks).
pub(crate) fn turn_is_dup(t: &TurnSlice) -> bool {
    t.user.as_ref().is_some_and(|u| u.also_in_summary)
        || t.assistant_eot().is_some_and(|a| a.also_in_summary)
}

/// True when a unit's fingerprint is a prefix-or-equal match of any summary fingerprint
/// (§6.2). The match is symmetric-prefix: a unit clipped to 80 chars matches a summary
/// bullet that begins with the same 80 chars, and vice-versa.
pub(crate) fn unit_matches_summary(unit: &TurnUnit, summary_fps: &[String]) -> bool {
    let unit_fp = fingerprint(&unit.text);
    if unit_fp.is_empty() {
        return false;
    }
    summary_fps.iter().any(|sfp| {
        !sfp.is_empty() && (unit_fp.starts_with(sfp.as_str()) || sfp.starts_with(unit_fp.as_str()))
    })
}

/// The number of compaction boundaries the selected turns span = the GREATEST
/// `compactions_before` among selected turns. The oldest selected turn sits behind that
/// many summaries, so the ascending render crosses exactly that many boundaries reaching
/// EOF. 0 when every selected turn is in the live (post-newest-summary) region.
pub(crate) fn spanned_boundary_count(turns: &[TurnSlice], selected: &[Selected]) -> usize {
    selected
        .iter()
        .filter_map(|s| turns.iter().find(|t| t.turn_index == s.turn_index))
        .map(|t| t.compactions_before)
        .max()
        .unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Window-range parsing (shared parser in `crate::text`)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a `--turn` token into a [`RangeSpec`] (the shared grammar), resolved per-session
/// against that transcript's own turn count (0-based).
pub(crate) fn parse_turn_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--turn", false)
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────────────────────
