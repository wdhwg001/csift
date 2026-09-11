//! classify(): the SCHEDULE family - a fired autonomous-loop / `ScheduleWakeup` tick, the
//! loop DRIVER ticks, and the prompt a scheduled task fires. All three share one producer
//! path and one ordering rule, so they are pinned together: the marker arms anchor at
//! content start and get first refusal, and the markerless fired prompt comes last.

use super::*;

#[test]
fn classify_meta_loop_variants() {
    // NB: `r##"…"##` delimiter - the JSON content has `:"# ` whose `"#` would close a
    // plain `r#"…"#` raw string early.
    let tick = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop tick\nproceed with the next step."}}"##,
    );
    assert_eq!(
        tick.classify(&ClassifyCtx::top_level()),
        vec![Class::MetaLoop]
    );
    let check = parse(
        r#"{"type":"user","message":{"role":"user","content":"Run the autonomous check and continue."}}"#,
    );
    assert_eq!(
        check.classify(&ClassifyCtx::top_level()),
        vec![Class::MetaLoop]
    );
    // The schedule.wakeup sentinel stays its OWN class (not folded into meta.loop).
    let wake = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
    );
    assert_eq!(
        wake.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
}

// ── P1c M2a: fired autonomous-loop / ScheduleWakeup timer tick → harness.schedule.wakeup ──

#[test]
fn classify_schedule_wakeup_fired_timer_markers() {
    // The real oracle-D12 record: isMeta, header "# Autonomous loop check", body "You're
    // being invoked on a timer …". Used to fall through to user.message (the M2 mislabel).
    let loop_check = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop check\n\nYou're being invoked on a timer while the user is away."}}"##,
    );
    assert_eq!(
        loop_check.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
    // The body sentence alone (no header) also routes to schedule.wakeup.
    let timer_only = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"You're being invoked on a timer to keep work moving."}}"#,
    );
    assert_eq!(
        timer_only.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
}

// ── v0.12.2: the loop markers anchor at CONTENT START, never mid-body ──

#[test]
fn loop_markers_anchor_at_content_start() {
    // A skill's instruction record EMBEDS the fired preamble to explain it. Both wakeup
    // markers sit deep in the body, so neither may claim the record: it is isMeta with no
    // marker at start, which the taxonomy models as nothing at all.
    let instructions = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# /loop - schedule the autonomous default\n\nThe user invoked /loop with no prompt. Each fire delivers:\n\n# Autonomous loop check\n\nYou're being invoked on a timer while the user is away or occupied."}}"##,
    );
    assert!(
        instructions.classify(&ClassifyCtx::top_level()).is_empty(),
        "a record QUOTING the preamble is not the fired tick"
    );
    // The same law for the meta.loop driver markers.
    let quotes_tick = parse(
        r##"{"type":"user","message":{"role":"user","content":"Here is what the driver says:\n\n# Autonomous loop tick\n\nRun the autonomous check using the loop instructions."}}"##,
    );
    assert_eq!(
        quotes_tick.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage],
        "a human quoting the driver keeps user.message"
    );
    // The dynamic sentinel quoted mid-prose is prose (the one corpus record under
    // harness.schedule.wakeup before this rule was a genuine operator prompt doing exactly
    // this, and it lost its own leaf).
    let quotes_sentinel = parse(
        r#"{"type":"user","message":{"role":"user","content":"csift models the <<autonomous-loop-dynamic>> sentinel; check the arm order."}}"#,
    );
    assert_eq!(
        quotes_sentinel.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

// ── v0.12.2: harness.schedule.fire, the prompt a scheduled task fires ──

#[test]
fn classify_scheduled_fire_prompt() {
    // The fired prompt: isMeta + promptSource "system", body = the armed text verbatim.
    let fired = parse(
        r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000011","parentUuid":"00000000-0000-4000-8000-000000000010","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"reply with the single word PEONY and nothing else"}}"#,
    );
    assert_eq!(
        fired.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleFire]
    );
    // isMeta ALONE is not the leaf: an unmarked isMeta pseudo-turn with no promptSource is
    // still excluded, exactly as before.
    let bare_meta = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"reply with the single word PEONY and nothing else"}}"#,
    );
    assert!(bare_meta.classify(&ClassifyCtx::top_level()).is_empty());
    // promptSource ALONE is not the leaf either: without the isMeta authorship flag the
    // record is the operator's, and it keeps user.message.
    let typed = parse(
        r#"{"type":"user","promptSource":"typed","message":{"role":"user","content":"reply with the single word PEONY and nothing else"}}"#,
    );
    assert_eq!(
        typed.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn scheduled_fire_refuses_a_peer_message() {
    // A cross-session peer message carries the SAME isMeta + promptSource stamp. It is a
    // delivered message, not a fired prompt, so the shared `is_peer_message` predicate
    // refuses it here and it keeps its inbox leaf.
    let peer = parse(
        r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000030","isMeta":true,"promptSource":"system","userType":"external","origin":{"kind":"peer","from":"uds:/Users/dev/relay.sock","verifiedPeerPid":4242},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"relay-7\">\nthe shared resolver landed\n</cross-session-message>\n\nThis came from another Claude session."}}"#,
    );
    assert_eq!(
        peer.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox]
    );
    assert!(!peer.is_scheduled_fire_prompt());
    // A TAGLESS delivery carries the relay preamble but no tag to detect, so the framing
    // guard cannot see it. The `origin` object still can, and it is what the harness itself
    // reads as the first test of its own foreign-input veto.
    let tagless = parse(
        r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000031","isMeta":true,"promptSource":"system","userType":"external","origin":{"kind":"peer","from":"probe-external","verifiedPeerPid":4242},"message":{"role":"user","content":"Another Claude session sent a message:\nreply with exactly PONG\n\nThis came from another Claude session."}}"#,
    );
    assert!(!tagless.is_scheduled_fire_prompt());
    assert!(tagless.classify(&ClassifyCtx::top_level()).is_empty());
}

