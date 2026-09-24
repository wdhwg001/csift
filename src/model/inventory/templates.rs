//! Axis 5: the isMeta-USER TEMPLATE families - 19 named templates, the human relay, and
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
        // The HUMAN column of every isMeta-user row is decided by ONE gate, and it is
        // not per template: the transcript render refuses an `isMeta` `type:"user"`
        // record unless its `origin.kind` is one of five (`channel`, `observer`,
        // `observer-activity`, `slack-ping`, `peer`), and no template here writes any of
        // them (claim CLS-036).
        Shape::IsMetaFamily("image-reference"),
        Some(Class::ContextImage),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::IsMetaFamily("skill-body"),
        Some(Class::ContextSkill),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        // The same skill body from a registration that ships no FILES: the registrar's
        // directory wrapper never runs, so the record carries the body with no prefix.
        // A separate shape because the head is what a reader greps for.
        Shape::IsMetaFamily("bundled-skill-body"),
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
        // The FOUR redo nudges plus the edit-failed notice. The three rows below carry
        // the same leaf and are separate shapes because the producer writes them on
        // different EVENTS, from different branches, with their own texts - which is what
        // a shape row records. What they share is the leaf's whole domain: the harness
        // continuing a turn the model did not finish, on its own instruction.
        Shape::IsMetaFamily("enforce-retry"),
        Some(Class::EnforceRetry),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        // The safety classifier stopped the response mid-turn. TWO texts, the second the
        // first plus a sentence about a call that was already running.
        Shape::IsMetaFamily("classifier-stop"),
        Some(Class::EnforceRetry),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        // The response hit the output-token cap and is told to resume mid-thought.
        Shape::IsMetaFamily("output-limit"),
        Some(Class::EnforceRetry),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        // The stream died with the response incomplete. TWO texts, chosen by LANE: a
        // main lane resumes from where the text stops, a subagent lane writes the whole
        // answer again because only its next message is delivered.
        Shape::IsMetaFamily("stream-cutoff"),
        Some(Class::EnforceRetry),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        // `origin.kind` reads `coordinator` here, which the render gate does not accept -
        // unlike the PEER relay's `peer`, which it does. The two relays are written by
        // the same formatter family and reach the model alike; only the person's view
        // differs.
        Shape::IsMetaFamily("coordinator-relay"),
        Some(Class::CommInbox),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        // The leaf is decided DEEPER, on the pulse the frame wraps: the inner
        // `<task-notification>`'s own kind picks one of the five notification leaves, so
        // no single leaf belongs in this row (the four transcript top-level types are the
        // other rows shaped this way, and the drift test names both cases by hand).
        // Its `origin.kind` reads `task-notification`, which the render gate refuses, so
        // the FRAMED pulse is hidden while the same pulse on a non-isMeta record is
        // shown: the frame is what makes it invisible, not the notification.
        Shape::IsMetaFamily("task-notification-frame"),
        None,
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
