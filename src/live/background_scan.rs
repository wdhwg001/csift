//! The background-task scanner: the five raw-byte needles, the launch and carrier
//! ingesters, the carrier-to-launch join, and the output-file stat. `background.rs`
//! owns the types, the lens and the report; this file owns the per-line work.

use super::*;
use std::collections::BTreeMap;

use memchr::memmem;

/// A completion carrier seen during the scan, resolved after every launch is known.
pub(crate) struct Carrier {
    pub(crate) task_ids: Vec<String>,
    pub(crate) tool_use_id: Option<String>,
    pub(crate) status: Option<String>,
    /// The `<event>` payload (a Monitor pulse carries its outcome here, no `<status>`).
    pub(crate) event: Option<String>,
    pub(crate) ts: Option<String>,
    pub(crate) orphan_summary: bool,
}

/// The five raw-byte needles (R13 law: bare value substrings, serialization-safe).
pub(crate) fn line_is_bg_candidate(line: &[u8]) -> bool {
    static FINDERS: std::sync::LazyLock<Vec<memmem::Finder<'static>>> =
        std::sync::LazyLock::new(|| {
            [
                &b"run_in_background"[..],
                b"Command running in background",
                b"async_launched",
                b"task-notification",
                b"stopped by the user",
                b"\"Monitor\"",
                b"Monitor started",
            ]
            .into_iter()
            .map(memmem::Finder::new)
            .collect()
        });
    FINDERS.iter().any(|f| f.find(line).is_some())
}

