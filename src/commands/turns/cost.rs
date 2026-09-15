//! Render GEOMETRY + the budget cost model: the exact lines each unit, marker and
//! banner emits, and the char cost charged for them. Every function here is called by
//! BOTH the renderer and the planner, which is what keeps summed-cost == summed-emitted.

use super::*;

/// The glyph that opens a unit's header line in the text render (`▽` user / `△` asst).
pub(crate) fn unit_glyph(role: Role) -> &'static str {
    match role {
        Role::User => "▽",
        Role::Assistant => "△",
    }
}

/// The EXACT header line a unit renders to in the text format (no trailing newline):
/// `▽ L{line}  {ROLE}  ({timestamp})[   (also in summary)]`. The renderer and the cost
/// model both call this, so the charged header length is byte-for-byte what is emitted -
/// the timestamp expansion (≈47 chars beyond the old flat-24 guess) is now counted, not
/// hidden. This is the core fix for the per-unit undercharge.
pub(crate) fn unit_header_line(unit: &TurnUnit) -> String {
    let dup = if unit.also_in_summary {
        "   (also in summary)"
    } else {
        ""
    };
    // A merged elicitation-sidecar unit (§3.10) has no physical jsonl line - render the
    // provenance locator instead of a fabricated `Lnnnn`.
    let locator = if unit.from_sidecar {
        "(elicitation sidecar)".to_string()
    } else {
        format!("L{}", unit.line_no)
    };
    // An inbound peer/teammate communication opener (GOLD §1) renders its comm LABEL + the
    // `<from> ⇨ self` direction in place of the bare role word, so a reader sees a peer message -
    // not a human turn - at a glance (parity with `search`'s inbox render). The dotted class path
    // stays lowercase (the canonical selector form); an ordinary unit keeps the UPPERCASE role.
    let role_field = match &unit.inbound {
        Some(ic) => format!("{}  {} ⇨ self", ic.class.path(), ic.from),
        None => unit.role.label().to_uppercase(),
    };
    format!(
        "{} {locator}  {role_field}  ({}){dup}",
        unit_glyph(unit.role),
        format_timestamp(unit.ts_utc.as_deref())
    )
}

/// The budget cost of one unit: its REAL header line + the rendered body, each as a
/// physical line (`chars + NEWLINE_COST`). The body ALREADY includes the `… [+K …] …`
/// elision scaffolding when truncated, so this is measured against the SAME render used
/// for output - summed cost == summed emitted chars (the budget test relies on it). No
/// separate marker term (that would double-count). The header length is the true
/// timestamp-dependent line, not a flat estimate.
pub(crate) fn unit_cost(unit: &TurnUnit) -> usize {
    let header_chars = unit_header_line(unit).chars().count() + NEWLINE_COST;
    let body_chars = render_unit_body(unit, None).body.chars().count() + NEWLINE_COST;
    header_chars + body_chars
}

/// The `[N tool calls]` marker line render cost INCLUDING its trailing newline (0 ⇒
/// omitted, no cost). Matches the exact `  [N tool calls]` line the text renderer emits.
pub(crate) fn marker_cost(tool_calls: usize) -> usize {
    if tool_calls == 0 {
        0
    } else {
        // "  [N tool calls]" + the trailing newline the emit callback appends.
        format!("  [{tool_calls} tool calls]").chars().count() + NEWLINE_COST
    }
}

/// The `[N image(s): …]` marker line - shown under the user line when a turn carries images
/// (a pasted image / tool screenshot), listing their stable `csift image` ids so a consumer
/// can `csift image <session> --id <ID> --out <dir>` to get the bytes back.
pub(crate) fn image_marker_line(ids: &[String]) -> String {
    let noun = if ids.len() == 1 { "image" } else { "images" };
    format!("  [{} {}: {}]", ids.len(), noun, ids.join(", "))
}

