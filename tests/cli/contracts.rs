//! Cross-command output contracts: range grammar, timestamps, honest empties, tolerant parsing.

use crate::harness::*;

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
    // similarity tip must point at the new one - the stale-knowledge recovery path.
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
    // - name AND offset together (zero conversion arithmetic left to the reader), the
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

    // DST correctness: a JANUARY instant under the SAME zone renders AEDT(UTC+11) -
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
fn time_window_bare_datetime_is_local_wall_clock_not_midnight() {
    // R9 §18a: jiff's civil-Date parser accepts a full datetime string (keeping only the
    // date part), so `--since "…T20:00:00"` (bare, no offset) silently collapsed to local
    // MIDNIGHT - a bounded window that read exactly like a quiet time period. Bare
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
    // routed it to the silent Ignore branch - `skipped_lines` reported 0 on a corrupted
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
        "\n", // blank - NOT malformed, never counted
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

#[test]
fn reserialized_spaced_json_records_are_full_citizens() {
    // R13: a valid-JSON record whose serialization differs from CC's compact wire
    // format by one space (`"role": "user"` - python json.dumps defaults, a jq /
    // editor round-trip) used to vanish one layer BEFORE any malformed counter
    // could see it: no preview, no record count, no search match, skipped_lines 0 -
    // invisible on every surface with zero disclosure. Stage-1 candidate detection
    // is now serialization-tolerant (`parse::line_has_role_marker`), so such
    // records are full citizens everywhere.
    let h = Home::new();
    let enc = "-Users-test-Projects-spaced";
    let sess = "eeeeeeee-aaaa-4bbb-8ccc-00000005aced";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type": "user", "uuid": "u1", "timestamp": "2026-06-07T05:00:00.000Z", "message": {"role": "user", "content": "SPACED_ALPHA question"}}"#,
            "\n",
            r#"{"type": "assistant", "uuid": "a1", "timestamp": "2026-06-07T05:00:01.000Z", "message": {"role": "assistant", "content": [{"type": "text", "text": "SPACED_BETA answer"}]}}"#,
            "\n",
        ),
    );
    let at = format!("@{sess}");
    let s = h.run(&["search", "SPACED_BETA", &at, "--no-subagents", "-c"]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert_eq!(
        s.stdout.trim(),
        "1",
        "a spaced record must match: {}",
        s.stdout
    );
    let st = h.run(&["stats", &at, "--no-subagents", "--format", "json"]);
    assert!(st.success, "stderr: {}", st.stderr);
    let row = &json_rows(&st.stdout, "session")[0];
    assert_eq!(row["user_records"], 1, "{}", st.stdout);
    assert_eq!(row["assistant_records"], 1, "{}", st.stdout);
    let l = h.run(&["list", &at, "--no-subagents", "--format", "json"]);
    assert!(l.success, "stderr: {}", l.stderr);
    let lr = &json_rows(&l.stdout, "session")[0];
    assert!(
        lr["first_user"]["excerpt"]
            .as_str()
            .is_some_and(|e| e.contains("SPACED_ALPHA")),
        "{}",
        l.stdout
    );
    assert!(
        lr["last_agent"]["excerpt"]
            .as_str()
            .is_some_and(|e| e.contains("SPACED_BETA")),
        "{}",
        l.stdout
    );
    assert_eq!(json_summary(&l.stdout)["skipped_lines"], 0, "{}", l.stdout);
}

#[test]
fn clean_run_notes_are_absent_in_terminal_modes() {
    // Mutation pin (duals of the `> 0` note gates): on a clean, uncapped run the -l /
    // --count-by / --raw surfaces must print NO zero-count accounting notes.
    let h = Home::new();
    let _ = header_collision_scenario(&h); // clean, three sessions, no caps in play
    let l = h.run(&["search", "SEEDWORD", "-l"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert!(
        !l.stderr.contains("note:"),
        "-l prints no notes on a clean uncapped run: {}",
        l.stderr
    );
    let cb = h.run(&["search", "SEEDWORD", "--count-by", "label"]);
    assert!(cb.success, "stderr: {}", cb.stderr);
    assert!(
        !cb.stderr.contains("dropped") && !cb.stderr.contains("malformed"),
        "--count-by prints no drop/malformed notes on a clean run: {}",
        cb.stderr
    );
    let raw = h.run(&["search", "SEEDWORD", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(
        !raw.stderr.contains("note:"),
        "--raw prints no notes on a clean uncapped run: {}",
        raw.stderr
    );
}

/// CRITICAL data-safety: an empty reconstruction (no recoverable history / over-budget) must
/// NOT clobber the `--out` destination and must NOT print a false `(wrote …)` line. Covers
/// recover --patches, recover --at, and turns over-budget.
#[test]
fn empty_out_never_clobbers_or_lies() {
    let h = populated_home();
    let scratch = h.root.join("precious.md");
    let seed = "PRECIOUS USER CONTENT\n";

    // recover --patches on a non-existent file → empty → must leave the file untouched.
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/tmp/no_such_file_xyz.md",
        "--patches",
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("wrote concatenated patches"),
        "false write line:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("left untouched"),
        "missing untouched note:\n{}",
        out.stderr
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).unwrap(),
        seed,
        "recover --patches clobbered --out"
    );

    // recover --at on a non-existent file → empty → untouched.
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/tmp/no_such_file_xyz.md",
        "--at",
        "1w",
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("wrote partial snapshot"),
        "false write line:\n{}",
        out.stdout
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).unwrap(),
        seed,
        "recover --at clobbered --out"
    );

    // turns with an impossibly small budget → nothing rendered → untouched + no false write.
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--budget",
        "5",
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("wrote full reconstruction"),
        "false write line:\n{}",
        out.stdout
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).unwrap(),
        seed,
        "turns clobbered --out"
    );

    // CONTROL: a NON-empty reconstruction DOES write (guard is not over-eager).
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote full reconstruction"),
        "real write missing:\n{}",
        out.stdout
    );
    let written = std::fs::read_to_string(&scratch).unwrap();
    assert_ne!(
        written, seed,
        "a non-empty turns reconstruction must overwrite --out"
    );
    assert!(!written.is_empty(), "written artifact is empty");
}
