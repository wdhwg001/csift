//! AutomationKind / AutomationTrigger -- the task-notification pulse model.

use super::TASK_NOTIFICATION_CLOSE;

/// The TRUE class of a `<task-notification>` automation trigger, parsed from the leading
/// classifier of its `<summary>` (verified against real sessions: the summary opens with
/// `Background command "…"`, `Dynamic workflow "…"`, or `Agent …`). This is the attribution
/// the P2 turn-segmentation lens demands - the old code hardcoded the literal `workflow` for
/// EVERY trigger, mislabeling background-command + agent pulses (81% on a captured session).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationKind {
    /// A `Background command "…"` completion pulse (a `&`-detached shell command CC ran).
    BackgroundCommand,
    /// A `Dynamic workflow "…"` completion pulse (an OMC / dynamic workflow run).
    Workflow,
    /// An `Agent …` completion pulse (a spawned subagent).
    Agent,
    /// A monitor / cron cadence COMPLETION pulse. Matches a `<task-notification>` whose summary
    /// EITHER opens `Monitor`/`scheduled`/`cron` (the captured-monitor shape: `Monitor event: …`)
    /// OR opens `Background command "…"` with a monitor-cadence token in the quoted command NAME
    /// (the captured-monitor shape: `Relaunch monitor timer (cycle N)`, `Re-arm corrected monitor …`,
    /// `nightly monitor tick (25min)`). The captured session's monitor loop is implemented as `&`-detached
    /// background commands, so without the quoted-name scan it ALL read as `background-command`
    /// and this class matched zero of it. NOTE: this still matches only `<task-notification>`
    /// pulses - the `ScheduleWakeup` wakeup-tick PROMPTS that drive a monitor/cron cadence are
    /// `isMeta:true` user records (not `<task-notification>`s) and are NOT segmented here (they
    /// bypass [`Record::automation_trigger`] entirely via the isMeta gate in
    /// [`Record::is_genuine_user`]); attributing those is a deferred enhancement.
    Monitor,
    /// Any other / unrecognized classifier - the safe fallback (renders `task`).
    Task,
}

impl AutomationKind {
    /// Classify from the `<summary>`. Case-insensitive on the known leading prefixes; anything
    /// else (or a missing summary) is [`AutomationKind::Task`]. The `monitor`/`scheduled`/`cron`
    /// LEADING prefixes route a Monitor-tool pulse or termination notice
    /// (`Monitor event: …` / `Monitor "…" …`) to [`AutomationKind::Monitor`]. A `Background
    /// command "…"` pulse is ALWAYS `background-command`, whatever its quoted name says
    /// (v0.10.0: the former quoted-name heuristic - `monitor`/`re-arm`/`liveness` in the name
    /// routed to `Monitor` - predates the real Monitor tool and double-booked; measured on one
    /// project it produced 40 `monitor` records against zero genuine Monitor pulses). This
    /// does NOT cover `ScheduleWakeup` wakeup-tick prompts (isMeta records that never reach
    /// this classifier).
    #[must_use]
    pub fn from_summary(summary: Option<&str>) -> Self {
        let s = summary.unwrap_or("").trim_start();
        // The classifiers are a fixed leading phrase; match the longest-distinguishing
        // prefix case-insensitively so a `Background command "…"` is not mistaken for `task`.
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("background command") {
            AutomationKind::BackgroundCommand
        } else if lower.starts_with("dynamic workflow") || lower.starts_with("workflow") {
            AutomationKind::Workflow
        } else if lower.starts_with("monitor")
            || lower.starts_with("scheduled")
            || lower.starts_with("cron")
        {
            AutomationKind::Monitor
        } else if lower.starts_with("agent") {
            AutomationKind::Agent
        } else {
            AutomationKind::Task
        }
    }

    /// The stable lowercase slug rendered in the `[<kind> <id> <status>]` label.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            AutomationKind::BackgroundCommand => "background-command",
            AutomationKind::Workflow => "workflow",
            AutomationKind::Agent => "agent",
            AutomationKind::Monitor => "monitor",
            AutomationKind::Task => "task",
        }
    }
}

