//! Taxonomy drift guards: the hand-kept ORACLE of all 73 dotted paths, the
//! hand-kept VISIBLE-FACE table of the leaves a bare selector skips, and the
//! category partition. Every table here is typed out by hand on purpose - a test
//! that derived its expectations from the code it guards would pass on any drift.

use super::*;

/// The hand-kept ORACLE: every leaf and its canonical dotted path, in taxonomy
/// order. A leaf added to the enum but not here fails the length pin below.
const ORACLE: &[(Class, &str)] = &[
    (Class::UserMessage, "user.message"),
    (Class::UserAnswer, "user.answer"),
    (Class::UserRejection, "user.rejection"),
    (Class::UserUnsent, "user.unsent"),
    (Class::UserRewound, "user.rewound"),
    (Class::UserQueued, "user.queued"),
    (Class::AgentMessagePlain, "agent.message.plain"),
    (Class::AgentMessageStructured, "agent.message.structured"),
    (Class::AgentThinkingSummary, "agent.thinking.summary"),
    (Class::AgentThinkingNarration, "agent.thinking.narration"),
    (Class::AgentThinkingOmitted, "agent.thinking.omitted"),
    (Class::AgentThinkingRedacted, "agent.thinking.redacted"),
    (Class::AgentToolUse, "agent.tool.use"),
    (Class::AgentToolResult, "agent.tool.result"),
    (Class::CommInbox, "agent.communication.inbox"),
    (Class::CommSent, "agent.communication.sent"),
    (Class::CommSignal, "agent.communication.signal"),
    (Class::CommChannel, "agent.communication.channel"),
    (Class::NotificationWorkflow, "harness.notification.workflow"),
    (Class::NotificationMonitor, "harness.notification.monitor"),
    (Class::NotificationSubagent, "harness.notification.subagent"),
    (
        Class::NotificationBackgroundCommand,
        "harness.notification.background-command",
    ),
    (Class::NotificationTask, "harness.notification.task"),
    (Class::CompactionSummary, "harness.compaction.summary"),
    (Class::CompactionBoundary, "harness.compaction.boundary"),
    (Class::CommandInvocation, "harness.command.invocation"),
    (Class::CommandStdout, "harness.command.stdout"),
    (Class::CommandCaveat, "harness.command.caveat"),
    (Class::InterruptUser, "harness.interrupt.user"),
    (Class::InterruptTool, "harness.interrupt.tool"),
    (Class::ScheduleWakeup, "harness.schedule.wakeup"),
    (Class::ScheduleEvent, "harness.schedule.event"),
    (Class::ScheduleFire, "harness.schedule.fire"),
    (Class::ScheduleTick, "harness.schedule.tick"),
    (Class::ResumePrompt, "harness.resume.prompt"),
    (Class::ResumePlaceholder, "harness.resume.placeholder"),
    (Class::HookContext, "harness.hook.context"),
    (Class::HookOutcome, "harness.hook.outcome"),
    (Class::HookFeedback, "harness.hook.feedback"),
    (Class::ContextReminder, "harness.context.reminder"),
    (Class::ContextListing, "harness.context.listing"),
    (Class::ContextPrompt, "harness.context.prompt"),
    (Class::ContextFile, "harness.context.file"),
    (Class::ContextMode, "harness.context.mode"),
    (Class::ContextIde, "harness.context.ide"),
    (Class::ContextTask, "harness.context.task"),
    (Class::ContextImage, "harness.context.image"),
    (Class::ContextSkill, "harness.context.skill"),
    (
        Class::EnforceStructuredOutput,
        "harness.enforce.structured-output",
    ),
    (Class::EnforceBrief, "harness.enforce.brief"),
    (Class::EnforceHandback, "harness.enforce.handback"),
    (Class::EnforceRetry, "harness.enforce.retry"),
    (Class::TurnDuration, "harness.turn.duration"),
    (Class::TurnStopHooks, "harness.turn.stop-hooks"),
    (Class::TurnRecap, "harness.turn.recap"),
    (Class::SystemRefusal, "harness.system.refusal"),
    (Class::SystemApiError, "harness.system.api-error"),
    (Class::SystemNotice, "harness.system.notice"),
    (Class::SystemKill, "harness.system.kill"),
    (Class::SystemCommand, "harness.system.command"),
    (
        Class::BookkeepingLastPrompt,
        "harness.bookkeeping.last-prompt",
    ),
    (Class::BookkeepingMode, "harness.bookkeeping.mode"),
    (Class::BookkeepingName, "harness.bookkeeping.name"),
    (Class::BookkeepingAtis, "harness.bookkeeping.atis"),
    (Class::BookkeepingBridge, "harness.bookkeeping.bridge"),
    (
        Class::BookkeepingSuppression,
        "harness.bookkeeping.suppression",
    ),
    (Class::BookkeepingPrLink, "harness.bookkeeping.pr-link"),
    (Class::BookkeepingArtifact, "harness.bookkeeping.artifact"),
    (
        Class::BookkeepingFileHistory,
        "harness.bookkeeping.file-history",
    ),
    (Class::BookkeepingCost, "harness.bookkeeping.cost"),
    (Class::BookkeepingFork, "harness.bookkeeping.fork"),
    (Class::BookkeepingContinued, "harness.bookkeeping.continued"),
    (Class::BookkeepingQueue, "harness.bookkeeping.queue"),
];

