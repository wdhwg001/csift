//! Role + Class: the role.class.sub label taxonomy (Class::ALL is the source of truth).

/// The top-level ROLE of a classified record (GOLD §2). The first dot-segment of every
/// [`Class::path`]. A multi-label record can span roles (e.g. an AUQ answer is both
/// [`Role::User`] and [`Role::Agent`]).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The human operator.
    User,
    /// The assistant (incl. its tool I/O and peer communication).
    Agent,
    /// Claude Code machinery (notifications, compaction, slash wrappers, interrupts, schedule).
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

/// A LEAF class in the role.class.sub taxonomy (GOLD §2). One variant per leaf; the dotted
/// [`Class::path`] is the canonical wire/selector form and [`Class::role`] its top-level role.
/// A record carries a `Vec<Class>` (multi-label, GOLD §3) via [`Record::classify`].
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// `user.message` - genuine human prose (incl. slash-command `<command-args>`).
    UserMessage,
    /// `user.answer` - an AskUserQuestion answer (the Q+options+answer unit). Dual-labeled
    /// with [`Class::AgentToolResult`] (it rides on the answering tool_result carrier).
    UserAnswer,
    /// `user.rejection` - a plan/tool rejection carrying a typed instruction. Dual-labeled
    /// with [`Class::AgentToolResult`].
    UserRejection,
    /// `user.unsent` - a SUPERSEDED turn-opener draft: sent, esc-recalled into the input
    /// box, edited and re-sent, leaving the original on disk sharing the resend's
    /// parentUuid. Assigned at the SCAN layer (the superseded set needs a LATER sibling,
    /// which a pure per-record classify cannot see); outside turn numbering; 99% never
    /// drew a reply. A recalled-then-ABANDONED message has no sibling and is
    /// structurally undetectable; a QUEUED text edited before dispatch never becomes a
    /// user record at all (it survives only in `queue-operation` lines).
    UserUnsent,
    /// `user.queued` - the human's text as it sat in the input QUEUE: a
    /// `queue-operation` line carrying `content` (an `enqueue`, a `popAll` recall to
    /// the input box, or a `remove` with the text). Not in the surviving conversation
    /// (no `message{}`); the dispatched twin, when there is one, is the later
    /// `user.message` record. Automation riders (a `<task-notification>` / peer
    /// message queued by the harness) are NOT the human and carry no label here;
    /// content-less `dequeue` lines carry nothing to search. The queue line has no
    /// join key (measured: 4-6 keys, no promptId/uuid), so `dispatched` is never
    /// asserted - only the verbatim `operation` + `reason` facts ride the hit.
    UserQueued,
    /// `agent.message` - the assistant's visible end-of-turn text block(s).
    AgentMessage,
    /// `agent.thinking` - a thinking block (see the GOLD-gap note re `redacted_thinking`).
    AgentThinking,
    /// `agent.thinking.narration` - a narration-tagged thinking block: an API-issued
    /// one-sentence summary of the reasoning beside it, NOT the reasoning itself. The
    /// tag hides inside the base64 `signature` (see `model/narration.rs`); the first
    /// leaf whose path is prefixed by another leaf (`-t agent.thinking` selects both).
    AgentThinkingNarration,
    /// `agent.tool.use` - a tool_use block (incl. a pending elicitation sidecar marker).
    AgentToolUse,
    /// `agent.tool.result` - a tool_result block (incl. errored).
    AgentToolResult,
    /// `agent.communication.inbox` - a received peer message / spawn prompt / subagent return.
    CommInbox,
    /// `agent.communication.sent` - a sent peer message (`SendMessage`) or a spawn.
    CommSent,
    /// `agent.communication.signal` - a control/status comm (idle_notification, shutdown_*).
    CommSignal,
    /// `harness.notification.workflow` - a `<task-notification>` for a dynamic/OMC workflow.
    NotificationWorkflow,
    /// `harness.notification.monitor` - a monitor/cron cadence completion pulse.
    NotificationMonitor,
    /// `harness.notification.subagent` - a spawned-subagent completion pulse (renamed from
    /// [`AutomationKind::Agent`] so it never collides with the `agent` role).
    NotificationSubagent,
    /// `harness.notification.background-command` - a `&`-detached shell command pulse.
    NotificationBackgroundCommand,
    /// `harness.notification.task` - any other / unclassified `<task-notification>`.
    NotificationTask,
    /// `harness.compaction.summary` - the `isCompactSummary` summary record.
    CompactionSummary,
    /// `harness.compaction.boundary` - the `system`/`compact_boundary` metrics record.
    CompactionBoundary,
    /// `harness.command.invocation` - a `<command-name>…` slash-command wrapper.
    CommandInvocation,
    /// `harness.command.stdout` - a `<local-command-stdout>…` local-command output.
    CommandStdout,
    /// `harness.interrupt.user` - `[Request interrupted by user]`.
    InterruptUser,
    /// `harness.interrupt.tool` - `[Request interrupted by user for tool use]`.
    InterruptTool,
    /// `harness.schedule.wakeup` - a fired `ScheduleWakeup` TIMER tick (its injected
    /// [`SCHEDULE_WAKEUP_MARKER`] prompt). Distinct from [`Class::MetaLoop`] (the
    /// autonomous-loop driver prose); the timer is the harness scheduler firing.
    ScheduleWakeup,
    /// `harness.schedule.continuation` - a `Continue from where you left off.` resume tick.
    ScheduleContinuation,
    /// `harness.meta.hook` - hook-injected feedback (stop-hook / `<local-command-caveat>` /
    /// edit-failed-retry), not the operator.
    MetaHook,
    /// `harness.meta.loop` - an autonomous-loop driver tick (`# Autonomous loop tick` /
    /// `Run the autonomous check`).
    MetaLoop,
    /// `harness.meta.attachment` - any OTHER `type:"attachment"` record's payload
    /// (edited_text_file, compact_file_reference, file snapshots, todos, …). Scanned only
    /// under `search --attachments` / `--count-by attachment` (or an explicit `show`
    /// address); the matchable text is the VERBATIM payload JSON. A hook-context payload
    /// stays the more specific [`Class::MetaHook`].
    MetaAttachment,
    /// `harness.meta.turn-duration` - the `system`/`turn_duration` end-of-turn
    /// telemetry record (`durationMs`, `messageCount`, and the optional
    /// `pendingBackgroundAgentCount` / `pendingWorkflowCount`): the structured body
    /// behind the REPL's "Done in Ns" / "Waiting for N agents" lines, which never land
    /// on disk themselves. No `message{}`: not in the surviving conversation.
    MetaTurnDuration,
    /// `harness.meta.away-summary` - the `system`/`away_summary` recap the harness
    /// generates (a side model call, config-gated) when the operator returns after 5+
    /// minutes away. Model-generated prose, yet no `message{}`: shown in the UI, not in
    /// the surviving conversation (measured 1,196/1,198 never re-appear downstream).
    MetaAwaySummary,
    /// `harness.meta.stop-hooks` - the `system`/`stop_hook_summary` execution ledger
    /// of the Stop hooks that ran at turn end (`hookInfos[].command` + durations,
    /// `hookErrors`, `preventedContinuation`). Distinct from [`Class::MetaHook`], which
    /// is the text a hook INJECTED into the model; this record is the run itself and
    /// its `hookAdditionalContext` is empty on every measured instance.
    MetaStopHooks,
    /// `harness.meta.snapshot` - a `file-history-snapshot` (the per-prompt tracked-file
    /// version table) or a `file-history-delta` (one path's version bump) line: the
    /// instrument `recover` reads for external settings writes (v0.9.4), now
    /// searchable by path and version. No `message{}`; a snapshot line has no
    /// top-level timestamp (the excerpt carries the nested one).
    MetaSnapshot,
    /// `harness.meta.system` (v0.10.1) - the catch-all for every OTHER `type:"system"`
    /// subtype the harness writes for its own UI and never sends to the model:
    /// `informational` (e.g. the Remote Control disconnect warning, `level:"warning"`),
    /// `api_error`, `model_refusal_fallback` / `model_refusal_no_fallback`,
    /// `agents_killed`, `local_command`, `scheduled_task_fire`, and any subtype a
    /// future build adds. Renders `[<subtype> <level>] <content>`. No `message{}`, so
    /// invisible by the same instrument as the rest of this family; gated like them.
    MetaSystem,
}

