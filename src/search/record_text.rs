//! Record-text resolution: class views, raw text, persisted output, excerpts.

use super::*;

/// True for the three `agent.communication.*` leaves (render `from ⇨ to`, GOLD §4).
pub(crate) fn is_comm_class(c: Class) -> bool {
    matches!(c, Class::CommInbox | Class::CommSent | Class::CommSignal)
}

/// Render the transcript owner's own id as the literal `self` on either side of a comm direction
/// (GOLD §3/§4 notation: `self ⇨ to`, `from ⇨ self`) — a verbose session uuid / bare agent hex on
/// the self side becomes `self`, while a peer id/name on the OTHER side is kept verbatim (a peer
/// never equals the owner). No-op when `owner_id` is `None`.
pub(crate) fn alias_self(
    dir: Option<(String, String)>,
    owner_id: Option<&str>,
) -> Option<(String, String)> {
    let Some(owner) = owner_id else {
        return dir;
    };
    dir.map(|(from, to)| {
        let sub = |s: String| if s == owner { "self".to_string() } else { s };
        (sub(from), sub(to))
    })
}

/// True for a RECORD-LEVEL text class — one classified from a record's string / text-block
/// content (NOT a per-block agent class, and NOT the tool_result duals `user.answer`/
/// `user.rejection`, which are handled in the ToolResult arm). Drives [`record_text_emission`].
pub(crate) fn is_record_text_class(c: Class) -> bool {
    matches!(
        c,
        Class::UserMessage
            | Class::CommInbox
            | Class::CommSignal
            | Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask
            | Class::CompactionSummary
            | Class::CompactionBoundary
            | Class::CommandInvocation
            | Class::CommandStdout
            | Class::InterruptUser
            | Class::InterruptTool
            | Class::ScheduleWakeup
            | Class::ScheduleContinuation
            | Class::MetaHook
            | Class::MetaLoop
    )
}

/// The richest SELECTED record-level text class + its display text (GOLD §3/§6). Iterates the
/// record's labels in `classify`'s richest-first order, taking the first record-text class that
/// is selected, then resolving its text source: a `<task-notification>` → `automation_label`; a
/// genuine/AUQ/rejection/teammate-prose/subagent-opener → `reconstructed_user_text`; any other
/// harness marker / compaction summary / teammate-signal → the raw string. `None` ⇒ no record-
/// text class is selected (or it has no text).
pub(crate) fn record_text_emission(
    rec: &Record,
    labels: &[Class],
    filter: LabelFilter<'_>,
    plan_index: &PlanIndex,
) -> Option<(Class, String)> {
    for &c in labels {
        if !is_record_text_class(c) || !filter.selected(c.path()) {
            continue;
        }
        let text = match c {
            Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask => rec.automation_label(),
            Class::UserMessage | Class::CommInbox => rec.reconstructed_user_text(Some(plan_index)),
            // A teammate signal rides on the raw string; `reconstructed_user_text` returns it for a
            // teammate record (it flattens the content), with the raw text as the fallback.
            Class::CommSignal => rec
                .reconstructed_user_text(Some(plan_index))
                .or_else(|| record_raw_text(rec)),
            _ => record_raw_text(rec),
        };
        if let Some(text) = text {
            return Some((c, text));
        }
    }
    None
}

