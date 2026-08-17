use crate::harness::*;

#[test]
fn search_signed_max_count_selects_the_window_ends() {
    // `--max-count N` keeps the EARLIEST N of the chronological stream, `-N` the LATEST N,
    // `0` stays uncapped; the kept exchanges always emit oldest-first among themselves, and
    // both ends disclose the window (banner: showing earliest/latest; footer: later/earlier
    // dropped).
    let h = Home::new();
    let _ = header_collision_scenario(&h); // COLLIDEONE (05h) < COLLIDETWO (06h) < SOLOWORD (07h)

    let first = h.run(&["search", "SEEDWORD", "--max-count", "1"]);
    assert!(first.success, "stderr: {}", first.stderr);
    assert!(
        first.stdout.contains("COLLIDEONE")
            && !first.stdout.contains("COLLIDETWO")
            && !first.stdout.contains("SOLOWORD"),
        "--max-count 1 keeps the chronologically EARLIEST exchange: {}",
        first.stdout
    );
    assert!(
        first.stdout.contains("showing earliest 1")
            && first.stdout.contains("2 later dropped by --max-count"),
        "disclosures at both ends: {}",
        first.stdout
    );

    let last = h.run(&["search", "SEEDWORD", "--max-count", "-1"]);
    assert!(last.success, "stderr: {}", last.stderr);
    assert!(
        last.stdout.contains("SOLOWORD")
            && !last.stdout.contains("COLLIDEONE")
            && !last.stdout.contains("COLLIDETWO"),
        "--max-count -1 keeps the chronologically LATEST exchange: {}",
        last.stdout
    );
    assert!(
        last.stdout.contains("showing latest 1")
            && last.stdout.contains("2 earlier dropped by --max-count"),
        "disclosures at both ends: {}",
        last.stdout
    );

    // A latest-N window still emits oldest-first among the kept exchanges.
    let two = h.run(&["search", "SEEDWORD", "--max-count", "-2"]);
    assert!(two.success, "stderr: {}", two.stderr);
    assert!(
        !two.stdout.contains("COLLIDEONE"),
        "the earliest exchange is outside the latest-2 window: {}",
        two.stdout
    );
    let pos2 = two.stdout.find("COLLIDETWO").expect("second kept");
    let pos3 = two.stdout.find("SOLOWORD").expect("third kept");
    assert!(
        pos2 < pos3,
        "kept exchanges emit oldest-first among themselves: {}",
        two.stdout
    );

    // `0` = uncapped (the crate-wide convention), no window note.
    let all = h.run(&["search", "SEEDWORD", "--max-count", "0"]);
    assert!(all.success, "stderr: {}", all.stderr);
    assert!(
        all.stdout.contains("COLLIDEONE")
            && all.stdout.contains("COLLIDETWO")
            && all.stdout.contains("SOLOWORD"),
        "--max-count 0 is uncapped: {}",
        all.stdout
    );
    assert!(
        !all.stdout.contains("showing "),
        "no window note when uncapped: {}",
        all.stdout
    );
}

#[test]
fn search_zero_match_diagnosis_discloses_skipped_lines() {
    // An absence claim over a corpus with malformed lines must disclose them: the stderr
    // zero-match diagnosis carries the skipped count (the fixture home has malformed lines).
    let h = populated_home();
    let out = h.run(&["search", "ZZNOSUCHPATTERNZZ"]);
    assert!(out.success, "a zero-match search exits 0: {}", out.stderr);
    assert!(
        out.stderr.contains("0 matches"),
        "diagnosis frames the absence: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("malformed line(s) skipped")
            && out.stderr.contains("parseable lines only"),
        "diagnosis disclosed the skipped lines: {}",
        out.stderr
    );
}

#[test]
fn search_session_filter_and_turn_range() {
    let h = populated_home();
    // --session selects the parent; --turn picks turn 1 only.
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--turn",
        "1..1",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("·t1"), "turn 1 header: {}", out.stdout);
    assert!(
        !out.stdout.contains("·t0"),
        "turn 0 excluded: {}",
        out.stdout
    );
}