#[allow(dead_code)]
impl Class {
    /// LLM-VISIBILITY (v0.9.4): true when the record's content is part of the
    /// conversation the model actually receives or produces. Exactly TWO leaves are
    /// invisible, each with a measured instrument:
    /// - `user.unsent`: a superseded draft is NOT in the surviving conversation -
    ///   Claude Code's own `compactMetadata.preservedMessages` accounting excludes
    ///   every draft uuid (0 of 772 measured), and the conversation DAG threads
    ///   through the resend sibling, never the draft. (Wording law: "not in the
    ///   surviving conversation", never "the model never saw it" - a few drafts
    ///   drew real replies before the retraction.)
    /// - `harness.compaction.boundary`: a system record with NO message field at
    ///   all - pure compaction metrics. (The compaction SUMMARY is visible: the
    ///   DAG threads through it; `isVisibleInTranscriptOnly` is a display flag on
    ///   summaries, not a delivery flag, and `isMeta` is an authorship flag -
    ///   neither is a visibility instrument.)
    ///
    /// v0.10.0 adds the promoted non-record line types, all invisible by the same
    /// instrument as the boundary: ZERO of them carries a `message{}` field (measured
    /// over every non-record line type in the corpus; every user/assistant record
    /// does), and Claude Code's
    /// own source labels them REPL-render internals. DAG threading is NOT the
    /// instrument here - later user records name a `turn_duration` uuid as parentUuid
    /// (chain continuity), and `preservedMessages` lists these uuids at the same rate
    /// as messages (it is a tail window, not a visibility filter).
    ///
    /// A bare ROLE selector (`-t user`) expands to visible leaves only; the
    /// glob form and explicit paths reach the invisible ones.
    #[must_use]
    pub fn llm_visible(self) -> bool {
        !matches!(
            self,
            Class::UserUnsent
                | Class::UserQueued
                | Class::CompactionBoundary
                | Class::MetaTurnDuration
                | Class::MetaAwaySummary
                | Class::MetaStopHooks
                | Class::MetaSnapshot
                | Class::MetaSystem
        )
    }

