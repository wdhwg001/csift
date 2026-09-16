//! The injections family: the isMeta-user templates csift already recognises, their
//! order, and the structural gate behind the fired scheduler prompt.

use super::*;
use crate::model::classify::injections;

/// One isMeta user record carrying `text` as its whole body.
fn meta(text: &str) -> String {
    let body = serde_json::to_string(text).expect("json string");
    format!(
        r#"{{"type":"user","isMeta":true,"uuid":"m1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":{body}}}}}"#
    )
}

#[test]
fn each_template_reaches_its_own_leaf() {
    let table: &[(&str, Class)] = &[
        ("Continue from where you left off.", Class::ResumePrompt),
        ("<<autonomous-loop-dynamic>>", Class::ScheduleWakeup),
        ("# Autonomous loop check", Class::ScheduleWakeup),
        ("You're being invoked on a timer", Class::ScheduleWakeup),
        ("Stop hook feedback: do it again", Class::HookFeedback),
        ("<local-command-caveat>ran /x", Class::CommandCaveat),
        (
            "The last Edit failed because the target file was modified",
            Class::EnforceRetry,
        ),
        ("# Autonomous loop tick", Class::ScheduleTick),
        ("Run the autonomous check", Class::ScheduleTick),
    ];
    for &(text, leaf) in table {
        let line = meta(text);
        assert_eq!(labels(&line), vec![leaf], "{text}");
        assert_eq!(
            injections::classify(&parse(&line), text),
            Some(leaf),
            "{text}"
        );
    }
    // Text matching no template yields nothing here; the caller's isMeta exclusion
    // then keeps it out of `user.message`.
    assert_eq!(
        injections::classify(&parse(&meta("just some prose")), "just some prose"),
        None
    );
    assert!(labels(&meta("just some prose")).is_empty());
}

/// EVERY text arm anchors at content start, after the same `trim_start` the section
/// discipline uses - so leading whitespace still reaches the leaf, and a template deeper
/// in a body belongs to the record QUOTING it. The two arms that used to test the
/// untrimmed string (feedback, caveat) and the one that used to test `contains`
/// (edit-retry) are pinned here beside a family they must no longer steal.
#[test]
fn every_template_arm_anchors_at_content_start() {
    let anchored: &[(&str, Class)] = &[
        ("Stop hook feedback: do it again", Class::HookFeedback),
        ("<local-command-caveat>ran /x", Class::CommandCaveat),
        (
            "The last Edit failed because the target file was modified",
            Class::EnforceRetry,
        ),
    ];
    for &(text, leaf) in anchored {
        // Leading whitespace is trimmed, exactly like the loop-marker arms.
        let padded = format!("\n  {text}");
        assert_eq!(labels(&meta(&padded)), vec![leaf], "padded: {text}");
        // The SAME template one sentence deep belongs to the record quoting it, which
        // for an isMeta record with no other template is no leaf at all.
        let quoted = format!("the harness wrote: {text}");
        assert!(labels(&meta(&quoted)).is_empty(), "quoted mid-body: {text}");
    }
    // The observed carrier of the edit-retry notice is a `Stop hook feedback:` body, and
    // the feedback arm sits ABOVE the retry arm, so such a record keeps the feedback leaf
    // whether or not the retry arm is anchored.
    assert_eq!(
        labels(&meta(
            "Stop hook feedback:\nThe last Edit failed because the target file was modified"
        )),
        vec![Class::HookFeedback]
    );
}

/// ONE formatter writes the hook-feedback template over a CLOSED four-name set, so all
/// four names reach the leaf. Only the `Stop` form has a corpus specimen; the other three
/// are derived from the producer's own name table, which is the whole reason they are
/// matched - a `TeammateIdle` feedback would otherwise be residue.
#[test]
fn every_blocking_hook_name_reaches_the_feedback_leaf() {
    for name in ["Stop", "TeammateIdle", "TaskCreated", "TaskCompleted"] {
        let body = format!("{name} hook feedback:\nfinish the sweep");
        assert_eq!(labels(&meta(&body)), vec![Class::HookFeedback], "{name}");
    }
    // A hook name the formatter does not carry is not the template.
    assert!(labels(&meta("SessionEnd hook feedback:\nbye")).is_empty());
    // And the template still needs the authorship flag: a person quoting it keeps
    // `user.message`, because the record they typed carries no `isMeta`.
    let human = r#"{"type":"user","uuid":"h9","message":{"role":"user","content":"Stop hook feedback:\nwhy did that fire?"}}"#;
    assert_eq!(labels(human), vec![Class::UserMessage]);
}

/// The ONE character csift's arm and the producer's recogniser disagree on, pinned so the
/// difference stays deliberate. The producer's separator is `" hook feedback:\n"`, so its
/// own test refuses a body whose newline is missing or rewritten; csift's prefixes stop at
/// the colon, so such a body still reaches the leaf. WIDER, never narrower: a record the
/// producer would refuse is findable, and no record it writes is missed.
#[test]
fn the_feedback_prefixes_stop_at_the_colon() {
    for p in injections::HOOK_FEEDBACK_PREFIXES {
        assert!(p.ends_with(':'), "{p} must stop at the colon");
        assert!(
            !p.contains('\n'),
            "{p} must not carry the separator newline"
        );
    }
    // A separator the producer's own `startsWith` would refuse still takes the leaf.
    assert_eq!(
        labels(&meta("Stop hook feedback: finish the sweep")),
        vec![Class::HookFeedback]
    );
}