/// A backgrounded shell launch (assistant tool_use) or its result / an async agent
/// launch (user tool_result with the sentinel `toolUseResult`).
pub(crate) fn ingest_launches(rec: &Record, lane: &str, tasks: &mut BTreeMap<String, BgTask>) {
    let Some(blocks) = rec.blocks() else {
        return;
    };
    for b in blocks {
        match b {
            Block::ToolUse {
                id: Some(id),
                name: Some(name),
                input: Some(input),
                ..
            } if (matches!(name.as_str(), "Bash" | "PowerShell")
                && input
                    .get("run_in_background")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
                || name == "Monitor" =>
            {
                let get = |k: &str| {
                    input
                        .get(k)
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                };
                // A websocket monitor has no command: its url is the thing to name.
                let command = get("command").or_else(|| {
                    input
                        .get("ws")
                        .and_then(|w| w.get("url"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });
                tasks.entry(id.clone()).or_insert(BgTask {
                    kind: if name == "Monitor" {
                        BgKind::Monitor
                    } else {
                        BgKind::Shell
                    },
                    id: None,
                    tool_use_id: id.clone(),
                    description: get("description"),
                    command,
                    launched_utc: rec.timestamp.clone(),
                    lane: lane.to_string(),
                    output_file: None,
                    state: BgState::Open,
                    returned_utc: None,
                    output_bytes: None,
                    output_age_secs: None,
                    ignored_by: None,
                });
            }
            Block::ToolResult {
                tool_use_id: Some(tuid),
                content,
                ..
            } => {
                if let Some(task) = tasks.get_mut(tuid) {
                    // The shell result: the task id + output path live in the text (and
                    // the id also in `toolUseResult.backgroundTaskId`); a Monitor arm reads
                    // `Monitor started (task <id>, …)` with `toolUseResult.taskId`.
                    let text = content
                        .as_ref()
                        .map(crate::model::tool_result_content_text)
                        .unwrap_or_default();
                    if task.id.is_none() {
                        task.id = after_marker(&text, "with ID: ")
                            .or_else(|| after_marker(&text, "(task "))
                            .or_else(|| {
                                let v = rec.tool_use_result_value()?;
                                v.get("backgroundTaskId")
                                    .or_else(|| v.get("taskId"))?
                                    .as_str()
                                    .map(str::to_string)
                            });
                    }
                    if task.output_file.is_none() {
                        task.output_file = after_marker(&text, "written to: ");
                    }
                    continue;
                }
                // An async agent launch: the sentinel status on the structured echo.
                let probe = rec.tur_probe();
                let launched = probe
                    .as_ref()
                    .and_then(|p| p.status.as_ref())
                    .and_then(serde_json::Value::as_str)
                    == Some("async_launched");
                if !launched {
                    continue;
                }
                let v = rec
                    .tool_use_result_value()
                    .unwrap_or(serde_json::Value::Null);
                let s = |k: &str| {
                    v.get(k)
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                };
                tasks.entry(tuid.clone()).or_insert(BgTask {
                    kind: BgKind::Agent,
                    id: s("agentId"),
                    tool_use_id: tuid.clone(),
                    description: s("description"),
                    command: None,
                    launched_utc: rec.timestamp.clone(),
                    lane: lane.to_string(),
                    output_file: s("outputFile"),
                    state: BgState::Open,
                    returned_utc: None,
                    output_bytes: None,
                    output_age_secs: None,
                    ignored_by: None,
                });
            }
            _ => {}
        }
    }
}

/// `<marker><token>.` - the shell result text's id / path fields (a token ends at the
/// first `.`-then-whitespace/end or whitespace).
pub(crate) fn after_marker(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let rest = &text[start..];
    let end = rest
        .char_indices()
        .find(|&(i, c)| {
            c.is_whitespace()
                || c == ','
                || c == ')'
                || (c == '.' && rest[i + 1..].starts_with([' ', '\n']))
        })
        .map_or(rest.len(), |(i, _)| i);
    let tok = rest[..end].trim_end_matches('.');
    (!tok.is_empty()).then(|| tok.to_string())
}

/// Every completion carrier on a MAIN-lane record: a user string record, a
/// `queue-operation` line, or a `queued_command` attachment, each holding one or more
/// `<task-notification>` sections; plus the unjoinable agents-stopped notice.
pub(crate) fn ingest_carriers(rec: &Record, carriers: &mut Vec<Carrier>, notes: &mut Vec<String>) {
    let Some(text) = carrier_text(rec) else {
        return;
    };
    if crate::model::is_agents_stopped_notice(&text) {
        // The notice rides both a queue enqueue line and the user record: one note per
        // (count, second). It names no id, so csift cannot say WHICH agents it stopped.
        let head = text.trim_start();
        let n = head.bytes().take_while(u8::is_ascii_digit).count();
        let count = if n == 0 { "1" } else { &head[..n] };
        let ts = rec.timestamp.as_deref().unwrap_or("?");
        let note = format!(
            "{count} background agent(s) were stopped by the user at {} - the notice names \
             no id, so csift cannot mark which agents it stopped",
            ts.get(..19).unwrap_or(ts)
        );
        if !notes.contains(&note) {
            notes.push(note);
        }
        return;
    }
    for section in text.split(TASK_NOTIFICATION_PREFIX).skip(1) {
        let task_ids = all_xml_tags(section, "task-id");
        let orphan = task_ids.iter().any(|t| t.starts_with("__orphan_summary__"));
        carriers.push(Carrier {
            task_ids,
            tool_use_id: extract_xml_tag(section, "tool-use-id"),
            status: extract_xml_tag(section, "status"),
            event: extract_xml_tag(section, "event"),
            ts: rec.timestamp.clone(),
            orphan_summary: orphan,
        });
    }
}

/// The text a main-lane record carries as a possible notification carrier: a user
/// string record, a `queue-operation` line's `content`, or a `queued_command`
/// attachment's `prompt`. A pulse absorbed mid-turn exists ONLY on the queue line and
/// the attachment (measured at 2.1.258 over the main transcripts of this corpus: 3218
/// pulse-bearing user records against 5893 queue enqueue lines), so a reader of
/// user records alone misses roughly every other completion.
pub(crate) fn carrier_text(rec: &Record) -> Option<String> {
    if rec.is_type("queue-operation") {
        rec.content_str().map(str::to_string)
    } else if rec.attachment_type().as_deref() == Some("queued_command") {
        rec.attachment_value()
            .and_then(|v| v.get("prompt")?.as_str().map(str::to_string))
    } else if let Some(Content::Text(s)) = rec.message.as_ref().and_then(|m| m.content.as_ref()) {
        Some(s.clone())
    } else {
        None
    }
}

/// The `<task-notification>` pulses a freshly appended main-lane record delivers, as
/// their rendered labels: a user record (the idle delivery), a queue-operation ENQUEUE
/// line, or a `queued_command` attachment (the mid-turn delivery). A queue `remove`
/// or `dequeue` line repeats a pulse the enqueue already carried, so it delivers none.
pub(crate) fn delivered_pulse_labels(rec: &Record) -> Vec<String> {
    if rec.is_type("queue-operation") && rec.operation.as_deref() != Some("enqueue") {
        return Vec::new();
    }
    let Some(text) = carrier_text(rec) else {
        return Vec::new();
    };
    text.split(TASK_NOTIFICATION_PREFIX)
        .skip(1)
        .map(|section| {
            crate::model::automation_label_for_section(&format!(
                "{TASK_NOTIFICATION_PREFIX}{section}"
            ))
        })
        .collect()
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

/// Join carriers to launches: `<tool-use-id>` first (exact), any `<task-id>` second.
/// The latest carrier wins (an agent notifies again after a resume).
pub(crate) fn resolve_carriers(
    tasks: &mut BTreeMap<String, BgTask>,
    carriers: &[Carrier],
    notes: &mut Vec<String>,
) {
    let mut by_id: BTreeMap<String, String> = BTreeMap::new();
    for (tuid, t) in tasks.iter() {
        if let Some(id) = &t.id {
            by_id.insert(id.clone(), tuid.clone());
        }
    }
    let mut orphaned = 0usize;
    for c in carriers {
        let mut keys: Vec<String> = Vec::new();
        if let Some(t) = &c.tool_use_id {
            if tasks.contains_key(t) {
                keys.push(t.clone());
            }
        }
        for id in &c.task_ids {
            if let Some(t) = by_id.get(id) {
                if !keys.contains(t) {
                    keys.push(t.clone());
                }
            }
        }
        for key in keys {
            if let Some(task) = tasks.get_mut(&key) {
                let state = if c.orphan_summary {
                    orphaned += 1;
                    Some(BgState::Stopped)
                } else if c.status.is_some() {
                    Some(BgState::from_status(c.status.as_deref()))
                } else if c
                    .event
                    .as_deref()
                    .is_some_and(|e| e.to_ascii_lowercase().contains("timed out"))
                {
                    Some(BgState::TimedOut)
                } else {
                    None // a Monitor event pulse: the monitor is still armed
                };
                if let Some(state) = state {
                    task.state = state;
                    task.returned_utc = c.ts.clone();
                }
            }
        }
    }
    if orphaned > 0 {
        notes.push(format!(
            "{orphaned} task(s) were reconciled as stopped by Claude Code at a later session \
             start (its orphan summary: no completion record; a UI stop, a Monitor timeout or \
             agent teardown leaves no transcript marker)"
        ));
    }
}

/// One `stat` per open task: is the output file still growing?
pub(crate) fn stat_output(t: &mut BgTask) {
    let Some(p) = t.output_file.as_deref() else {
        return;
    };
    let Ok(meta) = std::fs::metadata(p) else {
        return;
    };
    t.output_bytes = Some(meta.len());
    if let Ok(modified) = meta.modified() {
        if let Ok(age) = std::time::SystemTime::now().duration_since(modified) {
            t.output_age_secs = Some(i64::try_from(age.as_secs()).unwrap_or(i64::MAX));
        }
    }
}
