//! verbatim windows and scope: since/until, turn ranges, subagent opt-in, skipped-line notes.

use crate::harness::*;

#[test]
fn turns_defaults_to_top_level_only_no_subagent_span() {
    // FOOTGUN FIX: `turns <uuid>` with NO flags must reconstruct ONLY the top-level thread -
    // it must NOT span the session's subagents (unlike files/search). So a bare run prints no
    // `(subagent transcript)` blocks and no scope banner (one session in scope, rendered).
    let h = populated_home();
    let out = h.run(&["verbatim", at(SESS).as_str(), "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("SESSION {SESS}")),
        "the top-level thread must render: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("(subagent transcript)"),
        "turns must NOT span subagents by default: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("scope  "),
        "a single top-level session prints no scope banner: {}",
        out.stdout
    );
}

#[test]
fn turns_rich_filters_subagent_runs_too() {
    // The shared code path: a SUBAGENT transcript carrying a long agent run is richness-
    // filtered with the same flags (explicit --include-subagents opt-in). The subagent's pure
    // declarations collapse; its rich member + EOT survive.
    let h = turns_home();
    // A subagent sidecar with a long agent run under the session.
    let mut sub = String::new();
    sub.push_str(r#"{"type":"user","isSidechain":true,"agentId":"subrun","timestamp":"2026-06-07T09:00:00.000Z","message":{"role":"user","content":"subagent kicks off a long chain"}}"#);
    sub.push('\n');
    let msgs = [
        "SUBRICHFIRST found the cause in src/z.rs:7",
        "let me SUBDECL a",
        "now i will SUBDECL b",
        "let me SUBDECL c",
        "now let me SUBDECL d",
        "next i SUBDECL e",
        "let me SUBDECL f",
        "the SUBEOT final subagent answer",
    ];
    let mut ts = 1;
    for m in msgs {
        sub.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T09:00:{ts:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"s","name":"Bash","input":{{}}}}]}}}}"#
        ));
        sub.push('\n');
        ts += 1;
        sub.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T09:00:{ts:02}.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{m}"}}]}}}}"#
        ));
        sub.push('\n');
        ts += 1;
    }
    h.write(&format!("{ENC}/{SESS}/subagents/agent-subrun.jsonl"), &sub);

    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("SUBRICHFIRST"),
        "subagent rich member kept: {}",
        out.stdout
    );
    assert!(out.stdout.contains("SUBEOT"), "subagent EOT kept");
    assert!(
        !out.stdout.contains("SUBDECL"),
        "subagent pure declarations collapse under the shared richness path: {}",
        out.stdout
    );
}

#[test]
fn turns_spans_at_least_two_compaction_boundaries() {
    // THE HEADLINE: a 40K budget over the 3-summary fixture must span >= 2 boundaries,
    // and at least one selected unit must come from before the 2nd-newest summary
    // (compactions_before >= 2). Asserted on the compiled binary's JSON over real-shaped
    // committed data.
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
    let boundaries = objs
        .iter()
        .filter(|o| o["kind"] == "compaction_boundary")
        .count();
    assert!(
        boundaries >= 2,
        "must span >=2 compaction boundaries, got {boundaries}"
    );
    let deep = objs
        .iter()
        .filter(|o| o.get("role").is_some())
        .any(|o| o["compactions_before"].as_u64().unwrap_or(0) >= 2);
    assert!(
        deep,
        "at least one unit must predate the 2nd-newest summary"
    );
    // Each boundary record carries a line_no + summary_chars.
    for o in objs.iter().filter(|o| o["kind"] == "compaction_boundary") {
        assert!(o["line"].as_u64().unwrap() > 0);
        assert!(o["summary_chars"].as_u64().unwrap() > 0);
    }
}

#[test]
fn turns_include_subagents_opts_into_span_with_scope_banner() {
    // `--include-subagents` is the explicit opt-in for the rare cross-fan-out reconstruction;
    // it spans the subagents AND prints a scope banner that reports the TRUE top-level/subagent
    // split (never `0 top-level`, even though the budget applies per session).
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(subagent transcript)"),
        "--include-subagents must span subagents: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("scope  ") && out.stdout.contains("1 top-level"),
        "scope banner must report the targeted top-level (never 0 top-level): {}",
        out.stdout
    );
}

#[test]
fn turns_json_header_carries_true_scope_and_rendered_and_by_kind() {
    // The JSON session_header distinguishes TRUE scope (sessions_in_scope) from rendered
    // (sessions_rendered), and carries the per-class automation_by_kind breakdown.
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let first = out.stdout.lines().next().unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(first).expect("header json");
    assert_eq!(v["kind"], "header");
    assert!(
        v.get("sessions_in_scope").is_some(),
        "missing sessions_in_scope: {first}"
    );
    assert!(
        v.get("sessions_rendered").is_some(),
        "missing sessions_rendered: {first}"
    );
    assert_eq!(
        v["top_level_sessions"], 1,
        "targeted top-level counted: {first}"
    );
    let by = v
        .get("automation_by_kind")
        .expect("automation_by_kind present");
    for k in ["background-command", "agent", "workflow", "monitor", "task"] {
        assert!(by.get(k).is_some(), "by_kind missing class {k}: {first}");
    }
}

