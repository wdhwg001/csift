//! AutomationKind / AutomationTrigger -- the task-notification pulse model.

/// The TRUE class of a `<task-notification>` automation trigger, parsed from the leading
/// classifier of its `<summary>` (verified against real sessions: the summary opens with
/// `Background command "…"`, `Dynamic workflow "…"`, or `Agent …`). This is the attribution
/// the P2 turn-segmentation lens demands — the old code hardcoded the literal `workflow` for
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
    /// pulses — the `ScheduleWakeup` wakeup-tick PROMPTS that drive a monitor/cron cadence are
    /// `isMeta:true` user records (not `<task-notification>`s) and are NOT segmented here (they
    /// bypass [`Record::automation_trigger`] entirely via the isMeta gate in
    /// [`Record::is_genuine_user`]); attributing those is a deferred enhancement.
    Monitor,
    /// Any other / unrecognized classifier — the safe fallback (renders `task`).
    Task,
}

impl AutomationKind {
    /// Classify from the `<summary>`. Case-insensitive on the known leading prefixes; anything
    /// else (or a missing summary) is [`AutomationKind::Task`]. The `monitor`/`scheduled`/`cron`
    /// LEADING prefixes route a monitor-COMPLETION `<task-notification>` to
    /// [`AutomationKind::Monitor`] (the captured-monitor `Monitor event:` shape). ADDITIONALLY, a
    /// `Background command "…"` pulse whose QUOTED NAME carries a monitor-cadence token
    /// (`monitor`/`re-arm`/`relaunch monitor`/`liveness`) routes to `Monitor` too — the
    /// captured-monitor shape, where the monitor loop is a `&`-detached background command (a pure
    /// leading-prefix check disguised ALL of it as `background-command`). This does NOT cover
    /// `ScheduleWakeup` wakeup-tick prompts (isMeta records that never reach this classifier).
    #[must_use]
    pub fn from_summary(summary: Option<&str>) -> Self {
        let s = summary.unwrap_or("").trim_start();
        // The classifiers are a fixed leading phrase; match the longest-distinguishing
        // prefix case-insensitively so a `Background command "…"` is not mistaken for `task`.
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("background command") {
            // A monitor/cron cadence is FREQUENTLY implemented as a `&`-detached background
            // command whose QUOTED NAME is the monitor mechanism (`Background command
            // "Relaunch monitor timer (cycle 2)"` / `"Re-arm corrected monitor …"` /
            // `"nightly monitor tick (25min)"`). The leading classifier is `Background command`, so
            // a pure prefix check buried EVERY such pulse under `background-command` and the
            // `Monitor` class matched zero of them on a captured session. Route to `Monitor` when
            // the quoted command NAME carries a monitor-cadence token, so the dominant monitor
            // activity is attributed to its own class instead of disguised as generic bg-cmd.
            if quoted_name_is_monitor_cadence(s) {
                AutomationKind::Monitor
            } else {
                AutomationKind::BackgroundCommand
            }
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

/// True when a `Background command "<name>" …` summary's QUOTED command name names a
/// monitor / cron cadence — so the pulse is attributed to [`AutomationKind::Monitor`] rather
/// than the generic [`AutomationKind::BackgroundCommand`]. Extracts the substring between the
/// FIRST pair of double quotes (the command name) and matches a conservative set of
/// monitor-cadence tokens against it (case-insensitive): the standalone word `monitor`, or
/// `re-arm`, `relaunch monitor`, `liveness`. The match is restricted to the quoted NAME (never
/// the whole summary) so a background command that merely mentions "monitor" in trailing prose
/// is not over-captured; absent quotes, nothing matches (stays `BackgroundCommand`). Tokens
/// chosen to be strongly monitor-specific — `tick`/`cadence` alone are too broad and excluded.
pub(crate) fn quoted_name_is_monitor_cadence(summary: &str) -> bool {
    let Some(open) = summary.find('"') else {
        return false;
    };
    let rest = &summary[open + 1..];
    let Some(close) = rest.find('"') else {
        return false;
    };
    let name = rest[..close].to_ascii_lowercase();
    // The standalone word `monitor` (not a substring of a larger word) is the dominant signal;
    // `re-arm` / `relaunch monitor` / `liveness` cover the re-arming-loop names.
    name.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "monitor" || w == "liveness")
        || name.contains("re-arm")
        || name.contains("relaunch monitor")
}

/// A parsed `<task-notification>` automation trigger — the stable inner tags of a
/// machine-injected background-command / workflow / spawned-task completion notice. Every
/// field is `Option` because a malformed / partial notification must degrade gracefully
/// (the label still renders with `?`/`completed` fallbacks) rather than be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTrigger {
    /// The TRUE trigger class (parsed from the `<summary>` classifier) — the attribution the
    /// label renders, replacing the prior hardcoded `workflow`.
    pub kind: AutomationKind,
    /// The `<task-id>` (the workflow / background-command id), if present.
    pub task_id: Option<String>,
    /// The `<status>` (`completed` / `failed` / …), if present.
    pub status: Option<String>,
    /// The `<summary>` (the human-readable "what completed" line), if present.
    pub summary: Option<String>,
    /// The `<event>` payload, if present — where a Monitor / ScheduleWakeup pulse carries its
    /// real outcome (`STAGE2_OUTPUT_READY`, `[Monitor timed out — re-arm if needed.]`). Often
    /// the only outcome signal on a Monitor pulse (which usually has no `<status>`), so the
    /// label falls back to it instead of fabricating `completed`.
    pub event: Option<String>,
}

/// Extract the text between `<tag>` and `</tag>` in `s`, trimmed, or `None` when the tag
/// is absent or empty. Codepoint-safe: `str::find` returns ASCII byte offsets of the
/// (ASCII) tag delimiters, and the slice is taken on those offsets only — never inside the
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
/// and trim the ends, so an excerpt renders on one line. Does NOT truncate —
/// length capping with an explicit `… (+N chars)` marker is the caller's job.
pub(crate) fn normalize_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
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
// role.class.sub classification engine (GOLD plan §2–§6) — ADDITIVE, P1.
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
