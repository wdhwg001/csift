use crate::harness::*;

#[test]
fn turns_project_path_target_scans_the_project() {
    // A project-dir target (the encoded token) resolves every session under it. (A bare
    // `csift turns` with NO target at all is a hard error — budget × everything; see
    // `turns_requires_a_target`.)
    let h = turns_home();
    let out = h.run(&["verbatim", ENC, "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_json_single_side_units_present_under_tight_budget() {
    // A tight budget forces some single-side (user-only / assistant-only) selections in
    // the JSON output — exercise the single-side JSON emit path.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "2500",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    // Some units selected, budget respected.
    let units: usize = objs.iter().filter(|o| o.get("role").is_some()).count();
    assert!(units >= 1, "at least one unit under a tight budget");
    let sum: usize = objs
        .iter()
        .filter(|o| o.get("role").is_some())
        .map(|o| o["rendered_chars"].as_u64().unwrap() as usize + 24)
        .sum();
    assert!(sum <= 2500, "tight budget respected: {sum}");
}

#[test]
fn turns_since_window_filters_turns() {
    // A `--since` far in the future excludes every turn (timestamp-less or older) → no
    // selection. Exercises the time-window path in run_turns.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--since",
        "2999-01-01T00:00:00Z",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("no turns selected"), "{}", out.stdout);
}

#[test]
fn turns_multi_session_text_has_blank_separator_and_both_sessions() {
    // A project-dir target → both sessions in the project are rendered, separated by a
    // blank line (the `if !first { println!() }` arm). Sessions are sorted by id.
    let h = turns_two_sessions_home();
    let out = h.run(&["verbatim", ENC, "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let session_headers = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("SESSION "))
        .count();
    assert!(
        session_headers >= 2,
        "both sessions rendered: {}",
        out.stdout
    );
    // The clean session's content is present.
    assert!(out.stdout.contains("clean session ask"), "{}", out.stdout);
}

#[test]
fn turns_targeted_top_level_skipped_at_tiny_budget_is_reported_not_silent() {
    // CRITICAL: at a budget too small for the targeted top-level session's first round-trip,
    // the session must be reported with an explicit skip note (never silently absent), and the
    // scope banner must still count it as `1 top-level` in scope — not `0`.
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "120",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("SESSION {SESS}  skipped")),
        "the targeted top-level session must be reported as skipped, not silently dropped: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("1 top-level"),
        "scope banner must still report 1 top-level in scope (not 0): {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("raise --budget"),
        "the skip note must tell the user to raise --budget: {}",
        out.stdout
    );
}

#[test]
fn turns_clean_session_reports_no_skipped_lines() {
    // A session with NO malformed lines → the skipped-lines footer is OMITTED (the
    // `skipped_lines > 0` false arm).
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000003";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"clean ask one"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"clean reply one"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("malformed"),
        "clean session has no skipped-line footer: {}",
        out.stdout
    );
}

#[test]
fn turns_json_clean_session_emits_zero_skipped_terminator() {
    // JSON: a clean session now ALWAYS closes with a {"kind":"skipped_lines",
    // "skipped_lines":0} terminator (so a consumer can detect end-of-stream) — the record is
    // unconditional, mirroring search/files/recover.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000004";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"clean json ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"clean json reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let term = objs
        .iter()
        .find(|o| o["kind"] == "summary")
        .expect("clean session still emits the skipped_lines terminator");
    assert_eq!(term["skipped_lines"].as_u64().unwrap(), 0);
    assert!(objs.iter().any(|o| o["role"] == "user"));
}

#[test]
fn turns_main_fixture_text_reports_skipped_line() {
    // The main fixture HAS a malformed line → the skipped-lines footer appears (the
    // `skipped_lines > 0` TRUE arm in both text + an explicit count).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 malformed line(s) skipped"),
        "{}",
        out.stdout
    );
}

#[test]
fn turns_json_main_fixture_has_skipped_record() {
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    assert!(
        objs.iter()
            .any(|o| o["kind"] == "summary" && o["skipped_lines"].as_u64().unwrap() >= 1),
        "the malformed line is surfaced in JSON under the `skipped_lines` key"
    );
}

#[test]
fn turns_assistant_only_orphan_lead_renders() {
    // A session that LEADS with an assistant before any user is rare but real; turns
    // must render the orphan assistant side without panicking (the user-None render
    // arms). group_turn_indices folds the lead into turn 0.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000005";
    let mut s = String::new();
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"orphan lead before any user"}]}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"user","content":"the real first ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"real reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_turn_range_alone_is_not_a_conflict() {
    // --turn WITHOUT --since/--until is valid (the L186 false arm: turn_range set
    // but since/until both None). Restrict to turns 0..2.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--turn",
        "0..2",
        "--format",
        "json",
    ]);
    assert!(
        out.success,
        "a bare --turn must not conflict: {}",
        out.stderr
    );
    let objs = json_lines(&out.stdout);
    // No turn beyond index 2 selected.
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(o["turn_index"].as_u64().unwrap() <= 2, "turn cap: {o}");
    }
}

#[test]
fn turns_empty_session_file_is_safe() {
    // An empty jsonl (0 bytes) → mmap returns None → no turns, honest empty message.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000006";
    h.write(&format!("{ENC}/{sess}.jsonl"), "");
    let out = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("no turns selected"), "{}", out.stdout);
}

