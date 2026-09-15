//! Axis 5: the isMeta-USER TEMPLATE families - 15 named templates, the human relay, and
//! the residue arm.
//!
//! These are the shapes with no field to key on: the harness writes text into the
//! conversation as if the operator had typed it, and only the text says which producer
//! wrote it. Every row is DETECTED in `model/classify/injections.rs` - the drift test
//! beside this table builds one synthetic line per row and requires the leaf back - with
//! two exceptions that are themselves rows: the frame, whose leaf is decided by the pulse
//! it wraps, and the RESIDUE arm, which is the honest name for a template csift models
//! none of and reaches only through an address.
//!
//! All ride `type:"user"` lines, so a flagless scan already keeps them.

use super::*;

/// One row per isMeta template family. The name is csift's, not a field value.
pub(crate) const ROWS: &[Entry] = &[
    e(
        Shape::IsMetaFamily("local-command-caveat"),
        Some(Class::CommandCaveat),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("stop-hook-feedback"),
        Some(Class::HookFeedback),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("resume-prompt"),
        Some(Class::ResumePrompt),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("schedule-wakeup"),
        Some(Class::ScheduleWakeup),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("schedule-tick"),
        Some(Class::ScheduleTick),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("schedule-fire"),
        Some(Class::ScheduleFire),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("roster-reminder"),
        Some(Class::ContextReminder),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("image-reference"),
        Some(Class::ContextImage),
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::IsMetaFamily("skill-body"),
        Some(Class::ContextSkill),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("enforce-structured-output"),
        Some(Class::EnforceStructuredOutput),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("enforce-brief"),
        Some(Class::EnforceBrief),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("enforce-handback"),
        Some(Class::EnforceHandback),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("enforce-retry"),
        Some(Class::EnforceRetry),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("coordinator-relay"),
        Some(Class::CommInbox),
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        // The leaf is decided DEEPER, on the pulse the frame wraps: the inner
        // `<task-notification>`'s own kind picks one of the five notification leaves, so
        // no single leaf belongs in this row (the four transcript top-level types are the
        // other rows shaped this way, and the drift test names both cases by hand).
        Shape::IsMetaFamily("task-notification-frame"),
        None,
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::IsMetaFamily("human-relay"),
        Some(Class::UserMessage),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("residue"),
        None,
        Gate::Address,
        true,
        Human::Never,
    ),
];
