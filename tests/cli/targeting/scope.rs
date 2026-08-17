//! Scope assembly: --sessions-from, claude-home precedence, admission rules, fail-loud pins.

use crate::harness::*;

#[test]
fn pinned_id_matching_nothing_bails_never_silent_empty() {
    // AGENTS §4 fail-closed wall (T0.3): a PINNED id that resolves to no file must BAIL loud -
    // never a silent empty, never a widening to every project (the L56255 `--subagent` →
    // whole-corpus class). Both the full-uuid and the prefix forms are locked here so a future
    // resolver change cannot quietly reintroduce scope-widening.
    let h = populated_home();
    // A nonexistent FULL uuid pinned as a target (search's pattern is the 1st positional).
    let a = h.run(&["search", "carry", "@99999999-8888-4777-8666-555555555555"]);
    assert!(
        !a.success,
        "a nonexistent @uuid must error, not widen: {}",
        a.stdout
    );
    // A PREFIX that matches no session must bail, naming the prefix.
    let b = h.run(&["list", "@deadbeef"]);
    assert!(
        !b.success,
        "a no-match @prefix must error, not widen: {}",
        b.stdout
    );
    assert!(
        b.stderr.contains("deadbeef"),
        "the error names the unresolved prefix: {}",
        b.stderr
    );
}

