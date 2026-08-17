//! Per-subagent lifecycle: frozen-lane detection, status resolution, durations.

use super::*;

/// Compute the lifecycle of one subagent: read its transcript HEAD for the start
/// timestamp + TAIL for the completion timestamp & terminal-message signal, consult
/// the workflow journal for an explicit `result`, then resolve a status.
pub fn lifecycle(subagent: &Subagent, journals: &JournalCache) -> Result<SubagentLifecycle> {
    // agent_type + description were read from meta.json ONCE at discovery ([`make_subagent`])
    // and stored on the `Subagent` - no second meta read here (FIX3: kills one redundant
    // meta.json read + parse per subagent). The values are identical to `read_meta`'s.
    let agent_type = subagent.agent_type.clone();
    let description = subagent.description.clone();

    // HEAD: first record's timestamp == start. We do not need genuine-user logic
    // here - the very first record (isSidechain user seed) IS the start instant.
    let mut started_utc: Option<String> = None;
    let (head_skipped, head_consumed) = head_records(&subagent.path, |rec| {
        if let Some(ts) = &rec.timestamp {
            started_utc = Some(ts.clone());
            return false; // first timestamped record is enough
        }
        true
    })?;

    // TAIL: last record's timestamp == completion (best-effort), whether the transcript
    // terminates with a visible assistant message (a clean finish), AND whether the lane is
    // FROZEN at an unreturned tool_use. The frozen verdict comes from the NEWEST meaningful
    // record only (the first non-metadata record from EOF): if it is an assistant tool_use, no
    // tool_result followed it (it IS the last record) ⇒ the lane is blocked there, NOT done. The
    // terminal_agent_msg walk-back is UNCHANGED for every non-frozen lane.
    let mut completed_utc: Option<String> = None;
    let mut terminal_agent_msg = false;
    let mut saw_any = false;
    let mut newest_decided = false;
    let mut pending: Option<PendingToolUse> = None;
    // Disjoint from the head window (R12): a malformed line is never double-booked.
    let tail_skipped = tail_records(&subagent.path, head_consumed, |rec| {
        saw_any = true;
        if completed_utc.is_none() {
            if let Some(ts) = &rec.timestamp {
                completed_utc = Some(ts.clone());
            }
        }
        if !newest_decided {
            if let Some(tu) = newest_pending_tool_use(rec) {
                pending = Some(tu); // newest meaningful record is an unreturned tool_use → frozen
                newest_decided = true;
            } else if record_is_meaningful(rec) {
                newest_decided = true; // newest meaningful record is resolved/active → not frozen
            }
            // else: isMeta / system / metadata-only → keep looking for the newest meaningful one
        }
        // The newest assistant record carrying visible text == a clean end-of-turn.
        if !terminal_agent_msg && rec.agent_text().is_some() {
            terminal_agent_msg = true;
        }
        // Stop once we have the completion timestamp AND a terminal-message verdict;
        // if the very newest record has no text we still only need a couple of reads.
        completed_utc.is_none() || !terminal_agent_msg
    })?;

    let journal_done = journal_reports_completion(subagent, journals);
    // Clear the frozen signal when it would be meaningless: a journal-completed (workflow) agent is
    // trusted done regardless of a tail tool_use; and a transcript with NO timestamps has
    // undetermined timing (status Unknown), so we cannot claim "frozen" vs merely unreadable.
    if journal_done || started_utc.is_none() {
        pending = None;
    }
    // A genuinely frozen lane is NEVER "completed": override the terminal-text walk-back, which
    // would otherwise find an EARLIER end-of-turn (the assistant's text before the frozen
    // tool_use) and mis-report the stuck lane as done.
    let status = if pending.is_some() {
        SubagentStatus::Running
    } else {
        resolve_status(
            saw_any,
            journal_done,
            terminal_agent_msg,
            started_utc.is_some(),
        )
    };

    Ok(SubagentLifecycle {
        agent_type,
        description,
        started_utc,
        completed_utc,
        status,
        pending,
        skipped_lines: head_skipped + tail_skipped,
    })
}

