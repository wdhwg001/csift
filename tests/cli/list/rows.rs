//! list rows, previews, spanning, caps, window census.

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
    // On Windows the same arg resolves drive-relative and encodes with the drive
    // letter, so the expected project dir carries the `C-` head there. The twin
    // session carries no `cwd`, which the collision guard keeps.
    #[cfg(windows)]
    h.write(
        &format!("C-{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"windows twin\"}}\n",
    );
    let out = h.run(&["list", "/Users/testuser/Projects/foo"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "stdout: {}", out.stdout);
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
    // A session whose [first, last] span STRADDLES the window is still active in it - the
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
fn list_renders_clean_automation_and_inbound_previews() {
    // #14: `list`'s first/last previews must render a `<task-notification>` as its automation
    // attribution label and an inbound `<teammate-message>` as a clean inbound-comm line - never
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
    // genuine to stop at) and each booked the same malformed lines - an all-garbage
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
fn list_encoded_token_after_flag_ordering() {
    let h = populated_home();
    // Exercises normalize_argv: a leading-`-` encoded token THEN --format json.
    let out = h.run(&["list", ENC, "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
}

#[test]
fn list_at_uuid_filters_like_siblings() {
    // `list @<uuid>` is the SAME session filter every other subcommand carries - the `@<uuid>`
    // POSITIONAL must resolve to that one session and scope (no `--session` flag exists).
    let h = populated_home();
    let out = h.run(&["list", at(SESS).as_str(), "--no-subagents"]);
    assert!(
        out.success,
        "list @<uuid> must resolve; stderr: {}",
        out.stderr
    );
    assert!(out.stdout.contains(SESS));
    // Top-level-only: the subagent ids must NOT appear with --no-subagents.
    assert!(
        !out.stdout.contains("aaa111") && !out.stdout.contains("bbb222"),
        "--no-subagents must exclude subagent rows; got: {}",
        out.stdout
    );
}

#[test]
fn list_scope_banner_reports_pre_cap_scope() {
    // R7 §2.4: the scope banner / JSON header answer "how big is the covered range" - the
    // row flood-guard (`--max-count` / the unscoped default cap) must never shrink them.
    let h = populated_home(); // 1 top-level + 2 subagent = 3 in scope
    let lj = h.run(&["list", "--max-count", "2", "--format", "json"]);
    assert!(lj.success, "stderr: {}", lj.stderr);
    let header: serde_json::Value =
        serde_json::from_str(lj.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(
        header["sessions_in_scope"], 3,
        "header scope must be PRE-cap: {header}"
    );
    assert_eq!(
        json_rows(&lj.stdout, "session").len(),
        2,
        "rows stay capped"
    );
    assert_eq!(json_summary(&lj.stdout)["dropped_by_cap"], 1);

    let lt = h.run(&["list", "--max-count", "2"]);
    assert!(
        lt.stdout.contains("3 sessions in scope"),
        "text banner must be PRE-cap: {}",
        lt.stdout
    );
}

#[test]
fn list_skipped_lines_is_a_window_census_stats_is_the_authority() {
    // R12 §1 disclosure pin: a malformed line OUTSIDE list's head/tail windows is
    // invisible to `list` BY DESIGN (§7: list never scans the middle - full coverage
    // measured ~4× its unscoped runtime), while `stats` (a full scan) is the
    // corruption-census authority over the same bytes. Pinning BOTH numbers keeps the
    // divergence a documented contract instead of silent drift.
    let h = Home::new();
    let enc = "-Users-test-Projects-midtear";
    let sess = "eeeeeeee-aaaa-4bbb-8ccc-000000000150";
    let mut body = String::new();
    for i in 0..20 {
        if i == 9 {
            body.push_str("MID-FILE TEAR not json\n");
            continue;
        }
        let (ty, role) = if i % 2 == 0 {
            ("user", "user")
        } else {
            ("assistant", "assistant")
        };
        body.push_str(&format!(
            r#"{{"type":"{ty}","uuid":"m{i}","timestamp":"2026-06-07T05:00:{i:02}.000Z","message":{{"role":"{role}","content":[{{"type":"text","text":"msg {i}"}}]}}}}"#
        ));
        body.push('\n');
    }
    h.write(&format!("{enc}/{sess}.jsonl"), &body);
    let at = format!("@{sess}");
    let l = h.run(&["list", &at, "--no-subagents", "--format", "json"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert_eq!(
        json_summary(&l.stdout)["skipped_lines"],
        0,
        "the mid-file tear sits outside list's windows (disclosed design): {}",
        l.stdout
    );
    let s = h.run(&["stats", &at, "--no-subagents", "--format", "json"]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert_eq!(
        json_summary(&s.stdout)["skipped_lines"],
        1,
        "stats full-scans and must see the tear: {}",
        s.stdout
    );
}

#[test]
fn list_json_and_text_discriminate_subagent_id_domain_with_scope_banner() {
    // `list` spans subagents by default: a bare `csift list <uuid>` returns the top-level row
    // + each subagent row. JSON carries is_subagent + the re-feedable parent_session_id; text
    // leads with a scope banner and brands subagent rows SUBAGENT … · parent SESSION ….
    let h = Home::new();
    subagents_only_scenario(&h);

    let j = h.run(&["list", at(SESS).as_str(), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs = json_lines(&j.stdout);
    let top = objs
        .iter()
        .find(|o| o["session_id"] == serde_json::json!(SESS))
        .expect("top-level row present");
    assert_eq!(top["is_subagent"], serde_json::json!(false));
    assert_eq!(top["parent_session_id"], serde_json::json!(SESS));
    let sub = objs
        .iter()
        .find(|o| o["session_id"] == serde_json::json!("sub111"))
        .expect("subagent row present");
    assert_eq!(sub["is_subagent"], serde_json::json!(true));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));

    let t = h.run(&["list", at(SESS).as_str()]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout
            .contains("scope  2 sessions in scope (1 top-level + 1 subagent)"),
        "missing scope banner: {}",
        t.stdout
    );
    assert!(
        t.stdout
            .contains(&format!("SUBAGENT  sub111  ·  parent SESSION {SESS}")),
        "subagent row not branded: {}",
        t.stdout
    );

    // --no-subagents drops the banner + the subagent row entirely.
    let top_only = h.run(&["list", at(SESS).as_str(), "--no-subagents"]);
    assert!(top_only.success, "stderr: {}", top_only.stderr);
    assert!(
        !top_only.stdout.contains("scope  "),
        "no banner when no subagents in scope: {}",
        top_only.stdout
    );
    assert!(
        !top_only.stdout.contains("SUBAGENT"),
        "no subagent row under --no-subagents: {}",
        top_only.stdout
    );
}

#[test]
fn list_version_and_branch_are_last_seen_with_first_pairs() {
    // A session that upgraded CC and switched branch mid-flight: the base fields
    // report what the session is on NOW; the opening samples live in *_first, and
    // the text meta line shows the drift arrow. cwd stays first-seen on purpose.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","cwd":"/p","version":"2.0.100","gitBranch":"trunk","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/p/sub","version":"2.0.100","gitBranch":"trunk","message":{"role":"assistant","content":[{"type":"text","text":"working"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","cwd":"/p/sub","version":"2.0.200","gitBranch":"release","message":{"role":"user","content":"after the upgrade"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:01.000Z","cwd":"/p/sub","version":"2.0.200","gitBranch":"release","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    let json = h.run(&["list", at(SESS).as_str(), "--format", "json"]);
    assert!(json.success, "stderr: {}", json.stderr);
    let row: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "session")
        .expect("session row");
    assert_eq!(row["version"], "2.0.200", "base = last-seen");
    assert_eq!(row["version_first"], "2.0.100");
    assert_eq!(row["version_last"], "2.0.200");
    assert_eq!(row["git_branch"], "release");
    assert_eq!(row["git_branch_first"], "trunk");
    assert_eq!(row["git_branch_last"], "release");
    assert_eq!(row["cwd"], "/p", "cwd stays first-seen");

    let text = h.run(&["list", at(SESS).as_str()]);
    assert!(
        text.stdout
            .contains("branch trunk->release, CC 2.0.100->2.0.200"),
        "drift arrows: {}",
        text.stdout
    );

    // A stable session renders bare values and equal pairs.
    let h2 = Home::new();
    h2.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","cwd":"/p","version":"2.0.100","gitBranch":"main","message":{"role":"user","content":"only turn"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/p","version":"2.0.100","gitBranch":"main","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let stable = h2.run(&["list", at(SESS).as_str()]);
    assert!(
        stable.stdout.contains("branch main, CC 2.0.100"),
        "no arrow when stable: {}",
        stable.stdout
    );
    assert!(
        !stable.stdout.contains("->"),
        "a stable session never renders a drift arrow: {}",
        stable.stdout
    );
}

#[test]
fn text_rows_never_open_with_a_blank_line() {
    let h = populated_home();
    let out = h.run(&["list"]);
    assert!(
        !out.stdout.starts_with('\n'),
        "the first row opens the output; blanks only separate rows:\n{}",
        out.stdout
    );
}