#[test]
fn path_collision_does_not_leak_sibling_sessions_or_subagents() {
    // Two DIFFERENT cwds that encode to the SAME projects dir (§2.1 lossy collision):
    //   /Users/testuser/Projects/foo-bar   (a literal '-')
    //   /Users/testuser/Projects/foo_bar   (a '_'→'-')
    // both → -Users-testuser-Projects-foo-bar. CC stores both projects' sessions there;
    // csift must NOT leak the sibling's sessions (or its subagents) when you target one path.
    let h = Home::new();
    let enc = "-Users-testuser-Projects-foo-bar";
    let sess_a = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let sess_b = "0b1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let rec = |sess: &str, cwd: &str, body: &str| {
        serde_json::json!({
            "type":"user","uuid":"u0","sessionId":sess,"cwd":cwd,
            "version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z",
            "message":{"role":"user","content":body}
        })
        .to_string()
            + "\n"
    };
    // session A: cwd .../foo-bar ; session B (the colliding sibling): cwd .../foo_bar
    h.write(
        &format!("{enc}/{sess_a}.jsonl"),
        &rec(sess_a, "/Users/testuser/Projects/foo-bar", "i am session A"),
    );
    h.write(
        &format!("{enc}/{sess_b}.jsonl"),
        &rec(
            sess_b,
            "/Users/testuser/Projects/foo_bar",
            "i am session B sibling",
        ),
    );
    // B also spawned a subagent (lives under B's sidecar in the SAME shared dir).
    h.write(
        &format!("{enc}/{sess_b}/subagents/agent-bbb999.jsonl"),
        &(serde_json::json!({
            "type":"user","isSidechain":true,"agentId":"bbb999","timestamp":"2026-06-07T05:00:01.000Z",
            "message":{"role":"user","content":"sibling B subagent work"}
        })
        .to_string()
            + "\n"),
    );

    // Target the REAL path of A → must see ONLY A, never B or B's subagent.
    let out = h.run(&["list", "/Users/testuser/Projects/foo-bar"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(sess_a),
        "session A must be found:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains(sess_b) && !out.stdout.contains("bbb999"),
        "COLLISION LEAK: sibling B / its subagent must NOT appear:\n{}",
        out.stdout
    );

    // Targeting the sibling's real path → only B (and B's subagent surfaces in search).
    // (Which sessions matched is read off the `--format json` records' `session_id`.)
    let out_b = h.run(&[
        "search",
        "",
        "/Users/testuser/Projects/foo_bar",
        "-t",
        "user",
        "--format",
        "json",
    ]);
    assert!(out_b.success, "stderr: {}", out_b.stderr);
    assert!(out_b.stdout.contains(sess_b) || out_b.stdout.contains("bbb999"));
    assert!(
        !out_b.stdout.contains(sess_a),
        "A must not leak into B's scope:\n{}",
        out_b.stdout
    );

    // The EXPLICIT encoded-dir token is the user's chosen scope → NOT cwd-filtered (both show).
    let both = h.run(&["list", enc]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert!(
        both.stdout.contains(sess_a) && both.stdout.contains(sess_b),
        "an explicit encoded-dir token must show the whole dir:\n{}",
        both.stdout
    );
}

#[test]
fn custom_claude_home_via_env_var_and_flag() {
    // A Claude config dir RELOCATED away from $HOME/.claude - the rare custom-home case.
    let h = Home::new();
    let custom = h.root.join("relocated-claude");
    let jsonl = custom
        .join("projects")
        .join(ENC)
        .join(format!("{SESS}.jsonl"));
    std::fs::create_dir_all(jsonl.parent().unwrap()).unwrap();
    std::fs::write(
        &jsonl,
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","#,
            r#""cwd":"/Users/testuser/Projects/foo","timestamp":"2026-06-07T05:00:00.000Z","#,
            r#""message":{"role":"user","content":"relocated home marker xyzzy"}}"#,
            "\n",
        ),
    )
    .unwrap();
    let custom_s = custom.to_str().unwrap();
    let marker = "relocated home marker";

    // (0) Default ($HOME/.claude) does NOT see the relocated session - it lives elsewhere.
    let none = h.run(&["search", "xyzzy"]);
    assert!(none.success, "stderr: {}", none.stderr);
    assert!(
        !none.stdout.contains(marker),
        "default home must NOT see the relocated session:\n{}",
        none.stdout
    );

    // (1) $CLAUDE_CONFIG_DIR (Claude Code's own relocation var) redirects csift too.
    let via_env = h.run_with_env(&["search", "xyzzy"], &[("CLAUDE_CONFIG_DIR", custom_s)]);
    assert!(via_env.success, "stderr: {}", via_env.stderr);
    assert!(
        via_env.stdout.contains(marker),
        "CLAUDE_CONFIG_DIR must relocate the search:\n{}",
        via_env.stdout
    );

    // (2) `--claude-home` AFTER the subcommand (exercises normalize_argv global-flag path).
    let via_flag = h.run(&["search", "xyzzy", "--claude-home", custom_s]);
    assert!(via_flag.success, "stderr: {}", via_flag.stderr);
    assert!(
        via_flag.stdout.contains(marker),
        "--claude-home after the subcommand must relocate the search:\n{}",
        via_flag.stdout
    );

    // (3) `--claude-home` BEFORE the subcommand also works.
    let via_flag_pre = h.run(&["--claude-home", custom_s, "search", "xyzzy"]);
    assert!(via_flag_pre.success, "stderr: {}", via_flag_pre.stderr);
    assert!(
        via_flag_pre.stdout.contains(marker),
        "--claude-home before the subcommand must relocate the search:\n{}",
        via_flag_pre.stdout
    );

    // (4) Another subcommand (`list`) honors the override too - it is not search-specific.
    let list = h.run(&["list", "--claude-home", custom_s]);
    assert!(list.success, "stderr: {}", list.stderr);
    assert!(
        list.stdout.contains(SESS),
        "list must honor --claude-home:\n{}",
        list.stdout
    );

    // (5) Precedence: the flag beats $CLAUDE_CONFIG_DIR (env points at an empty config dir).
    let empty_cfg = h.root.join("empty-cfg");
    std::fs::create_dir_all(empty_cfg.join("projects")).unwrap();
    let both = h.run_with_env(
        &["search", "xyzzy", "--claude-home", custom_s],
        &[("CLAUDE_CONFIG_DIR", empty_cfg.to_str().unwrap())],
    );
    assert!(both.success, "stderr: {}", both.stderr);
    assert!(
        both.stdout.contains(marker),
        "--claude-home must win over CLAUDE_CONFIG_DIR:\n{}",
        both.stdout
    );
}

