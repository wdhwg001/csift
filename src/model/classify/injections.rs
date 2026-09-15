//! The isMeta-USER injection templates: text the harness writes INTO the conversation
//! as if the operator had typed it.
//!
//! These are the shapes with no FIELD to key on - the record looks like the operator's
//! own message and only the text says which producer wrote it - so every arm here is a
//! fixed template matched at CONTENT START, and a record that merely QUOTES a template
//! keeps its own leaf. Two arms are structural instead, and say so at the arm.
//!
//! The table covers every `Shape::IsMetaFamily` row of the shape inventory, which is
//! what the drift test beside that table asserts: a row without an arm is an advertised
//! leaf no record can carry, and an arm without a row is a mapping nothing records.
//! [`classify`] still returns `None` for a template csift models none of - the RESIDUE
//! row - and such a record falls through to the isMeta exclusion, reachable only by an
//! address, which renders it as an unmodeled unit.

use super::*;

/// The leaf an isMeta-user INJECTION template carries, or `None` when the text matches
/// no template csift models.
///
/// Order is load-bearing and mirrors the producers. The HUMAN relay comes first: it is
/// a real person's message that the harness merely delivered, so no template arm may
/// reach it. Then the `isMeta` guard, because every arm after it matches on TEXT and a
/// person can write any of those sentences. The resume loader's repair prompt is tested through its own `isMeta` gate
/// (a human message that merely OPENS with the sentence keeps `user.message`). The FIRED
/// timer tick is tested before the loop DRIVER tick, because `check` is not `tick` and
/// the driver prefix would otherwise swallow it. The `<system-reminder>` arm is LAST of
/// the reminder family, so a more specific reminder template lands on its own leaf.
#[must_use]
pub(crate) fn classify(rec: &Record, s: &str) -> Option<Class> {
    // user.message: a PERSON's message, delivered mid-turn into a running lane and
    // flagged isMeta because the harness submitted it rather than the operator's own
    // client. The discriminator is a FIELD, not the text - `origin.kind == "human"` -
    // which is why this arm leads: the relayed body is the person's own prose and any
    // template arm below could match whatever they happened to write.
    if rec.origin_kind().is_some_and(|k| k == "human") {
        return Some(Class::UserMessage);
    }
    // EVERY ARM BELOW MATCHES ON TEXT, AND `isMeta` IS WHAT PROTECTS A PERSON. A human can
    // write any sentence a template opens with - quote the reminder they were just shown,
    // paste the retry nudge they are asking about, open a message with `[Image: …]` - and
    // without this guard that message is relabelled out of `user` while it still opens its
    // turn. `isMeta` is the authorship flag the injecting call site stamps; Claude Code's
    // own recogniser tests it for the same reason, and the map's shape rows all read
    // `type:"user" isMeta:true template:<x>`. The two STRUCTURAL arms are above this line
    // because they read a field rather than the prose.
    if !rec.is_meta.unwrap_or(false) {
        return None;
    }
    // harness.resume.prompt: the LOADER's repair prompt for a resumed transcript that
    // ended on a dangling user record. This arm and `is_resume_prompt` must agree on the
    // set for the pairing index.
    if s.trim_start().starts_with(RESUME_PROMPT_MARKER) {
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
    // harness.enforce.retry: the harness's other redo nudges. EXACT equality over the
    // four texts, because Claude Code's own recogniser tests them that way - these are
    // whole messages, not prefixes of a longer body.
    if RETRY_NUDGE_TEXTS.contains(&at_start) {
        return Some(Class::EnforceRetry);
    }
    // The three other ENFORCEMENT templates, each named by the sentinel its writer
    // interpolates ahead of the prose it varies.
    if at_start.starts_with(ENFORCE_STRUCTURED_OUTPUT_PREFIX) {
        return Some(Class::EnforceStructuredOutput);
    }
    if at_start.starts_with(ENFORCE_BRIEF_PREFIX) {
        return Some(Class::EnforceBrief);
    }
    if at_start.starts_with(ENFORCE_HANDBACK_PREFIX) {
        return Some(Class::EnforceHandback);
    }
    // harness.schedule.tick: the autonomous-loop DRIVER tick, distinct from the fired
    // timer above.
    if at_start.starts_with(AUTONOMOUS_LOOP_TICK_PREFIX)
        || at_start.starts_with(AUTONOMOUS_CHECK_MARKER)
    {
        return Some(Class::ScheduleTick);
    }
    // harness.context.image: the harness's stand-in for a pasted or attached image.
    if IMAGE_REFERENCE_PREFIXES
        .iter()
        .any(|p| at_start.starts_with(p))
    {
        return Some(Class::ContextImage);
    }
    // harness.context.skill: a skill's whole instruction body, preloaded by the spawn
    // builder into a lane's opening array.
    if at_start.starts_with(SKILL_PRELOAD_PREFIX) {
        return Some(Class::ContextSkill);
    }
    // agent.communication.inbox: the COORDINATOR relay, the peer relay's twin. Both
    // formatters sit in the same span of the build and deliver through the same queue,
    // so a message from the coordinator lane is an inbound message like any other.
    if at_start.starts_with(COORDINATOR_RELAY_PREFIX) {
        return Some(Class::CommInbox);
    }
    // harness.context.reminder: LAST of the reminder family. Every more specific
    // `<system-reminder>` template is above; this arm is what keeps a reminder shape
    // csift has not named individually inside the family it belongs to instead of
    // dropping it into the residue.
    if at_start.starts_with(SYSTEM_REMINDER_PREFIX) {
        return Some(Class::ContextReminder);
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
