//! The evidence surfaces: registry probe, tail state machine, child lanes.

use super::*;

#[test]
fn registry_proc_start_parses_as_utc() {
    let t = parse_registry_proc_start("Sun Aug 16 09:04:23 2026").unwrap();
    assert_eq!(t.to_string(), "2026-08-16T09:04:23Z");
    assert!(parse_registry_proc_start("not a date").is_none());
}

#[test]
fn registry_proc_start_filetime_form_is_the_windows_rendering() {
    // A Windows row (CC 2.1.258): 100ns ticks since 1601 = the owner's creation instant.
    let t = parse_registry_proc_start("134328101803820142").unwrap();
    assert_eq!(t.to_string(), "2026-09-02T08:09:40.3820142Z");
    assert_eq!(parse_registry_proc_start(" 134328101803820142 "), Some(t));
    // Before the unix epoch or not a plausible tick count: no instant, guard skipped.
    assert!(filetime_to_timestamp(0).is_none());
    assert!(
        parse_registry_proc_start("116444736000000000").is_some(),
        "the unix epoch itself"
    );
    assert!(parse_registry_proc_start("").is_none());
    assert!(parse_registry_proc_start("12ab").is_none());
}

/// Render an instant the way the registry does on THIS platform (asctime UTC on unix,
/// FILETIME ticks on Windows) so the guard round-trips against the live probe.
fn registry_rendering(t: jiff::Timestamp) -> String {
    if cfg!(windows) {
        let secs = u64::try_from(t.as_second()).unwrap() + 11_644_473_600;
        let sub = u64::try_from(t.subsec_nanosecond()).unwrap() / 100;
        format!("{}", secs * 10_000_000 + sub)
    } else {
        jiff::fmt::strtime::format("%a %b %e %H:%M:%S %Y", &t.to_zoned(jiff::tz::TimeZone::UTC))
            .unwrap()
    }
}

// ── The pid probe against real processes (own pid = deterministically alive) ──

#[test]
fn probe_pid_own_process_guard_states() {
    let me = std::process::id();
    // No procStart on the registry side: alive, guard skipped.
    assert_eq!(
        probe_pid(me, None, None),
        PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped
        }
    );
    // An unparseable procStart degrades the same way.
    assert_eq!(
        probe_pid(me, Some("not a date"), None),
        PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped
        }
    );
    // The local domain is the registry's own vocabulary (`pidDomain`: `darwin`, `linux`,
    // `win32:<host>`), pinned as a literal per platform - not read back from the
    // function under test.
    let expected = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "win32"
    };
    assert_eq!(local_pid_domain(), expected);
    assert_eq!(
        probe_pid(me, None, Some(expected)),
        PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped
        }
    );
    // Our own pid domain is never foreign (with or without the host suffix).
    let mine = format!("{}:some-host", local_pid_domain());
    assert_eq!(
        probe_pid(me, None, Some(&mine)),
        PidLiveness::Alive {
            reuse_guard: ReuseGuard::Skipped
        }
    );
    // A row from another domain is never probed: its pid means nothing here.
    assert_eq!(
        probe_pid(me, None, Some("plan9:elsewhere")),
        PidLiveness::ForeignDomain("plan9:elsewhere".to_string())
    );
    // When the probe yields our real start instant, feeding it back in the registry's
    // own rendering must pass the guard; shifting it far must flag reuse.
    if let PsProbe::Alive(Some(act)) = ps_probe(me) {
        let reg = registry_rendering(act);
        assert_eq!(
            probe_pid(me, Some(&reg), None),
            PidLiveness::Alive {
                reuse_guard: ReuseGuard::Checked
            }
        );
        let old = registry_rendering(act - jiff::Span::new().hours(2));
        assert_eq!(probe_pid(me, Some(&old), None), PidLiveness::Reused);
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
    // A journal with a garbage line and an unknown event type: both are skipped,
    // the imbalance still counts. A sibling wf dir WITHOUT a journal contributes
    // nothing (and does not error).
    std::fs::create_dir_all(root.join("s1/subagents/workflows/wf_b")).unwrap();
    std::fs::write(
        wf.join("journal.jsonl"),
        "not json at all\n{\"type\":\"progress\"}\n{\"type\":\"started\",\"agentId\":\"a1\"}\n{\"type\":\"result\",\"agentId\":\"a1\"}\n{\"type\":\"started\",\"agentId\":\"a2\"}\n",
    )
    .unwrap();
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.journal_in_flight, 1, "{report:?}");
    assert_eq!(report.children.len(), 1);
    assert_eq!(report.children[0].state, "settled");
    assert_eq!(report.children[0].detail, "no timestamped tail");
    // 1 journal agent in flight; the settled child adds nothing.
    assert_eq!(report.live_count, 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn children_report_generating_needs_recency_and_no_end_turn() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("csift-live-ac-{}-{n}", std::process::id()));
    let main = root.join("s1.jsonl");
    std::fs::create_dir_all(root.join("s1/subagents")).unwrap();
    std::fs::write(&main, "").unwrap();
    let child = root.join("s1/subagents/agent-fedcba9876543210.jsonl");
    let now = jiff::Timestamp::now().to_string();
    // Paired tail + fresh record + last stop_reason NOT end_turn: mid-generation. The
    // record-tail instant is the signal (mtime is not consulted at all).
    let write_child = |use_ts: &str, tail: &str| {
        std::fs::write(
            &child,
            format!(
                concat!(
                    r#"{{"type":"assistant","timestamp":"{u}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}"#,
                    "\n{t}\n",
                ),
                u = use_ts,
                t = tail
            ),
        )
        .unwrap();
    };
    write_child(
        &now,
        &format!(
            r#"{{"type":"user","timestamp":"{now}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#
        ),
    );
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children.len(), 1);
    assert_eq!(report.children[0].state, "generating", "{report:?}");
    assert!(
        report.children[0].detail.contains("no end_turn yet"),
        "{report:?}"
    );
    assert_eq!(report.live_count, 1);

    // Same freshness but the last assistant record IS an end_turn: settled (a clean
    // finish seconds ago is not generation).
    write_child(
        &now,
        &format!(
            concat!(
                r#"{{"type":"user","timestamp":"{now}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#,
                "\n",
                r#"{{"type":"assistant","timestamp":"{now}","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"done"}}]}}}}"#
            ),
            now = now
        ),
    );
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children[0].state, "settled", "{report:?}");
    assert_eq!(report.live_count, 0);

    // No end_turn but the tail record is ancient: settled (stop_reason alone would
    // resurrect dead lanes - recency is the load-bearing conjunct).
    write_child(
        "2026-06-07T05:00:01Z",
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
    );
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children[0].state, "settled", "{report:?}");
    assert_eq!(report.live_count, 0);
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

