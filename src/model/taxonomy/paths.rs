//! [`Class::ALL`] and [`Class::path`]: the leaf space and its canonical dotted form.

use super::*;

#[allow(dead_code)]
impl Class {
    /// Every leaf [`Class`] in taxonomy order - the single source of truth for
    /// enumerating the class space. The `-t` selector table is built from it, the
    /// inventory is checked against it, and the hand-kept oracle table pins every
    /// path byte-exactly (a variant added to the enum but not here is caught by the
    /// `all_classes_cover_the_enum` test). Order: the 17 categories of
    /// [`Class::CATEGORIES`], and inside each the map's own leaf order.
    pub const ALL: &'static [Class] = &[
        // user (6)
        Class::UserMessage,
        Class::UserAnswer,
        Class::UserRejection,
        Class::UserUnsent,
        Class::UserRewound,
        Class::UserQueued,
        // agent.message (2)
        Class::AgentMessagePlain,
        Class::AgentMessageStructured,
        // agent.thinking (4)
        Class::AgentThinkingSummary,
        Class::AgentThinkingNarration,
        Class::AgentThinkingOmitted,
        Class::AgentThinkingRedacted,
        // agent.tool (2)
        Class::AgentToolUse,
        Class::AgentToolResult,
        // agent.communication (4)
        Class::CommInbox,
        Class::CommSent,
        Class::CommSignal,
        Class::CommChannel,
        // harness.notification (5)
        Class::NotificationWorkflow,
        Class::NotificationMonitor,
        Class::NotificationSubagent,
        Class::NotificationBackgroundCommand,
        Class::NotificationTask,
        // harness.compaction (2)
        Class::CompactionSummary,
        Class::CompactionBoundary,
        // harness.command (3)
        Class::CommandInvocation,
        Class::CommandStdout,
        Class::CommandCaveat,
        // harness.interrupt (2)
        Class::InterruptUser,
        Class::InterruptTool,
        // harness.schedule (4)
        Class::ScheduleWakeup,
        Class::ScheduleEvent,
        Class::ScheduleFire,
        Class::ScheduleTick,
        // harness.resume (2)
        Class::ResumePrompt,
        Class::ResumePlaceholder,
        // harness.hook (3)
        Class::HookContext,
        Class::HookOutcome,
        Class::HookFeedback,
        // harness.context (9)
        Class::ContextReminder,
        Class::ContextListing,
        Class::ContextPrompt,
        Class::ContextFile,
        Class::ContextMode,
        Class::ContextIde,
        Class::ContextTask,
        Class::ContextImage,
        Class::ContextSkill,
        // harness.enforce (4)
        Class::EnforceStructuredOutput,
        Class::EnforceBrief,
        Class::EnforceHandback,
        Class::EnforceRetry,
        // harness.turn (3)
        Class::TurnDuration,
        Class::TurnStopHooks,
        Class::TurnRecap,
        // harness.system (5)
        Class::SystemRefusal,
        Class::SystemApiError,
        Class::SystemNotice,
        Class::SystemKill,
        Class::SystemCommand,
        // harness.bookkeeping (13)
        Class::BookkeepingLastPrompt,
        Class::BookkeepingMode,
        Class::BookkeepingName,
        Class::BookkeepingAtis,
        Class::BookkeepingBridge,
        Class::BookkeepingSuppression,
        Class::BookkeepingPrLink,
        Class::BookkeepingArtifact,
        Class::BookkeepingFileHistory,
        Class::BookkeepingCost,
        Class::BookkeepingFork,
        Class::BookkeepingContinued,
        Class::BookkeepingQueue,
    ];

    /// The canonical dotted path - the `-t` selector form and the rendered label.
    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Class::UserMessage => "user.message",
            Class::UserAnswer => "user.answer",
            Class::UserRejection => "user.rejection",
            Class::UserUnsent => "user.unsent",
            Class::UserRewound => "user.rewound",
            Class::UserQueued => "user.queued",
            Class::AgentMessagePlain => "agent.message.plain",
            Class::AgentMessageStructured => "agent.message.structured",
            Class::AgentThinkingSummary => "agent.thinking.summary",
            Class::AgentThinkingNarration => "agent.thinking.narration",
            Class::AgentThinkingOmitted => "agent.thinking.omitted",
            Class::AgentThinkingRedacted => "agent.thinking.redacted",
            Class::AgentToolUse => "agent.tool.use",
            Class::AgentToolResult => "agent.tool.result",
            Class::CommInbox => "agent.communication.inbox",
            Class::CommSent => "agent.communication.sent",
            Class::CommSignal => "agent.communication.signal",
            Class::CommChannel => "agent.communication.channel",
            Class::NotificationWorkflow => "harness.notification.workflow",
            Class::NotificationMonitor => "harness.notification.monitor",
            Class::NotificationSubagent => "harness.notification.subagent",
            Class::NotificationBackgroundCommand => "harness.notification.background-command",
            Class::NotificationTask => "harness.notification.task",
            Class::CompactionSummary => "harness.compaction.summary",
            Class::CompactionBoundary => "harness.compaction.boundary",
            Class::CommandInvocation => "harness.command.invocation",
            Class::CommandStdout => "harness.command.stdout",
            Class::CommandCaveat => "harness.command.caveat",
            Class::InterruptUser => "harness.interrupt.user",
            Class::InterruptTool => "harness.interrupt.tool",
            Class::ScheduleWakeup => "harness.schedule.wakeup",
            Class::ScheduleEvent => "harness.schedule.event",
            Class::ScheduleFire => "harness.schedule.fire",
            Class::ScheduleTick => "harness.schedule.tick",
            Class::ResumePrompt => "harness.resume.prompt",
            Class::ResumePlaceholder => "harness.resume.placeholder",
            Class::HookContext => "harness.hook.context",
            Class::HookOutcome => "harness.hook.outcome",
            Class::HookFeedback => "harness.hook.feedback",
            Class::ContextReminder => "harness.context.reminder",
            Class::ContextListing => "harness.context.listing",
            Class::ContextPrompt => "harness.context.prompt",
            Class::ContextFile => "harness.context.file",
            Class::ContextMode => "harness.context.mode",
            Class::ContextIde => "harness.context.ide",
            Class::ContextTask => "harness.context.task",
            Class::ContextImage => "harness.context.image",
            Class::ContextSkill => "harness.context.skill",
            Class::EnforceStructuredOutput => "harness.enforce.structured-output",
            Class::EnforceBrief => "harness.enforce.brief",
            Class::EnforceHandback => "harness.enforce.handback",
            Class::EnforceRetry => "harness.enforce.retry",
            Class::TurnDuration => "harness.turn.duration",
            Class::TurnStopHooks => "harness.turn.stop-hooks",
            Class::TurnRecap => "harness.turn.recap",
            Class::SystemRefusal => "harness.system.refusal",
            Class::SystemApiError => "harness.system.api-error",
            Class::SystemNotice => "harness.system.notice",
            Class::SystemKill => "harness.system.kill",
            Class::SystemCommand => "harness.system.command",
            Class::BookkeepingLastPrompt => "harness.bookkeeping.last-prompt",
            Class::BookkeepingMode => "harness.bookkeeping.mode",
            Class::BookkeepingName => "harness.bookkeeping.name",
            Class::BookkeepingAtis => "harness.bookkeeping.atis",
            Class::BookkeepingBridge => "harness.bookkeeping.bridge",
            Class::BookkeepingSuppression => "harness.bookkeeping.suppression",
            Class::BookkeepingPrLink => "harness.bookkeeping.pr-link",
            Class::BookkeepingArtifact => "harness.bookkeeping.artifact",
            Class::BookkeepingFileHistory => "harness.bookkeeping.file-history",
            Class::BookkeepingCost => "harness.bookkeeping.cost",
            Class::BookkeepingFork => "harness.bookkeeping.fork",
            Class::BookkeepingContinued => "harness.bookkeeping.continued",
            Class::BookkeepingQueue => "harness.bookkeeping.queue",
        }
    }
}
