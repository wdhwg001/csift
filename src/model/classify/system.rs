//! The `type:"system"` subtypes: one arm per subtype the harness writes.
//!
//! A system record carries no `message{}`, so the compaction boundary aside it is the
//! harness talking to its own UI - except `local_command`, which the request assembler
//! re-mints as a user message (the ONE delivered subtype, which is why
//! [`Class::SystemCommand`] is LLM-visible while its four siblings are not).

use super::*;

/// The leaf a `type:"system"` record's `subtype` carries, or `None` for a subtype
/// csift models no leaf for - which an address then renders as an unmodeled unit
/// rather than a bail. The mapping is MECHANICAL: a subtype in, a leaf out, with no
/// look at the payload.
#[must_use]
pub(crate) fn classify(subtype: &str) -> Option<Class> {
    Some(match subtype {
        "compact_boundary" => Class::CompactionBoundary,
        // The three end-of-turn telemetry records.
        "turn_duration" => Class::TurnDuration,
        "stop_hook_summary" => Class::TurnStopHooks,
        "away_summary" => Class::TurnRecap,
        // The scheduler's own fire RECORD (the prompt it submits is a user record and
        // takes `harness.schedule.fire`).
        "scheduled_task_fire" => Class::ScheduleEvent,
        // Two subtypes, one event kind: whether a fallback model was tried is a FIELD
        // on the record, not a different event.
        "model_refusal_fallback" | "model_refusal_no_fallback" => Class::SystemRefusal,
        "api_error" => Class::SystemApiError,
        "informational" => Class::SystemNotice,
        "agents_killed" => Class::SystemKill,
        "local_command" => Class::SystemCommand,
        _ => return None,
    })
}

impl Record {
    /// True when this is the `system`/`compact_boundary` metrics record -
    /// `harness.compaction.boundary`.
    #[must_use]
    pub fn is_compact_boundary(&self) -> bool {
        self.is_type("system") && self.subtype.as_deref() == Some("compact_boundary")
    }
}