#[test]
fn the_wakeup_tick_wins_over_the_driver_tick() {
    // `# Autonomous loop check` and `# Autonomous loop tick` share a prefix up to the
    // last word, so the FIRED timer has to be tested first or the driver arm swallows
    // it. Both are content-start anchored: a record QUOTING either keeps its own leaf.
    assert_eq!(
        labels(&meta("# Autonomous loop check\nrun the sweep")),
        vec![Class::ScheduleWakeup]
    );
    assert_eq!(
        labels(&meta("# Autonomous loop tick\nrun the sweep")),
        vec![Class::ScheduleTick]
    );
    let quoting = r#"{"type":"user","uuid":"q1","message":{"role":"user","content":"the skill says to open with # Autonomous loop tick"}}"#;
    assert_eq!(labels(quoting), vec![Class::UserMessage]);
}

#[test]
fn the_resume_prompt_needs_the_ismeta_flag() {
    // The isMeta half is what protects a person who happens to START a real message
    // with the loader's sentence.
    let human = r#"{"type":"user","uuid":"h1","message":{"role":"user","content":"Continue from where you left off. I mean the migration."}}"#;
    assert_eq!(labels(human), vec![Class::UserMessage]);
    assert_eq!(
        labels(&meta("Continue from where you left off.")),
        vec![Class::ResumePrompt]
    );
}

#[test]
fn the_fired_prompt_gate_is_the_parent_link() {
    // The scheduler appends the fire RECORD and then submits the prompt, so the
    // submission's parentUuid IS that record's uuid. The prompt's own text is whatever
    // the operator armed, so nothing in it can be matched.
    let fire = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"fire-1","timestamp":"2026-06-07T05:00:00.000Z","content":"Running scheduled task (2026-06-07 05:00)"}"#,
    );
    let ix = ScheduleFireIndex::from_records(std::iter::once(&fire));
    let mut ctx = ClassifyCtx::top_level();
    ctx.schedule_fires = Some(&ix);
    let prompt = parse(
        r#"{"type":"user","isMeta":true,"uuid":"p1","parentUuid":"fire-1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"sweep the queue"}}"#,
    );
    assert!(prompt.is_scheduled_fire_prompt(&ctx));
    assert_eq!(prompt.classify(&ctx), vec![Class::ScheduleFire]);
    // The same record with NO index in hand is not claimed: the link is the only mark
    // the producer leaves on the prompt, so without it csift says nothing.
    assert!(!prompt.is_scheduled_fire_prompt(&ClassifyCtx::top_level()));
    // A prompt parented to an ordinary record is not a fired one, index or not.
    let other = parse(
        r#"{"type":"user","isMeta":true,"uuid":"p2","parentUuid":"not-a-fire","message":{"role":"user","content":"sweep the queue"}}"#,
    );
    assert!(!other.is_scheduled_fire_prompt(&ctx));
    // A fire record with an UNREADABLE instant still gates its prompt: the instant is a
    // display fact and the label must not depend on it.
    let tailless = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"fire-2","content":"Running scheduled task"}"#,
    );
    let ix2 = ScheduleFireIndex::from_records(std::iter::once(&tailless));
    let mut ctx2 = ClassifyCtx::top_level();
    ctx2.schedule_fires = Some(&ix2);
    let p2 = parse(
        r#"{"type":"user","isMeta":true,"uuid":"p3","parentUuid":"fire-2","message":{"role":"user","content":"armed text"}}"#,
    );
    assert!(p2.is_scheduled_fire_prompt(&ctx2));
    assert_eq!(ix2.instant_for(Some("fire-2")), None);
}

#[test]
fn an_inbound_peer_message_is_never_the_fired_prompt() {
    let fire = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"fire-3","content":"Running scheduled task (now)"}"#,
    );
    let ix = ScheduleFireIndex::from_records(std::iter::once(&fire));
    let mut ctx = ClassifyCtx::top_level();
    ctx.schedule_fires = Some(&ix);
    // The `origin` object the harness stamps on everything it did not submit itself.
    let with_origin = parse(
        r#"{"type":"user","isMeta":true,"uuid":"x1","parentUuid":"fire-3","origin":{"kind":"peer","from":"uds:/x"},"message":{"role":"user","content":"hello"}}"#,
    );
    assert!(!with_origin.is_scheduled_fire_prompt(&ctx));
    // The relay FRAMING, through the same predicate `user.queued` uses.
    let framed = parse(
        r#"{"type":"user","isMeta":true,"uuid":"x2","parentUuid":"fire-3","message":{"role":"user","content":"<teammate-message teammate_id=\"lead\">go</teammate-message>"}}"#,
    );
    assert!(!framed.is_scheduled_fire_prompt(&ctx));
}
