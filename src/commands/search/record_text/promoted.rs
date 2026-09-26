//! The NON-MESSAGE renders: the promoted line types, the message-less `type:"system"`
//! records, and the compaction boundary's `compactMetadata` excerpt.
//!
//! These are the shapes with no `message{}` to read, so each one's searchable and
//! rendered text is either the line's own verbatim `content` or a FABRICATED key=value
//! excerpt built from its fields. A fabricated form is not a byte substring of the
//! source line, so each registers a raw marker in `matcher::synth_marker_finders` -
//! without that the whole-file gate would prune a file whose only hit lies in the
//! fabricated head. A render for a family's own non-message leaves belongs here too.

use super::*;

/// The searchable + renderable text of a MESSAGE-LESS system record - in practice the
/// `compact_boundary` metrics record (the only message-less system record `classify` labels).
/// Combines the top-level `content` string (`"Conversation compacted ..."`) with a readable
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
    // A boundary's `parentUuid` is null; `logicalParentUuid` names the record the
    // compaction re-links to (harness ground truth) - surface it on the same line.
    if rec.is_compact_boundary() {
        if let Some(lp) = rec.logical_parent_uuid.as_deref() {
            parts.push(format!("[logicalParent={lp}]"));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The searchable + renderable text of a PROMOTED non-message line, keyed on
/// [`Record::promoted_class`]. `user.queued` / `harness.turn.recap` return the
/// verbatim `content` string (a raw byte substring, prefilter-safe). The other three
/// fabricate a bracketed key=value excerpt from the record's own fields - present
/// fields only, stable order - so the leaf is both matchable and legible:
/// - turn-duration:
///   `[turn duration: 1m 5s · durationMs=64911 messageCount=908 pendingBackgroundAgentCount=2]`;
/// - stop-hooks: `[stop hooks: count=N errors=M prevented=false]` + one `command (Nms)`
///   line per hook (the commands are the record's only text);
/// - snapshot: `[file-history snapshot at <ts>: <path>@vN, ...]` (paths sorted) or
///   `[file-history delta at <ts>: <path>@vN backup=<name>]` (the `backup=` suffix
///   only when the delta carries a non-null `backupFileName`).
///
/// `None` for a non-promoted record.
pub(crate) fn promoted_record_text(rec: &Record) -> Option<String> {
    match rec.promoted_class()? {
        Class::UserQueued | Class::TurnRecap => rec.content_str().map(str::to_string),
        Class::TurnDuration => {
            let mut fields: Vec<String> = Vec::new();
            let ms = Record::u64_field(rec.duration_ms.as_ref());
            if let Some(ms) = ms {
                fields.push(format!("{} · durationMs={ms}", fmt_ms(ms)));
            }
            for (key, v) in [
                ("messageCount", rec.message_count.as_ref()),
                (
                    "pendingBackgroundAgentCount",
                    rec.pending_background_agent_count.as_ref(),
                ),
                ("pendingWorkflowCount", rec.pending_workflow_count.as_ref()),
            ] {
                if let Some(n) = Record::u64_field(v) {
                    fields.push(format!("{key}={n}"));
                }
            }
            (!fields.is_empty()).then(|| format!("[turn duration: {}]", fields.join(" ")))
        }
        Class::TurnStopHooks => {
            let count = Record::u64_field(rec.hook_count.as_ref()).unwrap_or(0);
            let errors = rec
                .hook_errors
                .as_ref()
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            let prevented = rec.prevented_continuation.unwrap_or(false);
            let mut lines = vec![format!(
                "[stop hooks: count={count} errors={errors} prevented={prevented}]"
            )];
            if let Some(infos) = rec
                .hook_infos
                .as_ref()
                .and_then(serde_json::Value::as_array)
            {
                for info in infos {
                    let Some(cmd) = info.get("command").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    match Record::u64_field(info.get("durationMs")) {
                        Some(ms) => lines.push(format!("{cmd} ({ms}ms)")),
                        None => lines.push(cmd.to_string()),
                    }
                }
            }
            Some(lines.join("\n"))
        }
        Class::BookkeepingFileHistory => snapshot_record_text(rec),
        // The six `type:"system"` EVENT leaves are not here: their render is the system
        // family's (`record_text/system.rs`), reached through the render seam before this
        // renderer is consulted at all. What is left is the TURN family's three closers
        // plus the person's own queued text, which are the shapes this renderer still
        // decides.
        _ => None,
    }
}

/// The `harness.bookkeeping.file-history` excerpt for a `file-history-snapshot` (every tracked
/// path with its version, sorted) or a `file-history-delta` (the one path). The
/// snapshot line has no top-level timestamp; the nested one rides the excerpt.
fn snapshot_record_text(rec: &Record) -> Option<String> {
    let ver = |v: Option<&serde_json::Value>| {
        Record::u64_field(v).map_or_else(|| "?".to_string(), |n| n.to_string())
    };
    if rec.is_type("file-history-delta") {
        let path = rec.tracking_path.as_deref()?;
        let backup = rec.backup.as_ref();
        let version = ver(backup.and_then(|b| b.get("version")));
        let at = head_instant(rec.timestamp.as_deref());
        let name = backup
            .and_then(|b| b.get("backupFileName"))
            .and_then(serde_json::Value::as_str)
            .map(|n| format!(" backup={n}"))
            .unwrap_or_default();
        return Some(format!(
            "[file-history delta at {at}: {path}@v{version}{name}]"
        ));
    }
    let snap = rec.snapshot.as_ref()?;
    let at = head_instant(snap.get("timestamp").and_then(serde_json::Value::as_str));
    let mut entries: Vec<String> = snap
        .get("trackedFileBackups")
        .and_then(serde_json::Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(path, e)| format!("{path}@v{}", ver(e.get("version"))))
                .collect()
        })
        .unwrap_or_default();
    entries.sort_unstable();
    Some(format!(
        "[file-history snapshot at {at}: {}]",
        entries.join(", ")
    ))
}

