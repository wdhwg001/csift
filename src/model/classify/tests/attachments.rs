//! The attachments family: every payload `type` value maps to exactly one leaf, and
//! the two payload types whose leaf needs one more fact than their type.

use super::*;
use crate::model::classify::attachments;

/// One attachment record carrying `ty` as its payload type.
fn att(ty: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"a1","timestamp":"2026-06-07T05:00:00.000Z","attachment":{{"type":"{ty}"}}}}"#
    )
}

#[test]
fn every_payload_type_maps_to_its_leaf() {
    // The hand-kept table: every attachment payload type the map enumerates, with the
    // leaf it carries. Grouped in the map's own order so a reader can diff the two.
    let table: &[(&str, Class)] = &[
        ("hook_additional_context", Class::HookContext),
        ("hook_success", Class::HookOutcome),
        ("hook_blocking_error", Class::HookOutcome),
        ("hook_non_blocking_error", Class::HookOutcome),
        ("hook_cancelled", Class::HookOutcome),
        ("hook_stopped_continuation", Class::HookOutcome),
        ("hook_system_message", Class::HookOutcome),
        ("async_hook_response_batch", Class::HookOutcome),
        ("structured_output", Class::AgentMessageStructured),
        ("total_tokens_reminder", Class::ContextReminder),
        ("task_reminder", Class::ContextReminder),
        ("silent_turn_reminder", Class::ContextReminder),
        ("batching_reminder_sent", Class::ContextReminder),
        ("ultrathink_effort", Class::ContextReminder),
        ("skill_listing", Class::ContextListing),
        ("agent_listing_delta", Class::ContextListing),
        ("deferred_tools_delta", Class::ContextListing),
        ("deferred_tools_record", Class::ContextListing),
        ("invoked_skills", Class::ContextListing),
        ("instructions", Class::ContextPrompt),
        ("date", Class::ContextPrompt),
        ("session_context", Class::ContextPrompt),
        ("environment", Class::ContextPrompt),
        ("model", Class::ContextPrompt),
        ("prompt_snapshot", Class::ContextPrompt),
        ("date_change", Class::ContextPrompt),
        ("file", Class::ContextFile),
        ("edited_text_file", Class::ContextFile),
        ("compact_file_reference", Class::ContextFile),
        ("nested_memory", Class::ContextFile),
        ("plan_file_reference", Class::ContextFile),
        ("read_truncation_notice", Class::ContextFile),
        ("bash_output_audience_note", Class::ContextFile),
        ("plan_mode", Class::ContextMode),
        ("plan_mode_exit", Class::ContextMode),
        ("plan_mode_reentry", Class::ContextMode),
        ("auto_mode", Class::ContextMode),
        ("command_permissions", Class::ContextMode),
        ("opened_file_in_ide", Class::ContextIde),
        ("selected_lines_in_ide", Class::ContextIde),
        ("diagnostics", Class::ContextIde),
        ("task_status", Class::ContextTask),
        ("queued_command", Class::ContextTask),
        ("workflow_keyword_request", Class::ContextTask),
        ("goal_status", Class::ContextTask),
        ("remote_session_change", Class::ContextTask),
        ("relevant_memories", Class::ContextTask),
        ("memory_update", Class::ContextTask),
        ("thinking_stripped", Class::ContextTask),
    ];
    assert_eq!(table.len(), 49, "the attachment-type table is not 49 rows");
    for &(ty, leaf) in table {
        assert_eq!(attachments::classify(ty), Some(leaf), "{ty}");
        // `hook_additional_context` reaches its leaf through the hook/channel branch,
        // which runs before the payload table; every other type goes through the table.
        assert_eq!(labels(&att(ty)), vec![leaf], "{ty}");
    }
    // No two payload types disagree about their own leaf, and a type csift models no
    // leaf for stays unlabeled rather than borrowing a neighbour's.
    assert_eq!(attachments::classify("some_future_payload"), None);
    assert!(labels(&att("some_future_payload")).is_empty());
    // An attachment record with no payload type at all is tolerated the same way.
    assert!(labels(r#"{"type":"attachment","uuid":"a2","attachment":{}}"#).is_empty());
}

#[test]
fn a_human_queued_command_is_the_persons_message() {
    // The payload type says `queued_command` either way; the record's `origin` is what
    // separates the person's own mid-turn steer from a harness rider.
    let human = r#"{"type":"attachment","uuid":"q1","timestamp":"2026-06-07T05:00:00.000Z","origin":{"kind":"human"},"attachment":{"type":"queued_command","rendered":"run the sweep"}}"#;
    assert_eq!(labels(human), vec![Class::UserMessage]);
    let rider = r#"{"type":"attachment","uuid":"q2","timestamp":"2026-06-07T05:00:00.000Z","origin":{"kind":"peer"},"attachment":{"type":"queued_command"}}"#;
    assert_eq!(labels(rider), vec![Class::ContextTask]);
    // No origin object at all is not the human either - the harness stamps one on
    // everything it did not submit itself, so absence is not evidence of a person.
    assert_eq!(labels(&att("queued_command")), vec![Class::ContextTask]);
}

#[test]
fn the_context_leaves_split_by_producer_not_by_visibility() {
    // Every `harness.context` leaf is visible: the whole category is text the harness
    // put in front of the model. The gate is what keeps them off a default scan.
    for &c in Class::ALL {
        if c.category() == "harness.context" {
            assert!(c.llm_visible(), "{} must be visible", c.path());
        }
    }
    // A hook RUN's own record is not: it is a receipt, not injected text.
    assert!(!Class::HookOutcome.llm_visible());
    assert!(Class::HookContext.llm_visible());
}
