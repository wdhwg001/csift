//! Text rendering: document body, slices, boundary banners, turn text.

use super::*;

/// Everything the renderers need beyond the per-session scans + plans.
#[derive(Debug)]
pub(crate) struct RenderCtx {
    pub(crate) budget_chars: usize,
    pub(crate) rt_fraction: f64,
    pub(crate) skipped_lines: usize,
    /// The richness configuration - the renderer walks the same `select_agent_messages`
    /// survivor set the plan budgeted, so emitted == costed.
    pub(crate) cfg: RichnessCfg,
}

/// Look up the dedup-flagged `TurnSlice` for a selected turn index within the PLAN's
/// turns (NOT `ScanResult.turns`, which is un-flagged) so the renderer sees the
/// `also_in_summary` flag the plan set.
pub(crate) fn find_turn(plan: &SessionPlan, turn_index: usize) -> Option<&TurnSlice> {
    plan.turns.iter().find(|t| t.turn_index == turn_index)
}

pub(crate) fn render_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    out_path: Option<&Path>,
    slice: Option<usize>,
    window: usize,
    slices: Option<usize>,
) -> Result<()> {
    // ── Chunked-output mode (--slice): emit ONLY one ≤window-char chunk of the verbatim DOCUMENT
    // (the SAME body `--out` writes), with NO operational chrome - so a SessionStart hook can
    // inject it under the 10,000-char additionalContext cap. Two sub-modes:
    //
    //   • LEGACY (`--slice` alone): budget-driven. The document is whatever `--budget` selected,
    //     paginated into a VARIABLE number of chunks; `--slice i` emits the i-th. Concatenating
    //     1..K reproduces the document byte-for-byte. The per-role 600/900 body caps apply.
    //   • FIXED-FLEET (`--slices N`): the slice COUNT is the hard constraint (a fixed set of hooks
    //     can't grow). Bodies render whole up to one window - a turn is ellipsized ONLY if it
    //     ALONE exceeds a window - and only the NEWEST N chunks are kept; the oldest overflow is
    //     DISCARDED. So the emitted count is ALWAYS ≤N regardless of turn size. slice 1 = oldest
    //     KEPT, slice N = newest.
    //
    // An out-of-range index prints nothing (exit 0), so surplus hooks simply inject nothing. ──
    if let Some(n) = slice {
        // Fixed-fleet drops the per-role caps for a window cap (whole turns; ellipsize only a turn
        // bigger than a window). Legacy keeps the role caps (cap_override = None).
        let cap_override = slices.map(|_| window.saturating_sub(SLICE_BODY_HEADROOM).max(1));
        let doc = build_document_body(sessions, plans, &ctx.cfg, cap_override);
        let chunks = slice_into_windows(&doc, window);
        let idx = match slices {
            Some(n_slices) => {
                if n > n_slices {
                    return Ok(()); // index outside the fixed fleet → inject nothing
                }
                // Keep the newest n_slices chunks; drop the oldest (len - n_slices) overflow so
                // the count never exceeds the fleet. slice 1 maps to the oldest KEPT chunk.
                chunks.len().saturating_sub(n_slices) + (n - 1)
            }
            None => n - 1,
        };
        if let Some(chunk) = chunks.into_iter().nth(idx) {
            print!("{chunk}");
        }
        return Ok(());
    }

    let mut first = true;
    let mut any = false;
    let mut out_blob = String::new();

    // Fan-out scope banner. The banner reports the TRUE scope (EVERY discovered session,
    // split top-level/subagent) and - separately - how many rendered WITHIN budget, so the
    // budget value can never silently rewrite "scope" and a targeted top-level uuid can never
    // read as `0 top-level`. Printed whenever more than one session is in scope OR some
    // in-scope session was skipped by the budget; a lone session that rendered cleanly stays
    // silent (the common single-thread recovery case, zero added noise).
    let sc = scope_summary(sessions, plans);
    let any_skipped = sc.rendered < sc.in_scope;
    if sc.in_scope > 1 || any_skipped {
        // Reuse the shared `N session(s) in scope (X top-level + Y subagent)` wording (the same
        // fragment list/files/search/recover emit), then append turns' own budget clause.
        println!(
            "scope  {} · {} rendered within budget · budget {} chars is PER session → up to {} \
             chars total",
            crate::text::scope_span_fragment(sc.in_scope_top, sc.in_scope_sub),
            sc.rendered,
            ctx.budget_chars,
            ctx.budget_chars.saturating_mul(sc.rendered.max(1))
        );
        println!();
    }

    // A TARGETED top-level session that has restorable content but does NOT fit the budget
    // must be reported explicitly - never silently absent while unrelated subagents fill
    // stdout. Emit a per-session skip note (top-level sessions only; a skipped subagent is
    // fan-out noise the user did not ask for) carrying the budget it would need. A GENUINELY
    // EMPTY session (no restorable turns at all → `min_render_chars` is None) is left to the
    // terminal "no turns selected (empty session set …)" fallback - that case is already
    // honest and not a budget problem. `skipped_any` tracks only the budget-too-small notes,
    // separate from `any`, so the fallback still keys on whether a real block rendered.
    let mut skipped_any = false;
    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() && !sr.is_subagent {
            if let Some(min) = min_render_chars(sr, ctx.budget_chars, &ctx.cfg) {
                println!(
                    "SESSION {}  skipped — its first round-trip needs ≥ {} chars; \
                     raise --budget (now {})",
                    sr.session_id, min, ctx.budget_chars
                );
                skipped_any = true;
            }
        }
    }
    if skipped_any {
        println!();
    }

    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        any = true;
        if !first {
            println!();
        }
        first = false;

        let (n_user, n_asst) = count_sides(plan, &ctx.cfg);
        let n_automation = count_automation(plan);
        // Brand a spanned SUBAGENT block with the SAME shape every other session-emitting
        // surface uses (`list`/`files`/`search`): `SUBAGENT <hex>  ·  parent SESSION <uuid>`
        // - never token a bare non-re-feedable subagent hex as `SESSION` (the id-domain
        // overload r6 removed elsewhere), and surface the re-feedable parent uuid inline so a
        // turns-text reader has a re-feed path. A top-level uuid block stays `SESSION <uuid>`.
        if sr.is_subagent {
            println!(
                "SUBAGENT {}  ·  parent SESSION {}  (subagent transcript)",
                sr.session_id, sr.parent_session_id
            );
        } else {
            println!("SESSION {}", sr.session_id);
        }
        // `spanned K of N`: K = boundaries the budget-selected window crossed (a QUERY
        // property - a small budget can read 0 on a compaction-heavy session), N = the
        // session's true total in scope (the TRANSCRIPT property). Naming both kills the
        // R10 misread where `spanned 0` on a 4-boundary session looked like a bug until the
        // reader varied the budget (same disambiguation pattern as the automation
        // `in scope (not all selected)` note below).
        println!(
            "  budget {} chars · round-trip-fraction {:.2} · spanned {} of {} compaction boundaries in scope",
            ctx.budget_chars,
            ctx.rt_fraction,
            plan.spanned_boundaries,
            sr.summaries.len()
        );
        // Header automation note carries a PER-CLASS breakdown, not just the lumped scalar,
        // so a reader sees the composition (`2 background-command, 1 agent`) without scanning
        // every `[kind …]` label line in the body.
        let automation_note = if n_automation > 0 {
            let breakdown =
                automation_breakdown_text(&automation_by_kind(std::slice::from_ref(plan)));
            format!(
                " ({n_automation} automation trigger{}: {breakdown})",
                if n_automation == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        };
        println!(
            "  selected {} user{} + {} assistant units across {} turns · {} / {} chars used",
            n_user,
            automation_note,
            n_asst,
            plan.selected.len(),
            plan.rendered_chars,
            ctx.budget_chars
        );
        // Whole-session automation composition, INDEPENDENT of budget selection - so a
        // monitor-heavy session isn't silently read as "no automation" when the recency
        // window selected none of its deep pulses. Shown only when MORE automation exists in
        // scope than was selected (otherwise the selected note above already tells the truth).
        let in_scope_by = automation_in_scope_by_kind(std::slice::from_ref(plan));
        let in_scope_total: usize = in_scope_by.iter().sum();
        if in_scope_total > n_automation {
            println!(
                "  in scope (not all selected): {} automation trigger{} — {}",
                in_scope_total,
                if in_scope_total == 1 { "" } else { "s" },
                automation_breakdown_text(&in_scope_by)
            );
        }
        if let (Some(sline), true) = (plan.newest_summary_line, plan.dedup_demoted > 0) {
            println!(
                "  dedup: {} units also present in summary L{} (demoted, flagged)",
                plan.dedup_demoted, sline
            );
        }
        // Announce that ≥1 selected unit is a hook-backfilled elicitation-sidecar record (§3.10)
        // - the consumer is reading merged records, not raw native jsonl.
        if plan_has_sidecar(plan) {
            println!("  with elicitation sidecar");
        }
        println!("  {}", "─".repeat(60));

        // Walk the ascending selected set, inserting boundary banners as
        // `compactions_before` decreases toward EOF.
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            maybe_boundary_banner(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |s| {
                    println!("{s}");
                    out_blob.push_str(&s);
                    out_blob.push('\n');
                },
            );
            render_turn_text(turn, sel.sides, &ctx.cfg, None, &mut |s| {
                println!("{s}");
                out_blob.push_str(&s);
                out_blob.push('\n');
            });
        }
    }

    // The terminal fallback fires only when NOTHING rendered AND no per-session skip note
    // already explained why (a skip note is the more specific, actionable message).
    if !any && !skipped_any {
        println!("no turns selected (empty session set or budget too small)");
    }
    if ctx.skipped_lines > 0 {
        println!();
        println!("({})", crate::text::malformed_note(ctx.skipped_lines));
    }
    if let Some(p) = out_path {
        if crate::recover::write_out_guarded(p, &out_blob)? {
            println!();
            println!("(wrote full reconstruction to {})", p.display());
        }
    }
    Ok(())
}

