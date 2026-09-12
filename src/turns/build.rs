//! Build turn units from records; compaction summaries + fingerprints.

use super::*;

/// Build the per-turn slices + summary dedup sets from a session's line-numbered records.
///
/// Turn segmentation comes from the SURVIVAL AXIS ([`ChainView::turns`]), so only turns the
/// surviving conversation still reaches are built - a recalled draft, a rewound turn, and
/// a compaction re-anchor's replay copies never surface as phantom turns. That is not a
/// display choice: the compaction summariser reads the IN-MEMORY message array (claim
/// CMP-020), which by then no longer holds those turns, so replaying one would put text
/// into a "what the compaction clipped" reconstruction that no model ever saw. A compaction
/// summary is a turn MEMBER (it is excluded from genuine-user), so the walk is transparent
/// to it.
pub(crate) fn build(
    records: &[(usize, Record)],
    view: &ChainView,
    sidecar: &[Record],
) -> (Vec<TurnSlice>, Vec<SummaryInfo>) {
    let turns = view.turns();
    // ExitPlanMode plan pointers for this session, so a rejection-with-message turn
    // opener can surface `[plan: <path>]` (§4.2.4). Cheap; empty in a no-plan session.
    let plan_index = PlanIndex::from_records(records.iter().map(|(_, r)| r));

    // Summary line numbers in file order (for compactions_before + boundary banners).
    let mut summaries: Vec<SummaryInfo> = Vec::new();
    for (line_no, rec) in records {
        if rec.is_compact_summary.unwrap_or(false) {
            if let Some(body) = compact_summary_body(rec) {
                summaries.push(SummaryInfo {
                    line_no: *line_no,
                    fingerprints: summary_fingerprints(&body),
                    body_chars: body.chars().count(),
                    mode: rec.summary_compaction_mode(),
                });
            }
        }
    }
    // Newest summary line (max), for compactions_before accounting.
    let summary_lines: Vec<usize> = summaries.iter().map(|s| s.line_no).collect();

    let mut slices: Vec<TurnSlice> = Vec::with_capacity(turns.len());
    for (turn_index, idxs) in turns.iter().enumerate() {
        // A live turn whose every record this prefilter dropped carries nothing to replay.
        // It keeps its NUMBER (the view holds the empty group so the numbering never
        // shifts); it just contributes no slice.
        if idxs.is_empty() {
            continue;
        }
        let mut user: Option<TurnUnit> = None;
        let mut is_automation = false;
        let mut automation: Option<crate::model::AutomationTrigger> = None;
        let mut agents: Vec<AgentMsg> = Vec::new();
        let mut tool_calls = 0usize;
        let mut image_ids: Vec<String> = Vec::new();
        // Per-message attribution: tool_use / erroring tool_result blocks seen since the
        // PREVIOUS agent-text record (or turn start). Consumed (and zeroed) on each push.
        let mut pending_tool_calls = 0usize;
        let mut pending_failed = 0usize;

        for &i in idxs {
            let (line_no, rec) = (records[i].0, &records[i].1);
            let survival = view.survival(i).as_str();

            // Tool-call + erroring-tool-result counts for THIS record. The turn-wide
            // `tool_calls` accumulator (the `[N tool calls]` marker) is unchanged; the
            // `pending_*` counters additionally attribute the span to the NEXT agent msg.
            if let Some(blocks) = rec.blocks() {
                for b in blocks {
                    match b {
                        Block::ToolUse { .. } => {
                            tool_calls += 1;
                            pending_tool_calls += 1;
                        }
                        Block::ToolResult {
                            is_error: Some(true),
                            ..
                        } => {
                            pending_failed += 1;
                        }
                        _ => {}
                    }
                }
            }

            // Images this turn carries (pasted image / tool screenshot) → the `[N image(s)]`
            // marker. Cheap (only image-bearing lines pass turns' prefilter anyway).
            image_ids.extend(crate::image::image_ids_for_record(rec, line_no));

            // The turn opener (genuine human, an answered AskUserQuestion, or a tool-use
            // rejection-with-message). `group_turn_indices` opens a turn on `opens_turn`,
            // so the FIRST such record is the opener; keep the earliest. The rendered
            // body is the unified reconstruction (Q+options+answer for an AUQ; the typed
            // instruction + a `[plan: …]` pointer for a rejection).
            if user.is_none() && rec.opens_turn() {
                // An automation trigger (`<task-notification>`) opens a turn like a human
                // message, but its body must render as the parsed `[workflow <id> …]
                // <summary>` ATTRIBUTION label - never the raw `<task-id>`/`<output-file>`
                // XML wrapper. `automation_label` wins; otherwise the normal user-text
                // reconstruction applies.
                if let Some(label) = rec.automation_label() {
                    is_automation = true;
                    automation = rec.automation_trigger();
                    user = Some(make_unit(line_no, Role::User, &label, rec, survival));
                } else if let Some(ic) = rec.inbound_comm_preview() {
                    // An inbound PEER opener - `<teammate-message>` (GOLD §1) OR `<agent-message>`
                    // (FINDING-2, now an `opens_turn` boundary too): render the CLEAN body with an
                    // `agent.communication.{inbox,signal}  <from> ⇨ self` header in place of the raw
                    // XML it used to dump into the `▽ USER` lane. `inbound_comm_preview` covers BOTH
                    // peer forms (boundary-anchored, FINDING-1), so neither shows raw XML.
                    let mut u = make_unit(line_no, Role::User, &ic.body, rec, survival);
                    u.inbound = Some(ic);
                    user = Some(u);
                } else if let Some(text) = rec.reconstructed_user_text(Some(&plan_index)) {
                    user = Some(make_unit(line_no, Role::User, &text, rec, survival));
                }
            }

            // EVERY agent-text record becomes an AgentMsg (the model-expansion). The
            // pending tool/failed counters since the previous agent record are attributed
            // to THIS message (the placeholder's per-message Y / Z), then zeroed.
            if let Some(text) = rec.agent_text() {
                agents.push(AgentMsg {
                    unit: make_unit(line_no, Role::Assistant, &text, rec, survival),
                    // Provisional; reassigned by AgentPos after the loop.
                    pos: AgentPos::Last,
                    preceding_tool_calls: pending_tool_calls,
                    preceding_failed: pending_failed,
                });
                pending_tool_calls = 0;
                pending_failed = 0;
            }
        }

        // Assign positions: index 0 → First, last index → Last, the rest → Middle. A
        // single-element vec → that element is Last (the always-keep anchor: a 1-message
        // turn's sole reply is BOTH first and last, never dropped).
        let last = agents.len().saturating_sub(1);
        for (i, a) in agents.iter_mut().enumerate() {
            a.pos = if i == last {
                AgentPos::Last
            } else if i == 0 {
                AgentPos::First
            } else {
                AgentPos::Middle
            };
        }

        // `compactions_before` is keyed on the turn's CONTENT lines (its user opener /
        // agent messages), NOT on member records like a trailing summary that joins the
        // turn - a summary that opens a NEW compacted region must sit AFTER this turn's
        // content, so count summaries strictly above the turn's latest content line.
        let content_line = user
            .as_ref()
            .map(|u| u.line_no)
            .into_iter()
            .chain(agents.iter().map(|a| a.unit.line_no))
            .max()
            .unwrap_or(0);
        let compactions_before = summary_lines.iter().filter(|&&s| s > content_line).count();

        slices.push(TurnSlice {
            turn_index,
            user,
            tool_calls,
            image_ids,
            agents,
            compactions_before,
            is_automation,
            automation,
        });
    }

    // ── Elicitation-sidecar pending units (§3.10) ──
    // Each unresolved-pending elicitation becomes its OWN turn unit, appended AFTER the native
    // turns (a pending elicitation is the LATEST activity - it is what the session is currently
    // blocked on). It is rendered as the USER side (the question/plan/elicitation put TO the
    // user, awaiting the answer), with `from_sidecar` so the header shows `(elicitation
    // sidecar)` instead of a fabricated `Lnnnn`. `compactions_before` is 0 (it post-dates every
    // summary). These never dedup against a summary (a summary cannot quote a not-yet-answered
    // elicitation).
    for rec in sidecar {
        let Some(text) = crate::elicitation::pending_text(rec) else {
            continue;
        };
        let turn_index = slices.len();
        slices.push(TurnSlice {
            turn_index,
            user: Some(make_unit(0, Role::User, &text, rec, "live")),
            tool_calls: 0,
            image_ids: Vec::new(),
            agents: Vec::new(),
            compactions_before: 0,
            is_automation: false,
            automation: None,
        });
    }

    (slices, summaries)
}

