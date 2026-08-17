use crate::harness::*;

#[test]
fn pre_subcommand_global_flag_with_trailing_flags() {
    // REGRESSION (≤v0.4.1): normalize_argv assumed argv[1] was the subcommand, so a
    // GLOBAL flag placed BEFORE the subcommand disabled normalization entirely and the
    // allow_hyphen_values PATH positional swallowed every flag that followed a
    // positional — `csift --claude-home DIR list <ENC> --max-count 1` died with a
    // misleading "not a project target". The subcommand is now located by SCANNING over
    // declared root flags (+ their values), so "flag order is free" and "--claude-home
    // any position" hold in combination.
    let h = populated_home();
    let claude_home = h.projects().parent().unwrap().to_path_buf();
    let home_s = claude_home.to_str().unwrap().to_string();

    // (1) The dead quadrant: global flag BEFORE subcommand + value flag AFTER a positional.
    let out = h.run(&[
        "--claude-home",
        home_s.as_str(),
        "list",
        ENC,
        "--max-count",
        "1",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "row listed:\n{}", out.stdout);

    // (2) Same shape through `search` (PATTERN-first positional) with trailing --format json.
    let out = h.run(&[
        "--claude-home",
        home_s.as_str(),
        "search",
        "xyzzy-no-such-text",
        ENC,
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#""kind":"header""#),
        "envelope present even on zero matches:\n{}",
        out.stdout
    );

    // (3) The inline `--claude-home=DIR` form spans one token and is scanned over too.
    let eq = format!("--claude-home={home_s}");
    let out = h.run(&[eq.as_str(), "list", ENC, "--max-count", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));

    // (4) Guard: an UNKNOWN pre-subcommand flag aborts the scan and reaches clap's
    // standard unexpected-argument error (argv passes through untouched).
    let out = h.run(&["--bogus", "list"]);
    assert!(!out.success);
    assert!(out.stderr.contains("--bogus"), "stderr: {}", out.stderr);
}

#[test]
fn unknown_flag_reports_clean_error_not_project_dir_error() {
    // A typo'd / unknown flag on any scope-operating subcommand must surface as an
    // "unexpected argument" message (NOT the misleading "no Claude Code project dir named
    // --xxx"). The `--`-leading value-parser reject makes this uniform tool-wide.
    let h = populated_home();
    let at_sess = at(SESS);
    for args in [
        vec!["files", at_sess.as_str(), "--by-fil"],
        vec!["verbatim", at_sess.as_str(), "--budgett", "5000"],
        vec!["recover", "--bogus-flag"],
        vec!["agents", ENC, "--bogus"],
        vec!["list", "--by-fil"],
    ] {
        let out = h.run(&args);
        assert!(!out.success, "a bad flag must exit nonzero: {args:?}");
        assert!(
            out.stderr.contains("unexpected argument"),
            "expected an 'unexpected argument' message for {args:?}; got: {}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("no Claude Code project dir named"),
            "the misleading project-dir error must NOT appear for {args:?}; got: {}",
            out.stderr
        );
    }
}

#[test]
fn range_open_and_negative_forms() {
    let h = populated_home();
    let t = at(SESS);
    // Count exchanges under a turn spec (empty pattern = pure filter).
    let count = |spec: &str| -> String {
        let out = h.run(&[
            "search",
            "",
            t.as_str(),
            "--no-subagents",
            "--turn",
            spec,
            "-c",
        ]);
        assert!(out.success, "turn {spec:?} stderr: {}", out.stderr);
        out.stdout.trim().to_string()
    };
    // The top-level fixture has 2 genuine-user turns (index 0 and 1).
    assert_eq!(count("0..0"), "1", "turn 0 only");
    assert_eq!(count("1.."), "1", "open end: turn 1 → last");
    assert_eq!(count("..0"), "1", "open start: first → turn 0");
    assert_eq!(count("-1.."), "1", "from-end: the last 1 turn");
    assert_eq!(count("-2.."), "2", "from-end: the last 2 turns = both");
    // The `-1..` value begins with `-`; allow_hyphen_values must let it through (not be
    // mistaken for a flag). A closed reversal is still a hard error.
    let rev = h.run(&["search", "", t.as_str(), "--turn", "9..3", "-c"]);
    assert!(!rev.success, "a reversed closed range must error");
    // Line axis: `--line -1..` = the last physical jsonl line (the fixture's malformed tail).
    let raw = h.run(&["show", t.as_str(), "--line", "-1..", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(
        raw.stdout.contains("broken json"),
        "last line via -1..: {}",
        raw.stdout
    );
}

#[test]
fn range_grammar_is_n_or_dotdot_everywhere() {
    // ONE range-token grammar across every range flag: bare `N` (≡ N..N) or `START..END`;
    // the removed dash spelling is a HARD error that hands back the correct form.
    let (h, sess, _hex) = show_subagent_home();
    // `show --line A..B` fetches the span.
    let ok = h.run(&["show", &format!("@{sess}"), "--line", "1..2"]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(ok.stdout.contains("go"), "record in span: {}", ok.stdout);
    // The dash form errors and teaches the `..` grammar (no silent compat).
    let dash = h.run(&["show", &format!("@{sess}"), "--line", "1-2"]);
    assert!(
        !dash.success,
        "dash ranges must hard-error: {}",
        dash.stdout
    );
    assert!(
        dash.stderr.contains("START..END"),
        "the error teaches the ..-form: {}",
        dash.stderr
    );
    // `--turn` accepts bare N (≡ N..N).
    let bare = h.run(&["search", "go", &format!("@{sess}"), "--turn", "0"]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains("go"),
        "turn 0 matched via the bare-N shorthand: {}",
        bare.stdout
    );
    // ...and still rejects the dash form with the same teaching error.
    let tdash = h.run(&["search", "go", &format!("@{sess}"), "--turn", "0-1"]);
    assert!(!tdash.success);
    assert!(tdash.stderr.contains("START..END"), "got: {}", tdash.stderr);
}

#[test]
fn turn_range_old_spelling_hard_errors() {
    // v0.5.0 renamed `--turn-range` → `--turn` on every windowing command (zero-BC
    // policy: no alias). The old spelling must be an unknown argument, and clap's
    // similarity tip must point at the new one — the stale-knowledge recovery path.
    let h = populated_home();
    let out = h.run(&["search", "go", ENC, "--turn-range", "0"]);
    assert!(
        !out.success,
        "old spelling must hard-error:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("--turn-range"),
        "names the offending token: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("'--turn'"),
        "the tip names the new spelling: {}",
        out.stderr
    );
}

#[test]
fn timestamps_canonical_local_marker_everywhere() {
    // v0.5 W1-7: every TEXT timestamp is `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)`
    // — name AND offset together (zero conversion arithmetic left to the reader), the
    // raw-UTC parenthetical copy is GONE, and the marker is a FORMAT derived from the
    // system zone per instant, never a hardcoded value.
    let h = populated_home();
    let at_s = at(SESS);
    let tz_syd = [("TZ", "Australia/Sydney")];

    // populated_home's instants are June 2026 → Sydney winter = AEST(UTC+10).
    for cmd in [
        vec!["list", at_s.as_str(), "--no-subagents"],
        vec!["stats", at_s.as_str(), "--no-subagents"],
        vec!["search", "carry", at_s.as_str(), "--no-subagents"],
        vec!["show", at_s.as_str(), "--turn", "0"],
    ] {
        let out = h.run_with_env(&cmd, &tz_syd);
        assert!(out.success, "{cmd:?} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("AEST(UTC+10)"),
            "{cmd:?} missing the canonical marker:\n{}",
            out.stdout
        );
        assert!(
            !out.stdout.contains("Z)"),
            "{cmd:?} must carry no raw-UTC copy:\n{}",
            out.stdout
        );
    }

    // DST correctness: a JANUARY instant under the SAME zone renders AEDT(UTC+11) —
    // the offset is computed per instant, not per process.
    let jan = "77777777-8888-4999-8aaa-bbbbccccdddd";
    h.write(
        &format!("{ENC}/{jan}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"j1","sessionId":"77777777-8888-4999-8aaa-bbbbccccdddd","timestamp":"2026-01-15T05:00:00.000Z","message":{"role":"user","content":"summer question"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"j2","sessionId":"77777777-8888-4999-8aaa-bbbbccccdddd","timestamp":"2026-01-15T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"summer answer"}]}}"#,
            "\n",
        ),
    );
    let out = h.run_with_env(&["list", &format!("@{jan}")], &tz_syd);
    assert!(
        out.stdout.contains("AEDT(UTC+11)"),
        "January in Sydney is AEDT(UTC+11):\n{}",
        out.stdout
    );

    // Non-hardcode proof: the SAME June fixture under an Indian zone renders the
    // fractional, zero-padded form.
    let out = h.run_with_env(
        &["list", at_s.as_str(), "--no-subagents"],
        &[("TZ", "Asia/Kolkata")],
    );
    assert!(
        out.stdout.contains("IST(UTC+05:30)"),
        "Indian zone renders IST(UTC+05:30):\n{}",
        out.stdout
    );
}

#[test]
fn slash_command_wrapper_extracted_in_both_tag_orders() {
    // The slash-command wrapper appears in TWO tag orders in real corpora: OLD
    // (`<command-name>` first) and NEW (`<command-message>` first — current CC).
    // Detection must catch both; the rendered body is `/name args` (never wrapper XML),
    // and a pattern INSIDE the args matches through the literal prefilter + whole-file
    // gate (args are verbatim raw substrings).
    let h = Home::new();
    let sess = "5c5d5e5f-2222-4333-8444-955566677788";
    let body = concat!(
        r#"{"type":"user","uuid":"c1","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args>old order zqxjkvold</command-args>"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a1","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"first reply"}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c2","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"<command-message>csift</command-message>\n<command-name>/csift</command-name>\n<command-args>new order zqxjkvnew</command-args>"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a2","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"second reply"}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c3","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"<command-message>compact</command-message>\n<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
        "\n",
    );
    h.write(
        &format!("-Users-testuser-Projects-slash/{sess}.jsonl"),
        body,
    );
    let at = format!("@{sess}");

    // A literal inside the NEW-order args matches through prefilter/gate; the excerpt is
    // the extracted `/name args` form, never wrapper XML.
    let out = h.run(&["search", "zqxjkvnew", &at]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("/csift new order zqxjkvnew"),
        "extracted render: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("<command-args>"),
        "wrapper XML must not leak: {}",
        out.stdout
    );
    assert!(out.stdout.contains("user.message"), "{}", out.stdout);

    // Same for the OLD order.
    let out = h.run(&["search", "zqxjkvold", &at]);
    assert!(
        out.stdout.contains("/compact old order zqxjkvold"),
        "{}",
        out.stdout
    );

    // `show` renders the same extraction (shared engine).
    let out = h.run(&["show", &at, "--line", "3"]);
    assert!(
        out.stdout.contains("/csift new order zqxjkvnew"),
        "{}",
        out.stdout
    );

    // A NO-ARGS wrapper (either order) is machinery: never user.message, and it must NOT
    // count as a genuine turn opener (all three wrappers fold; no turn boundary shifts).
    let out = h.run(&["search", "", &at, "--count-by", "label", "--format", "json"]);
    let rows = json_rows(&out.stdout, "census");
    let user_msgs = rows
        .iter()
        .find(|r| r["key"] == "user.message")
        .and_then(|r| r["records"].as_u64())
        .unwrap_or(0);
    assert_eq!(user_msgs, 2, "only the two with-args wrappers: {rows:?}");
    let invocations = rows
        .iter()
        .find(|r| r["key"] == "harness.command.invocation")
        .and_then(|r| r["records"].as_u64())
        .unwrap_or(0);
    assert_eq!(invocations, 3, "all three wrappers: {rows:?}");

    // The explicit harness lens still reaches the wrapper form. (`-c` counts EXCHANGES,
    // and no wrapper opens a turn, so all three fold into the single turn-0 lead — the
    // per-RECORD count is the census assertion above.)
    let out = h.run(&["search", "", &at, "-t", "harness.command.invocation", "-c"]);
    assert_eq!(out.stdout.trim(), "1", "{}", out.stdout);
    let out = h.run(&["search", "", &at, "-t", "harness.command.invocation"]);
    assert!(
        out.stdout.contains("harness.command.invocation"),
        "{}",
        out.stdout
    );
}

#[test]
fn at_agent_hex_scopes_to_the_subtree() {
    // `@<agent-hex>` now SCOPES to that subagent (+ its topological descendants, unless
    // --no-subagents), per the rule "locating an agent: itself, or itself + descendants".
    // A realistic >=12-char hex is needed (a <=11-char token is a uuid PREFIX, not an agent).
    let enc = "-Users-testuser-Projects-agentscope";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let hex = "aaa111bbb222ccc33"; // 17 hex, like real agent ids
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Agent","input":{"description":"do it"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaa111bbb222ccc33","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub: the WIDGET work"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub done"}]}}"#, "\n",
        ),
    );

    // `search` within `@<agent-hex>` finds content in THAT subagent transcript.
    let s = h.run(&["search", "WIDGET", &format!("@{hex}")]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains(hex),
        "scoped to the subagent: {}",
        s.stdout
    );
    assert!(
        s.stdout.contains("subagent"),
        "branded as a subagent: {}",
        s.stdout
    );

    // A NON-EXISTENT agent hex → honest "no subagent found" (not a session error).
    let miss = h.run(&["search", "x", "@deadbeefdeadbeef0"]);
    assert!(!miss.success);
    assert!(
        miss.stderr.contains("no subagent") && miss.stderr.contains("agents"),
        "guides to agents listing: {}",
        miss.stderr
    );
}

