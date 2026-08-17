use crate::harness::*;

#[test]
fn list_text_renders_sessions_and_subagents() {
    let h = populated_home();
    let out = h.run(&["list"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("SESSION"),
        "no session header:\n{}",
        out.stdout
    );
    assert!(out.stdout.contains(SESS), "session id missing");
    // Identity meta line: branch + CC version + decoded cwd.
    assert!(out.stdout.contains("branch main"));
    assert!(out.stdout.contains("CC 2.1.0"));
    assert!(out.stdout.contains("/Users/testuser/Projects/foo"));
    // First/last previews are present.
    assert!(out.stdout.contains("why is the carry needed?"));
    // The malformed line is surfaced, never hidden.
    assert!(out.stdout.contains("malformed line(s) skipped"));
    // Subagents are spanned by default (the built-in sub's content shows up).
    assert!(out.stdout.contains("sub:") || out.stdout.contains("wf task"));
}

#[test]
fn list_json_is_one_object_per_session() {
    let h = populated_home();
    let out = h.run(&["list", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // A leading {kind:"header", …} scope record precedes the per-session objects
    // whenever the set spans ≥1 subagent (uniform JSON scope disclosure, same as turns).
    // Every OTHER non-empty line must be a JSON object with a session_id.
    let mut count = 0;
    let mut saw_header = false;
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("header") => {
                saw_header = true;
                // The header discloses the span with the shared scope field names.
                assert!(v.get("sessions_in_scope").is_some(), "header span: {line}");
                assert!(v.get("top_level_sessions").is_some(), "header span: {line}");
                assert!(v.get("subagent_sessions").is_some(), "header span: {line}");
                continue;
            }
            Some("summary") => continue,
            _ => {}
        }
        assert!(v.get("session_id").is_some(), "missing session_id: {line}");
        count += 1;
    }
    assert!(count >= 1, "expected at least the top-level session");
    // populated_home spans a subagent, so the header is emitted.
    assert!(
        saw_header,
        "expected a leading session_header in spanning list JSON"
    );
}

#[test]
fn list_no_subagents_restricts_to_top_level() {
    let h = populated_home();
    let with = h.run(&["list", "--format", "json"]);
    let without = h.run(&["list", "--no-subagents", "--format", "json"]);
    let count = |s: &str| s.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(
        count(&with.stdout) > count(&without.stdout),
        "subagents should add rows: with={} without={}",
        count(&with.stdout),
        count(&without.stdout)
    );
}

#[test]
fn list_empty_projects_says_no_sessions() {
    let h = Home::new(); // empty projects root
    let out = h.run(&["list"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no sessions found"),
        "got: {}",
        out.stdout
    );
}

#[test]
#[cfg(unix)]
fn list_follows_symlinked_project_dir() {
    // A project dir that is a SYMLINK to a real dir holding a session → exercises
    // all_project_dirs' `ft.is_symlink()` arm (it must `is_dir()`-resolve the link
    // and still list its sessions).
    use std::os::unix::fs::symlink;
    let h = populated_home();
    // Create a real dir elsewhere with a session, then symlink it into projects.
    let real = h.root.join("real-project");
    std::fs::create_dir_all(&real).unwrap();
    let other_sess = "cccccccc-0000-0000-0000-000000000003";
    std::fs::write(
        real.join(format!("{other_sess}.jsonl")),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"via symlink\"}}\n",
    )
    .unwrap();
    let link = h.projects().join("-Symlinked-Project");
    symlink(&real, &link).unwrap();
    let out = h.run(&["list"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(other_sess),
        "symlinked session listed: {}",
        out.stdout
    );
}

#[test]
fn list_real_path_target_is_encoded_and_resolved() {
    let h = populated_home();
    // A real cwd that encodes to the ENC dir we created. The leading `/` means
    // `s.contains('/')` is true, so `strip_projects_root_prefix`'s bare-token check
    // short-circuits and the arg is treated as a real path (the `!s.contains('/')`
    // false arm).
    let out = h.run(&["list", "/Users/testuser/Projects/foo"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
}

#[test]
fn list_ignores_stray_non_dir_entries_in_projects_root() {
    // A regular FILE sitting directly in `~/.claude/projects` must be ignored by
    // all_project_dirs (the `if is_dir` FALSE arm). The real project dir still lists.
    let h = populated_home();
    std::fs::write(h.projects().join("stray-file.txt"), "not a project dir").unwrap();
    let out = h.run(&["list"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "real project still listed");
}

#[test]
fn list_unknown_target_errors_nonzero() {
    let h = populated_home();
    let out = h.run(&["list", "/no/such/project/path/anywhere"]);
    assert!(!out.success, "an unresolvable target must exit nonzero");
    assert!(
        out.stderr.contains("csift: error:"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn list_bare_uuid_positional_routes_to_session() {
    // The scope-unification win: `csift list <uuid>` now identifies THAT one session via
    // the shared resolver (it previously encoded the uuid as a project dir and errored).
    let h = populated_home();
    let out = h.run(&["list", at(SESS).as_str()]);
    assert!(
        out.success,
        "bare-uuid positional must resolve via the shared resolver; stderr: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("no Claude Code project dir"),
        "a bare uuid must NOT be encoded as a project dir; stderr: {}",
        out.stderr
    );
    assert!(out.stdout.contains(SESS), "the session row is missing");
}

#[test]
fn list_session_without_branch_or_version_prints_cwd_only() {
    // A session whose first user record carries cwd but NEITHER gitBranch NOR version
    // → the meta string stays empty, so the render takes the `meta.is_empty()` TRUE
    // path (a plain `cwd` line with no `(...)` suffix). Exercises session.rs L283/L278.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"cwd\":\"/Users/testuser/Projects/foo\",\"message\":{\"role\":\"user\",\"content\":\"only cwd here\"}}\n",
    );
    let out = h.run(&["list", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("/Users/testuser/Projects/foo"));
    // No branch/version suffix.
    assert!(
        !out.stdout.contains("branch "),
        "no branch meta: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("CC "),
        "no version meta: {}",
        out.stdout
    );
}

#[test]
fn list_session_with_only_version_no_branch() {
    // version present, gitBranch absent → meta starts EMPTY at the branch check
    // (skips it), then becomes non-empty at the version check → the `(CC x)` suffix
    // without a leading branch. Exercises the version-with-empty-meta join arm.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"cwd\":\"/c\",\"version\":\"2.5.0\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
    );
    let out = h.run(&["list", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("CC 2.5.0"));
    assert!(!out.stdout.contains("branch "), "no branch: {}", out.stdout);
}

#[test]
fn list_window_admits_by_span_intersection() {
    // A session whose [first, last] span STRADDLES the window is still active in it — the
    // span-intersect rule, not a point rule (no single record needs to fall inside).
    let h = Home::new();
    let enc = "-Users-testuser-Projects-windowy";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"early bird"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-12-31T00:00:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"late reply"}]}}"#, "\n",
        ),
    );
    let straddle = h.run(&[
        "list",
        enc,
        "--since",
        "2026-06-01",
        "--until",
        "2026-06-02",
    ]);
    assert!(straddle.success, "stderr: {}", straddle.stderr);
    assert!(
        straddle.stdout.contains(sess),
        "a straddling session intersects the window: {}",
        straddle.stdout
    );
    // A window entirely OUTSIDE the span excludes the session.
    let outside = h.run(&["list", enc, "--since", "2027-01-01"]);
    assert!(outside.success, "stderr: {}", outside.stderr);
    assert!(
        !outside.stdout.contains(sess),
        "a disjoint window excludes: {}",
        outside.stdout
    );
}

#[test]
fn verbatim_no_compaction_note_and_list_sidecar_tristate() {
    // W2-8: `verbatim` on a session with ZERO compactions self-diagnoses (stderr) and
    // points at `show --turn` — the tail-peek misuse correction; --slice (the hook path)
    // stays quiet. W2-9: list rows carry the sidecar TRI-STATE (`sidecar_present`).
    let h = populated_home();
    let at = format!("@{SESS}");

    let out = h.run(&["verbatim", &at]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("has no compaction"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("csift show @"),
        "the note names the correct command: {}",
        out.stderr
    );

    let out = h.run(&["verbatim", &at, "--slice", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("has no compaction"),
        "--slice must stay quiet (hook path): {}",
        out.stderr
    );

    // Tri-state ①: no sidecar file → present:false (hook unknown — cannot conclude).
    let out = h.run(&["list", &at, "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert!(
        rows.iter().all(|r| r["sidecar_present"] == false),
        "{rows:?}"
    );

    // Tri-state ②: a sidecar with only a RESOLVED pair (nothing pending) → present:true,
    // with_elicitation_sidecar:false — "hook installed AND not blocked" is now assertable.
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        concat!(
            r#"{"type":"csift-elicitation-resolved","uuid":"r1","timestamp":"2026-06-07T05:00:10.000Z","#,
            r#""sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","csift":"elicitation-marker-v1","#,
            r#""csiftPhase":"resolved","csiftKind":"AskUserQuestion","csiftKey":"k1"}"#,
            "\n",
        ),
    );
    let out = h.run(&["list", &at, "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    let top: Vec<_> = rows.iter().filter(|r| r["is_subagent"] == false).collect();
    assert!(!top.is_empty());
    assert!(top.iter().all(|r| r["sidecar_present"] == true), "{rows:?}");
    assert!(
        top.iter().all(|r| r["with_elicitation_sidecar"] == false),
        "resolved-only sidecar has nothing pending: {rows:?}"
    );
}

#[test]
fn list_renders_clean_automation_and_inbound_previews() {
    // #14: `list`'s first/last previews must render a `<task-notification>` as its automation
    // attribution label and an inbound `<teammate-message>` as a clean inbound-comm line — never
    // the raw XML blobs they used to dump under `first ◂` / `last ◂`.
    let h = Home::new();
    let sess = "cccccccc-dddd-eeee-ffff-000000000000";
    let lines = [
        // first user record = an inbound teammate message (this session is a teammate; the lead
        // addresses it) → clean inbound-comm preview.
        r#"{"type":"user","uuid":"tm0","sessionId":"cccccccc-dddd-eeee-ffff-000000000000","cwd":"/Users/x/list","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\">repro the zzslider bug\n</teammate-message>\n\nThis came from another Claude session — not typed by your user."}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"tm0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#,
        // last user record = a task-notification automation pulse → clean attribution label.
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:20:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf99zzz</task-id>\n<output-file>/tmp/wf99zzz.output</output-file>\n<status>completed</status>\n<summary>Background command \"zzbuild step\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
    ];
    h.write(
        &format!("-Users-x-list/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    let l = h.run(&["list", at(sess).as_str(), "--no-subagents"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert!(
        l.stdout
            .contains("agent.communication.inbox  team-lead ⇨ self  repro the zzslider bug"),
        "first ◂ must render the clean inbound-comm preview; got: {}",
        l.stdout
    );
    assert!(
        l.stdout.contains("[background-command wf99zzz completed]"),
        "last ◂ must render the automation attribution label; got: {}",
        l.stdout
    );
    assert!(
        !l.stdout.contains("<teammate-message")
            && !l.stdout.contains("<task-notification>")
            && !l.stdout.contains("<output-file>"),
        "no raw XML wrapper may appear in list previews; got: {}",
        l.stdout
    );
}

#[test]
fn list_all_garbage_counts_each_line_once_not_twice() {
    // R12 §1.4: the head scan and the tail scan each walked the whole file (nothing
    // genuine to stop at) and each booked the same malformed lines — an all-garbage
    // file reported exactly 2× at every size. The tail scan now floors at the head
    // scan's consumed-end offset, so the two windows are disjoint.
    let h = Home::new();
    let enc = "-Users-test-Projects-garbage";
    let sess = "eeeeeeee-aaaa-4bbb-8ccc-000000000005";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        "GARBAGE 1\nGARBAGE 2\nGARBAGE 3\nGARBAGE 4\nGARBAGE 5\n",
    );
    let at = format!("@{sess}");
    let out = h.run(&["list", &at, "--no-subagents", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        json_summary(&out.stdout)["skipped_lines"],
        5,
        "each malformed line booked exactly once: {}",
        out.stdout
    );
    // Text mode: the note scope-qualifies the number (window census, not a whole-file
    // verdict) and routes the census question to stats.
    let t = h.run(&["list", &at, "--no-subagents"]);
    assert!(
        t.stdout
            .contains("5 malformed line(s) skipped (among the head/tail lines read"),
        "scope-qualified note missing: {}",
        t.stdout
    );
}

#[test]
fn agents_text_lists_lifecycle_rows() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"));
    assert!(out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("workflow"));
    assert!(out.stdout.contains("completed"));
    assert!(out.stdout.contains("started"));
    assert!(out.stdout.contains("duration"));
    assert!(out.stdout.contains("subagent(s)"));
    // The built-in carries a description line.
    assert!(out.stdout.contains("run the carry task"));
}

#[test]
fn agents_with_files_renders_changed_list_and_summary_json() {
    // A subagent that ACTUALLY changed a file → the `--with-files` text path renders the
    // `files N changed` + per-file create/op tag lines (vs the `(none)` arm), and
    // `--order-by start` exercises the start-axis label. Also covers files `--summary
    // --format json`.
    let h = Home::new();
    let sess = "33333333-4444-5555-6666-777777777777";
    h.write(
        &format!("-Users-x-w/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","cwd":"/Users/x/w","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"tk","name":"Task","input":{"description":"sub task"}}]}}"#, "\n",
        ),
    );
    // A subagent transcript that Writes a new file (so files_changed is non-empty).
    h.write(
        &format!("-Users-x-w/{sess}/subagents/agent-fff999.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"fff999","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub: write the file"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/x/w/new.rs","content":"x"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sc","parentUuid":"sa","timestamp":"2026-06-07T05:00:04.000Z","toolUseResult":{"type":"create","filePath":"/Users/x/w/new.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("-Users-x-w/{sess}/subagents/agent-fff999.meta.json"),
        r#"{"agentType":"executor","toolUseId":"tk"}"#,
    );
    let out = h.run(&[
        "agents",
        at(sess).as_str(),
        "--with-files",
        "--order-by",
        "start",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("changed") && out.stdout.contains("new.rs"),
        "files-changed list not rendered: {}",
        out.stdout
    );
    // The summary JSON path (the `json_grouped` summary arm + trailing summary object).
    let f = h.run(&[
        "files",
        at(sess).as_str(),
        "--by",
        "summary",
        "--format",
        "json",
    ]);
    assert!(f.success, "stderr: {}", f.stderr);
    let last = f
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(
        v.get("detail_level").and_then(|d| d.as_str()),
        Some("summary")
    );
}

#[test]
fn turns_help_lists_the_subcommand_and_flags() {
    let h = turns_home();
    let top = h.run(&["--help"]);
    assert!(
        top.stdout.contains("verbatim"),
        "top help lists the verbatim command: {}",
        top.stdout
    );
    let sub = h.run(&["verbatim", "--help"]);
    assert!(sub.stdout.contains("--budget"), "{}", sub.stdout);
    assert!(
        sub.stdout.contains("--round-trip-fraction"),
        "{}",
        sub.stdout
    );
    assert!(sub.stdout.contains("--max-compactions"), "{}", sub.stdout);
    assert!(
        !sub.stdout.contains("--budget-unit"),
        "budget is chars-only now: {}",
        sub.stdout
    );
}
