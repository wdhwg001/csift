//! v0.10.0 promoted non-record line types: queue-operation, the REPL-render system
//! subtypes (turn_duration / away_summary / stop_hook_summary) and the file-history
//! instrument lines. Each maps to exactly ONE LLM-invisible leaf; everything here is
//! tolerant (an odd shape yields `None`, never a crash).

use super::*;

impl Record {
    /// The single promoted leaf a NON-message line carries (v0.10.0), or `None` for a
    /// message record and for every line type that stays unmodeled (the session-state
    /// cache lines, the unpromoted system subtypes, a content-less queue `dequeue`).
    #[must_use]
    pub fn promoted_class(&self) -> Option<Class> {
        match self.r#type.as_deref()? {
            "queue-operation" => self.queued_class(),
            "system" => match self.subtype.as_deref()? {
                "turn_duration" => Some(Class::MetaTurnDuration),
                "away_summary" => Some(Class::MetaAwaySummary),
                "stop_hook_summary" => Some(Class::MetaStopHooks),
                _ => None,
            },
            "file-history-snapshot" | "file-history-delta" => Some(Class::MetaSnapshot),
            _ => None,
        }
    }

    /// `user.queued` iff the queue line carries the HUMAN's text. The queue also
    /// carries harness riders - a `<task-notification>` pulse or a peer message - whose
    /// delivered twin already classifies `harness.notification.*` / `agent.communication
    /// .inbox`; the same content-shape law that reparents those user records applies,
    /// so a rider is never the human here. A content-less line (`dequeue`) has nothing
    /// to search and carries no label.
    fn queued_class(&self) -> Option<Class> {
        let text = self.content_str()?;
        if text.trim().is_empty() {
            return None;
        }
        let at_boundary = text.trim_start();
        if at_boundary.starts_with(TASK_NOTIFICATION_PREFIX) || is_peer_message(text) {
            return None;
        }
        Some(Class::UserQueued)
    }

    /// The top-level `content` when it is a plain string (queue lines, away_summary,
    /// the compaction boundary's one-liner). `None` for any other shape.
    #[must_use]
    pub fn content_str(&self) -> Option<&str> {
        match self.content.as_ref()? {
            serde_json::Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// A tolerant integer read of a raw numeric `Value` field: an integer parses, a
    /// whole-number float rounds, anything else is absent.
    #[must_use]
    pub fn u64_field(v: Option<&serde_json::Value>) -> Option<u64> {
        let v = v?;
        if let Some(n) = v.as_u64() {
            return Some(n);
        }
        let f = v.as_f64()?;
        (f.is_finite() && f >= 0.0 && f.fract() == 0.0).then_some(f as u64)
    }
}