#[test]
fn legacy_flat_selector_error_names_the_successor() {
    let h = populated_home();
    for (legacy, successor) in [
        ("thinking", "agent.thinking"),
        ("tool", "agent.tool"),
        ("tool-response", "agent.tool.result"),
    ] {
        let out = h.run(&["search", "x", &at(SESS), "-t", legacy]);
        assert!(!out.success, "legacy selector must still hard-error");
        assert!(
            out.stderr.contains("pre-v0.5") && out.stderr.contains(successor),
            "'{legacy}' should point at '{successor}': {}",
            out.stderr
        );
    }
}

#[test]
fn time_window_bare_datetime_is_local_wall_clock_not_midnight() {
    // R9 §18a: jiff's civil-Date parser accepts a full datetime string (keeping only the
    // date part), so `--since "…T20:00:00"` (bare, no offset) silently collapsed to local
    // MIDNIGHT — a bounded window that read exactly like a quiet time period. Bare
    // datetimes are now system-LOCAL wall-clock time (the bare-date convention extended).
    let h = Home::new();
    let enc = "-Users-test-Projects-tw";
    let sess = "cccccccc-dddd-4eee-8fff-000000000000";
    // Two genuine user turns: 05:00Z (=15:00 AEST) and 09:00Z (=19:00 AEST).
    let body = concat!(
        r#"{"type":"user","uuid":"u1","sessionId":"cccccccc-dddd-4eee-8fff-000000000000","cwd":"/Users/test/Projects/tw","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"afternoon message"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply one"}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"u2","sessionId":"cccccccc-dddd-4eee-8fff-000000000000","timestamp":"2026-06-07T09:00:00.000Z","message":{"role":"user","content":"evening message"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T09:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply two"}]}}"#,
        "\n",
    );
    h.write(&format!("{enc}/{sess}.jsonl"), body);
    let tz = [("TZ", "Australia/Sydney")];
    let count = |since: &str| -> String {
        let out = h.run_with_env(
            &["search", "", &format!("@{sess}"), "--since", since, "-c"],
            &tz,
        );
        assert!(out.success, "since={since} stderr: {}", out.stderr);
        out.stdout.trim().to_string()
    };
    // Bare date = local midnight → both turns.
    assert_eq!(count("2026-06-07"), "2");
    // Bare datetime 16:00 AEST sits between the two (15:00 / 19:00 AEST) → exactly 1.
    // Under the old midnight-collapse this returned 2, identically to the bare date.
    assert_eq!(count("2026-06-07T16:00:00"), "1");
    // And a bare datetime PAST both → 0 (three distinct answers ⇒ time-of-day honored).
    assert_eq!(count("2026-06-07T20:00:00"), "0");
    // A malformed offset must still fail loud, never be re-read as local wall-clock.
    let bad = h.run_with_env(
        &[
            "search",
            "",
            &format!("@{sess}"),
            "--since",
            "2026-06-07T16:00:00+99:00",
        ],
        &tz,
    );
    assert!(!bad.success, "malformed offset must hard-error");
}