/// The raw textual body of a record for harness-marker matching: the bare string, or the text
/// blocks joined with `\n` (mirrors the engine's `raw_message_text`). For a MESSAGE-LESS record (a
/// `type:"system"` record — e.g. the `compact_boundary` metrics record) it falls back (D7) to the
/// top-level `content` plus a readable `compactMetadata` excerpt, so the boundary is BOTH matchable
/// and rendered. `None` when there is no text anywhere.
pub(crate) fn record_raw_text(rec: &Record) -> Option<String> {
    // Hook-injected additionalContext attachment: its joined content IS the record's text —
    // both the searchable body under `--additional-context` and the `show`-addressed render.
    if let Some(text) = rec.hook_additional_context_text() {
        return Some(text);
    }
    let Some(msg) = rec.message.as_ref() else {
        // No `message` blocks → a system record. D7: the boundary's content + compactMetadata.
        return system_record_text(rec);
    };
    match msg.content.as_ref()? {
        Content::Text(s) => Some(s.clone()),
        Content::Blocks(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
    }
}

/// D7: the searchable + renderable text of a MESSAGE-LESS system record — in practice the
/// `compact_boundary` metrics record (the only message-less system record `classify` labels).
/// Combines the top-level `content` string (`"Conversation compacted …"`) with a readable
/// `compactMetadata` excerpt so `-t harness.compaction.boundary` can both MATCH the boundary and SEE
/// what each compaction clipped. `None` when neither is present (no fabricated text).
pub(crate) fn system_record_text(rec: &Record) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(serde_json::Value::String(s)) = rec.content.as_ref() {
        let s = s.trim();
        if !s.is_empty() {
            parts.push(s.to_string());
        }
    }
    if let Some(excerpt) = rec
        .compact_metadata
        .as_ref()
        .and_then(compact_metadata_excerpt)
    {
        parts.push(excerpt);
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Render a `compact_boundary` record's `compactMetadata` object as a one-line readable excerpt —
/// `[compaction boundary: trigger=auto preTokens=1000 postTokens=200 durationMs=50]` (only the
/// present fields, stable order, scalars unquoted). `None` when it is not an object or carries none
/// of the known fields.
pub(crate) fn compact_metadata_excerpt(meta: &serde_json::Value) -> Option<String> {
    let obj = meta.as_object()?;
    let mut fields: Vec<String> = Vec::new();
    for key in ["trigger", "preTokens", "postTokens", "durationMs"] {
        if let Some(v) = obj.get(key) {
            let rendered = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            fields.push(format!("{key}={rendered}"));
        }
    }
    (!fields.is_empty()).then(|| format!("[compaction boundary: {}]", fields.join(" ")))
}

/// The communication [`Class`] a `tool_use` block carries (GOLD §3): a `SendMessage` →
/// `…sent`/`…signal`; a `Task`/`Agent`/`Workflow` spawn → `…sent`. `None` for any other tool.
/// REPLICATES the engine's per-record decision per-BLOCK (so a record with mixed comm/non-comm
/// tool_use blocks labels each correctly) — kept faithful to model.rs `classify_assistant`.
pub(crate) fn tool_use_comm_class(
    name: Option<&str>,
    input: Option<&serde_json::Value>,
) -> Option<Class> {
    match name? {
        "SendMessage" => Some(if send_message_is_signal(input) {
            Class::CommSignal
        } else {
            Class::CommSent
        }),
        n if is_spawn_tool_name(n) => Some(Class::CommSent),
        _ => None,
    }
}

/// Replica of the engine's spawn-tool set (model.rs `is_spawn_tool_name`).
pub(crate) fn is_spawn_tool_name(name: &str) -> bool {
    matches!(name, "Task" | "Agent" | "Workflow")
}

/// Replica of the engine's `send_message_is_signal` (model.rs): a `SendMessage` whose top-level
/// (or nested `message`) `type` is present and is NOT `message`/`direct` is a control SIGNAL.
pub(crate) fn send_message_is_signal(input: Option<&serde_json::Value>) -> bool {
    let Some(input) = input else {
        return false;
    };
    let type_at = |v: &serde_json::Value| {
        v.get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    if let Some(t) = type_at(input) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    if let Some(t) = input.get("message").and_then(type_at) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    false
}

/// Render a `tool_use` block to searchable text: `name {json-input}`. The name is
/// matched first so `csift search AskUserQuestion -t agent.tool.use` works; the input JSON is
/// included so a regex can match arguments too.
pub(crate) fn render_tool_use(name: Option<&str>, input: Option<&serde_json::Value>) -> String {
    let mut s = String::new();
    if let Some(n) = name {
        s.push_str(n);
    }
    if let Some(v) = input {
        s.push(' ');
        s.push_str(&v.to_string());
    }
    s
}

/// Resolve a `<persisted-output>` pointer (§4.6) to the referenced file's content.
/// On a read failure the inline text is kept and an explicit note appended — a
/// missing persisted file is reported, never fatal (SPEC §4.6).
pub(crate) fn resolve_persisted_text(path: &str, inline: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => format!("{inline}\n[csift: could not resolve persisted output {path}: {e}]"),
    }
}

/// The synthesized AUQ-answer string from a carrier's `tool_result` (§4.4). Matches
/// any known AUQ-answer marker (both shipped phrasings, see `model::AUQ_ANSWER_MARKERS`).
/// Test-only: production now surfaces the AUQ answer via the model's reconstructed unit
/// ([`Record::reconstructed_user_text`] → [`Record::auq_exchange`]), which prefers the
/// clean structured `toolUseResult.answers`; this helper backs the legacy-shape tests.
#[cfg(test)]
pub(crate) fn auq_answer_text(rec: &Record) -> Option<String> {
    let blocks = rec.blocks()?;
    for b in blocks {
        if let Block::ToolResult {
            content: Some(c), ..
        } = b
        {
            let t = tool_result_content_text(c);
            if crate::model::is_auq_answer_text(&t) {
                return Some(t);
            }
        }
    }
    None
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
/// Test-only: production excerpting goes through [`match_excerpt`], which carries the
/// caller's (possibly `--no-truncate`) budget; this fixed-budget wrapper backs the unit tests.
#[cfg(test)]
pub(crate) fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}

/// Build the inline excerpt, CENTERED on the match so a hit DEEP in a long message is
/// actually visible — not just the message head (the old behavior, which silently hid
/// any match past the first `max` chars and forced readers back to the raw jsonl).
///
/// `span` is the first match's BYTE range, or `None` for the pure filter (no specific
/// match → show the head). When the message fits in `max` chars it is shown whole.
/// Otherwise a `max`-char window is taken around the match (a quarter of the budget as
/// leading context), whitespace-normalized, with a leading `…` when content precedes
/// the window and the shared `… (+N chars)` marker when content follows — so clipping
/// on either side is explicit, never silent (SPEC §0).
///
/// Returns `(excerpt, truncated)` — `truncated` is true iff content was CLIPPED to fit `max`
/// (the head form when the normalized text exceeds `max`, or any match-centered window). Under
/// `--no-truncate`'s `usize::MAX` budget nothing is ever clipped, so `truncated` is always false there.
pub(crate) fn match_excerpt(
    text: &str,
    span: Option<(usize, usize)>,
    max: usize,
) -> (String, bool) {
    let total = text.chars().count();
    // Pure filter, or the whole message already fits (incl. `--no-truncate`'s `usize::MAX`): keep
    // the head-anchored form, capped at `max` (uncapped under `--no-truncate`). Truncated iff the
    // normalized body still overruns `max`.
    let head_form = |text: &str| -> (String, bool) {
        let norm = normalize_line(text);
        let truncated = norm.chars().count() > max;
        (crate::text::truncate_excerpt(&norm, max), truncated)
    };
    let start_byte = match span {
        Some((s, _)) if total > max => s,
        _ => return head_form(text),
    };
    // Char index of the match start; a non-char-boundary byte offset (possible with a
    // raw-byte regex) falls back to the head rather than panicking.
    let Some(prefix) = text.get(..start_byte) else {
        return head_form(text);
    };
    let match_char = prefix.chars().count();
    let win_start = match_char.saturating_sub(max / 4);
    let window: String = text.chars().skip(win_start).take(max).collect();
    let body = normalize_line(&window);
    let after = total.saturating_sub(win_start + max);
    let mut out = String::new();
    if win_start > 0 {
        out.push('…');
    }
    out.push_str(&body);
    if after > 0 {
        out.push_str(&format!("… (+{after} chars)"));
    }
    // The window form is only reached when `total > max`, so a `max`-char window necessarily
    // dropped surrounding content — this is always a truncated fragment.
    (out, true)
}

/// Parse a `--turn` token into a [`RangeSpec`] (the shared grammar: `N`/`A..B`/`N..`/
/// `..N`/`-k`), resolved per-file against that transcript's turn count (0-based).
pub(crate) fn parse_turn_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--turn", false)
}

// ── Rendering ──
//
// Timestamp formatting (system-local + raw UTC) lives in `crate::timez`, shared
// with `list` so the local-timezone choice is defined once.