#[test]
fn a_fired_tick_keeps_its_own_leaf_before_the_fire_arm() {
    // The autonomous-loop fires carry the SAME fire stamp, and their markers reach their own
    // leaves first: arm order, not a second predicate.
    let check = parse(
        r##"{"type":"user","isMeta":true,"promptSource":"system","message":{"role":"user","content":"# Autonomous loop check\n\nYou're being invoked on a timer while the user is away."}}"##,
    );
    assert_eq!(
        check.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
    let tick = parse(
        r##"{"type":"user","isMeta":true,"promptSource":"system","message":{"role":"user","content":"# Autonomous loop tick\n\nRun the autonomous check using the loop instructions."}}"##,
    );
    assert_eq!(
        tick.classify(&ClassifyCtx::top_level()),
        vec![Class::MetaLoop]
    );
}

#[test]
fn scheduled_fire_instant_reads_the_parenthesised_tail() {
    // The cron wording.
    let cron = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000010","content":"Running scheduled task (Jun 7 5:28am)"}"#,
    );
    assert!(cron.is_scheduled_task_fire());
    assert_eq!(cron.scheduled_fire_instant(), Some("Jun 7 5:28am"));
    // The wakeup wording, same shape, same read - which is why the tail is parsed rather
    // than either sentence.
    let wakeup = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000020","content":"Claude resuming /loop wakeup (Jun 7 6:40am)"}"#,
    );
    assert_eq!(wakeup.scheduled_fire_instant(), Some("Jun 7 6:40am"));
    // No parenthesised tail, an empty one, and a non-string content all read as absent -
    // never as a guessed instant.
    for line in [
        r#"{"type":"system","subtype":"scheduled_task_fire","content":"Running scheduled task"}"#,
        r#"{"type":"system","subtype":"scheduled_task_fire","content":"Running scheduled task ()"}"#,
        r#"{"type":"system","subtype":"scheduled_task_fire","content":{"when":"now"}}"#,
    ] {
        assert_eq!(parse(line).scheduled_fire_instant(), None, "line: {line}");
    }
    // Another system subtype is not a fire record whatever its content says.
    let other = parse(
        r#"{"type":"system","subtype":"informational","content":"Running scheduled task (Jun 7 5:28am)"}"#,
    );
    assert!(!other.is_scheduled_task_fire());
}

#[test]
fn schedule_fire_index_joins_by_parent_uuid() {
    let fire = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000010","content":"Running scheduled task (Jun 7 5:28am)"}"#,
    );
    let no_instant = parse(
        r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000040","content":"Running scheduled task"}"#,
    );
    let ix = crate::model::ScheduleFireIndex::from_records([&fire, &no_instant].into_iter());
    assert_eq!(
        ix.instant_for(Some("00000000-0000-4000-8000-000000000010")),
        Some("Jun 7 5:28am")
    );
    // A fire record whose instant is unreadable contributes no entry, so the prompt under
    // it reports no instant rather than an empty one.
    assert_eq!(
        ix.instant_for(Some("00000000-0000-4000-8000-000000000040")),
        None
    );
    assert_eq!(ix.instant_for(Some("nothing-here")), None);
    assert_eq!(ix.instant_for(None), None);
}

#[test]
fn loop_markers_tolerate_leading_whitespace() {
    // Content start is read after the shared `trim_start` (the FINDING-1 discipline), so a
    // leading newline in the delivered prompt still opens a tick.
    let wake = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"\n  # Autonomous loop check\n\nYou're being invoked on a timer."}}"##,
    );
    assert_eq!(
        wake.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
    let tick = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"\n# Autonomous loop tick\n\nproceed."}}"##,
    );
    assert_eq!(
        tick.classify(&ClassifyCtx::top_level()),
        vec![Class::MetaLoop]
    );
}

#[test]
fn classify_wakeup_check_vs_loop_tick_no_collision() {
    // "# Autonomous loop check" → schedule.wakeup; "# Autonomous loop tick" → meta.loop. The
    // two share the "# Autonomous loop " prefix but diverge at check/tick - must NOT collide.
    let check = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop check\nproceed."}}"##,
    );
    assert_eq!(
        check.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
    let tick = parse(
        r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop tick\nproceed."}}"##,
    );
    assert_eq!(
        tick.classify(&ClassifyCtx::top_level()),
        vec![Class::MetaLoop]
    );
    // The sentinel stays schedule.wakeup; "Run the autonomous check" stays meta.loop.
    let sentinel = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
    );
    assert_eq!(
        sentinel.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
    let run_check = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Run the autonomous check now."}}"#,
    );
    assert_eq!(
        run_check.classify(&ClassifyCtx::top_level()),
        vec![Class::MetaLoop]
    );
}