/// Build a [`TurnUnit`] from a record's already-normalized one-line `text`. The newline
/// positions come from the record's ORIGINAL (pre-normalization) body, expressed in `text`'s
/// own char coordinates, so the `L lines elided` note can count the newlines a CUT removed.
pub(crate) fn make_unit(
    line_no: usize,
    role: Role,
    text: &str,
    rec: &Record,
    survival: &'static str,
) -> TurnUnit {
    let newline_positions = body_newline_positions(rec, text);
    TurnUnit {
        line_no,
        role,
        full_chars: text.chars().count(),
        text: text.to_string(),
        newline_positions,
        ts_utc: rec.timestamp.clone(),
        also_in_summary: false,
        from_sidecar: rec.is_elicitation_marker(),
        inbound: None,
        survival,
    }
}

/// A record's ORIGINAL message body (pre-normalization): a bare-string body as-is, a block
/// body as the visible `text` blocks joined with `\n` (a block seam IS a line break in the
/// record's visual form, and normalizing either join yields the same one-line string).
/// `None` when the record carries no message body.
pub(crate) fn raw_body(rec: &Record) -> Option<String> {
    let content = rec.message.as_ref()?.content.as_ref()?;
    Some(match content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

/// Where the record's ORIGINAL body newlines sit in `text`'s char coordinates - the basis for
/// the `L lines elided` note, which counts only the ones a cap's cut removed.
///
/// The positions address the NORMALIZATION of the raw body, so they are returned ONLY when
/// `text` IS that normalization. Every ordinary user / assistant body is (both
/// `Record::agent_text` and `Record::genuine_user_text` return exactly it), while a FABRICATED
/// body is not: an AskUserQuestion scaffold, an automation attribution label and a peer-message
/// preview are strings csift composes, and the raw record's newlines do not sit anywhere in
/// them. For those the answer is an empty list - the note is omitted rather than reporting a
/// count from a different string, which is what the whole-message figure used to do.
pub(crate) fn body_newline_positions(rec: &Record, text: &str) -> Vec<u32> {
    let Some(raw) = raw_body(rec) else {
        return Vec::new();
    };
    if !raw.contains('\n') {
        return Vec::new();
    }
    let (normalized, positions) = crate::model::normalize_line_with_newlines(&raw);
    if normalized == text {
        positions
    } else {
        Vec::new()
    }
}

/// The body text of a compaction-summary record (a `type:"user"` `isCompactSummary`
/// record carrying string content). `genuine_user_text` filters summaries out, so we
/// read the `Content::Text` body directly. Returns `None` if it is not a string body.
pub(crate) fn compact_summary_body(rec: &Record) -> Option<String> {
    let content = rec.message.as_ref()?.content.as_ref()?;
    match content {
        Content::Text(s) => Some(s.clone()),
        // A summary always carries STRING content in real data; a block body would be
        // a genuine surprise - return None rather than guess.
        Content::Blocks(_) => None,
    }
}

/// Extract dedup fingerprints from a summary body: the §6 "All user messages" bullets
/// and the §9 verbatim last-assistant quote (the only verbatim turns a summary holds).
/// Each fingerprint is `normalize_line(text).to_lowercase()` truncated to the first
/// [`DEDUP_PREFIX`] chars. Conservative: when the structured sections are not found,
/// every `- ` bullet line in the body is fingerprinted (a superset - still strict per
/// line). Robust to summaries that omit the exact headers.
pub(crate) fn summary_fingerprints(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        // §6 bullets render as `- "<text>" …` (quoted) or `- <text>` (bare); §9 quote is
        // prose carrying a quoted run. Prefer the QUOTED inner (the verbatim turn text);
        // for an UNQUOTED bullet fall back to the whole bullet body. A bullet WITH quotes
        // but an empty inner contributes nothing (it would only fingerprint the quotes).
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let fp = match quoted_inner(rest) {
                Some(q) => fingerprint(&q),
                None => fingerprint(rest),
            };
            if !fp.is_empty() {
                out.push(fp);
            }
        } else if let Some(inner) = quoted_inner(trimmed) {
            let fp = fingerprint(&inner);
            if !fp.is_empty() {
                out.push(fp);
            }
        }
    }
    out
}

/// The text inside the FIRST pair of double-quotes on a line (the §9 quote / a quoted
/// §6 bullet body), or `None` if the line has no quoted run.
pub(crate) fn quoted_inner(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let rest = &s[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Normalized-prefix fingerprint of a candidate text (lowercased, whitespace-collapsed,
/// first [`DEDUP_PREFIX`] chars). Empty input → empty (never matches).
pub(crate) fn fingerprint(s: &str) -> String {
    let normalized = normalize_line(s).to_lowercase();
    normalized.chars().take(DEDUP_PREFIX).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Render cost model + ellipsis
// ─────────────────────────────────────────────────────────────────────────────