#[test]
fn turns_turn_range_and_since_intersect() {
    // Same rule as every sibling: the windows AND (the former bail was a leftover).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--turn",
        "0..2",
        "--since",
        "2h",
    ]);
    assert!(
        out.success,
        "combined windows intersect, never error: {}",
        out.stderr
    );
}

#[test]
fn turns_project_path_target_scans_the_project() {
    // A project-dir target (the encoded token) resolves every session under it. (A bare
    // `csift turns` with NO target at all is a hard error - budget x everything; see
    // `turns_requires_a_target`.)
    let h = turns_home();
    let out = h.run(&["verbatim", ENC, "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_since_window_filters_turns() {
    // A `--since` far in the future excludes every turn (timestamp-less or older) -> no
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
fn turns_targeted_top_level_skipped_at_tiny_budget_is_reported_not_silent() {
    // CRITICAL: at a budget too small for the targeted top-level session's first round-trip,
    // the session must be reported with an explicit skip note (never silently absent), and the
    // scope banner must still count it as `1 top-level` in scope - not `0`.
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
fn turns_prints_its_scope_banner_for_a_lone_session_the_budget_skipped() {
    // The banner prints when MORE THAN ONE session is in scope OR when the budget skipped
    // one, and the second disjunct is the only reason on a scope of exactly one. Without
    // it a targeted session that rendered NOTHING would print its skip note under no
    // heading at all - no scope line, no budget, nothing saying what the budget was that
    // it did not fit. The case above cannot reach this: it spans subagents, so the first
    // disjunct is already true.
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "120",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .starts_with("scope  1 session in scope (1 top-level + 0 subagent) · 0 rendered"),
        "a lone skipped session still gets the banner, and it counts the session as in \
         scope and zero as rendered:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("budget 120 chars is PER session"),
        "the banner carries the budget the skip note is measured against:\n{}",
        out.stdout
    );
}

#[test]
fn turns_clean_session_reports_no_skipped_lines() {
    // A session with NO malformed lines -> the skipped-lines footer is OMITTED (the
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
    // JSON: a clean session ALWAYS closes with a {"kind":"skipped_lines",
    // "skipped_lines":0} terminator (so a consumer can detect end-of-stream) - the record is
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
    // The main fixture HAS a malformed line -> the skipped-lines footer appears (the
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
fn turns_empty_session_file_is_safe() {
    // An empty jsonl (0 bytes) -> mmap returns None -> no turns, honest empty message.
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
fn turns_scan_skips_non_candidate_lines() {
    // A session containing NON-candidate records (a system metrics line, a
    // file-history-snapshot) interleaved with real turns -> the scan-time prefilter skips
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
    // The non-candidate lines are silently skipped (not malformed -> no skip count).
    assert!(
        !out.stdout.contains("malformed"),
        "non-candidate != malformed: {}",
        out.stdout
    );
}

/// The compaction banner sits between the turns that END before the summary's line and the
/// turns that end after it. A turn whose opener predates the summary and whose reply lands
/// after it (a compaction mid-turn) ends after it, so it renders BELOW the banner - which is
/// why the banner names where turns end and makes no claim about when they began.
#[test]
fn the_compaction_banner_names_where_the_turns_beside_it_end() {
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000c4501";
    let lines = [
        r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask ends before the cut"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"first reply"}]}}"#,
        r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"second ask opens before the cut"}}"#,
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"u2","timestamp":"2026-06-07T05:00:03.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":1000}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","timestamp":"2026-06-07T05:00:04.000Z","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued. Summary: two asks."}}"#,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"s1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"second reply lands after the cut"}]}}"#,
        r#"{"type":"user","uuid":"u3","parentUuid":"a2","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":"third ask after the cut"}}"#,
        r#"{"type":"assistant","uuid":"a3","parentUuid":"u3","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"third reply"}]}}"#,
    ];
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
    let out = h.run(&["verbatim", at(sess).as_str(), "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let banner =
        "══ compaction boundary · summary at L5 · (turns above end before it, turns below \
                  end after it) ══";
    assert_eq!(out.stdout.matches(banner).count(), 1, "{}", out.stdout);
    let at_ = |needle: &str| {
        out.stdout
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing:\n{}", out.stdout))
    };
    assert!(
        at_("first ask ends before the cut") < at_(banner),
        "{}",
        out.stdout
    );
    // The mid-turn shape: its opener (L3) predates the summary (L5), its reply (L6) does not,
    // and the turn renders below the banner.
    assert!(
        at_(banner) < at_("second ask opens before the cut"),
        "{}",
        out.stdout
    );
    assert!(
        at_(banner) < at_("second reply lands after the cut"),
        "{}",
        out.stdout
    );
    assert!(
        at_(banner) < at_("third ask after the cut"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("predate"), "{}", out.stdout);
}