/// A parsed `<task-notification>` automation trigger - the stable inner tags of a
/// machine-injected background-command / workflow / spawned-task completion notice. Every
/// field is `Option` because a malformed / partial notification must degrade gracefully
/// (the label still renders with `?`/`completed` fallbacks) rather than be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTrigger {
    /// The TRUE trigger class (parsed from the `<summary>` classifier) - the attribution the
    /// label renders, replacing the prior hardcoded `workflow`.
    pub kind: AutomationKind,
    /// The FIRST REAL `<task-id>` (the workflow / background-command id), if present -
    /// never an orphan-reconciliation sentinel, which names no task ([`section_task_ids`]).
    pub task_id: Option<String>,
    /// The `<status>` (`completed` / `failed` / …), if present.
    pub status: Option<String>,
    /// The `<summary>` (the human-readable "what completed" line), if present.
    pub summary: Option<String>,
    /// The `<event>` payload, if present - where a Monitor / ScheduleWakeup pulse carries its
    /// real outcome (`STAGE2_OUTPUT_READY`, `[Monitor timed out - re-arm if needed.]`). Often
    /// the only outcome signal on a Monitor pulse (which usually has no `<status>`), so the
    /// label falls back to it instead of fabricating `completed`.
    pub event: Option<String>,
}

/// The PREFIX of every `<task-id>` tag that names no task. At the next session start Claude
/// Code reconciles the tasks a previous session left open with ONE notification listing them
/// all, and it lists two sentinels among their ids: `__orphan_summary__:<kind>` names the kind
/// being reconciled, `__orphan_summary_live__:<id>` excludes a still-live task from the summary.
/// The pulse's own summary calls them "internal scan markers, not tasks", so a rendered id list
/// must never carry one.
pub(crate) const ORPHAN_SENTINEL_PREFIX: &str = "__orphan_summary";

/// The sentinel naming the reconciled KIND: `__orphan_summary__:<agent|shell|workflow>`.
pub(crate) const ORPHAN_KIND_SENTINEL: &str = "__orphan_summary__:";

/// The `<task-id>` tags of ONE `<task-notification>` section, split into the tasks and the
/// markers: every REAL id in document order, plus the kind an orphan-reconciliation sentinel
/// names. A reconciliation pulse closes SEVERAL tasks at once, so reading only the first id
/// hides the rest - and the first can itself be a sentinel, which names no task at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskIds {
    /// Every real task id the section names, sentinels excluded.
    pub ids: Vec<String>,
    /// The `agent` / `shell` / `workflow` an `__orphan_summary__:` sentinel names; `None` on
    /// an ordinary completion pulse.
    pub orphan_kind: Option<String>,
}

/// Split one `<task-notification>` section's `<task-id>` tags into [`TaskIds`]. Bounded to the
/// FIRST section, so a WHOLE-RECORD call on a batched record reads the same section the rest of
/// the label reads ([`extract_xml_tag`]'s first-occurrence semantics).
pub(crate) fn section_task_ids(section: &str) -> TaskIds {
    let scope = match section.find(TASK_NOTIFICATION_CLOSE) {
        Some(end) => &section[..end],
        None => section,
    };
    let mut out = TaskIds::default();
    for tag in all_xml_tags(scope, "task-id") {
        if let Some(kind) = tag.strip_prefix(ORPHAN_KIND_SENTINEL) {
            if out.orphan_kind.is_none() && !kind.is_empty() {
                out.orphan_kind = Some(kind.to_string());
            }
        } else if !tag.starts_with(ORPHAN_SENTINEL_PREFIX) {
            out.ids.push(tag);
        }
    }
    out
}

/// Every `<tag>…</tag>` value in order (an orphan summary carries several).
pub(crate) fn all_xml_tags(s: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(i) = s[at..].find(&open) {
        let start = at + i + open.len();
        let Some(j) = s[start..].find(&close) else {
            break;
        };
        let inner = s[start..start + j].trim();
        if !inner.is_empty() {
            out.push(inner.to_string());
        }
        at = start + j + close.len();
    }
    out
}