/// Build the verbatim DOCUMENT body (boundary banners + selected turn units) for every
/// in-scope session, with NO operational chrome. Byte-for-byte identical to the `out_blob`
/// that `render_text` accumulates for `--out` (same emit path: `maybe_boundary_banner` +
/// `render_turn_text`, each line followed by `\n`), so a `--slice` reconstruction and an
/// `--out` file carry the same content. Sessions concatenate with no separator (mirrors
/// `out_blob`); a `--slice` run is almost always a single top-level thread anyway.
pub(crate) fn build_document_body(
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    cfg: &RichnessCfg,
    cap_override: Option<usize>,
) -> String {
    let mut blob = String::new();
    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            maybe_boundary_banner(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |s| {
                    blob.push_str(&s);
                    blob.push('\n');
                },
            );
            render_turn_text(turn, sel.sides, cfg, cap_override, &mut |s| {
                blob.push_str(&s);
                blob.push('\n');
            });
        }
    }
    blob
}

/// Greedily pack a document's LINES into chunks of at most `window` CHARACTERS (Unicode
/// scalars - the unit Claude Code's 10,000-char additionalContext cap counts, so a CJK-heavy
/// document is NOT 3× over-counted the way a byte budget would). A line longer than the
/// window on its own is hard-split on a char boundary so NO emitted chunk ever exceeds
/// `window`. Concatenating the chunks in order reproduces `text` exactly (`split_inclusive`
/// keeps the newlines), so the slices reassemble losslessly across hook invocations.
pub(crate) fn slice_into_windows(text: &str, window: usize) -> Vec<String> {
    let window = window.max(1);
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_chars = 0usize;
    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if line_chars > window {
            // Oversized single line: flush the current chunk, then hard-split on char
            // boundaries so no emitted chunk exceeds the window. The trailing remainder
            // (< window) seeds the next chunk so following lines still pack onto it.
            if !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
                cur_chars = 0;
            }
            let mut piece = String::new();
            let mut piece_chars = 0usize;
            for ch in line.chars() {
                piece.push(ch);
                piece_chars += 1;
                if piece_chars == window {
                    chunks.push(std::mem::take(&mut piece));
                    piece_chars = 0;
                }
            }
            if !piece.is_empty() {
                cur = piece;
                cur_chars = piece_chars;
            }
            continue;
        }
        if cur_chars + line_chars > window && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
            cur_chars = 0;
        }
        cur.push_str(line);
        cur_chars += line_chars;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Emit a `══ compaction boundary ══` banner for every summary the ascending walk