/// The image-marker line render cost INCLUDING its trailing newline (0 ⇒ omitted, no cost).
/// Matches the exact line `render_turn_text` emits, so summed cost == summed emitted chars.
pub(crate) fn image_marker_cost(ids: &[String]) -> usize {
    if ids.is_empty() {
        0
    } else {
        image_marker_line(ids).chars().count() + NEWLINE_COST
    }
}

/// The EXACT compaction-boundary banner line a crossed summary renders to (no trailing
/// newline). The renderer and the budget reservation both call this so the reserved
/// banner length is byte-for-byte what is emitted.
pub(crate) fn boundary_banner_line(line_no: usize, mode: Option<SummarizeMode>) -> String {
    // C-33: a `/rewind` summarize is a compaction like any other for reconstruction, so the
    // mode only ANNOTATES the banner - and only when it is one of the two summarize gestures,
    // which keeps an ordinary compaction's banner byte-for-byte what it always was.
    let tag = match mode {
        Some(SummarizeMode::FromHere | SummarizeMode::UpToHere) => mode
            .and_then(SummarizeMode::direction)
            .map(|d| format!(" · summarize {d}"))
            .unwrap_or_default(),
        _ => String::new(),
    };
    format!(
        "{0} compaction boundary · summary at L{1}{2} · (turns below predate it) {0}",
        "══", line_no, tag
    )
}

/// The budget cost of one boundary banner as a physical line (`chars + NEWLINE_COST`).
pub(crate) fn banner_cost(line_no: usize, mode: Option<SummarizeMode>) -> usize {
    boundary_banner_line(line_no, mode).chars().count() + NEWLINE_COST
}

/// The EXACT total banner chars the render emits when the selected set spans `depth`
/// compaction boundaries: the render banners every summary ranked 1..=`depth` (rank from
/// newest = 1), each exactly once (`crossed_summaries` covers ranks `(0, depth]` across a
/// full ascending walk). `depth == 0` ⇒ no banners. This is charged INCREMENTALLY as
/// selection deepens the spanned count, so the banner budget is exact (never the
/// over-reservation of "all summaries"), keeping more room for real turns at small
/// budgets / summary-heavy sessions.
pub(crate) fn cumulative_banner_cost(summaries: &[SummaryInfo], depth: usize) -> usize {
    if depth == 0 {
        return 0;
    }
    // Rank by descending line number (newest = rank 1); the first `depth` of those are the
    // boundaries the ascending render crosses to reach a turn at that depth.
    let mut by_rank: Vec<(usize, Option<SummarizeMode>)> =
        summaries.iter().map(|s| (s.line_no, s.mode)).collect();
    by_rank.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    by_rank
        .into_iter()
        .take(depth)
        .map(|(line_no, mode)| banner_cost(line_no, mode))
        .sum()
}

