//! Agent-message selection: the keep/collapse decision, the collapse placeholder and its
//! previews, and the same-prefix re-send fold. The render GEOMETRY + budget cost model the
//! planner shares with the renderer lives beside it in `cost.rs`.

use super::*;

/// One entry in a turn's rendered assistant lane: a SURVIVING agent message, a PLACEHOLDER
/// standing in for a contiguous run of collapsed agent messages, or a message a LATER
/// re-send of itself supersedes (kept as a one-line marker - see [`mark_resends`]).
#[derive(Debug, Clone)]
pub(crate) enum AgentRender<'a> {
    Kept(&'a AgentMsg),
    Placeholder(PlaceholderSpan),
    /// `msg` is the EARLIER message; `by_line` the jsonl line of the later one that carries
    /// its whole body. The TEXT render prints the marker instead of a second copy of the
    /// prose; JSON still emits the unit in full, with `superseded_by_line` set.
    Superseded {
        msg: &'a AgentMsg,
        by_line: usize,
    },
}

/// A contiguous span of collapsed agent messages → one placeholder line. Carries the
/// X/Y/Z counts + the first/last elided jsonl line numbers so a consumer can `Read` the
/// raw range, plus WHAT was folded: the summed char count and one preview per substantive
/// member.
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
    /// N - the summed `full_chars` of the collapsed messages: how much prose the fold
    /// substituted, which the X/Y/Z counts alone never said.
    pub(crate) chars: usize,
    /// One entry per collapsed message of at least [`COLLAPSED_PREVIEW_MIN_CHARS`], in
    /// ascending line order - the fold's own disclosure of what it swallowed.
    pub(crate) previews: Vec<CollapsedPreview>,
}

/// The head of one collapsed agent message: its jsonl line + the first
/// [`COLLAPSED_PREVIEW_CHARS`] chars of its body (through `text::truncate_excerpt`, so the
/// excerpt states its own remainder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollapsedPreview {
    pub(crate) line: usize,
    pub(crate) excerpt: String,
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
/// Then STAGE 2, [`mark_resends`], turns a kept message whose whole body a LATER kept message
/// carries into a one-line `Superseded` marker. Produces an ordered list of
/// `{ Kept | Placeholder | Superseded }` in ascending agent order. EMPTY for a pure tool-call
/// turn (no agents).
pub(crate) fn select_agent_messages<'a>(
    turn: &'a TurnSlice,
    cfg: &RichnessCfg,
) -> Vec<AgentRender<'a>> {
    let mut lane = select_kept(turn, cfg);
    mark_resends(&mut lane);
    lane
}

