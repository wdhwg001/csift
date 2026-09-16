//! The `type:"attachment"` payload types: one arm per payload `type` value.
//!
//! An attachment record is the harness attaching something to the turn - context it
//! injected, a hook run's own record, a file it read, a mode it entered, the
//! structured answer a schema-bound subagent produced. The mapping is MECHANICAL: the
//! payload's `type` value in, a leaf out, with no look at the payload's body. Two
//! payload types need one more fact than their type and the CALLER supplies it:
//! `hook_additional_context` leads with [`Class::CommChannel`] when its first content
//! string is a csift-channel envelope, and `queued_command` is the HUMAN's own
//! mid-turn steer when the PAYLOAD's `origin.kind` says so.

use super::*;

/// The leaf an attachment payload `type` carries, or `None` for a type csift models
/// no leaf for - which an address then renders as an unmodeled unit rather than a
/// bail, and which a census reports as an attachment type with no treatment.
#[must_use]
pub(crate) fn classify(attachment_type: &str) -> Option<Class> {
    Some(match attachment_type {
        // ── harness.hook.context: the one payload the model reads as injected text ──
        "hook_additional_context" => Class::HookContext,

        // ── harness.hook.outcome: a hook RUN's own record ──
        "hook_success"
        | "hook_blocking_error"
        | "hook_non_blocking_error"
        | "hook_cancelled"
        | "hook_stopped_continuation"
        | "hook_system_message"
        | "async_hook_response_batch" => Class::HookOutcome,

        // ── agent.message.structured: text the MODEL produced ──
        "structured_output" => Class::AgentMessageStructured,

        // ── harness.context.reminder: a nudge re-injected each turn ──
        "total_tokens_reminder"
        | "task_reminder"
        | "silent_turn_reminder"
        | "batching_reminder_sent"
        | "ultrathink_effort" => Class::ContextReminder,

        // ── harness.context.listing: an inventory the harness injects ──
        "skill_listing"
        | "dynamic_skill"
        | "agent_listing_delta"
        | "deferred_tools_delta"
        | "deferred_tools_record"
        | "invoked_skills" => Class::ContextListing,

        // ── harness.context.prompt: a persisted system-prompt section ──
        "instructions" | "date" | "session_context" | "environment" | "model"
        | "prompt_snapshot" | "date_change" => Class::ContextPrompt,

        // ── harness.context.file: file content the harness attached ──
        "file"
        | "edited_text_file"
        | "compact_file_reference"
        | "nested_memory"
        | "plan_file_reference"
        | "read_truncation_notice"
        | "bash_output_audience_note" => Class::ContextFile,

        // ── harness.context.mode: a mode transition the harness recorded ──
        "plan_mode"
        | "plan_mode_exit"
        | "plan_mode_reentry"
        | "auto_mode"
        | "command_permissions" => Class::ContextMode,

        // ── harness.context.ide: IDE state ──
        "opened_file_in_ide" | "selected_lines_in_ide" | "diagnostics" => Class::ContextIde,

        // ── harness.context.task: task / goal / memory state ──
        // `queued_command` lands here only when it is NOT the human's own steer; the
        // caller tests the payload's `origin.kind` first (see [`payload_origin_is_human`]).
        "task_status"
        | "queued_command"
        | "workflow_keyword_request"
        | "goal_status"
        | "remote_session_change"
        | "relevant_memories"
        | "memory_update"
        | "thinking_stripped" => Class::ContextTask,

        _ => return None,
    })
}

impl Record {
    /// The attachment VIEW leaf this record carries, if any: the two leaves an ordinary
    /// query names that ride a payload whose TYPE carries a different leaf. One parse,
    /// and the origin is read for the queued case exactly as [`Record::classify`] reads
    /// it - so the scan's keep and the engine's answer cannot disagree about which
    /// records are views.
    ///
    /// `None` for every other record, including every ordinary attachment line.
    #[must_use]
    pub(crate) fn attachment_view_leaf(&self) -> Option<Class> {
        if !self.is_type("attachment") {
            return None;
        }
        let payload = self.attachment_value()?;
        match payload.get("type").and_then(serde_json::Value::as_str)? {
            "queued_command" if payload_origin_is_human(payload) => Some(Class::UserMessage),
            "structured_output" => Some(Class::AgentMessageStructured),
            _ => None,
        }
    }
}

/// True when a `queued_command` PAYLOAD carries the HUMAN's own mid-turn steer: its
/// `origin` object says `kind:"human"`. The harness queues its own riders through the
/// same payload type, so the origin - not the type - is the discriminator, exactly as
/// the queue LINE's content is.
///
/// The field sits INSIDE the payload, beside `commandMode`, never at the top level:
/// over two projects roots not one of 3,692 and 3,574 `queued_command` lines carries a
/// top-level `origin`, while 219 and 217 carry `attachment.origin`. A record-level
/// `origin` is the CROSS-SESSION shape (a `type:"user"` record, TURN-031); reading that
/// field here made the arm unreachable on every real line.
///
/// Takes the payload the caller already parsed, so the record is read once.
#[must_use]
pub(crate) fn payload_origin_is_human(payload: &serde_json::Value) -> bool {
    payload
        .get("origin")
        .and_then(|o| o.get("kind"))
        .and_then(serde_json::Value::as_str)
        == Some("human")
}
