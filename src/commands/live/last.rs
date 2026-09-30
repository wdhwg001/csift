//! The `last` section: the person's newest words and the newest assistant message, as
//! excerpts. Kept because a human reads the end of a turn and then goes to read the
//! session; the excerpt is a partial view of the final state and never a review of
//! the work (the help text carries the full warning).

use super::*;

use crate::model::{Class, ClassifyCtx};

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

/// The lines the walk parses: every role-bearing record, plus the person's mid-turn
/// steer, which rides a `queued_command` attachment line that carries no `role` key. The
/// needle is a bare value substring (the needle law), so a line that merely mentions the
/// word is parsed and then refused by the label test in [`person_words`].
fn admits_line(line: &[u8]) -> bool {
    crate::parse::line_has_role_marker(line)
        || memchr::memmem::find(line, b"queued_command").is_some()
}

/// The person's own words on one record, or `None`.
///
/// The slot is the newest record labelled `user.message`, `user.answer` or
/// `user.rejection`, whether or not it OPENED a turn: a slash command typed with
/// arguments is `user.message` beside `harness.command.invocation` and opens no turn, and
/// the section showed nothing at all for a session whose newest prompt was one. A machine
/// trigger - a `<task-notification>` pulse, a fired schedule, an injected template - is
/// not the person's words and never takes the slot.
///
/// The label comes from `classify` with no transcript context, so an AskUserQuestion
/// answer counts by its structured signals alone, as every caller without the file's
/// call index reads it. The TEXT is the one `search` renders for the same leaf: the
/// opener render, then the render seam (a relayed message's body, a queued steer's
/// conversion), so the excerpt here and the unit there are the same bytes.
fn person_words(rec: &Record) -> Option<String> {
    let ctx = ClassifyCtx::top_level();
    let class = rec.classify(&ctx).into_iter().find(|c| {
        matches!(
            c,
            Class::UserMessage | Class::UserAnswer | Class::UserRejection
        )
    })?;
    let text = rec.reconstructed_user_text(None).or_else(|| {
        (class == Class::UserMessage)
            .then(|| crate::commands::search::record_raw_text(rec, None, Some(class), &ctx))
            .flatten()
    })?;
    (!text.trim().is_empty()).then_some(text)
}

/// Newest-first tail read of `path` for the two anchors (bounded by the reader).
pub(crate) fn last_messages(path: &Path) -> Result<LastMessages> {
    let mut out = LastMessages::default();
    crate::parse::tail_records_prefiltered(path, admits_line, 0, |rec| {
        // A session resumed onto a dangling prompt ends with the LOADER's repair pair,
        // and neither half is this session's last exchange. The placeholder needs an explicit
        // skip - it is an assistant record with real text, so `agent_text` would hand back
        // "No response requested." as the newest reply. Its partner needs none: the prompt is
        // `harness.resume.prompt`, never one of the person's three leaves (pinned by a unit
        // test), so the walk continues to what the human actually last sent.
        if out.agent.is_none() && !rec.is_resume_placeholder() {
            if let Some(t) = rec.agent_text() {
                out.agent = Some(excerpt(rec.timestamp.clone(), &t));
            }
        }
        if out.user.is_none() {
            if let Some(t) = person_words(rec) {
                out.user = Some(excerpt(rec.timestamp.clone(), &t));
            }
        }
        out.user.is_none() || out.agent.is_none()
    })?;
    Ok(out)
}
