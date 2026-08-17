//! JSON emission: units, placeholders, boundaries, the envelope.

use super::*;

pub(crate) fn render_json(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    out_path: Option<&Path>,
) -> Result<()> {
    use serde_json::json;
    let mut out_blob = String::new();

    // A machine-readable HEADER object so a JSON consumer can recover the human/automation
    // split + the budget fan-out WITHOUT the text-only `selected N user (M automation
    // triggers)` line (which was previously absent from the JSON stream entirely). It is the
    // FIRST line; its `kind` discriminator (`session_header`) matches the existing
    // `compaction_boundary` / `collapsed_agents` boundary-object convention.
    let sc = scope_summary(sessions, plans);
    let total_user: usize = plans
        .iter()
        .filter(|p| !p.selected.is_empty())
        .map(|p| count_sides(p, &ctx.cfg).0)
        .sum();
    let total_automation: usize = plans
        .iter()
        .filter(|p| !p.selected.is_empty())
        .map(count_automation)
        .sum();
    // Per-class automation breakdown (the lens-required attribution composition), keyed by
    // the stable `AUTOMATION_KINDS` order, emitted as a `by_kind` object so a consumer never
    // re-derives it from the per-unit `trigger_kind` fields.
    let by = automation_by_kind(plans);
    let by_kind: serde_json::Map<String, serde_json::Value> = AUTOMATION_KINDS
        .iter()
        .zip(by.iter())
        .map(|(k, n)| (k.slug().to_string(), json!(n)))
        .collect();
    // The whole-session composition, INDEPENDENT of budget selection — so a monitor-dominated
    // session never reports `monitor:0` just because the recency window didn't reach the deep
    // pulses (the selected `automation_by_kind` can read 0 for a class that has dozens in
    // scope). A reader compares the two to see "much monitor activity exists, little selected".
    let in_scope_by = automation_in_scope_by_kind(plans);
    let in_scope_by_kind: serde_json::Map<String, serde_json::Value> = AUTOMATION_KINDS
        .iter()
        .zip(in_scope_by.iter())
        .map(|(k, n)| (k.slug().to_string(), json!(n)))
        .collect();
    // `sessions_in_scope` is the TRUE scope (every discovered session); `sessions_rendered` is
    // how many fit the budget. Keeping them distinct stops a `--budget` knob from silently
    // rewriting "scope" and keeps a targeted top-level uuid from reading as `0 top-level`.
    // Budget-accounting aggregates (R10): the text header's per-session numbers, summed —
    // so "did this reconstruction consume its budget / cross the compactions" is machine-
    // answerable without regex-parsing the text header (the machine format must never be
    // thinner than the human one). `boundaries_total` is the sessions' TRUE boundary count
    // in scope; `boundaries_spanned` is what the budget-selected windows crossed (a query
    // property — see the text header's `spanned K of N`).
    let total_assistant: usize = plans
        .iter()
        .filter(|p| !p.selected.is_empty())
        .map(|p| count_sides(p, &ctx.cfg).1)
        .sum();
    let chars_used: usize = plans.iter().map(|p| p.rendered_chars).sum();
    let boundaries_spanned: usize = plans.iter().map(|p| p.spanned_boundaries).sum();
    let boundaries_total: usize = sessions.iter().map(|sr| sr.summaries.len()).sum();
    let header = json!({
        "kind": "header",
        "command": "turns",
        "sessions_in_scope": sc.in_scope,
        "sessions_rendered": sc.rendered,
        "top_level_sessions": sc.in_scope_top,
        "subagent_sessions": sc.in_scope_sub,
        "budget_chars": ctx.budget_chars,
        "budget_is_per_session": true,
        "max_total_chars": ctx.budget_chars.saturating_mul(sc.rendered.max(1)),
        "round_trip_fraction": ctx.rt_fraction,
        "chars_used": chars_used,
        "boundaries_spanned": boundaries_spanned,
        "boundaries_total": boundaries_total,
        "selected_user": total_user,
        "selected_assistant": total_assistant,
        "automation_triggers": total_automation,
        "automation_by_kind": by_kind,
        "automation_in_scope_by_kind": in_scope_by_kind,
        // True when ≥1 selected unit was merged from the elicitation sidecar (§3.10) — the
        // machine echo of the per-session `with elicitation sidecar` text note.
        "with_elicitation_sidecar": plans.iter().any(plan_has_sidecar),
    });
    {
        let s = serde_json::to_string(&header)?;
        println!("{s}");
        out_blob.push_str(&s);
        out_blob.push('\n');
    }

    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            // Interleave boundary records the same way the text banners do.
            maybe_boundary_json(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |line_no, summary_chars| {
                    let obj = json!({
                        "kind": "compaction_boundary",
                        "line": line_no,
                        "summary_chars": summary_chars,
                    });
                    let s = serde_json::to_string(&obj).expect("serialize boundary");
                    println!("{s}");
                    out_blob.push_str(&s);
                    out_blob.push('\n');
                },
            );

            if let Some(u) = shown_user(turn, sel.sides) {
                emit_unit_json(sr, turn, u, &mut out_blob)?;
            }
            // The assistant lane: one object per KEPT agent message, plus a
            // `collapsed_agents` placeholder object per contiguous dropped span (carrying
            // X/Y/Z + the fetchable line range), in ascending agent order.
            for entry in shown_agent_lane(turn, sel.sides, &ctx.cfg) {
                match entry {
                    AgentRender::Kept(a) => emit_unit_json(sr, turn, &a.unit, &mut out_blob)?,
                    AgentRender::Placeholder(s) => {
                        emit_placeholder_json(sr, turn, &s, &mut out_blob)?
                    }
                }
            }
        }
    }

    // Trailing terminator object, emitted UNCONDITIONALLY (even when 0) so a JSONL consumer
    // can reliably detect end-of-stream for turns — matching search/files/recover, which
    // always close with a trailing summary. The key is `skipped_lines` (was a one-off `count`
    // alias, emitted only when > 0; both divergences are now removed for cross-subcommand
    // consistency).
    let term = crate::text::envelope_summary(json!({"skipped_lines": ctx.skipped_lines}));
    println!("{}", serde_json::to_string(&term)?);
    if let Some(p) = out_path {
        crate::recover::write_out_guarded(p, &out_blob)?;
    }
    Ok(())
}

