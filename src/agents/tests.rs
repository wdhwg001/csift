use super::*;
use crate::subagent::{ReturnedMsgSource, SubagentStatus};

fn node(
    trigger: Option<&str>,
    start: Option<&str>,
    complete: Option<&str>,
    kind: SubagentKind,
) -> SubagentNode {
    SubagentNode {
        agent_id: "abc123".to_string(),
        kind,
        parent_session_id: "sess".to_string(),
        parent_agent_id: None,
        spawn_tool_use_id: Some("toolu_x".to_string()),
        spawn_tool: Some("Agent".to_string()),
        workflow_id: None,
        agent_type: None,
        name: None,
        team_name: None,
        description: None,
        trigger_utc: trigger.map(str::to_string),
        started_utc: start.map(str::to_string),
        completed_utc: complete.map(str::to_string),
        last_activity_utc: complete.map(str::to_string),
        returned_message: None,
        returned_message_source: None,
        status: SubagentStatus::Completed,
        pending_tool_use_id: None,
        pending_tool_name: None,
        pending_classification: None,
        pending_since_utc: None,
        files_changed: Vec::new(),
        fork_parent_last_uuid: None,
        fork_context_length: None,
        depth: 0,
        children: Vec::new(),
        skipped_lines: 0,
    }
}

#[test]
fn kind_filter_empty_allows_all() {
    assert!(kind_allowed(SubagentKind::BuiltinTask, &[]));
    assert!(kind_allowed(SubagentKind::Workflow, &[]));
}

#[test]
fn kind_filter_restricts() {
    let want = vec![AgentKindFilter::Workflow];
    assert!(!kind_allowed(SubagentKind::BuiltinTask, &want));
    assert!(kind_allowed(SubagentKind::Workflow, &want));
}

#[test]
fn window_on_trigger_axis_is_the_default() {
    // Trigger at 05:00 → before a 06:00 lower bound → excluded on the TRIGGER axis.
    let w = TimeWindow::from_args(Some("2026-06-07T06:00:00Z"), None).unwrap();
    let n = node(
        Some("2026-06-07T05:00:00Z"),
        Some("2026-06-07T05:00:05Z"),
        Some("2026-06-07T07:00:00Z"),
        SubagentKind::BuiltinTask,
    );
    assert!(!window_admits(&n, &w, AgentTimeAxis::Trigger));
    // …but its COMPLETION (07:00) is inside the window.
    assert!(window_admits(&n, &w, AgentTimeAxis::Completion));
}

#[test]
fn trigger_and_start_can_diverge_across_the_bound() {
    // The trigger LAGS into start by seconds; a bound between them admits on one axis
    // but not the other - proving the axis choice is load-bearing.
    let w = TimeWindow::from_args(Some("2026-06-07T05:00:03Z"), None).unwrap();
    let n = node(
        Some("2026-06-07T05:00:00Z"), // triggered before the bound
        Some("2026-06-07T05:00:05Z"), // started after the bound
        Some("2026-06-07T05:10:00Z"),
        SubagentKind::BuiltinTask,
    );
    assert!(!window_admits(&n, &w, AgentTimeAxis::Trigger));
    assert!(window_admits(&n, &w, AgentTimeAxis::Start));
}

#[test]
fn unbounded_window_admits_even_missing_timestamp() {
    let w = TimeWindow::default();
    let n = node(None, None, None, SubagentKind::Workflow);
    assert!(window_admits(&n, &w, AgentTimeAxis::Trigger));
    assert!(window_admits(&n, &w, AgentTimeAxis::Start));
    assert!(window_admits(&n, &w, AgentTimeAxis::Completion));
}

#[test]
fn bounded_window_excludes_missing_axis_timestamp() {
    let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
    // No trigger timestamp at all → a bounded trigger-window must NOT admit it.
    let n = node(
        None,
        None,
        Some("2026-06-07T05:00:00Z"),
        SubagentKind::Workflow,
    );
    assert!(!window_admits(&n, &w, AgentTimeAxis::Trigger));
    // The completion axis (present + in range) does admit it.
    assert!(window_admits(&n, &w, AgentTimeAxis::Completion));
}

#[test]
fn one_line_collapses_and_marks_elision_count() {
    assert_eq!(one_line("a\n  b\tc"), "a b c");
    // A long multi-byte string truncated on a CHAR boundary never panics AND now marks
    // the dropped-char count explicitly (the never-silent-truncation contract - the old
    // bare `…` dropped the count). 400 chars in, 200 kept → `… (+200 chars)`.
    let multibyte = "🤖🎉✅🚀".repeat(100); // 400 chars
    let out = one_line(&multibyte);
    assert!(
        out.ends_with("… (+200 chars)"),
        "elision must carry the count, not a bare …: {out}"
    );
    assert!(out.starts_with(&"🤖🎉✅🚀".repeat(50))); // first 200 chars kept
}

#[test]
fn node_json_omits_returned_and_files_unless_requested() {
    let n = node(
        Some("2026-06-07T05:00:00Z"),
        Some("2026-06-07T05:00:05Z"),
        Some("2026-06-07T05:10:00Z"),
        SubagentKind::BuiltinTask,
    );
    let lean = View {
        want_returned: false,
        want_files: false,
        single_node: false,
    };
    let j = node_json(&n, &lean);
    assert!(j.get("returned_message").is_none());
    assert!(j.get("files_changed").is_none());
    // The trigger time IS surfaced and is the default duration anchor.
    assert_eq!(j["trigger_utc"], "2026-06-07T05:00:00Z");

    let rich = View {
        want_returned: true,
        want_files: true,
        single_node: false,
    };
    let j2 = node_json(&n, &rich);
    assert!(j2.get("returned_message").is_some());
    assert!(j2.get("files_changed").is_some());
}

#[test]
fn node_json_renders_returned_message_source() {
    let mut n = node(
        Some("2026-06-07T05:00:00Z"),
        None,
        None,
        SubagentKind::BuiltinTask,
    );
    n.returned_message = Some("the answer".to_string());
    n.returned_message_source = Some(ReturnedMsgSource::AsyncChildTail);
    let rich = View {
        want_returned: true,
        want_files: false,
        single_node: false,
    };
    let j = node_json(&n, &rich);
    assert_eq!(j["returned_message"], "the answer");
    assert_eq!(j["returned_message_source"], "async-child-tail");
}

#[test]
fn fmt_ms_compact() {
    assert_eq!(fmt_ms(3_000), "3s");
    assert_eq!(fmt_ms(125_000), "2m05s");
    assert_eq!(fmt_ms(3_700_000), "1h01m");
}