/// A file-history head's instant as TEXT: the canonical local form with milliseconds, the
/// one the `search` and `show` headers print above it, so a head never carries a zone its
/// header does not. The UTC string stays matchable in the unit's verbatim JSON body, and a
/// delta's own instant also rides the hit's `ts_utc`/`ts_local`. `?` when the line names
/// no instant.
fn head_instant(raw: Option<&str>) -> String {
    raw.map_or_else(
        || "?".to_string(),
        |ts| crate::timez::format_local_compact(Some(ts)),
    )
}

/// The shared duration formatter (`crate::text::fmt_ms`), re-exported so the unit
/// tests beside this file keep their unqualified name.
pub(crate) use crate::text::fmt_ms;

/// Render a `compact_boundary` record's `compactMetadata` object as a one-line readable excerpt -
/// `[compaction boundary: trigger=auto preTokens=1000 postTokens=200 durationMs=50]` (only the
/// present fields, stable order, scalars unquoted). `None` when it is not an object or carries none
/// of the known fields.
///
/// It also names the harness's own SURVIVOR fields, each printed only when present, so a
/// boundary carrying just the four scalars renders those four and nothing else:
/// `messagesSummarized=` (a `/rewind` summarize wrote this boundary - an ordinary compaction never
/// passes the argument), `cumulativeDroppedTokens=` (the running total of context tokens every
/// compaction of this session has dropped), `preserved=<N uuids, M allUuids, anchor <first8>>` (the
/// records the compaction KEPT: `uuids` is the on-disk subset, `allUuids` the in-memory superset,
/// which is why some `allUuids` entries resolve to no line) and `segment=<head first8>..<tail
/// first8>` (the kept run's ends). The full uuid lists are JSON-only - the excerpt prints counts and
/// the first-8 handles so a boundary line stays scannable. Every fabricated byte here is covered by
/// the `-t`-gated `compact_boundary` synth marker, which is a raw substring of the same line.
pub(crate) fn compact_metadata_excerpt(meta: &serde_json::Value) -> Option<String> {
    let obj = meta.as_object()?;
    let mut fields: Vec<String> = Vec::new();
    for key in [
        "trigger",
        "preTokens",
        "postTokens",
        "durationMs",
        "messagesSummarized",
        "cumulativeDroppedTokens",
    ] {
        if let Some(v) = obj.get(key) {
            let rendered = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            fields.push(format!("{key}={rendered}"));
        }
    }
    if let Some(preserved) = preserved_messages_fragment(obj.get("preservedMessages")) {
        fields.push(preserved);
    }
    if let Some(segment) = preserved_segment_fragment(obj.get("preservedSegment")) {
        fields.push(segment);
    }
    (!fields.is_empty()).then(|| format!("[compaction boundary: {}]", fields.join(" ")))
}

/// `preserved=<N uuids, M allUuids, anchor <first8>>` from `compactMetadata.preservedMessages` -
/// each part only when its key is there. `None` when the object carries none of the three.
fn preserved_messages_fragment(v: Option<&serde_json::Value>) -> Option<String> {
    let obj = v?.as_object()?;
    let mut parts: Vec<String> = Vec::new();
    for (key, word) in [("uuids", "uuids"), ("allUuids", "allUuids")] {
        if let Some(a) = obj.get(key).and_then(serde_json::Value::as_array) {
            parts.push(format!("{} {word}", a.len()));
        }
    }
    if let Some(anchor) = obj.get("anchorUuid").and_then(serde_json::Value::as_str) {
        parts.push(format!("anchor {}", short_uuid(anchor)));
    }
    (!parts.is_empty()).then(|| format!("preserved={}", parts.join(", ")))
}

/// `segment=<head first8>..<tail first8>` from `compactMetadata.preservedSegment`. `None` when
/// neither end is present; a one-ended object renders the end it has.
fn preserved_segment_fragment(v: Option<&serde_json::Value>) -> Option<String> {
    let obj = v?.as_object()?;
    let end = |key: &str| {
        obj.get(key)
            .and_then(serde_json::Value::as_str)
            .map(short_uuid)
            .unwrap_or_default()
    };
    let (head, tail) = (end("headUuid"), end("tailUuid"));
    (!(head.is_empty() && tail.is_empty())).then(|| format!("segment={head}..{tail}"))
}

/// The first 8 characters of a uuid (codepoint-safe; a shorter id renders whole) - the scannable
/// handle the boundary excerpt prints in place of a full uuid.
fn short_uuid(id: &str) -> String {
    id.chars().take(8).collect()
}
