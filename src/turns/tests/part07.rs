use super::*;

#[test]
fn min_render_chars_is_a_lower_bound_or_none_for_empty() {
    // A non-empty session reports a positive lower bound; an empty one reports None.
    let full = scan_with_turns(
        vec![mk_turn(0, Some("ask"), Some("reply"), 1, 0)],
        Vec::new(),
    );
    let min = min_render_chars(&full, 40000, &cfg());
    assert!(min.is_some_and(|m| m > 0));
    let mut empty = full;
    empty.turns.clear();
    assert_eq!(min_render_chars(&empty, 40000, &cfg()), None);

    // A user-ONLY turn (no agents) and an agent-ONLY turn (no user) each take only their
    // respective fold arm of the cheapest-side computation; both still yield a positive bound.
    let user_only = scan_with_turns(vec![mk_turn(0, Some("ask"), None, 0, 0)], Vec::new());
    assert!(min_render_chars(&user_only, 40000, &cfg()).is_some_and(|m| m > 0));
    let agent_only = scan_with_turns(vec![mk_turn(0, None, Some("reply"), 0, 0)], Vec::new());
    assert!(min_render_chars(&agent_only, 40000, &cfg()).is_some_and(|m| m > 0));
}

/// Build an AUTOMATION-opener turn of a given kind (a `<task-notification>` pulse) for the
/// per-class breakdown tests. `user` is the rendered opener text; the slice is flagged
/// `is_automation` and carries a parsed trigger of `kind`.
fn mk_automation_turn(
    turn_index: usize,
    kind: crate::model::AutomationKind,
    user: &str,
) -> TurnSlice {
    let mut t = mk_turn(turn_index, Some(user), Some("ack"), 0, 0);
    t.is_automation = true;
    t.automation = Some(crate::model::AutomationTrigger {
        kind,
        task_id: Some(format!("id{turn_index}")),
        status: Some("completed".to_string()),
        summary: Some(user.to_string()),
        event: None,
    });
    t
}

#[test]
fn automation_by_kind_breaks_down_per_class_not_lumped() {
    use crate::model::AutomationKind::*;
    // A session mixing 2 background-command + 1 agent + 1 monitor automation pulses plus a
    // human turn. The breakdown must report the composition, not a lumped scalar.
    let sr = scan_with_turns(
        vec![
            mk_turn(0, Some("human ask"), Some("human reply"), 1, 0),
            mk_automation_turn(1, BackgroundCommand, "bg one done"),
            mk_automation_turn(2, BackgroundCommand, "bg two done"),
            mk_automation_turn(3, Agent, "agent done"),
            mk_automation_turn(4, Monitor, "monitor fired"),
        ],
        Vec::new(),
    );
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    let by = automation_by_kind(std::slice::from_ref(&plan));
    // Order is [BackgroundCommand, Agent, Workflow, Monitor, Task].
    assert_eq!(by, [2, 1, 0, 1, 0]);
    let text = automation_breakdown_text(&by);
    assert_eq!(text, "2 background-command, 1 agent, 1 monitor");
    // The lumped total still agrees with the per-class sum.
    assert_eq!(count_automation(&plan), by.iter().sum::<usize>());
}

#[test]
fn automation_by_kind_covers_workflow_task_and_unparsed_fallback() {
    use crate::model::AutomationKind::*;
    // Exercise the remaining classes (Workflow, Task) AND the `automation == None` fallback —
    // an `is_automation` turn whose trigger failed to parse is attributed to `task`.
    let mut unparsed = mk_turn(3, Some("mystery pulse"), Some("ack"), 0, 0);
    unparsed.is_automation = true; // flagged, but .automation stays None
    let sr = scan_with_turns(
        vec![
            mk_automation_turn(0, Workflow, "wf done"),
            mk_automation_turn(1, Task, "task done"),
            mk_automation_turn(2, Workflow, "wf two done"),
            unparsed,
        ],
        Vec::new(),
    );
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    let by = automation_by_kind(std::slice::from_ref(&plan));
    // [BackgroundCommand, Agent, Workflow, Monitor, Task] — 2 workflow, 1 task (parsed) + 1
    // task (the None-fallback) = 2 task.
    assert_eq!(by, [0, 0, 2, 0, 2]);
    assert_eq!(automation_breakdown_text(&by), "2 workflow, 2 task");
}

#[test]
fn automation_breakdown_text_empty_when_no_triggers() {
    assert_eq!(automation_breakdown_text(&[0, 0, 0, 0, 0]), "");
}

#[test]
fn automation_in_scope_counts_every_notification_regardless_of_selection() {
    use crate::model::AutomationKind::*;
    // A monitor-heavy session: many monitor pulses + a couple workflow ones. Plan it under a
    // budget too small to select them all; `automation_in_scope_by_kind` must still report the
    // WHOLE-session composition (the fix for a header reading `monitor:0` on a monitor-dominated
    // session), whereas the SELECTED `automation_by_kind` may report fewer.
    let mut turns = vec![mk_turn(0, Some("human ask"), Some("human reply"), 1, 0)];
    for i in 1..=6 {
        turns.push(mk_automation_turn(i, Monitor, "monitor tick fired"));
    }
    turns.push(mk_automation_turn(7, Workflow, "wf done"));
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    // In-scope counts ALL pulses: [bg, agent, workflow, monitor, task] = 6 monitor + 1 workflow.
    let in_scope = automation_in_scope_by_kind(std::slice::from_ref(&plan));
    assert_eq!(in_scope, [0, 0, 1, 6, 0]);
    assert_eq!(in_scope.iter().sum::<usize>(), 7);
    // The selected breakdown is a SUBSET of the in-scope one (never larger in any class).
    let selected = automation_by_kind(std::slice::from_ref(&plan));
    for (sel, scope) in selected.iter().zip(in_scope.iter()) {
        assert!(sel <= scope, "selected per-class must not exceed in-scope");
    }
}