/// The newest-meaningful-record frozen check: if `rec` is an assistant carrying ≥1 tool_use block,
/// return the pending tool_use (the DANGEROUS Bash one if present, else the first) - because it is
/// the last record, no tool_result resolved it. `None` for any non-(assistant-with-tool_use) record.
pub(crate) fn newest_pending_tool_use(rec: &Record) -> Option<PendingToolUse> {
    if !rec.is_type("assistant") {
        return None;
    }
    let blocks = rec.blocks()?;
    let mut chosen: Option<(String, String, Option<String>)> = None;
    for b in blocks {
        let Block::ToolUse {
            id: Some(id),
            name: Some(name),
            input,
        } = b
        else {
            continue;
        };
        let command = if name == "Bash" {
            input
                .as_ref()
                .and_then(|v| v.get("command"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        } else {
            None
        };
        // Prefer a tool_use CC would hoist (dangerous rm) so classification is escalation-blocked.
        if command
            .as_deref()
            .is_some_and(crate::bash_danger::is_dangerous_rm)
        {
            return Some(PendingToolUse {
                tool_use_id: id.clone(),
                tool_name: name.clone(),
                command,
                since_utc: rec.timestamp.clone(),
            });
        }
        if chosen.is_none() {
            chosen = Some((id.clone(), name.clone(), command));
        }
    }
    chosen.map(|(tool_use_id, tool_name, command)| PendingToolUse {
        tool_use_id,
        tool_name,
        command,
        since_utc: rec.timestamp.clone(),
    })
}

/// True for a record that resolves/advances the lane - a tool_result carrier, a clean assistant
/// end-of-turn text, or a genuine user message. (NOT an unreturned tool_use, NOT isMeta/system
/// metadata.) Used to find the newest MEANINGFUL record when deciding the frozen verdict.
pub(crate) fn record_is_meaningful(rec: &Record) -> bool {
    rec.agent_text().is_some()
        || rec.is_genuine_user()
        || rec
            .blocks()
            .is_some_and(|bs| bs.iter().any(|b| matches!(b, Block::ToolResult { .. })))
}

/// Status resolution rule (honest, never over-claiming "failed"):
/// - journal `result` event present ⇒ `Completed` (the authoritative workflow signal);
/// - else a terminal visible-assistant message ⇒ `Completed` (clean transcript end);
/// - else if we saw records but no completion signal ⇒ `Running`;
/// - else (no records / no start) ⇒ `Unknown`.
pub(crate) fn resolve_status(
    saw_any: bool,
    journal_done: bool,
    terminal_agent_msg: bool,
    has_start: bool,
) -> SubagentStatus {
    if journal_done || terminal_agent_msg {
        SubagentStatus::Completed
    } else if saw_any && has_start {
        SubagentStatus::Running
    } else {
        SubagentStatus::Unknown
    }
}

/// Human-readable duration between start and completion (raw UTC ISO8601), e.g.
/// `14m20s`, `3s`, `2h05m`. `None` when either bound is missing/unparseable.
#[must_use]
pub fn duration_label(started: Option<&str>, completed: Option<&str>) -> Option<String> {
    let s: jiff::Timestamp = started?.parse().ok()?;
    let c: jiff::Timestamp = completed?.parse().ok()?;
    let secs = (c.as_second() - s.as_second()).max(0);
    Some(fmt_secs(secs))
}

/// Format a whole-second duration compactly.
pub(crate) fn fmt_secs(total: i64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

// ───────────────────────── TOPOLOGY (Part A) ─────────────────────────
//
// csift modelled subagents as a flat list of detached session-like files. The topology
// builder LINKS each subagent back to the parent tool_use that spawned it, via the
// `meta.json` `toolUseId` ⇆ parent transcript `tool_use.id` join. From that join it
// recovers (a) the TRUE trigger time (the parent tool_use ts, which the child-head ts
// lags by seconds), and (b) the RETURNED MESSAGE through a 3-way resolver. Workflow runs
// are surfaced as `WorkflowRun` parent nodes from the unscanned top-level
// `workflows/wf_*.json` manifests. The build is ADDITIVE - it reuses the existing
// discovery + lifecycle primitives, never rewrites them.
