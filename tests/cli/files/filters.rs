//! files windows and path filters: turn/time ranges, regex/glob, targets, errors.

use crate::harness::*;

#[test]
fn files_bare_uuid_positional_routes_to_session() {
    // The documented `csift files <uuid>` form (a bare uuid in the positional slot) now
    // resolves as a session filter across all projects, not as a (nonexistent) project
    // dir. Previously errored "no Claude Code project dir for …/<uuid>".
    let h = populated_home();
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "summary",
        "--no-subagents",
    ]);
    // Routing success = the command resolved the session and ran (exit 0), NOT the old
    // "no Claude Code project dir for …/<uuid>" hard error.
    assert!(
        out.success,
        "bare-uuid positional must resolve as a session; stderr: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("no Claude Code project dir"),
        "a bare uuid must NOT be encoded as a project dir; stderr: {}",
        out.stderr
    );
    // It ran the `files` summary over the real session (the synthetic top-level has no
    // Bash/Edit mutation, so the body is the honest empty rollup - the point is it ran).
    assert!(
        out.stdout.contains("detail=summary"),
        "files summary did not run; got: {}",
        out.stdout
    );
}

#[test]
fn files_turn_range_excludes_later_bash() {
    // --turn 0..0 keeps the turn-0 structured edits and DROPS the turn-1 bash rm.
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--turn", "0..0"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("turn=0..0"));
    // 5 mutations remain (2 writes + 3 edits), not 6 (the bash rm is in turn 1).
    assert!(
        out.stdout.contains("5 mutation(s)"),
        "turn 1 bash excluded: {}",
        out.stdout
    );
}

#[test]
fn files_turn_range_and_since_intersect() {
    // The ONE windowing rule: `--turn` and `--since`/`--until` AND together (the
    // former mutual-exclusion bail was a leftover - search/recover/stats always intersected).
    let h = files_scenario_home();
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--turn",
        "0..1",
        "--since",
        "2h",
    ]);
    assert!(
        out.success,
        "combined windows intersect, never error: {}",
        out.stderr
    );
    // The fixture's mutations are from 2026 - a `--since 2h` window admits nothing, and the
    // intersection propagates that honestly (exit 0).
    assert!(
        out.stdout.contains("no file mutations found") || !out.stdout.contains("L0"),
        "the intersected window filters: {}",
        out.stdout
    );
}

#[test]
fn files_since_window_keeps_only_later_mutations() {
    // A window starting at 06:00 drops all turn-0 structured edits (05:00) and keeps
    // only the turn-1 bash rm (06:00).
    let h = files_scenario_home();
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T06:00:00Z",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("1 mutation(s)"), "got: {}", out.stdout);
    assert!(out.stdout.contains("bash (heuristic)"));
}

#[test]
fn files_no_mutations_says_none() {
    // A session with a genuine user turn but no file-mutating tool use.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"just chatting"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","message":{"role":"assistant","content":[{"type":"text","text":"sure"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no file mutations found"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn files_detects_edit_before_read_boundaries() {
    // A session Writes /p/app.rs, then an Edit to it is rejected with `File has been modified
    // since read` (the file changed outside the tool stream). `files` surfaces that as an
    // Edit-before-Read boundary attributed to the file, carrying the jsonl line number - and
    // every row (mutation + boundary) now carries `Lnnnn` (the line-number threading fix).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/app.rs","content":"line\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/app.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/p/app.rs","old_string":"line","new_string":"LINE"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
        ),
    );

    // Text: timeline mutation rows carry Lnnnn; the boundary section names the file + kind + line.
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--by",
        "timeline",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("Edit-before-Read boundaries"),
        "boundary section present: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("/p/app.rs") && out.stdout.contains("modified_since_read"),
        "boundary attributed to the file with its kind: {}",
        out.stdout
    );
    // The failed edit (ed1) is NOT counted as a mutation; the Write IS, and its timeline row
    // carries the jsonl line. Footer reports the boundary count.
    assert!(
        out.stdout.contains("1 Edit-before-Read boundary(ies)"),
        "footer boundary count: {}",
        out.stdout
    );

    // JSON: a typed boundary object with line_no + the summary count.
    let j = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson parses"))
        .collect();
    let b = objs
        .iter()
        .find(|o| o.get("kind").and_then(|t| t.as_str()) == Some("boundary"))
        .expect("a boundary object");
    assert_eq!(b["path"], "/p/app.rs");
    assert_eq!(b["cause"], "modified_since_read");
    assert!(
        b["line"].as_u64().unwrap_or(0) >= 1,
        "boundary carries its jsonl line: {b}"
    );
    let summary = objs
        .iter()
        .find(|o| o.get("detail_level").is_some())
        .expect("trailing summary");
    assert_eq!(summary["edit_before_read_boundaries"], serde_json::json!(1));
}

#[test]
fn files_regex_filters_full_path() {
    let h = Home::new();
    path_filter_scenario(&h);
    // --regex '\.rs$' keeps ONLY the .rs path (and its boundary), drops .md + .txt.
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--by",
        "file",
        "--regex",
        r"\.rs$",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("src/lib.rs"),
        "kept .rs: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("readme.md") && !out.stdout.contains("notes.txt"),
        "regex must drop non-.rs paths: {}",
        out.stdout
    );
    // The boundary (on the .rs file) survives the same predicate.
    assert!(
        out.stdout.contains("Edit-before-Read boundaries"),
        "the .rs boundary must survive the filter: {}",
        out.stdout
    );
}