#[test]
fn malformed_non_candidate_lines_are_counted_never_invisible() {
    // R10: a syntactically-invalid line carries no role marker, so the §7 byte prefilter
    // routed it to the silent Ignore branch — `skipped_lines` reported 0 on a corrupted
    // file, indistinguishable from a clean one (the exact failure the malformed law
    // exists to rule out). The O(1) shape check now counts the two realistic corruption
    // shapes: free-text garbage (no leading '{') and crash-truncation (no trailing '}').
    let h = Home::new();
    let enc = "-Users-test-Projects-corrupt";
    let sess = "dddddddd-eeee-4fff-8000-111111111111";
    let body = concat!(
        r#"{"type":"user","uuid":"u1","sessionId":"dddddddd-eeee-4fff-8000-111111111111","cwd":"/Users/test/Projects/corrupt","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"only real record"}}"#,
        "\n",
        "THIS IS COMPLETE GARBAGE NOT JSON AT ALL !!!",
        "\n",
        // Crash-truncated mid-string: brace-opened, never closed. It CARRIES a role
        // marker, so it exercises the candidate parse-failure path (already counted
        // pre-R10) while the garbage line above exercises the new shape path.
        r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"te"#,
        "\n",
        "\n", // blank — NOT malformed, never counted
    );
    h.write(&format!("{enc}/{sess}.jsonl"), body);
    let at = format!("@{sess}");
    for args in [
        vec!["search", "", at.as_str(), "--no-subagents"],
        vec!["list", at.as_str(), "--no-subagents"],
        vec!["show", at.as_str(), "--turn", ".."],
        vec!["stats", at.as_str(), "--no-subagents"],
    ] {
        let mut a = args.clone();
        a.extend(["--format", "json"]);
        let out = h.run(&a);
        assert!(out.success, "{args:?} stderr: {}", out.stderr);
        assert_eq!(
            json_summary(&out.stdout)["skipped_lines"],
            2,
            "{args:?} must count BOTH corrupt lines: {}",
            out.stdout
        );
    }
    // Text mode surfaces the shared malformed note.
    let t = h.run(&["search", "", &at, "--no-subagents"]);
    assert!(
        format!("{}{}", t.stdout, t.stderr).contains("2 malformed line(s) skipped"),
        "text note missing: {} ||| {}",
        t.stdout,
        t.stderr
    );
}