#[test]
fn probe_pid_reports_a_reaped_pid_dead() {
    // A reliably dead pid: spawn a no-op, reap it. Exercises the ps-failure arm and its
    // /proc fallback check (absent on macOS, absent-for-the-pid on Linux) or, on
    // Windows, the PowerShell exit-3 arm.
    let mut child = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .unwrap()
    } else {
        std::process::Command::new("true").spawn().unwrap()
    };
    let pid = child.id();
    let _ = child.wait();
    assert_eq!(probe_pid(pid, None, None), PidLiveness::Dead);
}

#[test]
fn children_report_settles_a_lane_whose_completion_pulse_landed() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("csift-live-rt-{}-{n}", std::process::id()));
    let main = root.join("s1.jsonl");
    std::fs::create_dir_all(root.join("s1/subagents")).unwrap();
    std::fs::write(&main, "").unwrap();
    // A fresh, unreturned tail: in-flight by every tail rule...
    let child = root.join("s1/subagents/agent-0f1e2d3c4b5a6978.jsonl");
    let now = jiff::Timestamp::now().to_string();
    std::fs::write(
        &child,
        format!(
            r#"{{"type":"assistant","timestamp":"{now}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}"#
        ) + "\n",
    )
    .unwrap();
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children[0].state, "in-flight", "{report:?}");
    // ...until the main transcript's completion pulse names the agent: settled, the
    // harness's own word outranking the tail.
    let returned: std::collections::HashSet<String> =
        std::iter::once("0f1e2d3c4b5a6978".to_string()).collect();
    let report = children_report(&main, &returned).unwrap();
    assert_eq!(report.children[0].state, "settled", "{report:?}");
    assert!(
        report.children[0]
            .detail
            .contains("completion notification"),
        "{report:?}"
    );
    assert_eq!(report.live_count, 0);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn tail_shape_window_without_a_newline_yields_no_records() {
    // The 512 KB tail window is a plain positional read (v0.10.4). When the cut lands
    // inside one giant final line and no newline follows, the aligned window is empty
    // and the shape carries no records; nothing is parsed from a torn slice.
    let mut giant = String::from(
        r#"{"type":"user","uuid":"g0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"x"}}"#,
    );
    giant.push('\n');
    giant.push_str(&"y".repeat(700 * 1024)); // one unterminated line past the window
    let f = TempJsonl::new(&giant);
    let shape = tail_shape(&f.0).unwrap();
    assert_eq!(shape.records_seen, 0, "{shape:?}");
}
