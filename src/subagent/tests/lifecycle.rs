//! Per-agent lifecycle/status resolution: journals, terminal messages, frozen lanes.

use super::*;

#[test]
fn lifecycle_builtin_from_transcript_terminal_message() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    let builtin = subs
        .iter()
        .find(|s| s.kind == SubagentKind::BuiltinTask)
        .unwrap();
    let lc = lifecycle(builtin, &JournalCache::build(std::slice::from_ref(builtin))).unwrap();
    assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T05:00:00.000Z"));
    assert_eq!(
        lc.completed_utc.as_deref(),
        Some("2026-06-07T05:03:20.000Z")
    );
    // Terminal assistant text ⇒ completed (no journal for a built-in).
    assert_eq!(lc.status, SubagentStatus::Completed);
    assert_eq!(lc.agent_type.as_deref(), Some("oh-my-claudecode:executor"));
    assert_eq!(lc.description.as_deref(), Some("run it"));
}

#[test]
fn lifecycle_workflow_completion_from_journal_result() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    let wf = subs
        .iter()
        .find(|s| s.kind == SubagentKind::Workflow)
        .unwrap();
    let lc = lifecycle(wf, &JournalCache::build(std::slice::from_ref(wf))).unwrap();
    // The workflow transcript ends on a tool_use (no terminal text), but the
    // journal carries a `result` event ⇒ completed.
    assert_eq!(lc.status, SubagentStatus::Completed);
    assert_eq!(lc.agent_type.as_deref(), Some("workflow-subagent"));
    assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T06:00:00.000Z"));
    assert_eq!(
        lc.completed_utc.as_deref(),
        Some("2026-06-07T06:01:00.000Z")
    );
}

#[test]
fn journal_cache_first_result_event_wins_and_renders_nonstring() {
    let fx = Fixture::new();
    let enc = "-Users-testuser-Projects-jc";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let wf_dir = format!("{enc}/{SESS}/subagents/workflows/wf_jc1");
    for agent in ["aaa111", "bbb222"] {
        fx.write(
                &format!("{wf_dir}/agent-{agent}.jsonl"),
                &format!("{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"{agent}\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"s\"}}}}\n"),
            );
    }
    fx.write(
        &format!("{wf_dir}/journal.jsonl"),
        concat!(
            // FIRST result event for aaa111 carries NO payload — and first wins
            // (the former per-agent scan returned on its first match), so the
            // later "late" payload must never surface.
            "{\"type\":\"result\",\"agentId\":\"aaa111\"}\n",
            "{\"type\":\"result\",\"agentId\":\"aaa111\",\"result\":\"late\"}\n",
            "not json - skipped exactly as the direct scans skipped it\n",
            // A non-string payload is JSON-rendered so it is never lost.
            "{\"type\":\"result\",\"agentId\":\"bbb222\",\"result\":{\"k\":1}}\n",
        ),
    );
    let subs = discover_subagents(&session).unwrap();
    let cache = JournalCache::build(&subs);
    let a = subs.iter().find(|s| s.agent_id == "aaa111").unwrap();
    let b = subs.iter().find(|s| s.agent_id == "bbb222").unwrap();
    // Both report completion (a result event exists, payload or not)...
    assert!(journal_reports_completion(a, &cache));
    assert!(journal_reports_completion(b, &cache));
    // ...first-event-wins keeps aaa111's payload None; bbb222's renders compactly.
    assert_eq!(journal_result(a, &cache), None);
    assert_eq!(journal_result(b, &cache).as_deref(), Some("{\"k\":1}"));
}

#[test]
fn running_when_no_completion_signal() {
    let fx = Fixture::new();
    let enc = "-Users-testuser-Projects-bar";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // A subagent whose transcript ends mid-tool with NO journal result.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ccc333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ccc333\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T07:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert_eq!(lc.status, SubagentStatus::Running);
}

#[test]
fn duration_label_formats() {
    assert_eq!(
        duration_label(
            Some("2026-06-07T05:00:00.000Z"),
            Some("2026-06-07T05:03:20.000Z")
        )
        .as_deref(),
        Some("3m20s")
    );
    assert_eq!(
        duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T07:05:00Z")).as_deref(),
        Some("2h05m")
    );
    assert_eq!(
        duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T05:00:03Z")).as_deref(),
        Some("3s")
    );
    assert!(duration_label(None, Some("2026-06-07T05:00:00Z")).is_none());
}

