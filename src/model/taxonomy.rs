//! Role + Class: the role.category.leaf label taxonomy.
//!
//! [`Class::ALL`] is the single source of truth for the leaf space; [`paths`] carries
//! the dotted path of each leaf and [`facets`] the three facets a selector reads
//! (role, category, LLM visibility). Every leaf names the ON-DISK SHAPE it is read
//! from, so a reader can cross-check a rendered label against `show --raw`.

mod facets;
mod paths;

#[cfg(test)]
mod tests;

/// The top-level ROLE of a classified record. The first dot-segment of every
/// [`Class::path`]. A multi-label record can span roles (e.g. an AUQ answer is both
/// [`Role::User`] and [`Role::Agent`]).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The human operator.
    User,
    /// The assistant (incl. its tool I/O and peer communication).
    Agent,
    /// Claude Code machinery (notifications, compaction, slash wrappers, interrupts,
    /// schedule, hooks, injected context, telemetry, bookkeeping).
    Harness,
}

#[allow(dead_code)]
impl Role {
    /// The stable lowercase slug (the first dot-segment of a [`Class::path`]).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Agent => "agent",
            Role::Harness => "harness",
        }
    }
}

/// A LEAF class in the role.category.leaf taxonomy. One variant per leaf; the dotted
/// [`Class::path`] is the canonical wire/selector form, [`Class::role`] its top-level
/// role and [`Class::category`] the dotted CATEGORY it belongs to. A record carries a
/// `Vec<Class>` (multi-label) via [`Record::classify`].
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    // -- user (6) --
    /// `user.message` - the person's own message. `type:"user"`, `role:"user"`, string
    /// or `[{text}]` content, no `isMeta` (a with-args slash wrapper is dual-labeled,
    /// this leaf first) - or, when the person typed again mid-turn, the `queued_command`
    /// ATTACHMENT the harness folded in, whose payload `origin.kind` reads `human`.
    UserMessage,
    /// `user.answer` - the AskUserQuestion answer unit (the Q + options + answer). A
    /// `type:"user"` tool_result carrier with `toolUseResult.answers` / `.response`.
    /// Dual-labeled with [`Class::AgentToolResult`].
    UserAnswer,
    /// `user.rejection` - a tool rejected with a typed instruction: a tool_result
    /// carrier whose text carries the `To tell you how to proceed, the user said:`
    /// tail. Dual-labeled with [`Class::AgentToolResult`].
    UserRejection,
    /// `user.unsent` - the esc-recalled draft left on disk: a `type:"user"` opener
    /// sharing its `parentUuid` with a LATER opener and carrying no assistant
    /// descendant. Assigned at the SCAN layer (the fact lives in the DAG around the
    /// record); outside turn numbering, addressable.
    UserUnsent,
    /// `user.rewound` - an ANSWERED opener the conversation was rewound past: the same
    /// shape as [`Class::UserUnsent`] WITH an assistant descendant. Scan layer, outside
    /// turn numbering, addressable.
    UserRewound,
    /// `user.queued` - the person's text while it sat in the input queue: a
    /// `type:"queue-operation"` line carrying the human's own `content`.
    UserQueued,

    // -- agent.message (2) --
    /// `agent.message.plain` - the assistant's own text: a `type:"assistant"` record's
    /// `[{text}]` block(s).
    AgentMessagePlain,
    /// `agent.message.structured` - a schema-bound subagent's final structured answer:
    /// a `type:"attachment"` record whose payload `type` is `structured_output`,
    /// parented to the assistant record that produced it. The model PRODUCED this text,
    /// so it is LLM-visible although the harness never converts the attachment back.
    AgentMessageStructured,

    // -- agent.thinking (4) --
    /// `agent.thinking.summary` - the API-side summary of the reasoning (never the raw
    /// chain of thought): a `{type:"thinking", thinking, signature}` block whose
    /// signature tag reads `thinking`.
    AgentThinkingSummary,
    /// `agent.thinking.narration` - the API's progress update between tool calls: the
    /// same block shape with the signature tag `narration` (the tag hides in the base64
    /// `signature`; see `model/narration.rs`).
    AgentThinkingNarration,
    /// `agent.thinking.omitted` - a thinking block the API returned with NO summary
    /// text: `{type:"thinking", thinking:"", signature:<non-empty>}`. The signature is
    /// echoed back, but there is no text the model could read, so the leaf is invisible.
    AgentThinkingOmitted,
    /// `agent.thinking.redacted` - the API's safety-redacted ciphertext block,
    /// `{type:"redacted_thinking", data:<base64>}`. Parsed verbatim and echoed back;
    /// no readable text, so the leaf is invisible.
    AgentThinkingRedacted,

    // -- agent.tool (2) --
    /// `agent.tool.use` - a `type:"assistant"` record's `[{tool_use}]` block (incl. a
    /// pending elicitation sidecar marker).
    AgentToolUse,
    /// `agent.tool.result` - a `type:"user"` carrier's `[{tool_result}]` block plus its
    /// `toolUseResult` (incl. errored results).
    AgentToolResult,

    /// `agent.communication.inbox` - a received peer message, spawn prompt or subagent
    /// return.
    CommInbox,
    /// `agent.communication.sent` - a sent peer message (`SendMessage`) or a spawn.
    CommSent,
    /// `agent.communication.signal` - a control/status comm (idle_notification,
    /// shutdown_*).
    CommSignal,
    /// `agent.communication.channel` - a csift-channel delivery: a
    /// `hook_additional_context` attachment whose FIRST content string opens with the
    /// envelope header `[csift-channel v1 ...]`. A MESSAGE addressed at this lane, so it
    /// is the RICHEST view of a record that also carries [`Class::HookContext`] - and
    /// unlike that leaf it is admitted by a DEFAULT scan.
    CommChannel,

    // -- harness.notification (5) --
    /// `harness.notification.workflow` - a `<task-notification>` for a dynamic/OMC
    /// workflow.
    NotificationWorkflow,
    /// `harness.notification.monitor` - a monitor/cron cadence completion pulse.
    NotificationMonitor,
    /// `harness.notification.subagent` - a spawned-subagent completion pulse (named
    /// apart from the `agent` ROLE so the two never collide).
    NotificationSubagent,
    /// `harness.notification.background-command` - a backgrounded shell command pulse.
    NotificationBackgroundCommand,
    /// `harness.notification.task` - any other / unclassified `<task-notification>`.
    NotificationTask,

    // -- harness.compaction (2) --
    /// `harness.compaction.summary` - the `isCompactSummary` summary record.
    CompactionSummary,
    /// `harness.compaction.boundary` - the `type:"system"` / `subtype:"compact_boundary"`
    /// metrics record. No `message{}`, so it is not in the surviving conversation.
    CompactionBoundary,

    // -- harness.command (3) --
    /// `harness.command.invocation` - a `<command-name>` / `<command-message>` slash
    /// wrapper.
    CommandInvocation,
    /// `harness.command.stdout` - a `<local-command-stdout>...` local-command output.
    CommandStdout,
    /// `harness.command.caveat` - the caveat the slash-command EXECUTOR writes after a
    /// local command: an `isMeta` `type:"user"` record whose content opens
    /// `<local-command-caveat>`. Delivered to the model, never shown to the human.
    CommandCaveat,

    // -- harness.interrupt (2) --
    /// `harness.interrupt.user` - `[Request interrupted by user]`.
    InterruptUser,
    /// `harness.interrupt.tool` - `[Request interrupted by user for tool use]`.
    InterruptTool,

    // -- harness.schedule (4) --
    /// `harness.schedule.wakeup` - a fired `ScheduleWakeup` TIMER tick: the isMeta user
    /// record carrying one of the wakeup markers at content start.
    ScheduleWakeup,
    /// `harness.schedule.event` - the fire RECORD itself: `type:"system"`,
    /// `subtype:"scheduled_task_fire"`, whose `content` names the instant in a trailing
    /// parenthesis. No `message{}`: the banner is the human's, not the model's.
    ScheduleEvent,
    /// `harness.schedule.fire` - the PROMPT the scheduler submits: an `isMeta`
    /// `type:"user"` record whose `parentUuid` names the `system`/`scheduled_task_fire`
    /// record above it. Its TEXT is whatever the operator armed, so the link is the whole
    /// gate. Delivered; it does not open a turn, because the operator did not type it.
    ScheduleFire,
    /// `harness.schedule.tick` - the autonomous-loop DRIVER tick (`# Autonomous loop
    /// tick` / `Run the autonomous check` at content start), distinct from the fired
    /// timer above.
    ScheduleTick,

    // -- harness.resume (2) --
    /// `harness.resume.prompt` - the repair PROMPT the loader appends when a resumed
    /// transcript ends on a dangling user record: an `isMeta` `type:"user"` record
    /// whose text opens `Continue from where you left off.`. Delivered, never the
    /// operator's, so it opens no turn.
    ResumePrompt,
    /// `harness.resume.placeholder` - the repair PLACEHOLDER spliced after that
    /// trailing user record: an assistant record with the `<synthetic>` model and
    /// exactly the placeholder text. Whether it closes a PAIR is a fact on the hit, not
    /// a second leaf.
    ResumePlaceholder,

    // -- harness.hook (3) --
    /// `harness.hook.context` - context a hook injected: a `type:"attachment"` record
    /// whose payload `type` is `hook_additional_context` (with `hookName`, `hookEvent`
    /// and a `content` string array). Delivered to the model as a
    /// `<system-reminder>`-wrapped user message.
    HookContext,
    /// `harness.hook.outcome` - a hook RUN's own record: a `type:"attachment"` whose
    /// payload `type` is one of the `hook_*` family (`hook_success`,
    /// `hook_blocking_error`, ...) or `async_hook_response_batch`. Not delivered, except
    /// a `hook_success` that carries a rendered body.
    HookOutcome,
    /// `harness.hook.feedback` - a BLOCKING HOOK's feedback the model received: an
    /// `isMeta` `type:"user"` record whose content opens `<Name> hook feedback:` and
    /// then a NEWLINE. ONE builder joins three pieces - a hook name, the shared suffix
    /// ` hook feedback:\n`, and the feedback body - and four callers hand it the four
    /// names `Stop`, `TeammateIdle`, `TaskCreated`, `TaskCompleted`, so the ASSEMBLED
    /// prefix is a literal at no offset in the build while the suffix and the names are
    /// separate ones. `Stop hook feedback:` IS in the build three times, and none of the
    /// three is that writer: two are the TUI's own summary and row, both continuing with
    /// a SPACE, and the third is the interned copy of that same string (claim CLS-048).
    /// csift matches the prefix WITHOUT the newline, so a body that continues any other
    /// way still classifies; only the `Stop` name has a corpus specimen.
    HookFeedback,

    // -- harness.context (9) --
    /// `harness.context.reminder` - a nudge the harness re-injects each turn: the
    /// reminder attachment payloads, plus the isMeta `<system-reminder>` injections
    /// that match no more specific template.
    ContextReminder,
    /// `harness.context.listing` - an inventory the harness injects (skills, agents,
    /// deferred tools).
    ContextListing,
    /// `harness.context.prompt` - a persisted system-prompt section (instructions,
    /// date, environment, model, session context).
    ContextPrompt,
    /// `harness.context.file` - file content the harness attached (a read, an edited
    /// snapshot, a compact file reference, nested memory, a plan file).
    ContextFile,
    /// `harness.context.mode` - a mode transition the harness recorded (plan mode and
    /// its exits/re-entries, auto mode, command permissions).
    ContextMode,
    /// `harness.context.ide` - IDE state the harness attached (the open file, the
    /// selection, diagnostics).
    ContextIde,
    /// `harness.context.task` - task/goal/memory state the harness attached, including
    /// a `queued_command` that is NOT the human's own text.
    ContextTask,
    /// `harness.context.image` - an image the harness referenced into the conversation:
    /// the isMeta `[Image: ...]` reference records and the image-only isMeta records.
    ContextImage,
    /// `harness.context.skill` - an isMeta `type:"user"` record whose text OPENS with
    /// `Base directory for this skill:`, the instruction body a spawn builder preloaded
    /// rather than a skill anyone invoked this turn; on disk that body usually arrives
    /// as the first text block of an array.
    ContextSkill,

    // -- harness.enforce (4) --
    /// `harness.enforce.structured-output` - the redo nudge that tells the model it
    /// must call the StructuredOutput tool.
    EnforceStructuredOutput,
    /// `harness.enforce.brief` - a redo nudge telling the model it ended its turn
    /// without calling SendUserMessage.
    EnforceBrief,
    /// `harness.enforce.handback` - the subagent hand-back redo nudge.
    EnforceHandback,
    /// `harness.enforce.retry` - a redo nudge that tells the model to re-issue a call:
    /// the malformed-tool-call nudge, and the edit-failed-retry notice
    /// ([`EDIT_RETRY_MARKER`]).
    EnforceRetry,

    // -- harness.turn (3) --
    /// `harness.turn.duration` - the `type:"system"` / `subtype:"turn_duration"`
    /// end-of-turn telemetry record. No `message{}`.
    TurnDuration,
    /// `harness.turn.stop-hooks` - the `type:"system"` / `subtype:"stop_hook_summary"`
    /// ledger of the Stop hooks that ran at turn end. No `message{}`.
    TurnStopHooks,
    /// `harness.turn.recap` - the `type:"system"` / `subtype:"away_summary"` recap the
    /// harness generates when the operator returns after time away. No `message{}`.
    TurnRecap,

    // -- harness.system (5) --
    /// `harness.system.refusal` - a `type:"system"` record with subtype
    /// `model_refusal_fallback` / `model_refusal_no_fallback`.
    SystemRefusal,
    /// `harness.system.api-error` - a `type:"system"` record with subtype `api_error`.
    SystemApiError,
    /// `harness.system.notice` - a `type:"system"` record with subtype `informational`.
    SystemNotice,
    /// `harness.system.kill` - a `type:"system"` record with subtype `agents_killed`.
    SystemKill,
    /// `harness.system.command` - a `type:"system"` record with subtype
    /// `local_command`: the ONE system subtype the request assembler re-mints as a user
    /// message, so it is DELIVERED although it carries no `message{}`.
    SystemCommand,

    // -- harness.bookkeeping (13) --
    /// `harness.bookkeeping.last-prompt` - a `type:"last-prompt"` line (`leafUuid`,
    /// `explicit`, `lastPrompt`).
    BookkeepingLastPrompt,
    /// `harness.bookkeeping.mode` - a `type:"mode"` or `type:"permission-mode"` line.
    BookkeepingMode,
    /// `harness.bookkeeping.name` - a `type:"ai-title"` or `type:"agent-name"` line.
    BookkeepingName,
    /// `harness.bookkeeping.atis` - a `type:"atis-latch"` line.
    BookkeepingAtis,
    /// `harness.bookkeeping.bridge` - a `type:"bridge-session"` line.
    BookkeepingBridge,
    /// `harness.bookkeeping.suppression` - a `type:"history-suppression"` line.
    BookkeepingSuppression,
    /// `harness.bookkeeping.pr-link` - a `type:"pr-link"` line.
    BookkeepingPrLink,
    /// `harness.bookkeeping.artifact` - a `type:"frame-link"`,
    /// `type:"artifact-comment-monitor"` or `type:"artifact-autoreact-ledger"` line.
    BookkeepingArtifact,
    /// `harness.bookkeeping.file-history` - a `type:"file-history-snapshot"` (the
    /// per-prompt tracked-file version table) or `type:"file-history-delta"` (one
    /// path's version bump) line: the instrument `recover` reads for external writes.
    BookkeepingFileHistory,
    /// `harness.bookkeeping.cost` - a `type:"cost-state"` checkpoint line.
    BookkeepingCost,
    /// `harness.bookkeeping.fork` - a `type:"fork-context-ref"` line
    /// (`parentSessionId`, `parentLastUuid`).
    BookkeepingFork,
    /// `harness.bookkeeping.continued` - a `type:"continued-in"` line naming the child
    /// a background handoff minted.
    BookkeepingContinued,
    /// `harness.bookkeeping.queue` - a `type:"queue-operation"` line that is NOT the
    /// human's own content (a `dequeue`, or a harness rider).
    BookkeepingQueue,
}