#[test]
fn files_glob_filters_full_path() {
    let h = Home::new();
    path_filter_scenario(&h);
    // --glob '**/*.md' keeps ONLY the .md path; .rs + .txt (and the .rs boundary) drop out.
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--by",
        "file",
        "--glob",
        "**/*.md",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("readme.md"), "kept .md: {}", out.stdout);
    assert!(
        !out.stdout.contains("lib.rs") && !out.stdout.contains("notes.txt"),
        "glob must drop non-.md paths: {}",
        out.stdout
    );
    // The boundary is on the .rs file, which the glob filters out → no boundary section.
    assert!(
        !out.stdout.contains("Edit-before-Read boundaries"),
        "a filtered-out boundary must not show: {}",
        out.stdout
    );
}

#[test]
fn files_regex_and_glob_combine_as_and() {
    let h = Home::new();
    path_filter_scenario(&h);
    // Both filters AND: under a src/ dir AND ending in .rs → only lib.rs.
    let both = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--by",
        "file",
        "--glob",
        "**/src/**",
        "--regex",
        r"\.rs$",
    ]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert!(both.stdout.contains("src/lib.rs"), "got: {}", both.stdout);
    assert!(
        !both.stdout.contains("readme.md") && !both.stdout.contains("notes.txt"),
        "AND of glob+regex: {}",
        both.stdout
    );
    // A glob that matches src/ but a regex that excludes .rs → empty (the normal empty output).
    let empty = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--by",
        "file",
        "--glob",
        "**/src/**",
        "--regex",
        r"\.md$",
    ]);
    assert!(empty.success, "stderr: {}", empty.stderr);
    assert!(
        empty.stdout.contains("no file mutations found"),
        "an empty filtered set yields the empty output: {}",
        empty.stdout
    );
}

#[test]
fn files_invalid_regex_and_glob_are_hard_errors() {
    let h = Home::new();
    path_filter_scenario(&h);
    // Invalid regex → hard error (unbalanced paren).
    let bad_re = h.run(&["files", at(SESS).as_str(), "--regex", "("]);
    assert!(
        !bad_re.success,
        "invalid regex must fail: {}",
        bad_re.stdout
    );
    assert!(
        bad_re.stderr.contains("invalid --regex"),
        "regex error names the flag: {}",
        bad_re.stderr
    );
    // Invalid glob → hard error (unterminated `[` class).
    let bad_glob = h.run(&["files", at(SESS).as_str(), "--glob", "[abc"]);
    assert!(
        !bad_glob.success,
        "invalid glob must fail: {}",
        bad_glob.stdout
    );
    assert!(
        bad_glob.stderr.contains("invalid --glob"),
        "glob error names the flag: {}",
        bad_glob.stderr
    );
}

#[test]
fn files_detects_new_bash_idioms_end_to_end() {
    // The previously-MISSED idiom classes (fd-redirects, curl -o, --junit-xml=, dd of=,
    // zip) reach the real CLI surface and surface their /tmp destinations; the noisy
    // precision cases ($VAR, /dev/null) are dropped.
    let h = Home::new();
    let cmds = [
        ("pytest 2>/tmp/err.log", "/tmp/err.log"),
        ("make 1> /tmp/out.log", "/tmp/out.log"),
        ("svc &>/tmp/all.log", "/tmp/all.log"),
        ("curl https://x -o /tmp/dl.json", "/tmp/dl.json"),
        ("wget -O /tmp/w.bin https://y", "/tmp/w.bin"),
        ("pytest --junit-xml=/tmp/r.xml", "/tmp/r.xml"),
        ("dd if=/dev/zero of=/tmp/d.bin", "/tmp/d.bin"),
        ("zip /tmp/a.zip f1 f2", "/tmp/a.zip"),
    ];
    let mut lines = String::new();
    lines.push_str(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
    );
    lines.push('\n');
    for (n, (cmd, _)) in cmds.iter().enumerate() {
        lines.push_str(&format!(
            r#"{{"type":"assistant","uuid":"a{n}","timestamp":"2026-06-07T05:00:0{n}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b{n}","name":"Bash","input":{{"command":{}}}}}]}}}}"#,
            serde_json_string(cmd)
        ));
        lines.push('\n');
    }
    // A noisy command whose targets must be DROPPED (var + /dev/null sink).
    lines.push_str(
        r#"{"type":"assistant","uuid":"az","timestamp":"2026-06-07T05:00:09.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"bz","name":"Bash","input":{"command":"noisy 2>/dev/null > $OUT"}}]}}"#,
    );
    lines.push('\n');
    h.write(&format!("{ENC}/{SESS}.jsonl"), &lines);

    let out = h.run(&["files", at(SESS).as_str(), "--by", "file", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for (cmd, want) in cmds {
        assert!(
            out.stdout.contains(want),
            "idiom {cmd:?} should surface {want}: {}",
            out.stdout
        );
    }
    // Precision: the dropped pseudo-paths never appear.
    assert!(!out.stdout.contains("/dev/null"), "got: {}", out.stdout);
    assert!(!out.stdout.contains("$OUT"), "got: {}", out.stdout);
}

#[test]
fn files_unknown_session_errors() {
    let h = files_scenario_home();
    let out = h.run(&["files", at("00000000-0000-0000-0000-000000000000").as_str()]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn files_via_project_path_target() {
    let h = files_scenario_home();
    let out = h.run(&["files", ENC, "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("/tmp: 2 write"));
}