#[test]
fn kind_and_status_labels() {
    assert_eq!(SubagentKind::BuiltinTask.label(), "builtin-task");
    assert_eq!(SubagentKind::Workflow.label(), "workflow");
    assert_eq!(SubagentStatus::Completed.label(), "completed");
    assert_eq!(SubagentStatus::Running.label(), "running");
    assert_eq!(SubagentStatus::Unknown.label(), "unknown");
}

#[test]
fn journal_completion_false_for_builtin_no_workflow() {
    // A built-in subagent has no workflow_id → journal_reports_completion is false
    // via the `let Some(wf_id) else` arm (reached through lifecycle: a built-in
    // ending on a tool_use with no terminal text → Running, proving no journal).
    let fx = Fixture::new();
    let enc = "-Users-nojournal";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff666.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"fff666\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert_eq!(lc.status, SubagentStatus::Running);
}

#[test]
fn journal_completion_handles_missing_malformed_and_nonmatching() {
    // A workflow subagent whose journal exists but carries ONLY a `started` event
    // (no `result`) AND a malformed line AND a result for a DIFFERENT agent →
    // completion not reported from the journal; the transcript ends on a tool_use
    // (no terminal text) → Running. Exercises the journal scan's continue arms
    // (blank line, malformed line, non-result type, wrong agentId) + the
    // fall-off-the-end `false`.
    let fx = Fixture::new();
    let enc = "-Users-wfjournal";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_z/agent-ggg777.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ggg777\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:00:30.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    fx.write(
        &format!("{enc}/{SESS}/subagents/workflows/wf_z/journal.jsonl"),
        concat!(
            "\n",                                                   // blank line → continue
            "{ this is not valid json }\n",                         // malformed → continue
            "{\"type\":\"started\",\"agentId\":\"ggg777\"}\n",      // non-result → no match
            "{\"type\":\"result\",\"agentId\":\"someone-else\"}\n"  // result, wrong agent → no match
        ),
    );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert_eq!(
        lc.status,
        SubagentStatus::Running,
        "no matching result → not completed"
    );
}

#[test]
fn journal_completion_false_when_journal_absent() {
    // A workflow subagent with NO journal.jsonl at all → the `std::fs::read` Err
    // arm. The transcript here ends with terminal assistant text, so the AGENT is
    // still Completed (via transcript), but the journal path itself returns false.
    let fx = Fixture::new();
    let enc = "-Users-wfnojournal";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_n/agent-hhh888.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"hhh888\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:00:30.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done cleanly\"}]}}\n"
            ),
        );
    // intentionally NO journal.jsonl
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    // Completed via the transcript terminal message, not the (absent) journal.
    assert_eq!(lc.status, SubagentStatus::Completed);
}

#[test]
fn lifecycle_unknown_when_no_timestamps() {
    // A transcript whose records carry NO timestamp at all → started_utc stays
    // None, no terminal text, no journal → status Unknown (resolve_status's else
    // arm). The head scan also exercises the `if let Some(ts)` false arm (record
    // with no timestamp) before falling off the end.
    let fx = Fixture::new();
    let enc = "-Users-nots";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-iii999.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"iii999\",\"message\":{\"role\":\"user\",\"content\":\"no ts\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert!(lc.started_utc.is_none());
    assert_eq!(lc.status, SubagentStatus::Unknown);
}

#[test]
fn lifecycle_tail_terminal_flag_short_circuits_on_later_record() {
    // To reach the `!terminal_agent_msg` FALSE arm of the tail guard, the NEWEST
    // record must set terminal_agent_msg=true but leave completed_utc still None
    // (no timestamp) so the scan CONTINUES; the next (older) record is then
    // evaluated with terminal already true → `!terminal_agent_msg && …` short-
    // circuits on its false left operand.
    let fx = Fixture::new();
    let enc = "-Users-shortcirc";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-jjj000.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"jjj000\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                // older assistant WITH a timestamp (provides completed_utc later in the scan)
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"older text\"}]}}\n",
                // NEWEST assistant text but NO timestamp → terminal=true, completed_utc stays None → scan continues
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"newest no-ts text\"}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert_eq!(lc.status, SubagentStatus::Completed);
    // completed_utc comes from the older timestamped record (newest had none).
    assert_eq!(
        lc.completed_utc.as_deref(),
        Some("2026-06-07T05:00:10.000Z")
    );
}