#[test]
fn sessions_from_scopes_like_at_positionals() {
    let h = populated_home();
    // A bare id in a FILE scopes exactly like an `@` positional.
    let ids = h.root.join("ids.txt");
    std::fs::write(&ids, format!("{SESS}\n")).unwrap();
    let out = h.run(&["list", "--sessions-from", ids.to_str().unwrap()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(SESS),
        "scoped to the listed id: {}",
        out.stdout
    );
    // stdin (`-`) + the tolerated `@`-prefixed spelling.
    let out2 = h.run_with_stdin(&["list", "--sessions-from", "-"], &format!("@{SESS}\n"));
    assert!(out2.success, "stderr: {}", out2.stderr);
    assert!(
        out2.stdout.contains(SESS),
        "stdin form works: {}",
        out2.stdout
    );
    // A non-id token is a hard error NAMING it.
    std::fs::write(&ids, "not-an-id\n").unwrap();
    let bad = h.run(&["list", "--sessions-from", ids.to_str().unwrap()]);
    assert!(!bad.success);
    assert!(
        bad.stderr.contains("not-an-id"),
        "the error names the bad token: {}",
        bad.stderr
    );
    // An EMPTY list = an empty scope (honest empty, exit 0) - NEVER a widening to every
    // project (a pipeline stage that found nothing propagates nothing).
    std::fs::write(&ids, "\n").unwrap();
    let empty = h.run(&["list", "--sessions-from", ids.to_str().unwrap()]);
    assert!(empty.success, "stderr: {}", empty.stderr);
    assert!(
        !empty.stdout.contains(SESS),
        "an explicit empty list must not scan every project: {}",
        empty.stdout
    );
    // A MISSING file is a hard error (you named a list; it must exist).
    let missing = h.run(&["list", "--sessions-from", "/no/such/csift-ids.txt"]);
    assert!(!missing.success);
}

#[test]
fn unresolvable_target_errors_before_scope_warning() {
    // R9 §16.4: the empty-pattern "may emit a lot" advisory used to fire BEFORE target
    // resolution - a warning about a run that was never going to happen. Resolution now
    // fails first; the advisory never fires on an unreachable target.
    let h = populated_home();
    let out = h.run(&["search", "", "@abc"]);
    assert!(!out.success, "3-char prefix must hard-error");
    assert!(
        out.stderr.contains("too short"),
        "the @-grammar error must fire: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("may emit a lot"),
        "no scope advisory for a run that never happens: {}",
        out.stderr
    );
}

#[test]
fn sessions_from_accepts_every_id_shape() {
    // Mutation pin: the --sessions-from token gate accepts each id shape INDEPENDENTLY -
    // a full uuid, a 4-11-hex uuid prefix, and an agent id (the `||` chain must not
    // collapse into a conjunction).
    let h = populated_home();
    for tok in [SESS.to_string(), SESS[..8].to_string()] {
        let out = h.run_with_stdin(&["list", "--sessions-from", "-"], &format!("{tok}\n"));
        assert!(out.success, "token {tok}: {}", out.stderr);
        assert!(
            out.stdout.contains(SESS),
            "token {tok} resolves the session: {}",
            out.stdout
        );
    }
}

#[test]
fn resolve_long_path_uses_prefix_scan_fallback() {
    // A project whose ENCODED cwd exceeds 200 chars is stored by Claude Code as
    // `<first-200>-<hash>` (the hash is not reconstructible - Bun vs djb2). csift must
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

#[test]
fn scan_admits_only_top_level_jsonl_files() {
    // Mutation pin: the admit condition is is_file AND .jsonl (a stray text file and a
    // DIRECTORY named *.jsonl must both be ignored by the top-level enumeration).
    let h = Home::new();
    h.write(
        "-Users-testuser-Projects-adm/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl",
        "{\"type\":\"user\",\"uuid\":\"u0\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"real\"}}\n",
    );
    h.write(
        "-Users-testuser-Projects-adm/note.txt",
        "not a transcript\n",
    );
    h.write(
        "-Users-testuser-Projects-adm/decoy.jsonl/inner.jsonl",
        "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:01.000Z\",\"message\":{\"role\":\"user\",\"content\":\"decoy\"}}\n",
    );
    let o = h.run(&[
        "list",
        "-Users-testuser-Projects-adm",
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(o.success, "stderr: {}", o.stderr);
    assert_eq!(
        o.stdout.matches("\"session_id\"").count(),
        1,
        "exactly the real session:\n{}",
        o.stdout
    );
    assert!(
        !o.stdout.contains("decoy"),
        "the decoy dir never scans:\n{}",
        o.stdout
    );
}