/// Emit one unit as a JSON object. The `text` field is ALWAYS the full verbatim
/// message (json is for machines that do their own windowing); the truncation metadata
/// describes what the TEXT render would show.
pub(crate) fn emit_unit_json(
    sr: &ScanResult,
    turn: &TurnSlice,
    unit: &TurnUnit,
    out_blob: &mut String,
) -> Result<()> {
    use serde_json::json;
    let r = render_unit_body(unit, None);
    let mut obj = json!({
        "kind": "turn",
        "session_id": sr.session_id,
        // Id-domain discriminator (the r5 shape): `is_subagent` flags a bare-hex subagent
        // unit; `parent_session_id` is the always-re-feedable owning uuid (= session_id for
        // a top-level unit). A subagent `session_id` is NOT a re-feedable `@<uuid>` target.
        "is_subagent": sr.is_subagent,
        "parent_session_id": sr.parent_session_id,
        "turn_index": turn.turn_index,
        // A merged elicitation-sidecar unit (§3.10) has NO physical line — `line` is null
        // and `source:"elicitation-sidecar"` marks the provenance; a native unit omits `source`.
        "line": if unit.from_sidecar { serde_json::Value::Null } else { json!(unit.line_no) },
        "source": if unit.from_sidecar { json!("elicitation-sidecar") } else { serde_json::Value::Null },
        "role": unit.role.label(),
        "ts_utc": unit.ts_utc,
        "ts_local": unit.ts_utc.as_deref().and_then(local_iso),
        "tool_calls": turn.tool_calls,
        "full_chars": unit.full_chars,
        "rendered_chars": r.rendered_chars,
        "truncated": r.truncated,
        "elided_chars": r.elided_chars,
        "elided_lines": r.elided_lines,
        "also_in_summary": unit.also_in_summary,
        "compactions_before": turn.compactions_before,
        "text": unit.text,
    });
    // STRUCTURED automation attribution on a USER segment: a machine pulse opener carries
    // `is_automation:true` + the parsed trigger CLASS / id / status as fields, so a JSON
    // consumer distinguishes a human turn from an automation pulse WITHOUT regexing the
    // `[<kind> …]` text prefix out of the prose. A human user turn carries
    // `is_automation:false` and omits the trigger fields. (An assistant unit is never an
    // automation opener, so it always renders `is_automation:false`.)
    if let Some(map) = obj.as_object_mut() {
        let is_user_automation = unit.role == Role::User && turn.is_automation;
        map.insert("is_automation".into(), json!(is_user_automation));
        if is_user_automation {
            if let Some(t) = turn.automation.as_ref() {
                map.insert("trigger_kind".into(), json!(t.kind.slug()));
                map.insert("task_id".into(), json!(t.task_id));
                map.insert("status".into(), json!(t.status));
                // `event` is the Monitor/ScheduleWakeup real-outcome tag (null on non-monitor
                // pulses). Surfaced so a JSON consumer sees a timed-out / event-bearing monitor
                // verbatim rather than inferring `completed` from an absent status.
                map.insert("event".into(), json!(t.event));
            }
        }
        // STRUCTURED inbound-comm attribution (GOLD §1): a `<teammate-message>` opener carries
        // `is_inbound_comm:true` + the comm `label` / `from` / `to` (== `self`) so a JSON consumer
        // distinguishes a peer message from a human turn WITHOUT regexing the header. An ordinary
        // unit omits these fields (absence ⇒ not an inbound comm).
        if let Some(ic) = unit.inbound.as_ref() {
            map.insert("is_inbound_comm".into(), json!(true));
            map.insert("comm_label".into(), json!(ic.class.path()));
            map.insert("comm_from".into(), json!(ic.from));
            map.insert("comm_to".into(), json!("self"));
        }
    }
    let s = serde_json::to_string(&obj)?;
    println!("{s}");
    out_blob.push_str(&s);
    out_blob.push('\n');
    Ok(())
}

