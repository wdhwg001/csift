//! Axis 5: the isMeta-USER TEMPLATE families - 13 named templates, the human relay, and
//! the residue arm.
//!
//! These are the shapes with no field to key on: the harness writes text into the
//! conversation as if the operator had typed it, and only the text says which producer
//! wrote it. The families csift already recognises are in `model/classify/injections.rs`;
//! the rest are rows here with no leaf yet, which is exactly what makes this table an
//! extension point rather than a claim of completeness.
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