/// crosses on the way to a turn with `compactions_before == current`. Crossings are
/// keyed on the summary RANK from newest (newest = rank 1): moving from a turn at
/// cb=`prev` to one at cb=`current` (`current < prev`) crosses every summary ranked
/// `(current, prev]`, each bannered once, in ascending line order. The FIRST turn
/// (`prev == None`) crosses NOTHING - there are no restored turns below it, so the
/// summaries older than it (which it predates) are not bannered.
pub(crate) fn maybe_boundary_banner(
    prev: &mut Option<usize>,
    current: usize,
    summaries: &[SummaryInfo],
    emit: &mut dyn FnMut(String),
) {
    for s in crossed_summaries(summaries, *prev, current) {
        emit(boundary_banner_line(s.line_no));
    }
    *prev = Some(current);
}

/// The summaries crossed when the ascending cursor moves from a turn at cb=`from` to a
/// turn at cb=`to`. A summary's rank from newest is its 1-based position when sorted by
/// descending line number; it is crossed when `to < rank <= from`. The FIRST turn
/// (`from == None`) seeds the cursor at its OWN depth (crosses nothing) - a summary
/// older than every selected turn has no restored turn below it, so it is never
/// bannered. Total banners across a full walk therefore equal the GREATEST cb selected
/// (the spanned-boundary count). Returned in ascending line order so banners read
/// forward.
pub(crate) fn crossed_summaries(
    summaries: &[SummaryInfo],
    from: Option<usize>,
    to: usize,
) -> Vec<&SummaryInfo> {
    // The first turn seeds the cursor at its own depth → no crossing on arrival.
    let Some(from) = from else {
        return Vec::new();
    };
    if to >= from {
        return Vec::new();
    }
    // Rank by descending line number (newest = rank 1).
    let mut by_rank: Vec<&SummaryInfo> = summaries.iter().collect();
    by_rank.sort_by(|a, b| b.line_no.cmp(&a.line_no));
    let mut out: Vec<&SummaryInfo> = Vec::new();
    for (i, s) in by_rank.iter().enumerate() {
        let rank = i + 1; // newest = 1
        if rank > to && rank <= from {
            out.push(*s);
        }
    }
    out.sort_by_key(|s| s.line_no);
    out
}

