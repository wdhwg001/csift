use crate::harness::*;

#[test]
fn whoami_both_env_vars_blank_errors() {
    // Both the canonical and the alias env vars are present but BLANK → both
    // `!v.trim().is_empty()` checks are false → detect returns None → the ambiguous
    // guidance error fires.
    let h = populated_home();
    let out = h.run_with_env(
        &["whoami"],
        &[
            ("CLAUDE_CODE_SESSION_ID", "  "),
            ("CODEX_COMPANION_SESSION_ID", ""),
        ],
    );
    assert!(!out.success, "both-blank must error");
    assert!(out.stderr.contains("@<uuid>"), "stderr: {}", out.stderr);
}

#[test]
fn whoami_always_prints_path() {
    // The resolved jsonl path is ALWAYS printed (no flag - `--show-path` was removed).
    let h = populated_home();
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "output: {}", out.stdout);
    assert!(
        out.stdout.contains("path"),
        "path line always present: {}",
        out.stdout
    );
}

#[test]
fn whoami_json_format() {
    let h = populated_home();
    let out = h.run_with_env(
        &["whoami", "--format", "json"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let v = json_rows(&out.stdout, "identity").remove(0);
    assert_eq!(v.get("session_id").and_then(|s| s.as_str()), Some(SESS));
    assert!(v.get("path").and_then(|p| p.as_str()).is_some());
    json_summary(&out.stdout);
}

#[test]
fn whoami_alias_env_used_when_canonical_absent() {
    let h = populated_home();
    let out = h.run_with_env(&["whoami"], &[("CODEX_COMPANION_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
}

#[test]
fn whoami_without_env_errors_with_guidance() {
    let h = populated_home();
    let out = h.run(&["whoami"]); // env removed by run()
    assert!(!out.success, "no session env must exit nonzero");
    assert!(out.stderr.contains("@<uuid>"), "stderr: {}", out.stderr);
    assert!(out.stderr.contains("mtime"));
}

#[test]
fn whoami_fast_path_via_cwd_encoding() {
    // Exercise locate_transcript's FAST path: when $PWD encodes to a project dir that
    // holds `<id>.jsonl`, it is found without the scan fallback. We set the child's
    // cwd to a real directory and place the session file under the encoding of THAT
    // path, so encode_cwd($PWD) == the dir name.
    let h = Home::new();
    let cwd = h.root.join("work").join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    // The child resolves symlinks in its cwd (on macOS `/var` → `/private/var`), so
    // encode the CANONICAL path - that's what `current_dir()` reports inside the
    // binary, and what its fast-path `encode_cwd` will produce.
    let canon = std::fs::canonicalize(&cwd).unwrap();
    let enc: String = canon
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let sid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    h.write(
        &format!("{enc}/{sid}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let out = h.run_full(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", sid)], Some(&cwd));
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(sid));
    assert!(
        out.stdout.contains(".jsonl"),
        "fast-path should resolve the file"
    );
    // The path must be the cwd-encoded dir (fast path), not some other project.
    assert!(
        out.stdout.contains(&enc),
        "fast-path dir used: {}",
        out.stdout
    );
}

#[test]
fn whoami_fast_path_dir_present_but_file_absent_falls_to_scan() {
    // The fast-path encodes $PWD to a dir that EXISTS but does NOT hold the session
    // file (the `candidate.is_file()` FALSE arm), so the scan fallback finds it in a
    // DIFFERENT project dir.
    let h = Home::new();
    let cwd = h.root.join("here");
    std::fs::create_dir_all(&cwd).unwrap();
    let canon = std::fs::canonicalize(&cwd).unwrap();
    let enc_cwd: String = canon
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Create the cwd-encoded project dir but WITHOUT the session file.
    h.write(&format!("{enc_cwd}/unrelated.jsonl"), "{}\n");
    // Put the actual session in a DIFFERENT project dir → only the scan finds it.
    let sid = "dddddddd-0000-0000-0000-000000000004";
    h.write(
        &format!("-Other-Project/{sid}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let out = h.run_full(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", sid)], Some(&cwd));
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(sid));
    assert!(
        out.stdout.contains("Other-Project"),
        "scan fallback resolved it: {}",
        out.stdout
    );
}

#[test]
fn whoami_scan_skips_dirs_without_the_file() {
    // The scan fallback iterates project dirs in sorted order. A dir that sorts FIRST
    // but lacks the session file exercises the `candidate.is_file()` FALSE arm (skip),
    // then a later dir holding the file is found. (Fast-path is bypassed: the child's
    // cwd does not encode to either project dir.)
    let h = Home::new();
    let sid = "eeeeeeee-0000-0000-0000-000000000005";
    // `-AAA-first` sorts before `-ZZZ-second`; only the second holds the file.
    h.write(&format!("-AAA-first/unrelated-{sid}-decoy.jsonl"), "{}\n");
    h.write(
        &format!("-ZZZ-second/{sid}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", sid)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("-ZZZ-second"),
        "found in the second dir: {}",
        out.stdout
    );
}

#[test]
fn whoami_blank_env_value_is_ignored_then_falls_to_alias() {
    // A blank canonical var (trim → empty) is ignored, and the non-blank alias is
    // used instead (the `!v.trim().is_empty()` false arm on the canonical, true on
    // the alias).
    let h = populated_home();
    let out = h.run_with_env(
        &["whoami"],
        &[
            ("CLAUDE_CODE_SESSION_ID", "   "),
            ("CODEX_COMPANION_SESSION_ID", SESS),
        ],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
}

#[test]
fn whoami_trims_surrounding_whitespace() {
    // A canonical var with surrounding whitespace is trimmed to the bare id.
    let h = populated_home();
    let padded = format!("  {SESS}  ");
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", &padded)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
    // The path resolves, proving the trimmed id was used for the filename lookup.
    assert!(out.stdout.contains(".jsonl"));
}

#[test]
fn plan_reverse_finds_the_session_bound_to_a_plan_file() {
    // The inverse direction: given a PLAN FILE, find which session is bound to it.
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let bound = plans_dir.join("nested-prancing-popcorn.md");
    std::fs::write(&bound, "the plan\n").unwrap();
    let bound_abs = bound.to_string_lossy().into_owned();
    let other_abs = plans_dir
        .join("unrelated.md")
        .to_string_lossy()
        .into_owned();
    write_planning_session(&h, SESS, &bound_abs, &other_abs);

    // Reverse → the bound session, scanning all projects (no target given).
    let out = h.run(&["plan", "--reverse", &bound_abs, "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let v = json_rows(&out.stdout, "plan").remove(0);
    assert_eq!(
        v["session_id"].as_str(),
        Some(SESS),
        "found the bound session: {}",
        out.stdout
    );
    assert_eq!(v["plan_file"].as_str(), Some(bound_abs.as_str()));
    assert_eq!(v["is_subagent"].as_bool(), Some(false));

    // A plan file nobody is bound to → honest empty (stdout empty, stderr note), not an error.
    let none = h.run(&["plan", "--reverse", &other_abs]);
    assert!(
        none.success,
        "empty reverse is not an error: {}",
        none.stderr
    );
    assert!(
        none.stdout.lines().all(|l| !l.starts_with("session")),
        "no bound session: {}",
        none.stdout
    );
    assert!(
        none.stderr.contains("no session in scope is bound"),
        "honest empty note: {}",
        none.stderr
    );
}

#[test]
fn plan_no_binding_is_honest_not_an_error() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"hi"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["plan", at(SESS).as_str()]);
    assert!(
        out.success,
        "no plan is a valid answer, not an error: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("no plan file is bound"),
        "should note the empty result: {}",
        out.stderr
    );
}

#[test]
fn whoami_at_trap_walks_the_upstream_ancestry_chain() {
    // `whoami @trap:<marker>` resolves the calling SUBAGENT env-independently (its own id is
    // withheld from its env) and prints the full upstream chain self -> ... -> top-level root.
    let enc = "-Users-testuser-Projects-whotrap";
    let hex = "ddd444eee555fff66";
    let h = Home::new();
    h.write(
        &format!("{enc}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sp1","name":"Agent","input":{"description":"child","subagent_type":"general"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{SESS}/subagents/agent-{hex}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"ddd444eee555fff66","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"child task"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"bw","name":"Bash","input":{"command":"csift whoami @trap:VelvetOtterGlade8412"}}]}}"#, "\n",
        ),
    );
    let o = h.run_with_env(
        &["whoami", "@trap:VelvetOtterGlade8412"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(o.success, "stderr: {}", o.stderr);
    assert!(
        o.stdout.contains(hex),
        "chain names the subagent: {}",
        o.stdout
    );
    assert!(
        o.stdout.contains(SESS),
        "chain reaches the root: {}",
        o.stdout
    );
}

#[test]
fn plan_spans_subagents_by_default_and_restricts() {
    // Same span-contract pin for `plan`: a plan_mode binding carried ONLY by a subagent
    // transcript resolves by default and disappears under --no-subagents.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"no plan up here"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub333.jsonl"),
        concat!(
            r#"{"type":"attachment","isSidechain":true,"agentId":"sub333","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"/p/plans/quiet-harbor-relay.md","planExists":false},"uuid":"att1","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
        ),
    );
    let span = h.run(&["plan", at(SESS).as_str()]);
    assert!(span.success, "stderr: {}", span.stderr);
    assert!(
        span.stdout.contains("quiet-harbor-relay"),
        "plan spans subagents by default: {}",
        span.stdout
    );
    let top = h.run(&["plan", at(SESS).as_str(), "--no-subagents"]);
    assert!(
        !top.stdout.contains("quiet-harbor-relay"),
        "--no-subagents restricts plan to the top level: {}",
        top.stdout
    );
}

#[test]
fn plan_surfaces_subagent_bound_plan() {
    // A SUBAGENT that entered Plan Mode binds a plan with an `-agent-<hex>` path; `plan`
    // (spanning subagents) must surface it, flagged as a subagent with its parent uuid.
    const PSESS: &str = "feedface-1111-2222-3333-444455556666";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let sub_plan = plans_dir
        .join("goofy-finding-kettle-agent-aaaaaaaaaaaaaaaaa.md")
        .to_string_lossy()
        .into_owned();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"spawn a planning worker"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let sub_jsonl = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"feed01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan the thing"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"feed01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUBPLAN__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__SUBPLAN__", &jpath(&sub_plan));
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-feed01.jsonl"),
        &sub_jsonl,
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-feed01.meta.json"),
        r#"{"agentType":"general-purpose","description":"planner","toolUseId":"t0"}"#,
    );

    let out = h.run(&["plan", at(PSESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let v: serde_json::Value = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["is_subagent"].as_bool() == Some(true))
        .unwrap_or_else(|| panic!("no subagent plan in:\n{}", out.stdout));
    assert_eq!(v["plan_file"].as_str(), Some(sub_plan.as_str()));
    assert_eq!(
        v["parent_session_id"].as_str(),
        Some(PSESS),
        "carries the re-feedable parent"
    );
}

#[test]
fn plan_text_lists_top_level_then_subagent_plans() {
    // A session AND a subagent both planned → text output lists both, TOP-LEVEL FIRST, with
    // the subagent flagged and carrying its parent uuid.
    const PSESS: &str = "11112222-3333-4444-5555-666677778888";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let top_path = plans_dir.join("top-level-plan.md");
    // The top-level plan EXISTS on disk; the subagent's does not → the [exists]/[missing]
    // flag must reflect disk reality, per-row.
    std::fs::write(&top_path, "the top plan\n").unwrap();
    let top = top_path.to_string_lossy().into_owned();
    let sub = plans_dir
        .join("worker-plan-agent-bbbbbbbbbbbbbbbbb.md")
        .to_string_lossy()
        .into_owned();
    let top_jsonl = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"plan"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":false,"attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"__TOP__","planExists":false},"uuid":"att0","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__TOP__", &jpath(&top));
    h.write(&format!("{ENC}/{PSESS}.jsonl"), &top_jsonl);
    let sub_jsonl = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"bbbb01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan the subtask"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"bbbb01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUB__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__SUB__", &jpath(&sub));
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-bbbb01.jsonl"),
        &sub_jsonl,
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-bbbb01.meta.json"),
        r#"{"agentType":"general-purpose","description":"worker","toolUseId":"t0"}"#,
    );

    let out = h.run(&["plan", at(PSESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    let top_pos = out
        .stdout
        .find(&format!("session  {PSESS}"))
        .expect("top-level line");
    let sub_pos = out.stdout.find("(subagent)").expect("subagent line");
    assert!(top_pos < sub_pos, "top-level listed first:\n{}", out.stdout);
    assert!(out.stdout.contains("top-level-plan.md"), "{}", out.stdout);
    assert!(
        out.stdout.contains("worker-plan-agent-")
            && out.stdout.contains(&format!("parent   {PSESS}")),
        "subagent plan carries its parent:\n{}",
        out.stdout
    );
    // The on-disk top plan reads [exists]; the missing subagent plan reads [missing].
    assert!(
        out.stdout.contains("[exists]") && out.stdout.contains("[missing]"),
        "per-row exists/missing flag tracks disk:\n{}",
        out.stdout
    );
}

#[test]
fn whoami_with_session_env_resolves_path() {
    let h = populated_home();
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
    assert!(out.stdout.contains("path"), "should locate the jsonl path");
    assert!(out.stdout.contains(".jsonl"));
}

#[test]
fn whoami_prints_not_found_note_when_unresolved() {
    let h = Home::new(); // empty projects → the session id won't resolve to a file
    let out = h.run_with_env(
        &["whoami"],
        &[(
            "CLAUDE_CODE_SESSION_ID",
            "ffffffff-0000-0000-0000-000000000000",
        )],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("ffffffff-0000-0000-0000-000000000000"));
    assert!(out.stdout.contains("not found"), "got: {}", out.stdout);
}

#[test]
fn whoami_text_prints_not_found_note_when_unresolved() {
    // The path line is ALWAYS printed; a session id that resolves to no file gets a `not found`
    // note (the old `--show-path`-gated silence was removed).
    let h = Home::new();
    let out = h.run_with_env(
        &["whoami"],
        &[(
            "CLAUDE_CODE_SESSION_ID",
            "11111111-2222-3333-4444-555555555555",
        )],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("11111111-2222-3333-4444-555555555555"));
    assert!(
        out.stdout.contains("not found"),
        "not-found note always present when unresolved: {}",
        out.stdout
    );
    assert!(
        out.stdout.to_lowercase().contains("path"),
        "path line always present: {}",
        out.stdout
    );
}

#[test]
fn plan_resolves_bound_plan_not_an_edited_other_plan() {
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let bound = plans_dir.join("nested-prancing-popcorn.md");
    std::fs::write(&bound, "the plan\n").unwrap();
    let bound_abs = bound.to_string_lossy().into_owned();
    let other_abs = plans_dir
        .join("someone-elses-plan.md")
        .to_string_lossy()
        .into_owned();
    write_planning_session(&h, SESS, &bound_abs, &other_abs);

    let out = h.run(&["plan", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let v = json_rows(&out.stdout, "plan").remove(0);
    assert_eq!(
        v["plan_file"].as_str(),
        Some(bound_abs.as_str()),
        "resolved the plan_mode-bound plan, NOT the edited-other plan: {}",
        out.stdout
    );
    assert_eq!(v["is_subagent"].as_bool(), Some(false));
    assert_eq!(
        v["plan_exists"].as_bool(),
        Some(true),
        "bound plan exists on disk"
    );
    assert_eq!(v["session_id"].as_str(), Some(SESS));
}

#[test]
fn plan_no_target_resolves_calling_session_from_env() {
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let bound_abs = plans_dir.join("env-plan.md").to_string_lossy().into_owned();
    let other_abs = plans_dir.join("decoy.md").to_string_lossy().into_owned();
    write_planning_session(&h, SESS, &bound_abs, &other_abs);

    // With CLAUDE_CODE_SESSION_ID set, `csift plan` (no target) answers "MY plan file".
    let out = h.run_with_env(
        &["plan", "--format", "json"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let v = json_rows(&out.stdout, "plan").remove(0);
    assert_eq!(v["plan_file"].as_str(), Some(bound_abs.as_str()));
    assert_eq!(v["session_id"].as_str(), Some(SESS));

    // Without the env var AND no target, it must NOT guess - it errors with guidance.
    let out2 = h.run(&["plan"]);
    assert!(
        !out2.success,
        "no env + no target must not guess: {}",
        out2.stdout
    );
    assert!(
        out2.stderr.contains("CLAUDE_CODE_SESSION_ID"),
        "should point at the env var: {}",
        out2.stderr
    );
}