#[test]
fn automation_in_scope_empty_when_no_automation() {
    // A purely-human session has no in-scope automation in any class.
    let sr = scan_with_turns(
        vec![mk_turn(0, Some("ask"), Some("reply"), 0, 0)],
        Vec::new(),
    );
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert_eq!(
        automation_in_scope_by_kind(std::slice::from_ref(&plan)),
        [0, 0, 0, 0, 0]
    );
}

#[test]
fn automation_by_kind_skips_non_user_and_missing_turns() {
    // Exercise the two guard arms in `automation_by_kind`: an AssistantOnly selection (does not
    // SHOW the user side → skipped) and a selection pointing at a turn_index that is not present
    // in `plan.turns` (find_turn None → skipped). Neither contributes to the breakdown.
    let mut auto = mk_turn(0, Some("pulse"), Some("ack"), 0, 0);
    auto.is_automation = true;
    auto.automation = Some(crate::model::AutomationTrigger {
        kind: crate::model::AutomationKind::Agent,
        task_id: Some("id0".to_string()),
        status: Some("completed".to_string()),
        summary: Some("pulse".to_string()),
        event: None,
    });
    let plan = SessionPlan {
        selected: vec![
            // AssistantOnly over the automation turn → the !shows_user guard skips it.
            Selected {
                turn_index: 0,
                sides: SelSides::AssistantOnly,
            },
            // A Both selection at a turn_index NOT in `turns` → the find_turn None guard skips it.
            Selected {
                turn_index: 99,
                sides: SelSides::Both,
            },
        ],
        turns: vec![auto],
        spanned_boundaries: 0,
        rendered_chars: 0,
        newest_summary_line: None,
        dedup_demoted: 0,
    };
    let by = automation_by_kind(std::slice::from_ref(&plan));
    assert_eq!(by, [0, 0, 0, 0, 0], "both selections must be skipped");
}

#[test]
fn min_render_chars_none_when_turn_has_no_sides() {
    // A turn with NEITHER a user side NOR any agent message contributes `usize::MAX` to the
    // min fold; with that being the only turn, `min_render_chars` returns None (the
    // `cheapest == usize::MAX` guard), distinct from a positive lower bound.
    let sideless = TurnSlice {
        turn_index: 0,
        user: None,
        tool_calls: 0,
        image_ids: Vec::new(),
        agents: Vec::new(),
        compactions_before: 0,
        is_automation: false,
        automation: None,
    };
    let sr = scan_with_turns(vec![sideless], Vec::new());
    assert_eq!(min_render_chars(&sr, 40000, &cfg()), None);
}

#[test]
fn turn_carries_parsed_automation_trigger_for_json() {
    // The scan path stores the parsed trigger on the slice when the opener is a
    // <task-notification>; the JSON emitter reads `trigger_kind`/`task_id`/`status` off it.
    let line = r#"{"type":"user","message":{"role":"user","content":"<task-notification><task-id>wf_42</task-id><status>completed</status><summary>Dynamic workflow \"x\" completed</summary></task-notification>"}}"#;
    let rec: crate::model::Record = serde_json::from_str(line).expect("record");
    let trig = rec.automation_trigger().expect("a trigger");
    assert_eq!(trig.kind.slug(), "workflow");
    assert_eq!(trig.task_id.as_deref(), Some("wf_42"));
    assert_eq!(trig.status.as_deref(), Some("completed"));
}

// ── --slice chunked output (slice_into_windows) ──

#[test]
fn slice_windows_concatenate_back_to_the_source() {
    let doc = "line one\nline two\nthree\nfour five six\n";
    let chunks = slice_into_windows(doc, 12);
    assert_eq!(chunks.concat(), doc, "lossless reassembly across slices");
    for c in &chunks {
        assert!(c.chars().count() <= 12, "chunk over window: {c:?}");
    }
    assert!(chunks.len() > 1, "doc spans multiple windows");
}

#[test]
fn slice_windows_count_chars_not_bytes() {
    // Each `🛠` is 4 BYTES but 1 CHARACTER. A 6-char window fits 5 wrenches + newline
    // (21 bytes), proving the window counts Unicode scalars — the unit Claude Code's
    // additionalContext cap uses — not bytes (a byte budget would split after the first).
    let line = "🛠🛠🛠🛠🛠\n"; // 6 chars, 21 bytes
    let chunks = slice_into_windows(line, 6);
    assert_eq!(
        chunks.len(),
        1,
        "6 chars fit one 6-char window despite 21 bytes"
    );
    assert_eq!(chunks[0], line);
}

#[test]
fn slice_windows_hard_split_an_oversized_line_on_char_boundaries() {
    // A single line longer than the window is hard-split so NO chunk exceeds it — and never
    // mid-`🛠` (char boundary). Window 2, line of 5 wrenches (no trailing newline).
    let line = "🛠🛠🛠🛠🛠";
    let chunks = slice_into_windows(line, 2);
    assert_eq!(chunks.concat(), line, "lossless even when hard-splitting");
    for c in &chunks {
        assert!(c.chars().count() <= 2);
        assert!(c.chars().all(|ch| ch == '🛠'), "no broken char: {c:?}");
    }
    assert_eq!(chunks, vec!["🛠🛠", "🛠🛠", "🛠"]);
}

#[test]
fn slice_windows_empty_input_yields_no_chunks() {
    assert!(slice_into_windows("", 10).is_empty());
}
