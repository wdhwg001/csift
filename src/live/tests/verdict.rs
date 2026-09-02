//! The assess() join matrix: precedence, evidence rows, degradation notes.

use super::*;

#[test]
fn assess_dead_process_names_the_mid_tool_or_settled_shape() {
    let dead = PidLiveness::Dead;
    let mid = assess(
        Some(&row("busy", Some(7))),
        Some(&dead),
        &shape(Some(("Bash", "2026-06-07T05:00:05Z")), None, 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(mid.verdict, Verdict::StaleDead);
    assert!(note_text(&mid).contains("MID-TOOL"), "{:?}", mid.notes);
    let settled = assess(
        Some(&row("idle", Some(7))),
        Some(&PidLiveness::Reused),
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(settled.verdict, Verdict::StaleDead);
    assert!(
        note_text(&settled).contains("ended after its last turn"),
        "{:?}",
        settled.notes
    );
    assert!(
        settled
            .evidence
            .iter()
            .any(|e| e.surface == "pid" && e.value.contains("reused")),
        "{:?}",
        settled.evidence
    );
}

#[test]
fn assess_precedence_hitl_registry_children_eot() {
    // Pending elicitation outranks a running-shaped registry.
    let hitl = assess(
        Some(&row("busy", Some(7))),
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &["AskUserQuestion".to_string()],
        false,
    );
    assert_eq!(hitl.verdict, Verdict::WaitingHitl);
    assert!(
        hitl.evidence
            .iter()
            .any(|e| e.surface == "sidecar" && e.value.contains("AskUserQuestion")),
        "{:?}",
        hitl.evidence
    );
    // Registry `shell` = idle with a background shell still running: NEVER a running
    // shape. With nothing counted by the background scan it is idle-eot plus the
    // "does not count" note; with an open shell counted it is the seventh verdict.
    let shell = assess(
        Some(&row("shell", Some(7))),
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(shell.verdict, Verdict::IdleEot);
    assert!(
        note_text(&shell).contains("does not count"),
        "{:?}",
        shell.notes
    );
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT]), None);
    let open = report(&t, &BackgroundLens::from_args(None, &[]).unwrap());
    // Even a tail whose last stop_reason is not an end_turn reads idle under `shell`
    // (the harness computed idle before relabeling it).
    let shell_open = assess_full(
        Some(&row("shell", Some(7))),
        None,
        &shape(None, Some("tool_use"), 3),
        &ChildrenReport::default(),
        &[],
        false,
        &open,
    );
    assert_eq!(shell_open.verdict, Verdict::IdleBackgroundOpen);
    assert!(
        note_text(&shell_open).contains("the background section lists it"),
        "{:?}",
        shell_open.notes
    );
    // `busy` stays the registry's running signal.
    let busy = assess(
        Some(&row("busy", Some(7))),
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(busy.verdict, Verdict::Running);
    // The shell note is written for the `shell` status only - never for a live `busy`
    // row (or any other non-shell status).
    assert!(
        !note_text(&busy).contains("registry status shell"),
        "{:?}",
        busy.notes
    );
    assert!(
        !note_text(&hitl).contains("registry status shell"),
        "{:?}",
        hitl.notes
    );
    // Live children outrank the end_turn tail; the F7 note rides the verdict.
    let kids = ChildrenReport {
        children: vec![ChildState {
            session_id: "c1d2e3f4a5b60718".to_string(),
            state: "in-flight",
            detail: "unreturned Bash call".to_string(),
        }],
        journal_in_flight: 2,
        live_count: 3,
    };
    let wc = assess(
        None,
        None,
        &shape(None, Some("end_turn"), 3),
        &kids,
        &[],
        false,
    );
    assert_eq!(wc.verdict, Verdict::WaitingChildren);
    assert!(
        wc.evidence
            .iter()
            .any(|e| e.surface == "children" && e.value.contains("2 workflow agent(s) in flight")),
        "{:?}",
        wc.evidence
    );
    assert!(
        note_text(&wc).contains("permission prompt"),
        "{:?}",
        wc.notes
    );
    // Clean end of turn, nothing live: idle-eot, F7 note again.
    let idle = assess(
        None,
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(idle.verdict, Verdict::IdleEot);
    assert!(
        note_text(&idle).contains("permission prompt"),
        "{:?}",
        idle.notes
    );
}

#[test]
fn assess_unknown_arms_and_degradation_notes() {
    // Empty tail: unknown, named.
    let empty = assess(
        None,
        None,
        &shape(None, None, 0),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(empty.verdict, Verdict::Unknown);
    assert!(
        note_text(&empty).contains("no records readable"),
        "{:?}",
        empty.notes
    );
    // A tail with records but no rankable shape: unknown with the stop_reason named.
    let odd = assess(
        None,
        None,
        &shape(None, Some("max_tokens"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(odd.verdict, Verdict::Unknown);
    assert!(note_text(&odd).contains("max_tokens"), "{:?}", odd.notes);
    // Registry-absent notes differ by lane.
    assert!(
        note_text(&odd).contains("not currently registered"),
        "{:?}",
        odd.notes
    );
    let sub = assess(
        None,
        None,
        &shape(None, None, 3),
        &ChildrenReport::default(),
        &[],
        true,
    );
    assert!(
        note_text(&sub).contains("top-level interactive sessions only"),
        "{:?}",
        sub.notes
    );
    // Pid-probe degradations carry their own notes.
    let skipped = assess(
        Some(&row("idle", Some(7))),
        Some(&PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped,
        }),
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert!(
        note_text(&skipped).contains("reuse guard was skipped"),
        "{:?}",
        skipped.notes
    );
    let unavail = assess(
        Some(&row("idle", Some(7))),
        Some(&PidLiveness::Unavailable),
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert!(
        note_text(&unavail).contains("stale-dead is undecidable"),
        "{:?}",
        unavail.notes
    );
}

#[test]
fn registry_age_evidence_is_seconds_since_transition() {
    let now_ms = jiff::Timestamp::now().as_millisecond();
    let r = RegistryRow {
        pid: Some(7),
        status: Some("idle".to_string()),
        status_updated_at_ms: Some(now_ms - 60_000),
        proc_start: None,
        pid_domain: None,
    };
    let a = assess(
        Some(&r),
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    // An idle registry row is not a running shape: the settled tail rules.
    assert_eq!(a.verdict, Verdict::IdleEot);
    let age = a
        .evidence
        .iter()
        .find(|e| e.surface == "registry")
        .unwrap()
        .age_secs
        .unwrap();
    assert!((50..=120).contains(&age), "{age}");
    // An empty children report earns NO children evidence row.
    assert!(a.evidence.iter().all(|e| e.surface != "children"));
}

#[test]
fn registry_waiting_and_a_tail_dialog_are_hitl_without_a_sidecar() {
    // The registry's `waiting` status = blocked on a dialog: hitl even with a settled
    // tail and no sidecar, the note naming the dialog kinds and the freshness caveat.
    let waiting = assess(
        Some(&row("waiting", Some(7))),
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(waiting.verdict, Verdict::WaitingHitl);
    assert!(
        note_text(&waiting).contains("blocked on a dialog")
            && note_text(&waiting).contains("transition-written"),
        "{:?}",
        waiting.notes
    );
    // An unreturned AskUserQuestion at the tail (a multi-question ask lands at question
    // time): hitl, not running - and the note explains the single-question exception.
    let ask = assess(
        Some(&row("busy", Some(7))),
        None,
        &shape(Some(("AskUserQuestion", "2026-06-07T05:00:05Z")), None, 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(ask.verdict, Verdict::WaitingHitl);
    assert!(
        note_text(&ask).contains("unreturned AskUserQuestion/ExitPlanMode")
            && note_text(&ask).contains("single-question"),
        "{:?}",
        ask.notes
    );
    let plan = assess(
        None,
        None,
        &shape(Some(("ExitPlanMode", "2026-06-07T05:00:05Z")), None, 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(plan.verdict, Verdict::WaitingHitl);
    // A plain unreturned tool stays running; a dead process still outranks the dialog.
    let bash = assess(
        None,
        None,
        &shape(Some(("Bash", "2026-06-07T05:00:05Z")), None, 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(bash.verdict, Verdict::Running);
    let dead_ask = assess(
        Some(&row("waiting", Some(7))),
        Some(&PidLiveness::Dead),
        &shape(Some(("AskUserQuestion", "2026-06-07T05:00:05Z")), None, 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(dead_ask.verdict, Verdict::StaleDead);
}

#[test]
fn permission_prompt_note_names_the_registry_status_when_a_row_exists() {
    let with_row = assess(
        Some(&row("idle", Some(7))),
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(with_row.verdict, Verdict::IdleEot);
    assert!(
        note_text(&with_row).contains("registry row (status idle) would read `waiting`"),
        "{:?}",
        with_row.notes
    );
    let no_row = assess(
        None,
        None,
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert!(
        note_text(&no_row).contains("lives only in Claude Code process memory"),
        "{:?}",
        no_row.notes
    );
    // A foreign-domain row: the pid evidence names the domain, the note says undecidable.
    let foreign = assess(
        Some(&row("busy", Some(7))),
        Some(&PidLiveness::ForeignDomain("win32:elsewhere".to_string())),
        &shape(None, Some("end_turn"), 3),
        &ChildrenReport::default(),
        &[],
        false,
    );
    assert_eq!(foreign.verdict, Verdict::Running);
    assert!(
        foreign
            .evidence
            .iter()
            .any(|e| e.surface == "pid" && e.value.contains("win32:elsewhere")),
        "{:?}",
        foreign.evidence
    );
    assert!(
        note_text(&foreign).contains("another pid domain"),
        "{:?}",
        foreign.notes
    );
}