/// The keep/collapse decision alone (STAGE 1), before [`mark_resends`] folds a re-send.
fn select_kept<'a>(turn: &'a TurnSlice, cfg: &RichnessCfg) -> Vec<AgentRender<'a>> {
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
            let preview = collapsed_preview(&a.unit);
            match span.as_mut() {
                Some(s) => {
                    s.messages += 1;
                    s.tool_calls += a.preceding_tool_calls;
                    s.failed += a.preceding_failed;
                    s.last_line = line;
                    s.chars += a.unit.full_chars;
                    s.previews.extend(preview);
                }
                None => {
                    span = Some(PlaceholderSpan {
                        messages: 1,
                        tool_calls: a.preceding_tool_calls,
                        failed: a.preceding_failed,
                        first_line: line,
                        last_line: line,
                        chars: a.unit.full_chars,
                        previews: preview.into_iter().collect(),
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

/// The re-send grouping HEAD of an agent body: the lowercase of its first [`DEDUP_PREFIX`]
/// chars, which is exactly the summary-dedup `fingerprint` composition (the body reaching a
/// `TurnUnit` is already `normalize_line`d by the extractor that built it, so no second
/// whitespace collapse is needed). `None` for a body under `DEDUP_PREFIX` chars: the
/// fingerprint is only the strict discriminator its own doc claims at FULL length, and below it
/// the one-line marker would cost more than the body it replaces.
pub(crate) fn resend_head(unit: &TurnUnit) -> Option<String> {
    if unit.text.chars().count() < DEDUP_PREFIX {
        return None;
    }
    Some(
        unit.text
            .chars()
            .take(DEDUP_PREFIX)
            .collect::<String>()
            .to_lowercase(),
    )
}

/// Fold a SAME-PREFIX RE-SEND: when a kept agent message's whole body is a PREFIX of a LATER
/// kept message's body in the same turn, the later one carries everything the earlier said
/// plus any addendum, so the earlier renders as a one-line marker instead of a second copy of
/// the prose. The LATER message is the survivor (it is the one the turn ended on).
///
/// CONTAINMENT is the predicate, not prefix EQUALITY, and that is a measured choice. Two
/// messages can share a long boilerplate opening and then say entirely different things:
/// censused over every top-level transcript of one corpus (80 files, 6,468 turns, 35,052
/// assistant messages), 53 same-turn pairs shared their first 80 normalized chars, only 1 of
/// the 53 was a true duplicate, 25 diverged before char 120, and on 22 the LATER member was
/// SHORTER than the earlier - so suppressing the earlier on an 80-char match alone would drop
/// prose the survivor does not carry. Requiring containment makes the fold loss-free: every
/// char of the suppressed body is present in the message that replaces it. Under that rule the
/// same corpus yields 2 folds, both exact duplicates.
///
/// A message already inside a [`PlaceholderSpan`] is not considered (it is suppressed anyway),
/// and the survivor must itself be KEPT - suppressing the earlier in favour of a body that the
/// fold also swallowed would hide both.
pub(crate) fn mark_resends(lane: &mut [AgentRender<'_>]) {
    // Group the kept entries by their 80-char head - an O(1)-per-message hash - so the full
    // containment test only runs inside a group, which is rare (21 of 6,468 corpus turns had
    // one at all). Without the grouping a turn under `--agent-msgs all` with hundreds of kept
    // messages would pay a quadratic string walk on every cost evaluation.
    let mut groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, entry) in lane.iter().enumerate() {
        if let AgentRender::Kept(a) = entry {
            if let Some(head) = resend_head(&a.unit) {
                groups.entry(head).or_default().push(i);
            }
        }
    }
    let mut marks: Vec<(usize, usize)> = Vec::new(); // (lane index, survivor's jsonl line)
    for idxs in groups.values() {
        if idxs.len() < 2 {
            continue;
        }
        let keys: Vec<String> = idxs
            .iter()
            .map(|&i| match &lane[i] {
                AgentRender::Kept(a) => a.unit.text.to_lowercase(),
                _ => String::new(),
            })
            .collect();
        for (pos, &i) in idxs.iter().enumerate() {
            // The LATEST container wins: a turn that re-sent twice supersedes onto the last.
            for later in (pos + 1..idxs.len()).rev() {
                if keys[later].starts_with(&keys[pos]) {
                    if let AgentRender::Kept(a) = &lane[idxs[later]] {
                        marks.push((i, a.unit.line_no));
                    }
                    break;
                }
            }
        }
    }
    for (i, by_line) in marks {
        if let AgentRender::Kept(a) = lane[i] {
            lane[i] = AgentRender::Superseded { msg: a, by_line };
        }
    }
}

/// The EXACT line a superseded message renders to (no trailing newline):
///   `△ L{line}  [superseded by the same-prefix re-send at L{m}, N chars]`
/// N is the SUPPRESSED body's own char count - what the text render is not reprinting. The
/// name says what was MATCHED (a later message carrying this one's whole body), not a producer:
/// a `Stop hook feedback:` record blocking a turn end is the shape that motivated the rule, but
/// over one corpus none of the 36 such records produced a same-prefix pair (their neighbouring
/// assistant messages shared 0 to 4 normalized chars, and 0 of the 30 same-turn pairs reached
/// 20), and none of the same-prefix pairs that DO exist has one between its members.
pub(crate) fn superseded_line(msg: &AgentMsg, by_line: usize) -> String {
    format!(
        "△ L{}  [superseded by the same-prefix re-send at L{by_line}, {} chars]",
        msg.unit.line_no, msg.unit.full_chars
    )
}

/// The budget cost of one superseded marker as a physical line (`chars + NEWLINE_COST`) - the
/// suppressed body contributes nothing, which is what makes the fold a saving.
pub(crate) fn superseded_cost(msg: &AgentMsg, by_line: usize) -> usize {
    superseded_line(msg, by_line).chars().count() + NEWLINE_COST
}

/// Pluralize a noun by count: `1 thing` / `N things`.
pub(crate) fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The PREVIEW of one collapsed message, or `None` when its body is under
/// [`COLLAPSED_PREVIEW_MIN_CHARS`] (a short body is exactly the declaration the fold exists to
/// swallow, and a preview of it would cost more than it says).
pub(crate) fn collapsed_preview(unit: &TurnUnit) -> Option<CollapsedPreview> {
    (unit.full_chars >= COLLAPSED_PREVIEW_MIN_CHARS).then(|| CollapsedPreview {
        line: unit.line_no,
        excerpt: crate::text::truncate_excerpt(&unit.text, COLLAPSED_PREVIEW_CHARS),
    })
}

/// The EXACT placeholder line a collapsed span renders to (no trailing newline):
///   `△ L{first}–L{last}  [X agent message(s) collapsed, N chars, Y tool call(s)[, Z failed]]`
/// X/Y are always shown (Y even at 0 - a zero-tool reasoning span is informative); the Z
/// clause is OMITTED when Z == 0. N is the summed `full_chars` of the collapsed bodies, so the
/// marker says HOW MUCH prose it stands for and not only how many messages. Pluralization is
/// INDEPENDENT per noun; "failed" is an adjective (never pluralized). A single-message span
/// renders `L{n}` (no range dash).
pub(crate) fn agent_placeholder_line(span: &PlaceholderSpan) -> String {
    let range = if span.first_line == span.last_line {
        format!("L{}", span.first_line)
    } else {
        format!("L{}–L{}", span.first_line, span.last_line)
    };
    let msgs = plural(span.messages, "agent message");
    let tools = plural(span.tool_calls, "tool call");
    let body = if span.failed == 0 {
        format!("[{msgs} collapsed, {} chars, {tools}]", span.chars)
    } else {
        format!(
            "[{msgs} collapsed, {} chars, {tools}, {} failed]",
            span.chars, span.failed
        )
    };
    format!("△ {range}  {body}")
}

/// The EXACT line for one collapsed message's preview (no trailing newline), indented under
/// the fold marker: `    L{line}  {first 60 chars}… (+N chars)`.
pub(crate) fn collapsed_preview_line(p: &CollapsedPreview) -> String {
    format!("    L{}  {}", p.line, p.excerpt)
}

/// EVERY line a collapsed span emits: the fold marker, then one preview line per collapsed
/// message that reached [`COLLAPSED_PREVIEW_MIN_CHARS`]. The renderer and the cost model both
/// walk THIS list, so the charged length is byte-for-byte what is emitted.
pub(crate) fn agent_placeholder_lines(span: &PlaceholderSpan) -> Vec<String> {
    let mut out = vec![agent_placeholder_line(span)];
    out.extend(span.previews.iter().map(collapsed_preview_line));
    out
}

/// The budget cost of one placeholder as physical lines (`chars + NEWLINE_COST` each). The
/// placeholder SUBSTITUTES the dropped bodies (they contribute zero unit cost), so only the
/// marker's and previews' own chars are charged - keeping summed-cost == summed-emitted.
pub(crate) fn agent_placeholder_cost(span: &PlaceholderSpan) -> usize {
    agent_placeholder_lines(span)
        .iter()
        .map(|l| l.chars().count() + NEWLINE_COST)
        .sum()
}