#[test]
fn resolve_status_all_arms() {
    // Direct unit coverage of every resolve_status branch.
    assert_eq!(
        resolve_status(true, true, false, true),
        SubagentStatus::Completed
    ); // journal
    assert_eq!(
        resolve_status(true, false, true, true),
        SubagentStatus::Completed
    ); // terminal msg
    assert_eq!(
        resolve_status(true, false, false, true),
        SubagentStatus::Running
    ); // saw + start
    assert_eq!(
        resolve_status(false, false, false, false),
        SubagentStatus::Unknown
    ); // nothing
    assert_eq!(
        resolve_status(true, false, false, false),
        SubagentStatus::Unknown
    ); // saw but no start
}

#[test]
fn duration_label_none_for_unparseable_bounds() {
    // Both bounds present but one is unparseable → None (the `.parse().ok()?` arm).
    assert!(duration_label(Some("garbage"), Some("2026-06-07T05:00:00Z")).is_none());
    assert!(duration_label(Some("2026-06-07T05:00:00Z"), Some("not-a-time")).is_none());
    // Completion before start → clamped to 0 ("0s"), never negative.
    assert_eq!(
        duration_label(Some("2026-06-07T05:00:10Z"), Some("2026-06-07T05:00:00Z")).as_deref(),
        Some("0s")
    );
}

#[test]
fn frozen_lane_classifies_escalation_blocked_vs_awaiting_execution() {
    let fx = Fixture::new();
    let enc = "-Users-frozen";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    // (1) FROZEN at a dangerous-rm Bash (unreturned), PRECEDED by assistant TEXT — the exact
    // L629→L630 shape that made the old walk-back mis-report `completed`. → escalation-blocked.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aesc111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aesc111\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"teardown\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Now removing the scratch files.\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_rm\",\"name\":\"Bash\",\"input\":{\"command\":\"for f in a b; do rm -rf \\\"$SCRATCH/$f\\\"; done\"}}]}}\n"
            ),
        );
    // (2) FROZEN at a non-danger tool_use (Read, unreturned) → awaiting-execution.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-await22.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"await22\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"read\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_read\",\"name\":\"Read\",\"input\":{\"file_path\":\"/x/big.log\"}}]}}\n"
            ),
        );
    // (3) RESOLVED: a dangerous Bash whose tool_result + closing text arrived → NOT pending.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-done333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"done333\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"teardown\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_rm2\",\"name\":\"Bash\",\"input\":{\"command\":\"rm -rf $SCRATCH/*\"}}]}}\n",
                "{\"type\":\"user\",\"timestamp\":\"2026-06-07T05:40:00.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_rm2\",\"content\":\"done\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:41:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Teardown complete.\"}]}}\n"
            ),
        );

    let nodes = build_topology(&session, false).unwrap();
    let esc = nodes.iter().find(|n| n.agent_id == "aesc111").unwrap();
    // The frozen escalation lane is RUNNING (not completed — the bug) + escalation-blocked.
    assert_eq!(esc.status, SubagentStatus::Running);
    assert_eq!(
        esc.pending_classification,
        Some(PendingClassification::EscalationBlocked)
    );
    assert_eq!(esc.pending_tool_name.as_deref(), Some("Bash"));
    assert_eq!(esc.pending_tool_use_id.as_deref(), Some("toolu_rm"));
    assert_eq!(
        esc.pending_since_utc.as_deref(),
        Some("2026-06-07T05:02:00.000Z")
    );

    let awa = nodes.iter().find(|n| n.agent_id == "await22").unwrap();
    assert_eq!(awa.status, SubagentStatus::Running);
    assert_eq!(
        awa.pending_classification,
        Some(PendingClassification::AwaitingExecution)
    );
    assert_eq!(awa.pending_tool_name.as_deref(), Some("Read"));

    let done = nodes.iter().find(|n| n.agent_id == "done333").unwrap();
    assert_eq!(done.status, SubagentStatus::Completed);
    assert!(done.pending_classification.is_none());
}
