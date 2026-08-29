//! The evidence surfaces: registry probe, tail state machine, child lanes.

use super::*;

#[test]
fn registry_proc_start_parses_as_utc() {
    let t = parse_registry_proc_start("Sun Aug 16 09:04:23 2026").unwrap();
    assert_eq!(t.to_string(), "2026-08-16T09:04:23Z");
    assert!(parse_registry_proc_start("not a date").is_none());
}

// ── The pid probe against real processes (own pid = deterministically alive) ──

#[cfg(unix)]
#[test]
fn probe_pid_own_process_guard_states() {
    let me = std::process::id();
    // No procStart on the registry side: alive, guard skipped.
    assert_eq!(
        probe_pid(me, None),
        PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped
        }
    );
    // An unparseable procStart degrades the same way.
    assert_eq!(
        probe_pid(me, Some("not a date")),
        PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped
        }
    );
    // When ps yields our real start instant, feeding it back UTC-rendered must pass the
    // guard; shifting it far must flag reuse.
    if let PsProbe::Alive(Some(act)) = ps_probe(me) {
        let utc = act.to_zoned(jiff::tz::TimeZone::UTC);
        let reg = jiff::fmt::strtime::format("%a %b %e %H:%M:%S %Y", &utc).unwrap();
        assert_eq!(
            probe_pid(me, Some(&reg)),
            PidLiveness::Alive {
                reuse_guard: ReuseGuard::Checked
            }
        );
        let shifted = act - jiff::Span::new().hours(2);
        let old = jiff::fmt::strtime::format(
            "%a %b %e %H:%M:%S %Y",
            &shifted.to_zoned(jiff::tz::TimeZone::UTC),
        )
        .unwrap();
        assert_eq!(probe_pid(me, Some(&old)), PidLiveness::Reused);
    }
}