/// Emit a collapsed-agent-span placeholder as a JSON record (the machine twin of the
/// text `△ L… [X agent messages, Y tool calls, Z failed]` line). Carries the exact X/Y/Z
/// counts + the fetchable first/last jsonl line so a consumer can `Read` the raw range.
pub(crate) fn emit_placeholder_json(
    sr: &ScanResult,
    turn: &TurnSlice,
    span: &PlaceholderSpan,
    out_blob: &mut String,
) -> Result<()> {
    use serde_json::json;
    let obj = json!({
        "kind": "collapsed_agents",
        "session_id": sr.session_id,
        "is_subagent": sr.is_subagent,
        "parent_session_id": sr.parent_session_id,
        "turn_index": turn.turn_index,
        "agent_messages": span.messages,
        "tool_calls": span.tool_calls,
        "failed": span.failed,
        "first_line": span.first_line,
        "last_line": span.last_line,
        "compactions_before": turn.compactions_before,
        // Ready-to-run fetch of the collapsed span, addressed at the OWNING transcript.
        "refetch": if span.first_line == span.last_line {
            format!("csift show @{} --line {}", sr.session_id, span.first_line)
        } else {
            format!("csift show @{} --line {}..{}", sr.session_id, span.first_line, span.last_line)
        },
    });
    let s = serde_json::to_string(&obj)?;
    println!("{s}");
    out_blob.push_str(&s);
    out_blob.push('\n');
    Ok(())
}

/// JSON twin of [`maybe_boundary_banner`]: invoke `emit(line_no, summary_chars)` for
/// each crossed boundary, in ascending line order, exactly once each.
pub(crate) fn maybe_boundary_json(
    prev: &mut Option<usize>,
    current: usize,
    summaries: &[SummaryInfo],
    emit: &mut dyn FnMut(usize, usize),
) {
    for s in crossed_summaries(summaries, *prev, current) {
        emit(s.line_no, s.body_chars);
    }
    *prev = Some(current);
}
