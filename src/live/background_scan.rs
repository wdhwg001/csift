//! The background-task scanner: the raw-byte needles, the launch and carrier
//! ingesters, the receipt-minted launches and their second pass, the carrier-to-launch
//! join, and the output-file stat. `background.rs` owns the types, the lens and the
//! report; this file owns the per-line work.

use super::*;
use std::collections::BTreeMap;

use memchr::memmem;

/// The four arms of Claude Code's ONE background-ack template, by their opening clause.
/// Three of them are entrances the model never asked for, and they are the only trace:
/// the launching `tool_use` carries no `run_in_background` and is never rewritten.
const MANUAL_HEAD: &str = "Command was manually backgrounded by user with ID: ";
const DELIVER_HEAD: &str = "Command was moved to the background (ID: ";
const TIMEOUT_HEAD: &str = "Command did not complete within its ";
const TIMEOUT_TAIL: &str = "s timeout and was moved to the background (ID: ";
/// Each arm's own closing clause - the second half of the sentence, without which the
/// text is a RENDERING of the template rather than a receipt of one.
const OUTPUT_CLAUSE: &str = ". Output is being written to: ";
const DELIVER_CLAUSE: &str = ") so that a message";

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

/// The raw-byte needles (R13 law: bare value substrings, serialization-safe). The last
/// two reach the receipts of the three harness-side entrances; the second of them covers
/// both the timeout and the deliver-message sentence.
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
                b"manually backgrounded by user with ID: ",
                b"moved to the background (ID: ",
            ]
            .into_iter()
            .map(memmem::Finder::new)
            .collect()
        });
    FINDERS.iter().any(|f| f.find(line).is_some())
}

/// A `local_bash` task id (claim BG-009): the kind prefix `b` plus exactly 8 base36
/// characters. A rendering of the template has `${e}` or `<id>` in that slot instead.
fn is_task_id(s: &str) -> bool {
    s.len() == 9
        && s.starts_with('b')
        && s[1..]
            .bytes()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
}

/// The `<id>` sitting between here and `close`, and whatever follows it - `None` unless
/// the slot holds a well-formed task id.
fn id_before<'a>(rest: &'a str, close: &str) -> Option<(&'a str, &'a str)> {
    let (id, tail) = rest.split_once(close)?;
    is_task_id(id).then_some((id, tail))
}

/// The manual arm's output path names the task's OWN file, so its stem is the task id.
fn path_stem_is(tail: &str, id: &str) -> bool {
    let Some(path) = tail.split_whitespace().next() else {
        return false;
    };
    let base = path
        .trim_end_matches('.')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    base.split('.').next() == Some(id)
}

/// The entrance a shell receipt announces, or `None` when the text is not one of the
/// three harness-side arms. Matching the OPENING clause is not enough: a transcript can
/// hold a RENDERING of the template (a grep of the harness binary is a real corpus
/// shape), and minting a task from one would be this fix's own mirror image. So each arm
/// must carry its whole sentence - the opening clause, a task id of the `local_bash`
/// grammar in the id slot, and the arm's own closing clause: the manual arm's output
/// path must name the task's own file, the timeout arm needs its `<N>s timeout` digits
/// and a `)` right after the id, and the deliver arm needs its message clause.
pub(crate) fn receipt_entrance(text: &str) -> Option<(BgEntrance, String)> {
    let t = text.trim_start();
    if let Some(rest) = t.strip_prefix(MANUAL_HEAD) {
        let (id, tail) = id_before(rest, OUTPUT_CLAUSE)?;
        return path_stem_is(tail, id).then(|| (BgEntrance::User, id.to_string()));
    }
    if let Some(rest) = t.strip_prefix(DELIVER_HEAD) {
        let (id, _) = id_before(rest, DELIVER_CLAUSE)?;
        return Some((BgEntrance::DeliverMessage, id.to_string()));
    }
    let (secs, rest) = t.strip_prefix(TIMEOUT_HEAD)?.split_once(TIMEOUT_TAIL)?;
    if secs.is_empty() || !secs.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (id, _) = id_before(rest, ")")?;
    Some((BgEntrance::Timeout, id.to_string()))
}

/// A backgrounded shell launch (assistant tool_use) or its result / an async agent
/// launch (user tool_result with the sentinel `toolUseResult`) / a shell the harness
/// moved into the background, which exists only as its receipt. `minted` collects the
/// tool_use ids of the last kind, for the second pass that recovers their launch lines.
pub(crate) fn ingest_launches(
    rec: &Record,
    lane: &str,
    tasks: &mut BTreeMap<String, BgTask>,
    minted: &mut Vec<String>,
) {
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
                    entered_by: (name != "Monitor").then_some(BgEntrance::Model),
                    timed_out_after_ms: None,
                    launch_note: None,
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
            } => ingest_result(rec, tuid, content.as_ref(), lane, tasks, minted),
            _ => {}
        }
    }
}

