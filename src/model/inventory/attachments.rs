//! Axis 3: the payload `type` inside a `type:"attachment"` line - 52 values.
//!
//! An attachment is the harness attaching something to the turn. Almost every row here
//! is gated: a flagless scan parses no attachment line, and the leaf is reached by an
//! explicit selector or by `--attachments`. THREE exceptions reach a default scan, and
//! what they have in common is that each is words someone OWNED rather than machinery -
//! a `hook_additional_context` payload whose FIRST content string opens with the csift
//! envelope header (`agent.communication.channel`, a message addressed at this lane), a
//! `queued_command` payload whose own `origin.kind` reads `human` (`user.message`, the
//! person's mid-turn steer) and a `structured_output` payload (`agent.message
//! .structured`, the model's own final answer). The first is detected by the envelope
//! literal; the other two carry `Gate::DefaultNeedle` and their own value needles.
//!
//! `llm` here is the leaf default. The payload types in [`crate::model::NEVER_CONVERTS`]
//! never convert to model-visible content at all - they are the harness's own drop arms -
//! and one more, `hook_success`, converts only on the few records that carry a rendered
//! body; that is a per-RECORD verdict and lives in `Record::delivery_override`, not in
//! this column. The HUMAN column is read off the render's own hide set where that list
//! names the type, and is `(unstated)` where it does not.

use super::*;

/// One row per attachment payload `type`, grouped by leaf in the map's own order.
pub(crate) const ROWS: &[Entry] = &[
    // harness.hook.context: the injected text the model reads.
    e(
        Shape::AttachmentType("hook_additional_context"),
        Some(Class::HookContext),
        Gate::Selector,
        true,
        Human::Never,
    ),
    // harness.hook.outcome: a hook RUN's own record.
    e(
        Shape::AttachmentType("hook_success"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Verbose,
    ),
    e(
        Shape::AttachmentType("hook_blocking_error"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Verbose,
    ),
    e(
        Shape::AttachmentType("hook_non_blocking_error"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Verbose,
    ),
    e(
        Shape::AttachmentType("hook_cancelled"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Verbose,
    ),
    e(
        Shape::AttachmentType("hook_stopped_continuation"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Verbose,
    ),
    e(
        Shape::AttachmentType("hook_system_message"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Verbose,
    ),
    e(
        Shape::AttachmentType("async_hook_response_batch"),
        Some(Class::HookOutcome),
        Gate::Attachments,
        false,
        Human::Unstated,
    ),
    // agent.message.structured: text the MODEL produced.
    e(
        Shape::AttachmentType("structured_output"),
        Some(Class::AgentMessageStructured),
        Gate::DefaultNeedle,
        false,
        Human::Never,
    ),
    // harness.context.reminder
    e(
        Shape::AttachmentType("total_tokens_reminder"),
        Some(Class::ContextReminder),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("task_reminder"),
        Some(Class::ContextReminder),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("silent_turn_reminder"),
        Some(Class::ContextReminder),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("batching_reminder_sent"),
        Some(Class::ContextReminder),
        Gate::Attachments,
        false,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("ultrathink_effort"),
        Some(Class::ContextReminder),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    // harness.context.listing
    e(
        Shape::AttachmentType("skill_listing"),
        Some(Class::ContextListing),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("agent_listing_delta"),
        Some(Class::ContextListing),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("deferred_tools_delta"),
        Some(Class::ContextListing),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("deferred_tools_record"),
        Some(Class::ContextListing),
        Gate::Attachments,
        false,
        Human::Unstated,
    ),
    e(
        // One corpus specimen (c1: 1 record over 9,484 files; c2: 0 over 10,299), and its
        // payload is names plus a directory - `skillDir`, `skillNames`, `displayPath` -
        // rather than a body, so it lists what is available like its four neighbours.
        Shape::AttachmentType("dynamic_skill"),
        Some(Class::ContextListing),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("invoked_skills"),
        Some(Class::ContextListing),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        // The one member of this group whose payload the converter turns into CONTENT:
        // a `# MCP Server Instructions` user message built from `addedBlocks` and
        // `addedNames`, so the model receives the instructions of every server that
        // joined. The person does not: the render's own hide set names this payload type,
        // which is what the `Never` here is read off rather than inferred from the leaf.
        Shape::AttachmentType("mcp_instructions_delta"),
        Some(Class::ContextListing),
        Gate::Attachments,
        true,
        Human::Never,
    ),
    // harness.context.prompt
    e(
        Shape::AttachmentType("instructions"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("date"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("session_context"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("environment"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("model"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("prompt_snapshot"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        false,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("date_change"),
        Some(Class::ContextPrompt),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    // harness.context.file
    e(
        Shape::AttachmentType("file"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("edited_text_file"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("compact_file_reference"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("nested_memory"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("plan_file_reference"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("read_truncation_notice"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("bash_output_audience_note"),
        Some(Class::ContextFile),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    // harness.context.mode
    e(
        Shape::AttachmentType("plan_mode"),
        Some(Class::ContextMode),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("plan_mode_exit"),
        Some(Class::ContextMode),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("plan_mode_reentry"),
        Some(Class::ContextMode),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("auto_mode"),
        Some(Class::ContextMode),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("command_permissions"),
        Some(Class::ContextMode),
        Gate::Attachments,
        false,
        Human::Unstated,
    ),
    // harness.context.ide
    e(
        Shape::AttachmentType("opened_file_in_ide"),
        Some(Class::ContextIde),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("selected_lines_in_ide"),
        Some(Class::ContextIde),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("diagnostics"),
        Some(Class::ContextIde),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    // harness.context.task
    e(
        Shape::AttachmentType("task_status"),
        Some(Class::ContextTask),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    // The one row whose GATE and LEAF answer to different questions: the needle is
    // default-on because the payload can be the PERSON's own mid-turn steer
    // (`attachment.origin.kind` = "human", which classifies `user.message`), and the
    // row's own leaf is what the REST of them carry.
    e(
        Shape::AttachmentType("queued_command"),
        Some(Class::ContextTask),
        Gate::DefaultNeedle,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("workflow_keyword_request"),
        Some(Class::ContextTask),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("goal_status"),
        Some(Class::ContextTask),
        Gate::Attachments,
        // The conversion table drops it (`goal_status:()=>[]`), so the model never
        // receives one even though `harness.context.task` is otherwise visible.
        false,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("remote_session_change"),
        Some(Class::ContextTask),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("relevant_memories"),
        Some(Class::ContextTask),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        Shape::AttachmentType("memory_update"),
        Some(Class::ContextTask),
        Gate::Attachments,
        true,
        Human::Unstated,
    ),
    e(
        // The render's hide set names this payload type, so the `Never` here and on the
        // row below are read off that list rather than inferred from the leaf.
        Shape::AttachmentType("thinking_stripped"),
        Some(Class::ContextTask),
        Gate::Attachments,
        false,
        Human::Never,
    ),
    e(
        // The harness's accounting of the thinking blocks it dropped from the request -
        // which model, which query source, how many blocks and turns went, and why. Its
        // converter arm is an EXPLICIT empty list, so the model receives none of it, and
        // the render hides it, so neither does the person: it is bookkeeping the
        // transcript keeps and nobody reads.
        Shape::AttachmentType("thinking_drop"),
        Some(Class::ContextTask),
        Gate::Attachments,
        false,
        Human::Never,
    ),
];