/// The hand-kept VISIBLE-FACE table: every leaf a bare role or category selector
/// skips because the model receives no text from it. Typed out leaf by leaf so a
/// visibility flip anywhere fails here rather than silently widening a bare `-t`.
const INVISIBLE: &[Class] = &[
    Class::UserUnsent,
    Class::UserRewound,
    Class::UserQueued,
    Class::AgentThinkingOmitted,
    Class::AgentThinkingRedacted,
    Class::CompactionBoundary,
    Class::ScheduleEvent,
    Class::HookOutcome,
    Class::TurnDuration,
    Class::TurnStopHooks,
    Class::TurnRecap,
    Class::SystemRefusal,
    Class::SystemApiError,
    Class::SystemNotice,
    Class::SystemKill,
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

#[test]
fn oracle_pins_every_path_and_the_leaf_count() {
    assert_eq!(ORACLE.len(), 73, "the oracle table is not 73 rows");
    assert_eq!(
        Class::ALL.len(),
        ORACLE.len(),
        "Class::ALL drifted from the oracle table"
    );
    for (i, &(c, p)) in ORACLE.iter().enumerate() {
        assert_eq!(c.path(), p, "path mismatch for {c:?}");
        assert_eq!(Class::ALL[i], c, "Class::ALL order drifted at index {i}");
    }
    let mut paths: Vec<&str> = Class::ALL.iter().map(|c| c.path()).collect();
    paths.sort_unstable();
    let n = paths.len();
    paths.dedup();
    assert_eq!(paths.len(), n, "duplicate path in Class::ALL");
}

#[test]
fn visible_face_is_exactly_the_hand_kept_invisible_set() {
    assert_eq!(INVISIBLE.len(), 28, "the invisible table is not 28 rows");
    for &c in INVISIBLE {
        assert!(!c.llm_visible(), "{c:?} should be invisible");
    }
    for &c in Class::ALL {
        let listed = INVISIBLE.contains(&c);
        assert_eq!(
            c.llm_visible(),
            !listed,
            "{c:?} visibility disagrees with the hand-kept table"
        );
    }
    // The two leaves whose visibility is the interesting call, pinned by name: a
    // structured answer the MODEL produced is visible; the one delivered system
    // subtype is visible although its record carries no `message{}`.
    assert!(Class::AgentMessageStructured.llm_visible());
    assert!(Class::SystemCommand.llm_visible());
}

#[test]
fn categories_partition_every_leaf() {
    assert_eq!(Class::CATEGORIES.len(), 17, "the category list is not 17");
    for &c in Class::ALL {
        let cat = c.category();
        assert!(
            Class::CATEGORIES.contains(&cat),
            "{c:?} names a category outside CATEGORIES: {cat}"
        );
        // The category is a dot-SEGMENT prefix of the path, and the user leaves are
        // the only ones whose category IS their role.
        let rest = c
            .path()
            .strip_prefix(cat)
            .unwrap_or_else(|| panic!("{cat} is not a prefix of {}", c.path()));
        assert!(
            rest.starts_with('.'),
            "{cat} is not a segment prefix of {}",
            c.path()
        );
        assert_eq!(
            c.role().as_str(),
            c.path().split('.').next().unwrap(),
            "role/path head mismatch for {c:?}"
        );
    }
    // No category is empty, and the list is in the same order the leaves appear in.
    let mut order: Vec<&str> = Vec::new();
    for &c in Class::ALL {
        if order.last() != Some(&c.category()) {
            order.push(c.category());
        }
    }
    assert_eq!(order, Class::CATEGORIES, "CATEGORIES order drifted");
}

#[test]
fn role_as_str_and_class_role_partition() {
    assert_eq!(Role::User.as_str(), "user");
    assert_eq!(Role::Agent.as_str(), "agent");
    assert_eq!(Role::Harness.as_str(), "harness");
    assert_eq!(Class::UserAnswer.role(), Role::User);
    assert_eq!(Class::CommSignal.role(), Role::Agent);
    assert_eq!(Class::AgentToolResult.role(), Role::Agent);
    assert_eq!(Class::CompactionBoundary.role(), Role::Harness);
    assert_eq!(Class::BookkeepingQueue.role(), Role::Harness);
    assert_eq!(Class::UserQueued.category(), "user");
    assert_eq!(Class::AgentMessagePlain.category(), "agent.message");
    assert_eq!(Class::BookkeepingCost.category(), "harness.bookkeeping");
}