#[test]
fn turns_valid_round_trip_fraction_accepted() {
    // A fraction strictly inside (0,1) is accepted (the L189 false arm — valid input).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--round-trip-fraction",
        "0.7",
    ]);
    assert!(out.success, "0.7 is a valid fraction: {}", out.stderr);
    assert!(
        out.stdout.contains("round-trip-fraction 0.70"),
        "{}",
        out.stdout
    );
}

#[test]
fn turns_nonzero_budget_accepted() {
    // A positive budget passes the L195 check (false arm).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "1000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_since_and_until_both_bound_the_window() {
    // BOTH --since and --until set (the L186 inner `||` both-bounds path + the time
    // window contains() both arms). A wide window admits the fixture's turns.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--since",
        "2026-06-07T00:00:00Z",
        "--until",
        "2026-06-08T00:00:00Z",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_budget_is_chars_only() {
    // `--budget` is CHARS, period (the token-unit mode and its silent-4x default trap
    // are gone; ≈4 chars/token is a documented sizing rule of thumb, not a flag).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "8000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("budget 8000 chars"), "{}", out.stdout);
}

#[test]
fn turns_turn_range_excludes_out_of_window_turns() {
    // A turn that excludes the LOW turns (the L278 `turn_index < lo` true arm) and
    // the HIGH turns (`turn_index > hi` true arm).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--turn",
        "3..5",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        let ti = o["turn_index"].as_u64().unwrap();
        assert!((3..=5).contains(&ti), "turn {ti} outside 3..5");
    }
}

#[test]
fn turns_multi_session_json_runs_both() {
    // JSON over two sessions (a project-dir target) → both sessions' units emitted.
    let h = turns_two_sessions_home();
    let out = h.run(&[
        "verbatim",
        ENC,
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sessions: std::collections::BTreeSet<&str> = objs
        .iter()
        .filter_map(|o| o.get("session_id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        sessions.len() >= 2,
        "both sessions present in JSON: {sessions:?}"
    );
}

#[test]
fn turns_scan_skips_non_candidate_lines() {
    // A session containing NON-candidate records (a system metrics line, a
    // file-history-snapshot) interleaved with real turns → the scan-time prefilter skips
    // them (the `!line_is_turn_candidate` true arm) without affecting the turns.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000007";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask before noise"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"system","subtype":"turn_duration","durationMs":1234}"#);
    s.push('\n');
    s.push_str(
        r#"{"type":"file-history-snapshot","snapshot":{"messageId":"m","trackedFileBackups":{}}}"#,
    );
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply after noise"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("ask before noise"), "{}", out.stdout);
    assert!(out.stdout.contains("reply after noise"), "{}", out.stdout);
    // The non-candidate lines are silently skipped (not malformed → no skip count).
    assert!(
        !out.stdout.contains("malformed"),
        "non-candidate != malformed: {}",
        out.stdout
    );
}

#[test]
fn turns_agent_msgs_rich_placeholder_range_is_fetchable_and_attributed() {
    // The JSON form carries a `collapsed_agents` record with X/Y/Z + first/last line so a
    // consumer can Read the raw range; Y is non-zero (each collapsed msg had a tool_use).
    let h = turns_home();
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let objs = json_lines(&json.stdout);
    let ph = objs
        .iter()
        .find(|o| o["kind"] == "collapsed_agents")
        .expect("a collapsed_agents placeholder record");
    assert!(ph["agent_messages"].as_u64().unwrap() >= 1);
    assert!(
        ph["tool_calls"].as_u64().unwrap() >= 1,
        "Y attributes the span's tool calls"
    );
    let first = ph["first_line"].as_u64().unwrap();
    let last = ph["last_line"].as_u64().unwrap();
    assert!(first <= last && first > 0, "a fetchable jsonl line range");
}

#[test]
fn turns_agent_msgs_all_keeps_every_message_no_placeholder() {
    // `--agent-msgs all` emits every agent message of the long run, no placeholder.
    let h = turns_home();
    let all = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "all",
    ]);
    assert!(all.success, "stderr: {}", all.stderr);
    // Even the pure declarations appear verbatim now.
    assert!(
        all.stdout.contains("LETMEDECL"),
        "all keeps declarations: {}",
        all.stdout
    );
    assert!(all.stdout.contains("AGENTRICHFIRST") && all.stdout.contains("AGENTEOT"));
    // No collapsed-agents placeholder line.
    assert!(
        !all.stdout.contains("agent messages]") && !all.stdout.contains("agent message]"),
        "all mode emits no placeholder: {}",
        all.stdout
    );
}

/// The executable re-capture procedure for the baseline above — NOT a behavioral test
/// (ignored by default; the fixture is a temp Home, so no hand-run command can reproduce
/// it). Writes the current eot-only output to tests/turns_pre_feature_baseline.txt.
#[test]
#[ignore = "capture tool — rewrites tests/turns_pre_feature_baseline.txt; run only on an intended output change"]
fn recapture_turns_pre_feature_baseline() {
    let h = turns_home();
    let out = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--agent-msgs",
            "eot-only",
        ],
        &[("TZ", "UTC")],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let dest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/turns_pre_feature_baseline.txt");
    std::fs::write(&dest, &out.stdout).expect("write baseline");
    eprintln!("captured {} bytes to {}", out.stdout.len(), dest.display());
}