    /// Every leaf [`Class`] in taxonomy order (GOLD §2). The single source of truth for
    /// enumerating the class space - P2 builds the `-t` selector table from it, and tests
    /// assert `path()`/`role()` exhaustively over it (a new variant added to the enum but not
    /// here is caught by the `all_classes_cover_the_enum` test). Order: user, agent (+comm),
    /// harness (notification, compaction, command, interrupt, schedule, meta).
    pub const ALL: &'static [Class] = &[
        Class::UserMessage,
        Class::UserAnswer,
        Class::UserRejection,
        Class::UserUnsent,
        Class::UserQueued,
        Class::AgentMessage,
        Class::AgentThinking,
        Class::AgentThinkingNarration,
        Class::AgentToolUse,
        Class::AgentToolResult,
        Class::CommInbox,
        Class::CommSent,
        Class::CommSignal,
        Class::NotificationWorkflow,
        Class::NotificationMonitor,
        Class::NotificationSubagent,
        Class::NotificationBackgroundCommand,
        Class::NotificationTask,
        Class::CompactionSummary,
        Class::CompactionBoundary,
        Class::CommandInvocation,
        Class::CommandStdout,
        Class::InterruptUser,
        Class::InterruptTool,
        Class::ScheduleWakeup,
        Class::ScheduleContinuation,
        Class::MetaHook,
        Class::MetaLoop,
        Class::MetaAttachment,
        Class::MetaTurnDuration,
        Class::MetaAwaySummary,
        Class::MetaStopHooks,
        Class::MetaSnapshot,
        Class::MetaSystem,
    ];

    /// The canonical dotted path (GOLD §2) - the `-t` selector form (P2) and render label.
    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Class::UserMessage => "user.message",
            Class::UserAnswer => "user.answer",
            Class::UserRejection => "user.rejection",
            Class::UserUnsent => "user.unsent",
            Class::UserQueued => "user.queued",
            Class::AgentMessage => "agent.message",
            Class::AgentThinking => "agent.thinking",
            Class::AgentThinkingNarration => "agent.thinking.narration",
            Class::AgentToolUse => "agent.tool.use",
            Class::AgentToolResult => "agent.tool.result",
            Class::CommInbox => "agent.communication.inbox",
            Class::CommSent => "agent.communication.sent",
            Class::CommSignal => "agent.communication.signal",
            Class::NotificationWorkflow => "harness.notification.workflow",
            Class::NotificationMonitor => "harness.notification.monitor",
            Class::NotificationSubagent => "harness.notification.subagent",
            Class::NotificationBackgroundCommand => "harness.notification.background-command",
            Class::NotificationTask => "harness.notification.task",
            Class::CompactionSummary => "harness.compaction.summary",
            Class::CompactionBoundary => "harness.compaction.boundary",
            Class::CommandInvocation => "harness.command.invocation",
            Class::CommandStdout => "harness.command.stdout",
            Class::InterruptUser => "harness.interrupt.user",
            Class::InterruptTool => "harness.interrupt.tool",
            Class::ScheduleWakeup => "harness.schedule.wakeup",
            Class::ScheduleContinuation => "harness.schedule.continuation",
            Class::MetaHook => "harness.meta.hook",
            Class::MetaLoop => "harness.meta.loop",
            Class::MetaAttachment => "harness.meta.attachment",
            Class::MetaTurnDuration => "harness.meta.turn-duration",
            Class::MetaAwaySummary => "harness.meta.away-summary",
            Class::MetaStopHooks => "harness.meta.stop-hooks",
            Class::MetaSnapshot => "harness.meta.snapshot",
            Class::MetaSystem => "harness.meta.system",
        }
    }

    /// The top-level role (the first dot-segment of [`Class::path`]). Exhaustive (no
    /// wildcard) so a future leaf forces an explicit role decision at compile time.
    #[must_use]
    pub fn role(self) -> Role {
        match self {
            Class::UserMessage
            | Class::UserAnswer
            | Class::UserRejection
            | Class::UserUnsent
            | Class::UserQueued => Role::User,
            Class::AgentMessage
            | Class::AgentThinking
            | Class::AgentThinkingNarration
            | Class::AgentToolUse
            | Class::AgentToolResult
            | Class::CommInbox
            | Class::CommSent
            | Class::CommSignal => Role::Agent,
            Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask
            | Class::CompactionSummary
            | Class::CompactionBoundary
            | Class::CommandInvocation
            | Class::CommandStdout
            | Class::InterruptUser
            | Class::InterruptTool
            | Class::ScheduleWakeup
            | Class::ScheduleContinuation
            | Class::MetaHook
            | Class::MetaLoop
            | Class::MetaAttachment
            | Class::MetaTurnDuration
            | Class::MetaAwaySummary
            | Class::MetaStopHooks
            | Class::MetaSnapshot
            | Class::MetaSystem => Role::Harness,
        }
    }
}