#[test]
fn search_turn_range_intersects_with_time_window() {
    // --turn ∧ --since/--until INTERSECT (both filters AND) — the old
    // mutual-exclusion interface law is gone. An impossible intersection (turns exist,
    // but none inside the window) is an honest empty result, exit 0.
    let h = populated_home();
    let ok = h.run(&[
        "search",
        "carry",
        &at(SESS),
        "--turn",
        "0..1",
        "--until",
        "2027-01-01",
    ]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(
        ok.stdout.contains("carry"),
        "in-range ∧ in-window matches: {}",
        ok.stdout
    );
    let none = h.run(&[
        "search",
        "carry",
        &at(SESS),
        "--turn",
        "0..1",
        "--until",
        "2020-01-01",
    ]);
    assert!(none.success, "an empty intersection is not an error");
    assert!(
        none.stdout.contains("no matching exchanges"),
        "window excludes everything: {}",
        none.stdout
    );
}

#[test]
fn search_unknown_session_errors() {
    let h = populated_home();
    let out = h.run(&[
        "search",
        "x",
        at("00000000-0000-0000-0000-000000000000").as_str(),
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_invalid_regex_errors() {
    let h = populated_home();
    let out = h.run(&["search", "(unclosed"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("invalid regex"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_since_until_window() {
    let h = populated_home();
    // A window that starts at 06:00 drops turn 0 (05:00) and keeps turn 1.
    let out = h.run(&[
        "search",
        "",
        "--since",
        "2026-06-07T06:00:00Z",
        "--no-subagents",
        at(SESS).as_str(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("·t1"),
        "turn 1 surfaced: {}",
        out.stdout
    );
}

#[test]
fn turns_and_search_label_automation_triggers() {
    // A `<task-notification>` automation trigger opens a turn but must render as the
    // parsed `[<kind> <id> …]` ATTRIBUTION label — with the TRUE kind parsed from the
    // summary (a `Background command "…"` summary renders `background-command`, NOT the old
    // blanket `workflow`) — never the raw XML blob — and `turns` reports the automation
    // count in its header.
    let h = Home::new();
    let sess = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/Users/x/p","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the build please"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Starting the build now."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf12abc</task-id>\n<tool-use-id>toolu_z</tool-use-id>\n<output-file>/tmp/wf12abc.output</output-file>\n<status>completed</status>\n<summary>Background command \"Run the build\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The build finished cleanly."}]}}"#,
        // A SECOND automation trigger → exercises the PLURAL header arm (N == 2).
        r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:20:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf34def</task-id>\n<status>completed</status>\n<summary>Background command \"Run the tests\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"n1","timestamp":"2026-06-07T05:20:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"All tests passed."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-p/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    // turns: the header reports the automation count; the body shows the attribution label
    // and never the raw XML.
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout
            .contains("(2 automation triggers: 2 background-command)"),
        "header must report the plural automation count + per-class breakdown; got: {}",
        t.stdout
    );
    assert!(
        t.stdout.contains("[background-command wf12abc completed]"),
        "automation opener must render with its TRUE kind (background-command); got: {}",
        t.stdout
    );
    assert!(
        !t.stdout.contains("<task-notification>") && !t.stdout.contains("<output-file>"),
        "raw task-notification XML must NOT appear; got: {}",
        t.stdout
    );

    // search: the same attribution label is matchable; the raw blob is not surfaced. The
    // `<task-notification>` now classifies as `harness.notification.background-command` (NOT
    // `user` — the §1 reparent), so it surfaces under that selector (or `-t harness.notification`).
    let s = h.run(&[
        "search",
        "background-command",
        "-t",
        "harness.notification.background-command",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains("[background-command wf12abc completed]"),
        "search must surface the attribution label under harness.notification; got: {}",
        s.stdout
    );
    // And it must NOT surface under `-t user` anymore (the reparent — regression guard).
    let not_user = h.run(&[
        "search",
        "background-command",
        "-t",
        "user",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(not_user.success, "stderr: {}", not_user.stderr);
    assert!(
        not_user.stdout.contains("no matching exchanges"),
        "a <task-notification> must NOT surface under -t user; got: {}",
        not_user.stdout
    );
    assert!(
        !s.stdout.contains("<output-file>"),
        "search must not surface the raw XML wrapper; got: {}",
        s.stdout
    );
}

#[test]
fn turns_single_automation_trigger_uses_singular_header() {
    // Exactly ONE automation trigger → the SINGULAR header arm ("1 automation trigger").
    let h = Home::new();
    let sess = "11111111-2222-3333-4444-555555555555";
    let lines = [
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/q","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"On it."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>onejob</task-id>\n<status>completed</status>\n<summary>One background job completed</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-q/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains("(1 automation trigger:") && !t.stdout.contains("triggers:"),
        "header must use the SINGULAR form; got: {}",
        t.stdout
    );
    // The per-class breakdown names the class (here `task`, the fallback for a generic
    // "background job" summary), not just the lumped count.
    assert!(
        t.stdout.contains("(1 automation trigger: 1 task)"),
        "header must carry the per-class breakdown; got: {}",
        t.stdout
    );
}

#[test]
fn turns_json_emits_session_header_and_structured_automation() {
    // JSON consumers get (a) a leading {kind:"header",…} object carrying the
    // human/automation split + budget fan-out, and (b) STRUCTURED automation attribution on
    // the user-segment object (is_automation + trigger_kind + task_id + status) — not just a
    // text prefix to regex. A monitor-tick pulse renders trigger_kind "monitor".
    let h = Home::new();
    let sess = "22222222-3333-4444-5555-666666666666";
    let lines = [
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/m","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"On it."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>mon1</task-id>\n<status>completed</status>\n<summary>Monitor event: \"suite re-run completion\"</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Noted."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-m/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    let first = t.stdout.lines().next().unwrap_or("");
    assert!(
        first.contains("\"kind\":\"header\"")
            && first.contains("\"budget_is_per_session\":true")
            && first.contains("\"automation_triggers\":1"),
        "first JSON line must be the session_header with the automation split; got: {first}"
    );
    // The automation USER object carries the STRUCTURED attribution + the monitor kind.
    assert!(
        t.stdout.contains("\"is_automation\":true")
            && t.stdout.contains("\"trigger_kind\":\"monitor\"")
            && t.stdout.contains("\"task_id\":\"mon1\""),
        "the automation user segment must carry structured trigger fields; got: {}",
        t.stdout
    );
    // A HUMAN user object carries is_automation:false (and no trigger_kind).
    assert!(
        t.stdout.contains("\"is_automation\":false"),
        "a human user segment must carry is_automation:false; got: {}",
        t.stdout
    );
}

#[test]
fn search_empty_session_file_is_skipped() {
    // A project whose session file is EMPTY → search_one_file's `mmap_bytes → None`
    // arm (empty file). The search succeeds with no matches.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), ""); // zero-byte session
    let out = h.run(&["search", "anything"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn search_resolve_persisted_reads_pointed_file() {
    // --resolve-persisted: a tool_result carrying a <persisted-output> pointer to a
    // real file whose body contains a token absent from the inline preview. The token
    // matches ONLY with resolution on.
    let h = Home::new();
    // The persisted target file lives under the temp HOME so it is real + readable.
    let target = h.root.join("persisted-body.txt");
    std::fs::write(&target, "deep persisted body with token quuxmarker here").unwrap();
    let session_line = format!(
        r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"q"}}}}
{{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"call0","name":"Bash","input":{{}}}}]}}}}
{{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"call0","content":"<persisted-output>\nOutput too large. Full output saved to: {}\n\nPreview (first 2KB):\n(no token in preview)\n</persisted-output>"}}]}}}}
"#,
        target.display()
    );
    h.write(&format!("{ENC}/{SESS}.jsonl"), &session_line);

    // Without resolution: the token is only in the file, not inline → no match.
    let without = h.run(&["search", "quuxmarker", "--no-subagents"]);
    assert!(without.success, "stderr: {}", without.stderr);
    assert!(
        without.stdout.contains("no matching exchanges"),
        "inline should not match: {}",
        without.stdout
    );

    // With resolution: the file is read, the token is found → a match.
    let with = h.run(&[
        "search",
        "quuxmarker",
        "--resolve-persisted",
        "--no-subagents",
    ]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(
        with.stdout.contains("agent.tool.result"),
        "resolved match: {}",
        with.stdout
    );
    assert!(with.stdout.contains("matched 1"));
}