#[test]
fn tail_shape_reads_pairing_stop_reason_and_holds_torn_tails() {
    // Empty file: nothing seen, nothing claimed.
    let empty = TempJsonl::new("");
    let s = tail_shape(&empty.0).unwrap();
    assert_eq!(s.records_seen, 0);
    assert!(s.unreturned_use.is_none() && s.last_stop_reason.is_none());

    // Paired use+result plus an end_turn: settled shape.
    let settled = TempJsonl::new(concat!(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
        "\n",
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#,
        "\n",
    ));
    let s = tail_shape(&settled.0).unwrap();
    assert!(s.unreturned_use.is_none(), "{s:?}");
    assert_eq!(s.last_stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(s.last_ts_utc.as_deref(), Some("2026-06-07T05:00:03Z"));
    assert_eq!(s.records_seen, 3);

    // An unreturned use is reported with its tool name.
    let open = TempJsonl::new(concat!(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t2","name":"Grep","input":{"pattern":"x"}}]}}"#,
        "\n",
    ));
    let s = tail_shape(&open.0).unwrap();
    assert_eq!(
        s.unreturned_use.as_ref().map(|(t, _)| t.as_str()),
        Some("Grep")
    );

    // A torn final line (no newline) is HELD, and a garbage line inside the window is
    // skipped, never counted as a record.
    let torn = TempJsonl::new(concat!(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#,
        "\n",
        "not json at all\n",
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t3","name":"Bash"#,
    ));
    let s = tail_shape(&torn.0).unwrap();
    assert_eq!(s.records_seen, 1, "torn tail held, garbage skipped: {s:?}");
    assert!(s.unreturned_use.is_none(), "{s:?}");
}

#[test]
fn age_secs_handles_absent_and_garbage() {
    assert_eq!(age_secs(None), None);
    assert_eq!(age_secs(Some("not a ts")), None);
    assert_eq!(
        age_secs(Some("2126-01-01T00:00:00Z")),
        Some(0),
        "future clamps to 0"
    );
    assert!(age_secs(Some("2026-01-01T00:00:00Z")).unwrap() > 1_000_000);
}

#[test]
fn children_report_tolerates_garbage_journals_and_quiet_children() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("csift-live-ch-{}-{n}", std::process::id()));
    let main = root.join("s1.jsonl");
    let wf = root.join("s1/subagents/workflows/wf_a");
    std::fs::create_dir_all(&wf).unwrap();
    std::fs::write(&main, "").unwrap();
    // A child whose tail has records but no timestamp: settled, honestly labeled.
    let child = root.join("s1/subagents/agent-0123456789abcdef.jsonl");
    std::fs::write(&child, "{\"type\":\"agent-name\"}\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&child)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    // A journal with a garbage line: the line is skipped, the imbalance still counts.
    std::fs::write(
        wf.join("journal.jsonl"),
        "not json at all\n{\"type\":\"started\",\"agentId\":\"a1\"}\n{\"type\":\"result\",\"agentId\":\"a1\"}\n{\"type\":\"started\",\"agentId\":\"a2\"}\n",
    )
    .unwrap();
    let report = children_report(&main).unwrap();
    assert_eq!(report.journal_in_flight, 1, "{report:?}");
    assert_eq!(report.children.len(), 1);
    assert_eq!(report.children[0].state, "settled");
    assert_eq!(report.children[0].detail, "no timestamped tail");
    // 1 journal agent in flight; the settled child adds nothing.
    assert_eq!(report.live_count, 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn children_report_reads_fresh_growth_as_active() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("csift-live-ac-{}-{n}", std::process::id()));
    let main = root.join("s1.jsonl");
    std::fs::create_dir_all(root.join("s1/subagents")).unwrap();
    std::fs::write(&main, "").unwrap();
    // Settled tail (paired call) but JUST written: recency is real evidence on a child
    // lane (child transcripts grow only from the child's own flow).
    let child = root.join("s1/subagents/agent-fedcba9876543210.jsonl");
    std::fs::write(
        &child,
        concat!(
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#, "\n",
            r#"{"type":"user","timestamp":"2026-06-07T05:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#, "\n",
        ),
    )
    .unwrap();
    let report = children_report(&main).unwrap();
    assert_eq!(report.children.len(), 1);
    assert_eq!(report.children[0].state, "active", "{report:?}");
    assert!(
        report.children[0].detail.contains("transcript grew"),
        "{report:?}"
    );
    assert_eq!(report.live_count, 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn tail_shape_stops_early_once_all_answers_are_in() {
    // 12 records; the answers (unreturned use + stop_reason) sit in the newest 3, so
    // the walk must stop at the >=8 floor without touching the oldest records.
    let mut content = String::new();
    for i in 0..9 {
        content.push_str(&format!(
            "{{\"type\":\"user\",\"timestamp\":\"2026-06-07T04:00:{i:02}Z\",\"message\":{{\"role\":\"user\",\"content\":\"filler {i}\"}}}}\n"
        ));
    }
    content.push_str(concat!(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"text","text":"launching"}]}}"#, "\n",
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t9","name":"Bash","input":{}}]}}"#, "\n",
    ));
    let f = TempJsonl::new(&content);
    let s = tail_shape(&f.0).unwrap();
    assert_eq!(
        s.unreturned_use.as_ref().map(|(t, _)| t.as_str()),
        Some("Bash")
    );
    assert_eq!(s.last_stop_reason.as_deref(), Some("tool_use"));
    assert!(
        s.records_seen >= 8 && s.records_seen < 11,
        "bounded walk stopped early: {s:?}"
    );
}

#[test]
fn tail_window_spans_a_real_turn_tail() {
    // The end_turn record sits ~380KB before EOF: far outside any shrunken window,
    // well inside the real 512KB one - and the file is big enough to exercise the
    // window-alignment arm.
    let filler = |i: usize| {
        format!(
            "{{\"type\":\"user\",\"timestamp\":\"2026-06-07T05:00:00Z\",\"message\":{{\"role\":\"user\",\"content\":\"filler block {i} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"}}}}\n"
        )
    };
    let mut content = String::new();
    for i in 0..1000 {
        content.push_str(&filler(i));
    }
    content.push_str(concat!(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#,
        "\n"
    ));
    for i in 1000..3500 {
        content.push_str(&filler(i));
    }
    assert!(
        content.len() > 512 * 1024,
        "the fixture must exceed the window"
    );
    let f = TempJsonl::new(&content);
    let s = tail_shape(&f.0).unwrap();
    assert_eq!(s.last_stop_reason.as_deref(), Some("end_turn"), "{s:?}");
}

#[cfg(unix)]
#[test]
fn probe_pid_reports_a_reaped_pid_dead() {
    // A reliably dead pid: spawn `true`, reap it. Exercises the ps-failure arm and its
    // /proc fallback check (absent on macOS, absent-for-the-pid on Linux).
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    let _ = child.wait();
    assert_eq!(probe_pid(pid, None), PidLiveness::Dead);
}
