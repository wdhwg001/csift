//! The `harness.bookkeeping` family: the loader's NON-TRANSCRIPT line types.
//!
//! One arm per top-level `type` value the harness writes beside the conversation.
//! None of these lines carries a `uuid` or a `message{}`, so none of them reaches the
//! model; all are gated (a selector must name the leaf) and all stay addressable by
//! line. The table is MECHANICAL - a line type in, a leaf out - and it is the single
//! place that mapping is written down (`model::inventory` checks itself against it).

use super::*;

/// The leaf a non-transcript top-level `type` carries, or `None` for a line type
/// csift models no leaf for (which an address then renders as an unmodeled unit).
///
/// `queue-operation` maps here only when the line is NOT the human's own queued text;
/// the caller decides that first ([`Record::queued_class`]), because the discriminator
/// is the line's CONTENT, not its type.
#[must_use]
pub(crate) fn classify(line_type: &str) -> Option<Class> {
    Some(match line_type {
        "last-prompt" => Class::BookkeepingLastPrompt,
        // Two line types, one leaf: the session's mode and its permission mode are
        // the same fact recorded by two writers.
        "mode" | "permission-mode" => Class::BookkeepingMode,
        // The generated session title and the generated agent name, likewise.
        "ai-title" | "agent-name" => Class::BookkeepingName,
        "atis-latch" => Class::BookkeepingAtis,
        "bridge-session" => Class::BookkeepingBridge,
        "history-suppression" => Class::BookkeepingSuppression,
        "pr-link" => Class::BookkeepingPrLink,
        // The three artifact ledgers share one leaf: they record ids for the same
        // artifact surface.
        "frame-link" | "artifact-comment-monitor" | "artifact-autoreact-ledger" => {
            Class::BookkeepingArtifact
        }
        // The per-prompt table and the per-write delta are one instrument.
        "file-history-snapshot" | "file-history-delta" => Class::BookkeepingFileHistory,
        "cost-state" => Class::BookkeepingCost,
        "fork-context-ref" => Class::BookkeepingFork,
        "continued-in" => Class::BookkeepingContinued,
        "queue-operation" => Class::BookkeepingQueue,
        _ => return None,
    })
}

impl Record {
    /// `user.queued` iff the queue line carries the HUMAN's text. The queue also
    /// carries harness riders - a `<task-notification>` pulse or a peer message - whose
    /// delivered twin already classifies `harness.notification.*` /
    /// `agent.communication.inbox`; the same content-shape law that reparents those
    /// user records applies, so a rider is never the human here. A rider and a
    /// content-less `dequeue` fall through to `harness.bookkeeping.queue`.
    #[must_use]
    pub(crate) fn queued_class(&self) -> Option<Class> {
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

    /// The single leaf a NON-MESSAGE line carries: the human's queued text, else the
    /// bookkeeping leaf of its own line type, else the system leaf of its subtype.
    /// `None` for a message record and for every line type that stays unmodeled.
    #[must_use]
    pub fn promoted_class(&self) -> Option<Class> {
        match self.r#type.as_deref()? {
            "queue-operation" => self.queued_class().or(Some(Class::BookkeepingQueue)),
            "system" => system::classify(self.subtype.as_deref()?),
            other => classify(other),
        }
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