/// One `tool_result` block: the ack of a launch already known, the receipt of a
/// harness-side entrance (which is the launch's only trace), or an async agent's
/// sentinel echo.
fn ingest_result(
    rec: &Record,
    tuid: &str,
    content: Option<&serde_json::Value>,
    lane: &str,
    tasks: &mut BTreeMap<String, BgTask>,
    minted: &mut Vec<String>,
) {
    let text = content
        .map(crate::model::tool_result_content_text)
        .unwrap_or_default();
    if let Some(task) = tasks.get_mut(tuid) {
        // The shell result: the task id + output path live in the text (and the id also
        // in `toolUseResult.backgroundTaskId`); a Monitor arm reads `Monitor started
        // (task <id>, …)` with `toolUseResult.taskId`.
        if task.id.is_none() {
            task.id = after_marker(&text, "with ID: ")
                .or_else(|| after_marker(&text, "(task "))
                .or_else(|| after_marker(&text, "(ID: "))
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
        return;
    }
    // A shell the harness moved into the background: ctrl+b, a timeout, or a message
    // delivery. Its launching tool_use carries no flag, so the receipt mints the row and
    // the second pass recovers the launch line.
    if let Some((entered_by, id)) = receipt_entrance(&text) {
        let receipt_ts = rec.timestamp.clone();
        tasks.entry(tuid.to_string()).or_insert(BgTask {
            kind: BgKind::Shell,
            id: Some(id),
            tool_use_id: tuid.to_string(),
            description: None,
            command: None,
            entered_by: Some(entered_by),
            timed_out_after_ms: rec
                .tool_use_result_value()
                .and_then(|v| v.get("timedOutAfterMs")?.as_i64()),
            launch_note: Some(format!(
                "launched-at unknown; receipt at {}",
                receipt_ts
                    .as_deref()
                    .map_or("(no timestamp)", |ts| ts.get(..19).unwrap_or(ts))
            )),
            launched_utc: receipt_ts,
            lane: lane.to_string(),
            output_file: after_marker(&text, "written to: "),
            state: BgState::Open,
            returned_utc: None,
            output_bytes: None,
            output_age_secs: None,
            ignored_by: None,
        });
        minted.push(tuid.to_string());
        return;
    }
    // An async agent launch: the sentinel status on the structured echo.
    let probe = rec.tur_probe();
    let launched = probe
        .as_ref()
        .and_then(|p| p.status.as_ref())
        .and_then(serde_json::Value::as_str)
        == Some("async_launched");
    if !launched {
        return;
    }
    let v = rec
        .tool_use_result_value()
        .unwrap_or(serde_json::Value::Null);
    let s = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    tasks.entry(tuid.to_string()).or_insert(BgTask {
        kind: BgKind::Agent,
        id: s("agentId"),
        tool_use_id: tuid.to_string(),
        description: s("description"),
        command: None,
        entered_by: None,
        timed_out_after_ms: None,
        launch_note: None,
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

/// The SECOND pass, run once per file that minted a row from a receipt. The launching
/// `tool_use` line carries no background needle, so it is unreachable from the first
/// pass; this one is keyed on the minted tool_use ids themselves - bare value
/// substrings, so a reserialized line still matches (the R13 needle law). A row whose
/// launch line is not here (torn, or externalised) keeps its `launch_note` and its
/// receipt instant: an unknown launch time is disclosed, never fabricated.
pub(crate) fn fill_receipt_launches(
    bytes: &[u8],
    minted: &[String],
    tasks: &mut BTreeMap<String, BgTask>,
) {
    let finders: Vec<memmem::Finder<'_>> = minted
        .iter()
        .map(|id| memmem::Finder::new(id.as_bytes()))
        .collect();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let end = memchr::memchr(b'\n', &bytes[pos..]).map_or(bytes.len(), |i| pos + i);
        let line = &bytes[pos..end];
        pos = end + 1;
        if !finders.iter().any(|f| f.find(line).is_some()) {
            continue;
        }
        let Ok(Some(rec)) = crate::parse::parse_line(line) else {
            continue;
        };
        let Some(blocks) = rec.blocks() else {
            continue;
        };
        for b in blocks {
            let Block::ToolUse {
                id: Some(id),
                input,
                ..
            } = b
            else {
                continue;
            };
            let Some(task) = tasks.get_mut(id) else {
                continue;
            };
            // Only a row still carrying the receipt-minted note is waiting for a launch.
            if task.launch_note.is_none() {
                continue;
            }
            let get = |k: &str| {
                input
                    .as_ref()?
                    .get(k)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            };
            task.command = get("command");
            task.description = get("description");
            if let Some(ts) = rec.timestamp.clone() {
                task.launched_utc = Some(ts);
                task.launch_note = None;
            }
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
