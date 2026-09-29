//! The three facets a `-t` selector reads off a leaf: its CATEGORY, its ROLE and
//! whether the model receives the leaf's content at all.

use super::*;

#[allow(dead_code)]
impl Class {
    /// The 17 CATEGORIES, in taxonomy order - the dotted middle tier of the label
    /// space. Every entry is a valid `-t` selector and every leaf's path opens with
    /// exactly one of them (a unit test pins both directions against [`Class::ALL`]).
    /// The six `user` leaves have no middle segment, so their category IS the role.
    pub const CATEGORIES: &'static [&'static str] = &[
        "user",
        "agent.message",
        "agent.thinking",
        "agent.tool",
        "agent.communication",
        "harness.notification",
        "harness.compaction",
        "harness.command",
        "harness.interrupt",
        "harness.schedule",
        "harness.resume",
        "harness.hook",
        "harness.context",
        "harness.enforce",
        "harness.turn",
        "harness.system",
        "harness.bookkeeping",
    ];

    /// This leaf's CATEGORY: the role plus the middle segment of [`Class::path`], or
    /// the bare role for the six `user` leaves, which have no middle segment.
    /// Exhaustive (no wildcard) so a new leaf forces an explicit decision at compile
    /// time - and since [`Class::role`] reads this answer, that one decision settles
    /// both facets.
    #[must_use]
    pub fn category(self) -> &'static str {
        match self {
            Class::UserMessage
            | Class::UserAnswer
            | Class::UserRejection
            | Class::UserUnsent
            | Class::UserRewound
            | Class::UserQueued => "user",
            Class::AgentMessagePlain | Class::AgentMessageStructured => "agent.message",
            Class::AgentThinkingSummary
            | Class::AgentThinkingNarration
            | Class::AgentThinkingOmitted
            | Class::AgentThinkingRedacted => "agent.thinking",
            Class::AgentToolUse | Class::AgentToolResult => "agent.tool",
            Class::CommInbox | Class::CommSent | Class::CommSignal | Class::CommChannel => {
                "agent.communication"
            }
            Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask => "harness.notification",
            Class::CompactionSummary | Class::CompactionBoundary => "harness.compaction",
            Class::CommandInvocation | Class::CommandStdout | Class::CommandCaveat => {
                "harness.command"
            }
            Class::InterruptUser | Class::InterruptTool => "harness.interrupt",
            Class::ScheduleWakeup
            | Class::ScheduleEvent
            | Class::ScheduleFire
            | Class::ScheduleTick => "harness.schedule",
            Class::ResumePrompt | Class::ResumePlaceholder => "harness.resume",
            Class::HookContext | Class::HookOutcome | Class::HookFeedback => "harness.hook",
            Class::ContextReminder
            | Class::ContextListing
            | Class::ContextPrompt
            | Class::ContextFile
            | Class::ContextMode
            | Class::ContextIde
            | Class::ContextTask
            | Class::ContextImage
            | Class::ContextSkill => "harness.context",
            Class::EnforceStructuredOutput
            | Class::EnforceBrief
            | Class::EnforceHandback
            | Class::EnforceRetry => "harness.enforce",
            Class::TurnDuration | Class::TurnStopHooks | Class::TurnRecap => "harness.turn",
            Class::SystemRefusal
            | Class::SystemApiError
            | Class::SystemNotice
            | Class::SystemKill
            | Class::SystemCommand => "harness.system",
            Class::BookkeepingLastPrompt
            | Class::BookkeepingMode
            | Class::BookkeepingName
            | Class::BookkeepingAtis
            | Class::BookkeepingBridge
            | Class::BookkeepingSuppression
            | Class::BookkeepingPrLink
            | Class::BookkeepingArtifact
            | Class::BookkeepingFileHistory
            | Class::BookkeepingCost
            | Class::BookkeepingFork
            | Class::BookkeepingContinued
            | Class::BookkeepingQueue => "harness.bookkeeping",
        }
    }

    /// The top-level role - the first dot-segment of [`Class::path`], read off
    /// [`Class::category`] so the two can never disagree. Only three roles exist and
    /// the category answer is exhaustive, so the final arm is the harness one rather
    /// than a guess; a unit test pins `path().starts_with(role().as_str())` for every
    /// leaf in [`Class::ALL`].
    #[must_use]
    pub fn role(self) -> Role {
        match self.category().split('.').next() {
            Some("user") => Role::User,
            Some("agent") => Role::Agent,
            _ => Role::Harness,
        }
    }

    /// LLM-VISIBILITY: true when the leaf's content is part of the conversation the
    /// model actually receives OR PRODUCES. The starting point for a BARE role or
    /// category selector, which asks for the conversation and nothing else; every
    /// other selector form (a glob, an intermediate prefix, an exact leaf) reaches
    /// every leaf regardless.
    ///
    /// 28 of the 73 leaves are invisible, and the reasons group:
    /// - the two SCAN-LAYER user leaves (`user.unsent`, `user.rewound`): ordinary
    ///   user records the surviving conversation does not thread through;
    /// - `user.queued` and the whole `harness.bookkeeping` category: non-transcript
    ///   lines with no `message{}` at all, which the loader never admits;
    /// - the two textless thinking leaves (`omitted`, `redacted`): a block the model
    ///   echoes back by signature, carrying no text it could read;
    /// - `harness.compaction.boundary`, `harness.schedule.event` and the three
    ///   `harness.turn` leaves: message-less `type:"system"` records written for the
    ///   REPL's own banners;
    /// - four of the five `harness.system` leaves: the request assembler's drop
    ///   predicate skips every system subtype but `local_command`, which is why
    ///   `harness.system.command` is visible here;
    /// - `harness.hook.outcome`: a hook RUN's record, which the harness converts to
    ///   nothing - with the per-RECORD exception of a `hook_success` carrying a
    ///   rendered body ([`Record::delivery_override`]).
    ///
    /// `agent.message.structured` is VISIBLE although the harness never converts the
    /// attachment back: the leaf default answers "is this text part of the
    /// conversation", and the model produced it.
    ///
    /// The leaf default is never the whole answer. Delivery is decided per RECORD by
    /// [`Record::delivery_override`], and survival per record by the conversation
    /// chain; a bare selector consults all three.
    #[must_use]
    pub fn llm_visible(self) -> bool {
        !matches!(
            self,
            Class::UserUnsent
                | Class::UserRewound
                | Class::UserQueued
                | Class::AgentThinkingOmitted
                | Class::AgentThinkingRedacted
                | Class::CompactionBoundary
                | Class::ScheduleEvent
                | Class::HookOutcome
                | Class::TurnDuration
                | Class::TurnStopHooks
                | Class::TurnRecap
                | Class::SystemRefusal
                | Class::SystemApiError
                | Class::SystemNotice
                | Class::SystemKill
                | Class::BookkeepingLastPrompt
                | Class::BookkeepingMode
                | Class::BookkeepingName
                | Class::BookkeepingAtis
                | Class::BookkeepingBridge
                | Class::BookkeepingSuppression
                | Class::BookkeepingPrLink
                | Class::BookkeepingArtifact
                | Class::BookkeepingFileHistory
                | Class::BookkeepingCost
                | Class::BookkeepingFork
                | Class::BookkeepingContinued
                | Class::BookkeepingQueue
        )
    }

    /// The three leaves a scheduler SUBMISSION carries: the prompt a fire record submits is
    /// `harness.schedule.fire`, or `.wakeup` / `.tick` when its resolved text opens with a
    /// loop marker (claims CLS-031, CLS-032). The fire instant and the `fires` edge are facts
    /// of such a prompt's parent link. Any OTHER record whose parent happens to be a fire
    /// record - Claude Code parents every append to the running tail (claim CLS-049), so a
    /// `<task-notification>` arriving next is one - is no submission and carries neither.
    #[must_use]
    pub fn is_scheduler_submission(self) -> bool {
        matches!(
            self,
            Class::ScheduleFire | Class::ScheduleWakeup | Class::ScheduleTick
        )
    }
}
