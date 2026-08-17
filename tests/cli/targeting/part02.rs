use crate::harness::*;

#[test]
fn target_at_uuid_prefix_resolves_unique_and_errors_on_ambiguity() {
    // `@<first-segment>` (the emergent shorthand): a short hex prefix resolves the UNIQUE
    // session whose uuid starts with it, and errors (never silently picks) when ambiguous.
    let h = Home::new();
    // Two sessions sharing the 8-hex first segment `0a1b2c3d`, and one distinct (`deadbeef`).
    let a = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let b = "0a1b2c3d-ffff-4a6b-8c7d-9e0f1a2b3c4d";
    let c = "deadbeef-1111-2222-3333-444455556666";
    for s in [a, b, c] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            &format!(
                "{{\"type\":\"user\",\"sessionId\":\"{s}\",\"cwd\":\"/p\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n"
            ),
        );
    }
    // A unique prefix → that one session (list shows its id).
    let uniq = h.run(&["list", "@deadbeef"]);
    assert!(uniq.success, "stderr: {}", uniq.stderr);
    assert!(
        uniq.stdout.contains(c),
        "unique prefix resolved: {}",
        uniq.stdout
    );
    assert!(
        !uniq.stdout.contains(a),
        "scoped to ONLY the matched session: {}",
        uniq.stdout
    );

    // An ambiguous prefix → error listing BOTH candidates, never a silent pick.
    let amb = h.run(&["list", "@0a1b2c3d"]);
    assert!(!amb.success, "ambiguous prefix must error: {}", amb.stdout);
    assert!(
        amb.stderr.contains("AMBIGUOUS"),
        "says ambiguous: {}",
        amb.stderr
    );
    assert!(
        amb.stderr.contains(a) && amb.stderr.contains(b),
        "lists both candidates: {}",
        amb.stderr
    );

    // A prefix nobody starts with → honest no-match.
    let none = h.run(&["list", "@99999999"]);
    assert!(!none.success);
    assert!(
        none.stderr.contains("no session or agent id starts with"),
        "{}",
        none.stderr
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

    // Without the env var AND no target, it must NOT guess — it errors with guidance.
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

#[test]
fn resolve_long_path_uses_prefix_scan_fallback() {
    // A project whose ENCODED cwd exceeds 200 chars is stored by Claude Code as
    // `<first-200>-<hash>` (the hash is not reconstructible — Bun vs djb2). csift must
    // PREFIX-SCAN to find it, mirroring CC's findProjectDir. Regression: csift used to look
    // up the full >200-char name (which never exists on disk) and bail.
    let h = Home::new();
    let seg = "a".repeat(210);
    let long_cwd = format!("/Users/testuser/Projects/{seg}");
    let encoded: String = long_cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    assert!(encoded.len() > 200);
    let dir_name = format!("{}-deadbeef", &encoded[..200]); // CC's truncate+hash form
    let rec = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":long_cwd,
        "version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":"hello from a deeply nested project"}
    });
    h.write(&format!("{dir_name}/{SESS}.jsonl"), &format!("{rec}\n"));
    let out = h.run(&["list", long_cwd.as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(SESS),
        "long-path session not found via prefix-scan:\n{}",
        out.stdout
    );
}

#[test]
fn resolved_pair_is_not_merged() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Which branch should I target?"
            ),
            resolved_line("toolu_AQ1", "2026-06-27T01:05:00.000Z"),
        ),
    );
    // The question is gone from the sidecar's unresolved set → search does NOT find it via merge.
    let out = h.run(&["search", "Which branch should I target", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "a resolved pair must not be merged:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("with elicitation sidecar"),
        "no merged records → no note:\n{}",
        out.stdout
    );
}

#[test]
fn targeting_a_sidecar_file_directly_errors() {
    let h = sidecar_session_home();
    let sidecar = h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line("toolu_AQ1", "2026-06-27T01:02:03.000Z", "q")
        ),
    );
    let out = h.run(&["search", "q", sidecar.to_str().unwrap()]);
    assert!(
        !out.success,
        "targeting a sidecar file must error:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("csift elicitation sidecar")
            && out.stderr.contains("cannot be searched directly"),
        "the rejection message must name the sidecar:\n{}",
        out.stderr
    );
}

#[test]
fn targeting_a_renamed_sidecar_errors_via_content_sniff() {
    let h = sidecar_session_home();
    // A sidecar moved / renamed to a non-`elicitations.jsonl` name → content sniff still rejects.
    let renamed = h.write(
        &format!("{ENC}/{SESS}/backup-markers.jsonl"),
        &format!(
            "{}\n{}\n",
            auq_pending_line("toolu_AQ1", "2026-06-27T01:02:03.000Z", "q"),
            resolved_line("toolu_AQ1", "2026-06-27T01:05:00.000Z"),
        ),
    );
    let out = h.run(&["search", "q", renamed.to_str().unwrap()]);
    assert!(
        !out.success,
        "a renamed sidecar must still error:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("csift elicitation sidecar"),
        "the content-sniff rejection must name the sidecar:\n{}",
        out.stderr
    );
}
