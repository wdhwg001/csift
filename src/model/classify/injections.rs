//! The isMeta-USER injection templates: text the harness writes INTO the conversation
//! as if the operator had typed it.
//!
//! Every arm here is a fixed template the harness emits, matched at CONTENT START
//! unless the template's own producer writes it elsewhere, so a record that merely
//! QUOTES a template keeps its own leaf. This module is an EXTENSION POINT, not a
//! finished table: it carries the templates csift already recognises, and
//! [`classify`] returns `None` for every other isMeta record, which then falls through
//! to the isMeta exclusion exactly as it does today.

use super::*;

/// The leaf an isMeta-user INJECTION template carries, or `None` when the text matches
/// no template csift models.
///
/// Order is load-bearing and mirrors the producers: the resume loader's repair prompt
/// is tested through its own `isMeta` gate (a human message that merely OPENS with the
/// sentence keeps `user.message`); the FIRED timer tick is tested before the loop
/// DRIVER tick, because `check` is not `tick` and the driver prefix would otherwise
/// swallow it.
#[must_use]
pub(crate) fn classify(rec: &Record, s: &str) -> Option<Class> {
    // harness.resume.prompt: the LOADER's repair prompt for a resumed transcript that
    // ended on a dangling user record. The isMeta half is what protects a human who
    // happens to START a real message with this sentence; Claude Code's own recogniser
    // tests isMeta too, and this arm and `is_resume_prompt` must agree on the set for
    // the pairing index.
    if rec.is_meta.unwrap_or(false) && s.trim_start().starts_with(RESUME_PROMPT_MARKER) {
        return Some(Class::ResumePrompt);
    }
    // All the marker arms anchor at CONTENT START: the harness injects the whole
    // delivered prompt, so a real tick carries its marker at offset 0 and a marker
    // found mid-body belongs to a record QUOTING the prompt.
    let at_start = s.trim_start();
    // harness.schedule.wakeup: the FIRED autonomous-loop / ScheduleWakeup timer tick.
    if at_start.starts_with(SCHEDULE_WAKEUP_MARKER)
        || at_start.starts_with(SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX)
        || at_start.starts_with(SCHEDULE_WAKEUP_TIMER_MARKER)
    {
        return Some(Class::ScheduleWakeup);
    }
    // harness.hook.feedback: the Stop hook's blocking feedback, which the model reads.
    if s.starts_with(STOP_HOOK_FEEDBACK_PREFIX) {
        return Some(Class::HookFeedback);
    }
    // harness.command.caveat: the caveat the slash-command EXECUTOR writes after a
    // local command. Its producer is the command executor, never a hook, which is why
    // it sits in the command family and not beside the feedback arm above.
    if s.starts_with(LOCAL_COMMAND_CAVEAT_PREFIX) {
        return Some(Class::CommandCaveat);
    }
    // harness.enforce.retry: a redo nudge telling the model to re-issue a call. This
    // arm is the edit-failed-retry notice; the marker is matched ANYWHERE because the
    // harness appends it to the failed tool's own result text rather than writing it
    // as a standalone prompt.
    if s.contains(EDIT_RETRY_MARKER) {
        return Some(Class::EnforceRetry);
    }
    // harness.schedule.tick: the autonomous-loop DRIVER tick, distinct from the fired
    // timer above.
    if at_start.starts_with(AUTONOMOUS_LOOP_TICK_PREFIX)
        || at_start.starts_with(AUTONOMOUS_CHECK_MARKER)
    {
        return Some(Class::ScheduleTick);
    }
    None
}

impl Record {
    /// Is this the PROMPT a scheduled task fired (`harness.schedule.fire`)? The gate is
    /// STRUCTURAL: an `isMeta` `type:"user"` record whose `parentUuid` names a
    /// `system`/`scheduled_task_fire` record in the same transcript - the scheduler
    /// appends the fire record and then submits the prompt, so the submission's parent
    /// IS the record it just wrote.
    ///
    /// The prompt's own TEXT is whatever the operator armed, so no text test can find
    /// one; the parent link is the only mark the producer leaves on the prompt itself.
    /// Reading the link rather than the `promptSource` stamp is what reaches the
    /// prompts written before that stamp existed.
    ///
    /// Two guards still refuse an inbound message from elsewhere, which shares the
    /// isMeta authorship flag: the relay FRAMING, through the same [`is_peer_message`]
    /// predicate `user.queued` uses, and the `origin` OBJECT the harness stamps on
    /// everything it did not submit itself. A fired prompt carries neither.
    ///
    /// The marker-carrying tick prompts are refused by ARM ORDER in
    /// [`Record::classify_user_string`]: they reach their own leaves first.
    #[must_use]
    pub fn is_scheduled_fire_prompt(&self, ctx: &ClassifyCtx) -> bool {
        if !self.is_type("user") || !self.is_meta.unwrap_or(false) {
            return false;
        }
        if self.origin.is_some() {
            return false;
        }
        if !ctx
            .schedule_fires
            .is_some_and(|ix| ix.is_fire_parent(self.parent_uuid.as_deref()))
        {
            return false;
        }
        !self
            .raw_message_text()
            .is_some_and(|raw| is_peer_message(&raw))
    }
}
