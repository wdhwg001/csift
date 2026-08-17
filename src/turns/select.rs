//! Agent-message selection, collapse placeholders, unit costs + banners.

use super::*;

/// One entry in a turn's rendered assistant lane: a SURVIVING agent message, or a
/// PLACEHOLDER standing in for a contiguous run of collapsed agent messages.
#[derive(Debug, Clone)]
pub(crate) enum AgentRender<'a> {
    Kept(&'a AgentMsg),
    Placeholder(PlaceholderSpan),
}

/// A contiguous span of collapsed agent messages → one placeholder line. Carries the
/// X/Y/Z counts + the first/last elided jsonl line numbers so a consumer can `Read` the
/// raw range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaceholderSpan {
    /// X - collapsed agent messages in this span (≥1).
    pub(crate) messages: usize,
    /// Y - `tool_use` blocks owned by the collapsed span's preceding spans.
    pub(crate) tool_calls: usize,
    /// Z - erroring `tool_result` blocks in that same span.
    pub(crate) failed: usize,
    /// First / last jsonl line of the collapsed agent records (for the fetchable range).
    pub(crate) first_line: usize,
    pub(crate) last_line: usize,
}

/// Decide a turn's SURVIVING agent messages + the collapsed placeholder spans, per mode +
/// cfg (STAGE 1 - operates on WHOLE messages, never touches `ASST_CAP`):
///   • `Longest` (DEFAULT) - keep the LONGEST agent message (by `full_chars`) + the FIRST
///     when substantive (`full_chars >= rich_min_chars`) + each RICH middle; collapse
///     everything else into placeholders. Applies to every multi-message turn; on a
///     single-message turn the sole message is kept. Tie on length → the LAST maximum.
///   • `EotOnly` - only the last agent message (force last-only; never a placeholder).
///   • `All` - every agent message, no filtering, no placeholder.
///   • `Rich` - on a LONG run (`agents.len() > run_threshold`): the LAST is always kept;
///     the FIRST is kept by position privilege under `keep_first` (else decided as a
///     middle); each MIDDLE is kept UNLESS it is a proven pure declaration (keep-on-
///     doubt - drop requires proof). Contiguous dropped runs fuse into one placeholder.
///     A short run (`<= run_threshold`) keeps every agent message verbatim.
/// Produces an ordered list of `{ Kept | Placeholder }` in ascending agent order. EMPTY
/// for a pure tool-call turn (no agents).
pub(crate) fn select_agent_messages<'a>(
    turn: &'a TurnSlice,
    cfg: &RichnessCfg,
) -> Vec<AgentRender<'a>> {
    let agents = &turn.agents;
    if agents.is_empty() {
        return Vec::new();
    }
    match cfg.mode {
        AgentMsgMode::Longest => {
            // The DEFAULT. A single-message turn keeps its sole message (it is both first
            // and longest); no richness eval, no placeholder.
            if agents.len() == 1 {
                return vec![AgentRender::Kept(&agents[0])];
            }
            // The LONGEST agent message is ALWAYS kept (the substantive Rich Response).
            // `max_by_key` returns the LAST maximum on ties, so an all-equal run picks the
            // same index the old `agents.last()` default did - the documented tie rule.
            let longest = agents
                .iter()
                .enumerate()
                .max_by_key(|(_, a)| a.unit.full_chars)
                .map(|(i, _)| i)
                .expect("non-empty");
            let last = agents.len() - 1;
            // Per-message keep decision. Additive over the longest pick:
            //   • the LONGEST index - ALWAYS (the substantive response; may also be first/
            //     middle/last, the position privileges below merely add MORE survivors).
            //   • the FIRST - kept when SUBSTANTIVE (`full_chars >= rich_min_chars`); an
            //     opening plan / early finding worth preserving. A short "let me look"
            //     opener is below the gate → collapses.
            //   • the LAST - kept when SUBSTANTIVE or RICH (so a real closing answer
            //     survives, but a ~50-char throwaway wrap-up collapses - the headline
            //     case). When the last IS the longest it is already kept above.
            //   • each MIDDLE - kept when RICH (`agent_msg_is_rich`); a major finding can
            //     live mid-run.
            let keep = |i: usize, a: &AgentMsg| -> bool {
                if i == longest {
                    return true; // The substantive Rich Response - always.
                }
                if i == 0 {
                    return a.unit.full_chars >= cfg.rich_min_chars; // FIRST if substantive.
                }
                if i == last {
                    return agent_msg_is_rich(&a.unit.text, cfg); // LAST if it carries info.
                }
                agent_msg_is_rich(&a.unit.text, cfg) // MIDDLE if rich.
            };
            collapse_unkept(agents, keep)
        }
        AgentMsgMode::EotOnly => {
            // Only the last (the EOT anchor) - reproduces the pre-expansion output.
            vec![AgentRender::Kept(agents.last().expect("non-empty"))]
        }
        AgentMsgMode::All => agents.iter().map(AgentRender::Kept).collect(),
        AgentMsgMode::Rich => {
            // Short run (or exactly at the threshold) → keep everything verbatim.
            if agents.len() <= cfg.run_threshold {
                return agents.iter().map(AgentRender::Kept).collect();
            }
            let last = agents.len() - 1;
            // Per-message keep decision (KEEP-ON-DOUBT is the spine: collapse only PROVEN
            // pure declarations; keep everything uncertain):
            //   • LAST  - ALWAYS kept (the outcome / EOT anchor; position overrides drop).
            //   • FIRST - the first-matters / immediate-reply case. With `keep_first`
            //     (DEFAULT) the position privilege keeps it unconditionally (the opening
            //     message often states the plan / an early finding worth preserving). With
            //     `--no-keep-first` the privilege is dropped and the first is decided
            //     exactly as a MIDDLE (kept unless droppable - so a rich first still
            //     survives, a "let me look into this" declaration first collapses).
            //   • MIDDLE - kept unless droppable; a sudden rich middle survives whole.
            let keep = |i: usize, a: &AgentMsg| -> bool {
                if i == last {
                    return true; // LAST anchor - always (overrides the drop predicate).
                }
                if i == 0 && cfg.keep_first {
                    return true; // FIRST + position privilege - kept merely for being first.
                }
                // MIDDLE (and a `--no-keep-first` FIRST): keep unless proven droppable.
                !agent_msg_is_droppable(&a.unit.text, cfg)
            };
            collapse_unkept(agents, keep)
        }
    }
}

