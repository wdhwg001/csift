//! The `last` section: the newest human prompt and the newest assistant message, as
//! excerpts. Kept because a human reads the end of a turn and then goes to read the
//! session; the excerpt is a partial view of the final state and never a review of
//! the work (the help text carries the full warning).

use super::*;

/// The excerpt cap: the context-excerpt budget `search` uses, with the explicit
/// `… (+N chars)` marker when clipped.
const LAST_EXCERPT_CHARS: usize = 400;

#[derive(Debug, Clone)]
pub(crate) struct LastMsg {
    pub(crate) ts_utc: Option<String>,
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LastMessages {
    pub(crate) user: Option<LastMsg>,
    pub(crate) agent: Option<LastMsg>,
}

fn excerpt(ts: Option<String>, text: &str) -> LastMsg {
    let norm = crate::model::normalize_line(text);
    let truncated = norm.chars().count() > LAST_EXCERPT_CHARS;
    LastMsg {
        ts_utc: ts,
        text: crate::text::truncate_excerpt(&norm, LAST_EXCERPT_CHARS),
        truncated,
    }
}

/// Newest-first tail read of `path` for the two anchors (bounded by the reader).
pub(crate) fn last_messages(path: &Path) -> Result<LastMessages> {
    let mut out = LastMessages::default();
    crate::parse::tail_records_prefiltered(path, crate::parse::line_has_role_marker, 0, |rec| {
        if out.agent.is_none() {
            if let Some(t) = rec.agent_text() {
                out.agent = Some(excerpt(rec.timestamp.clone(), &t));
            }
        }
        if out.user.is_none() {
            // A genuine prompt, or the machine trigger that opened the turn (an
            // automation pulse renders as its label, never the raw XML).
            let text = rec.automation_label().or_else(|| {
                rec.is_genuine_user()
                    .then(|| rec.reconstructed_user_text(None))
                    .flatten()
            });
            if let Some(t) = text {
                out.user = Some(excerpt(rec.timestamp.clone(), &t));
            }
        }
        out.user.is_none() || out.agent.is_none()
    })?;
    Ok(out)
}
