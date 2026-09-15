//! Axis 2: the `subtype` of a `type:"system"` line - the 11 observed values.
//!
//! A system record carries no `message{}`, so the model receives none of them except
//! `local_command`, which the request assembler re-mints as a user message. The
//! compaction boundary is the one subtype a FLAGLESS scan reaches (its own `&&`-gated
//! keep); every other subtype needs a selector naming its leaf.

use super::*;

/// One row per observed `subtype`.
pub(crate) const ROWS: &[Entry] = &[
    e(
        Shape::SystemSubtype("compact_boundary"),
        Some(Class::CompactionBoundary),
        Gate::Default,
        false,
        Human::Always,
    ),
    e(
        Shape::SystemSubtype("turn_duration"),
        Some(Class::TurnDuration),
        Gate::Selector,
        false,
        Human::Unstated,
    ),
    e(
        Shape::SystemSubtype("stop_hook_summary"),
        Some(Class::TurnStopHooks),
        Gate::Selector,
        false,
        Human::Unstated,
    ),
    e(
        Shape::SystemSubtype("away_summary"),
        Some(Class::TurnRecap),
        Gate::Selector,
        false,
        Human::Unstated,
    ),
    e(
        Shape::SystemSubtype("scheduled_task_fire"),
        Some(Class::ScheduleEvent),
        Gate::Selector,
        false,
        Human::Always,
    ),
    e(
        Shape::SystemSubtype("model_refusal_fallback"),
        Some(Class::SystemRefusal),
        Gate::Selector,
        false,
        Human::Verbose,
    ),
    e(
        Shape::SystemSubtype("model_refusal_no_fallback"),
        Some(Class::SystemRefusal),
        Gate::Selector,
        false,
        Human::Never,
    ),
    e(
        Shape::SystemSubtype("api_error"),
        Some(Class::SystemApiError),
        Gate::Selector,
        false,
        Human::Never,
    ),
    e(
        Shape::SystemSubtype("informational"),
        Some(Class::SystemNotice),
        Gate::Selector,
        false,
        Human::Verbose,
    ),
    e(
        Shape::SystemSubtype("agents_killed"),
        Some(Class::SystemKill),
        Gate::Selector,
        false,
        Human::Always,
    ),
    e(
        Shape::SystemSubtype("local_command"),
        Some(Class::SystemCommand),
        Gate::Selector,
        true,
        Human::Always,
    ),
];