/// A worst-case (provable upper-bound) char count of the document header block emitted by
/// [`render_text`] (the `SESSION` line, the budget line, the selected line, the optional
/// dedup line, and the 60-wide rule). Every numeric placeholder is widened to its
/// session maximum so the real block is always ≤ this. The 60-wide rule glyph `─` and the
/// banner/units glyphs are multi-byte but counted by `chars()`, matching the render.
pub(crate) fn doc_header_block_max_chars(sr: &ScanResult, budget: usize) -> usize {
    let turns = sr.turns.len();
    let summaries = sr.summaries.len();
    // The assistant-units count printed in the selected line can EXCEED `turns` under the
    // richness model (a turn can keep >1 agent message - `All` mode keeps every one), so
    // its worst case is the total agent messages across all turns. The user-units count is
    // still ≤ turns (one opener per turn).
    let max_agent_units = sr.turns.iter().map(|t| t.agents.len()).sum::<usize>();
    let max_line = sr
        .summaries
        .iter()
        .map(|s| s.line_no)
        .chain(sr.turns.iter().map(turn_latest_line))
        .max()
        .unwrap_or(0);
    // Upper bounds: user units ≤ turns; assistant units ≤ total agent messages; char
    // figures ≤ budget; the summary line ≤ max_line; dedup count ≤ both anchors of every
    // turn (2·turns). Render each worst-case line with the SAME format strings the
    // renderer uses, then sum their char lengths (+ newline).
    let line_session = format!("SESSION {}", sr.session_id);
    let line_budget = format!(
        "  budget {} chars · round-trip-fraction {:.2} · spanned {} of {} compaction boundaries in scope",
        budget, 0.0_f64, summaries, summaries
    );
    // The `selected` line carries the automation note ` (N automation triggers)` ONLY when
    // the session actually HAS automation-trigger turns (N ≤ turns). Reserve that space
    // only then, so a session with no automation pulses keeps the exact pre-feature header
    // budget (the note is a no-op string otherwise).
    let has_automation = sr.turns.iter().any(|t| t.is_automation);
    let line_selected = if has_automation {
        format!(
            "  selected {} user ({} automation triggers) + {} assistant units across {} turns · {} / {} chars used",
            turns, turns, max_agent_units, turns, budget, budget
        )
    } else {
        format!(
            "  selected {} user + {} assistant units across {} turns · {} / {} chars used",
            turns, max_agent_units, turns, budget, budget
        )
    };
    let line_dedup = format!(
        "  dedup: {} units also present in summary L{} (demoted, flagged)",
        2 * turns,
        max_line
    );
    let line_rule = format!("  {}", "─".repeat(60));
    [
        line_session,
        line_budget,
        line_selected,
        line_dedup,
        line_rule,
    ]
    .iter()
    .map(|l| l.chars().count() + NEWLINE_COST)
    .sum()
}

/// The cost of a turn's ASSISTANT LANE under the richness selection: the sum of each
/// SURVIVING agent message's `unit_cost` + each collapsed placeholder's
/// `agent_placeholder_cost`. This is the SAME walk the renderer + json emitter use, so
/// summed cost == summed emitted chars. In `EotOnly` mode it equals the single-EOT
/// `unit_cost` exactly (the lane is just `[Kept(last)]`) - the non-breaking guarantee.
pub(crate) fn assistant_lane_cost(turn: &TurnSlice, cfg: &RichnessCfg) -> usize {
    select_agent_messages(turn, cfg)
        .iter()
        .map(|r| match r {
            AgentRender::Kept(a) => unit_cost(&a.unit),
            AgentRender::Placeholder(s) => agent_placeholder_cost(s),
            AgentRender::Superseded { msg, by_line } => superseded_cost(msg, *by_line),
        })
        .sum()
}

/// Cost of a whole turn at the chosen selection granularity (`sides`): both sides +
/// the `[N tool calls]` marker when both are taken; a single side (no marker) otherwise.
/// The assistant side now sums the kept agent messages + placeholders (richness model);
/// in `EotOnly` mode that reduces to the single EOT, so existing budgets are unchanged.
/// This is the SAME accounting the renderer uses, so summed cost == summed rendered chars
/// (the budget test relies on it).
pub(crate) fn turn_cost(turn: &TurnSlice, sides: SelSides, cfg: &RichnessCfg) -> usize {
    let mut c = 0;
    if matches!(sides, SelSides::Both | SelSides::UserOnly) {
        if let Some(u) = &turn.user {
            c += unit_cost(u);
            // The image marker renders directly under a SHOWN user line (so it is tied to
            // the user side, charged whenever that side is taken - 0 when no images).
            c += image_marker_cost(&turn.image_ids);
        }
    }
    // The marker is only rendered BETWEEN the user and the assistant lane, so it is
    // charged only on a both-sides selection (a single-side emit shows no marker).
    if matches!(sides, SelSides::Both) {
        c += marker_cost(turn.tool_calls);
    }
    if matches!(sides, SelSides::Both | SelSides::AssistantOnly) {
        c += assistant_lane_cost(turn, cfg);
    }
    c
}