/// Extract the text between `<tag>` and `</tag>` in `s`, trimmed, or `None` when the tag
/// is absent or empty. Codepoint-safe: `str::find` returns ASCII byte offsets of the
/// (ASCII) tag delimiters, and the slice is taken on those offsets only - never inside the
/// (possibly CJK) body. A missing close tag yields `None` (never a runaway slice).
pub(crate) fn extract_xml_tag(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let end_rel = s[start..].find(&close)?;
    let inner = s[start..start + end_rel].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// Collapse all runs of ASCII whitespace (incl. newlines/tabs) to single spaces
/// and trim the ends, so an excerpt renders on one line. Does NOT truncate -
/// length capping with an explicit `… (+N chars)` marker is the caller's job.
pub(crate) fn normalize_line(s: &str) -> String {
    normalize_collapse(s, |_| {})
}

/// [`normalize_line`] plus, for every `\n` in `s`, the CHAR offset in the RESULT at which
/// that newline's collapsed whitespace run landed. The offsets are what lets a consumer of
/// the one-line form still answer a question about the original's LINES - a newline is not a
/// character of the result, so its only address is the position of the space that replaced
/// its run (several newlines in one run therefore share an offset, which is correct: they
/// were collapsed together). Offsets are ascending. A newline in a LEADING run reads 0 (that
/// run pushes no space) and one in the TRAILING run reads the result's own length (its space
/// is trimmed away), so both sit outside any interior span by construction.
pub(crate) fn normalize_line_with_newlines(s: &str) -> (String, Vec<u32>) {
    let mut positions: Vec<u32> = Vec::new();
    let out = normalize_collapse(s, |at| positions.push(at));
    (out, positions)
}

/// The ONE whitespace-collapse walk both public forms run, so the string they produce can
/// never drift apart. `on_newline` is invoked, for each `\n`, with the result-char offset of
/// the space standing in for its run; [`normalize_line`] passes a no-op that inlines away.
fn normalize_collapse(s: &str, mut on_newline: impl FnMut(u32)) -> String {
    let mut out = String::with_capacity(s.len());
    // Result-char count so far, and the offset of the current run's collapsed space.
    let mut chars = 0usize;
    let mut run_at = 0usize;
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                run_at = chars;
                if !out.is_empty() {
                    out.push(' ');
                    chars += 1;
                }
            }
            prev_ws = true;
            if ch == '\n' {
                on_newline(run_at.min(u32::MAX as usize) as u32);
            }
        } else {
            out.push(ch);
            chars += 1;
            prev_ws = false;
        }
    }
    // Trim a possible trailing space from the run-collapse above.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

// ============================================================================
// role.class.sub classification engine (GOLD plan §2–§6) - ADDITIVE, P1.
//
// This is the NEW taxonomy core, testable in isolation. It is NOT yet wired into any
// consumer (the legacy `cli::Category` + `-t` selector still drive output); P2 cuts the
// surfaces over to [`Record::classify`] and removes the old enum. Until then the new
// items carry a targeted `#[allow(dead_code)]` (the binary never calls them yet).
//
// GOLD GAPS surfaced during P1 (reported upstream, not silently absorbed):
//   - `harness.schedule.wakeup`: the FIRED autonomous-loop / `ScheduleWakeup` timer tick is
//     detected via its fixed markers ([`SCHEDULE_WAKEUP_MARKER`] sentinel +
//     [`SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX`] / [`SCHEDULE_WAKEUP_TIMER_MARKER`], P1c M2a). A
//     GENERIC cron/monitor tick's injected prompt is still operator-authored free text with no
//     universal marker; such an isMeta tick that matches no marker is EXCLUDED (P1c M2b: an
//     isMeta record is never `user.message`), not mislabeled. The `ScheduleWakeup` *tool_use*
//     (the agent ARMING a wakeup) is classified `agent.tool.use`, not the harness tick.
//   - `agent.thinking` covers BOTH [`Block::Thinking`] and [`Block::RedactedThinking`] (the
//     encrypted/opaque thinking form). The latter is UNATTESTED in the current corpus (oracle
//     B3/G7) so it is exercised by a SYNTHETIC fixture; it carries no readable text, so the
//     render surfaces a `[redacted thinking]` placeholder while still classifying `agent.thinking`.
// ============================================================================
