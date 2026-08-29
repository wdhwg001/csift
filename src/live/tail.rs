//! Transcript tail state machine: what the newest records say is happening.
//!
//! Reads the FINAL window of a transcript (bounded, never the whole file), walks it
//! backward, and reports the liveness-relevant shape: the newest UNRETURNED tool call
//! (a use whose id has no later result = a tool in flight, or a process dead mid-tool),
//! the last assistant `stop_reason`, and the last record's instant. A record is only
//! trusted from a COMPLETE line (torn tails are skipped by the newline framing).
//!
//! F9: growth alone is never activity - a main transcript grows while idle (enqueues,
//! attachments). This module classifies WHAT the tail is, not whether the file moved.

use super::*;

/// The bounded tail window in bytes: generous enough to cover any real turn tail
/// (hundreds of records), small enough to stay O(1) against a 300MB transcript.
const TAIL_WINDOW_BYTES: usize = 512 * 1024;

/// What a transcript's tail says right now.
#[derive(Debug, Clone, Default)]
pub(crate) struct TailShape {
    /// The newest tool_use whose id has NO later tool_result in the window: a tool in
    /// flight (or dead mid-tool; the pid probe disambiguates). `(tool name, use ts)`.
    pub(crate) unreturned_use: Option<(String, Option<String>)>,
    /// The newest assistant record's `stop_reason` (trustworthy on the MAIN lane;
    /// null is NORMAL mid-message on subagents).
    pub(crate) last_stop_reason: Option<String>,
    /// The newest record's timestamp (any type that carries one).
    pub(crate) last_ts_utc: Option<String>,
    /// Records inspected (evidence sizing; 0 = empty/unreadable file).
    pub(crate) records_seen: usize,
}

/// Read + classify the tail window of `path`.
pub(crate) fn tail_shape(path: &Path) -> Result<TailShape> {
    let mut shape = TailShape::default();
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(shape);
    };
    let bytes: &[u8] = &mmap;
    let start = bytes.len().saturating_sub(TAIL_WINDOW_BYTES);
    // Align to a line start (skip the partial line the cut landed in), unless we have
    // the whole file.
    let window = if start == 0 {
        bytes
    } else {
        match memchr::memchr(b'\n', &bytes[start..]) {
            Some(nl) => &bytes[start + nl + 1..],
            None => &bytes[bytes.len()..],
        }
    };

    // Complete lines only: a torn final line (no trailing newline) is held, not parsed.
    let mut lines: Vec<&[u8]> = Vec::new();
    let mut pos = 0usize;
    while pos < window.len() {
        match memchr::memchr(b'\n', &window[pos..]) {
            Some(nl) => {
                lines.push(&window[pos..pos + nl]);
                pos += nl + 1;
            }
            None => break, // torn tail: skip (the next poll re-reads it complete)
        }
    }

    // Backward walk: result ids seen so far are LATER in file order, so a use id absent
    // from that set is unreturned as of the tail.
    let mut later_result_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in lines.iter().rev() {
        let Ok(Some(rec)) = crate::parse::parse_line(line) else {
            continue;
        };
        shape.records_seen += 1;
        if shape.last_ts_utc.is_none() {
            shape.last_ts_utc = rec.timestamp.clone();
        }
        if shape.last_stop_reason.is_none() && rec.r#type.as_deref() == Some("assistant") {
            if let Some(sr) = rec.message.as_ref().and_then(|m| m.stop_reason.as_deref()) {
                shape.last_stop_reason = Some(sr.to_string());
            }
        }
        if let Some(blocks) = rec.blocks() {
            // Within one record, results precede later uses only across records; collect
            // results first so a same-record use+result (never real) stays paired.
            for b in blocks {
                if let crate::model::Block::ToolResult {
                    tool_use_id: Some(id),
                    ..
                } = b
                {
                    later_result_ids.insert(id.clone());
                }
            }
            if shape.unreturned_use.is_none() {
                for b in blocks {
                    if let crate::model::Block::ToolUse {
                        id: Some(id), name, ..
                    } = b
                    {
                        if !later_result_ids.contains(id) {
                            shape.unreturned_use = Some((
                                name.clone().unwrap_or_else(|| "(unnamed)".to_string()),
                                rec.timestamp.clone(),
                            ));
                        }
                    }
                }
            }
        }
        // Enough signal: stop once we have all three answers (bounded work even inside
        // the window).
        if shape.unreturned_use.is_some()
            && shape.last_stop_reason.is_some()
            && shape.records_seen >= 8
        {
            break;
        }
    }
    Ok(shape)
}

/// Seconds between an ISO instant and now; `None` when absent/unparseable.
pub(crate) fn age_secs(ts_utc: Option<&str>) -> Option<i64> {
    let t: jiff::Timestamp = ts_utc?.parse().ok()?;
    Some((jiff::Timestamp::now().as_second() - t.as_second()).max(0))
}