/// Walk an agent run, KEEPING each message the `keep` predicate accepts and FUSING every
/// contiguous run of un-kept messages into one [`PlaceholderSpan`] (X/Y/Z counts + the
/// first/last elided jsonl line). Shared by the `Longest` and `Rich` selection arms so the
/// placeholder accounting (and thus the summed-cost == summed-emitted invariant) is
/// identical for both. Produces `{ Kept | Placeholder }` in ascending agent order.
pub(crate) fn collapse_unkept<'a>(
    agents: &'a [AgentMsg],
    keep: impl Fn(usize, &AgentMsg) -> bool,
) -> Vec<AgentRender<'a>> {
    let mut out: Vec<AgentRender> = Vec::new();
    let mut span: Option<PlaceholderSpan> = None;
    for (i, a) in agents.iter().enumerate() {
        if keep(i, a) {
            if let Some(s) = span.take() {
                out.push(AgentRender::Placeholder(s));
            }
            out.push(AgentRender::Kept(a));
        } else {
            // Extend (or open) the current contiguous collapsed span.
            let line = a.unit.line_no;
            match span.as_mut() {
                Some(s) => {
                    s.messages += 1;
                    s.tool_calls += a.preceding_tool_calls;
                    s.failed += a.preceding_failed;
                    s.last_line = line;
                }
                None => {
                    span = Some(PlaceholderSpan {
                        messages: 1,
                        tool_calls: a.preceding_tool_calls,
                        failed: a.preceding_failed,
                        first_line: line,
                        last_line: line,
                    });
                }
            }
        }
    }
    if let Some(s) = span.take() {
        out.push(AgentRender::Placeholder(s));
    }
    out
}

/// Pluralize a noun by count: `1 thing` / `N things`.
pub(crate) fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The EXACT placeholder line a collapsed span renders to (no trailing newline):
///   `△ L{first}–L{last}  [X agent message(s), Y tool call(s)[, Z failed]]`
/// X/Y are always shown (Y even at 0 - a zero-tool reasoning span is informative); the Z
/// clause is OMITTED when Z == 0. Pluralization is INDEPENDENT per noun; "failed" is an
/// adjective (never pluralized). A single-message span renders `L{n}` (no range dash).
pub(crate) fn agent_placeholder_line(span: &PlaceholderSpan) -> String {
    let range = if span.first_line == span.last_line {
        format!("L{}", span.first_line)
    } else {
        format!("L{}–L{}", span.first_line, span.last_line)
    };
    let msgs = plural(span.messages, "agent message");
    let tools = plural(span.tool_calls, "tool call");
    let body = if span.failed == 0 {
        format!("[{msgs}, {tools}]")
    } else {
        format!("[{msgs}, {tools}, {} failed]", span.failed)
    };
    format!("△ {range}  {body}")
}

/// The budget cost of one placeholder line as a physical line (`chars + NEWLINE_COST`).
/// The placeholder SUBSTITUTES the dropped bodies (they contribute zero unit cost), so
/// only this line's own chars are charged - keeping summed-cost == summed-emitted.
pub(crate) fn agent_placeholder_cost(span: &PlaceholderSpan) -> usize {
    agent_placeholder_line(span).chars().count() + NEWLINE_COST
}

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
pub(crate) fn boundary_banner_line(line_no: usize) -> String {
    format!(
        "{0} compaction boundary · summary at L{1} · (turns below predate it) {0}",
        "══", line_no
    )
}

/// The budget cost of one boundary banner as a physical line (`chars + NEWLINE_COST`).
pub(crate) fn banner_cost(line_no: usize) -> usize {
    boundary_banner_line(line_no).chars().count() + NEWLINE_COST
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
    let mut by_rank: Vec<usize> = summaries.iter().map(|s| s.line_no).collect();
    by_rank.sort_unstable_by(|a, b| b.cmp(a));
    by_rank.into_iter().take(depth).map(banner_cost).sum()
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
