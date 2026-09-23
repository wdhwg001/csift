//! Axis 1: the top-level `type` of a jsonl line - 22 values.
//!
//! The four TRANSCRIPT types carry no leaf of their own: what a `user`, `assistant`,
//! `system` or `attachment` line classifies as is decided on a deeper axis (its blocks,
//! its subtype, its payload type, its template). The other 18 are the loader's
//! NON-TRANSCRIPT set - no `uuid`, no `message{}`, never in the model's context - and
//! each carries exactly one `harness.bookkeeping` leaf.

use super::*;

/// One row per top-level `type`. Transcript types first, then the bookkeeping set in the
/// order the map lists it.
pub(crate) const ROWS: &[Entry] = &[
    // -- the four transcript types: the leaf is decided deeper --
    e(
        Shape::TopLevelType("user"),
        None,
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::TopLevelType("assistant"),
        None,
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::TopLevelType("system"),
        None,
        Gate::Selector,
        false,
        Human::Verbose,
    ),
    e(
        Shape::TopLevelType("attachment"),
        None,
        Gate::Attachments,
        true,
        Human::Never,
    ),
    // -- the 18 non-transcript types --
    e(
        Shape::TopLevelType("last-prompt"),
        Some(Class::BookkeepingLastPrompt),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("mode"),
        Some(Class::BookkeepingMode),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("permission-mode"),
        Some(Class::BookkeepingMode),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("ai-title"),
        Some(Class::BookkeepingName),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("agent-name"),
        Some(Class::BookkeepingName),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("atis-latch"),
        Some(Class::BookkeepingAtis),
        Gate::Selector,
        false,
        // No reader of this LINE renders anything: every occurrence of the type is a
        // write-policy table, a writer, a loader dispatch into a per-session map, or a
        // fork re-writer, and the value it carries is read back only by the latch store
        // (claim QT-053).
        Human::Never,
    ),
    e(
        Shape::TopLevelType("bridge-session"),
        Some(Class::BookkeepingBridge),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("history-suppression"),
        Some(Class::BookkeepingSuppression),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("pr-link"),
        Some(Class::BookkeepingPrLink),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("frame-link"),
        Some(Class::BookkeepingArtifact),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("artifact-comment-monitor"),
        Some(Class::BookkeepingArtifact),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("artifact-autoreact-ledger"),
        Some(Class::BookkeepingArtifact),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("file-history-snapshot"),
        Some(Class::BookkeepingFileHistory),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("file-history-delta"),
        Some(Class::BookkeepingFileHistory),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("cost-state"),
        Some(Class::BookkeepingCost),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("fork-context-ref"),
        Some(Class::BookkeepingFork),
        Gate::Selector,
        false,
        // Its one reader is the agent transcript builder, which concatenates the parent
        // session's prefix in front of an agent's messages - message CONTENT, not a
        // display surface, so a person never sees the line or its effect (claim QT-052).
        Human::Never,
    ),
    e(
        Shape::TopLevelType("continued-in"),
        Some(Class::BookkeepingContinued),
        Gate::Selector,
        false,
        Human::Indirect,
    ),
    e(
        Shape::TopLevelType("queue-operation"),
        Some(Class::BookkeepingQueue),
        Gate::Selector,
        false,
        // Nothing in the executable reads it back at all, so nothing renders the LINE to
        // anybody. The person saw their own text in the queue UI, which is the
        // `user.queued` leaf's fact and not this one (claim QT-052).
        Human::Never,
    ),
];