/// Render one turn's selected side(s) to the text format.
/// Whether a selection shows the user side / assistant side.
pub(crate) fn shows_user(sides: SelSides) -> bool {
    matches!(sides, SelSides::Both | SelSides::UserOnly)
}
pub(crate) fn shows_assistant(sides: SelSides) -> bool {
    matches!(sides, SelSides::Both | SelSides::AssistantOnly)
}

/// The user side of a turn IF the selection shows it AND it exists. Centralizes the
/// `show_user && Some` logic so BOTH renderers share one (unit-testable) decision.
pub(crate) fn shown_user(turn: &TurnSlice, sides: SelSides) -> Option<&TurnUnit> {
    if shows_user(sides) {
        turn.user.as_ref()
    } else {
        None
    }
}
/// The assistant LANE of a turn IF the selection shows it: the richness-selected
/// survivor list (kept agent messages + collapsed placeholders). EMPTY when the selection
/// hides the assistant side or the turn has no agent messages. In `EotOnly` mode this is
/// just `[Kept(last)]`, so a single-EOT emit is reproduced byte-for-byte.
pub(crate) fn shown_agent_lane<'a>(
    turn: &'a TurnSlice,
    sides: SelSides,
    cfg: &RichnessCfg,
) -> Vec<AgentRender<'a>> {
    if shows_assistant(sides) {
        select_agent_messages(turn, cfg)
    } else {
        Vec::new()
    }
}

pub(crate) fn render_turn_text(
    turn: &TurnSlice,
    sides: SelSides,
    cfg: &RichnessCfg,
    cap_override: Option<usize>,
    emit: &mut dyn FnMut(String),
) {
    if let Some(u) = shown_user(turn, sides) {
        emit_unit_text(u, cap_override, emit);
        // Image marker directly under the user line (charged by `image_marker_cost` in
        // `turn_cost` on the same user-side selection - keeps summed-cost == emitted).
        if !turn.image_ids.is_empty() {
            emit(image_marker_line(&turn.image_ids));
        }
    }
    if matches!(sides, SelSides::Both) && turn.tool_calls > 0 {
        emit(format!("  [{} tool calls]", turn.tool_calls));
    }
    for entry in shown_agent_lane(turn, sides, cfg) {
        match entry {
            AgentRender::Kept(a) => emit_unit_text(&a.unit, cap_override, emit),
            AgentRender::Placeholder(s) => emit(agent_placeholder_line(&s)),
        }
    }
}

/// Emit a unit's header line + rendered (possibly truncated) body. The header string is
/// produced by [`unit_header_line`] - the SAME function the cost model charges - so the
/// emitted line is byte-for-byte what the budget accounted.
pub(crate) fn emit_unit_text(
    unit: &TurnUnit,
    cap_override: Option<usize>,
    emit: &mut dyn FnMut(String),
) {
    emit(unit_header_line(unit));
    let r = render_unit_body(unit, cap_override);
    emit(r.body);
}