#[test]
fn search_teammate_message_is_inbox_not_user_regression() {
    // GOLD §1 / oracle §H: an inbound `<teammate-message>` is `type:user/role:user/string` and
    // matches NO synthetic marker, so the OLD `is_genuine_user` counted it as the human. The
    // cutover classifies it `agent.communication.inbox` (from ⇨ self) and DROPS it from `user`.
    let h = Home::new();
    let sess = "11111111-2222-3333-4444-555555555555";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"11111111-2222-3333-4444-555555555555","cwd":"/Users/x/team","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the human asks about throughput"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        r#"{"type":"user","uuid":"tm0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"VSMultiRegion\" color=\"blue\">\nplease check the rate limit handling\n</teammate-message>"}}"#,
    ];
    h.write(
        &format!("-Users-x-team/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    // Under `-t agent.communication.inbox` it surfaces WITH the `from ⇨ to` direction.
    let inbox = h.run(&[
        "search",
        "rate limit",
        "-t",
        "agent.communication.inbox",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("agent.communication.inbox"),
        "teammate message must classify as inbox; got: {}",
        inbox.stdout
    );
    assert!(
        inbox.stdout.contains("VSMultiRegion ⇨"),
        "the comm direction `from ⇨ to` must render; got: {}",
        inbox.stdout
    );

    // Under `-t user` it must NOT appear (the §1 bug fix) — the human turn does, the peer does not.
    let user = h.run(&[
        "search",
        "rate limit",
        "-t",
        "user",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(user.success, "stderr: {}", user.stderr);
    assert!(
        user.stdout.contains("no matching exchanges"),
        "a teammate message must NOT surface under -t user; got: {}",
        user.stdout
    );
}
