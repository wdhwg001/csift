use crate::harness::*;

#[test]
fn custom_claude_home_via_env_var_and_flag() {
    // A Claude config dir RELOCATED away from $HOME/.claude — the rare custom-home case.
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

    // (0) Default ($HOME/.claude) does NOT see the relocated session — it lives elsewhere.
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

    // (4) Another subcommand (`list`) honors the override too — it is not search-specific.
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
    // An EMPTY list = an empty scope (honest empty, exit 0) — NEVER a widening to every
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
fn unresolvable_target_errors_before_scope_warning() {
    // R9 §16.4: the empty-pattern "may emit a lot" advisory used to fire BEFORE target
    // resolution — a warning about a run that was never going to happen. Resolution now
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
fn trap_resolves_a_powershell_shell_command() {
    // On Windows without Git-for-Windows bash, CC's shell tool is a SEPARATE tool named
    // `PowerShell` (same `input.command` field — extracted from the 2.1.228 binary). A
    // marker riding a PowerShell tool_use must resolve @trap exactly like a Bash one.
    let h = Home::new();
    let enc = "C--Users-dev-winproj";
    let sess = "aabbccdd-eeff-4000-8000-00000000000f";
    let hex = "ddd444eee555fff66";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"win go"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"ddd444eee555fff66","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"sub: the PSTRAPWORK task"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ps1","name":"PowerShell","input":{"command":"csift search PSTRAPWORK @trap:QuietHarborRelay5271"}}]}}"#, "\n",
        ),
    );
    let out = h.run_with_env(
        &["search", "PSTRAPWORK", "@trap:QuietHarborRelay5271"],
        &[("CLAUDE_CODE_SESSION_ID", sess)],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(hex),
        "the PowerShell-carried marker scopes to the subagent: {}",
        out.stdout
    );
}

#[test]
fn windows_drive_encoded_dir_targets_resolve() {
    // A Windows cwd (`C:\Users\dev\winproj`) encodes to a DRIVE-LETTER-led projects dir
    // (`C--Users-dev-winproj` — verbatim from CC's sanitizer), which leads with a letter,
    // not `-`. Both target forms must resolve it: the bare positional token and the
    // `@`-prefixed form.
    let h = Home::new();
    let enc = "C--Users-dev-winproj";
    let sess = "99aabbcc-ddee-4000-8000-00000000000e";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"win work"}}"#, "\n",
        ),
    );
    let bare = h.run(&["list", enc]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains(sess),
        "bare drive-encoded token resolves: {}",
        bare.stdout
    );
    let at_form = h.run(&["list", &format!("@{enc}")]);
    assert!(at_form.success, "stderr: {}", at_form.stderr);
    assert!(
        at_form.stdout.contains(sess),
        "@-prefixed drive-encoded token resolves: {}",
        at_form.stdout
    );
}

#[test]
fn sessions_from_accepts_every_id_shape() {
    // Mutation pin: the --sessions-from token gate accepts each id shape INDEPENDENTLY —
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
fn target_jsonl_file_path_scopes_to_session() {
    // THE marquee fix: an LLM that has the session's transcript PATH (from ls/find) passes it
    // directly. Before, csift re-encoded the whole path into a bogus dir and errored.
    let h = populated_home();
    let jsonl = h.projects().join(format!("{ENC}/{SESS}.jsonl"));
    let out = h.run(&["search", "carry", jsonl.to_str().unwrap(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}\u{b7}t", &SESS[..8])),
        "scoped to the session: {}",
        out.stdout
    );
    // A non-existent jsonl target errors honestly (never fabricates a dir).
    let bad = h.run(&["search", "carry", "/no/such/session.jsonl"]);
    assert!(!bad.success);
    assert!(
        bad.stderr.contains("no session transcript at"),
        "honest missing-file error: {}",
        bad.stderr
    );
}

#[test]
fn target_at_uuid_routes_to_session() {
    // `@<uuid>` is the explicit session-id target (the form that will replace --session).
    let h = populated_home();
    let out = h.run(&["agents", &format!("@{SESS}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("builtin-task"),
        "resolved the session: {}",
        out.stdout
    );
}

#[test]
fn target_at_main_resolves_env_session() {
    // `@main` resolves the calling session from CLAUDE_CODE_SESSION_ID.
    let h = populated_home();
    let out = h.run_with_env(&["agents", "@main"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("builtin-task"),
        "@main → env session: {}",
        out.stdout
    );
    // @main with no env errors with guidance.
    let no_env = h.run(&["agents", "@main"]);
    assert!(!no_env.success);
    assert!(
        no_env.stderr.contains("CLAUDE_CODE_SESSION_ID is not set"),
        "guidance when env absent: {}",
        no_env.stderr
    );
}

#[test]
fn target_at_trap_resolves_caller_via_bash_marker() {
    // `@trap:<marker>` finds the transcript whose Bash `csift` command carries a unique, literal
    // marker the caller embedded. A subagent match → that subagent (+ its subtree); a main-thread
    // match → the session. CC flushes the assistant tool_use to disk BEFORE the command runs, so
    // the very command that launched csift is already greppable.
    let enc = "-Users-testuser-Projects-trap";
    let hex = "aaa111bbb222ccc33"; // 17 hex, like a real agent id
    let h = Home::new();
    // MAIN transcript carries the MAIN marker in a `csift` Bash command.
    h.write(
        &format!("{enc}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/p","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Agent","input":{"description":"spawn"}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"bm","name":"Bash","input":{"command":"csift list @trap:MossyLanternCove6024"}}]}}"#, "\n",
        ),
    );
    // SUBAGENT transcript carries the SUB marker in its own `csift` Bash command + content to find.
    h.write(
        &format!("{enc}/{SESS}/subagents/agent-{hex}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaa111bbb222ccc33","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":"sub: the TRAPSUBWORK task"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"bs","name":"Bash","input":{"command":"csift search TRAPSUBWORK @trap:GildedHeronVale7391"}}]}}"#, "\n",
        ),
    );

    // SUBAGENT match → scopes to that subagent (branded as a subagent, its hex shown).
    let sub = h.run_with_env(
        &["search", "TRAPSUBWORK", "@trap:GildedHeronVale7391"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(sub.success, "stderr: {}", sub.stderr);
    assert!(
        sub.stdout.contains(hex),
        "scoped to the subagent: {}",
        sub.stdout
    );
    assert!(
        sub.stdout.contains("subagent"),
        "branded as a subagent: {}",
        sub.stdout
    );

    // MAIN-thread match → resolves the SESSION itself.
    let main = h.run_with_env(
        &["list", "@trap:MossyLanternCove6024"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(main.success, "stderr: {}", main.stderr);
    assert!(
        main.stdout.contains(SESS),
        "resolved the session: {}",
        main.stdout
    );

    // A VALID marker nobody embedded → honest "not found" (never a silent empty result).
    let miss = h.run_with_env(
        &["search", "x", "@trap:WistfulAmberGlen8135"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(!miss.success);
    assert!(
        miss.stderr.contains("not found") && miss.stderr.contains("csift"),
        "guides back to the literal-csift requirement: {}",
        miss.stderr
    );
    // The no-match error routes BOTH timing paths: the main thread to `@main` (its own
    // record only flushes after the command completes, so a first use always misses) and
    // the retry to the SAME marker (a fresh one restarts the race).
    assert!(
        miss.stderr.contains("@main") && miss.stderr.contains("SAME marker"),
        "routes the main-thread flush race: {}",
        miss.stderr
    );
}

#[test]
fn target_at_trap_rejects_lazy_markers_and_noncsift_commands() {
    let h = Home::new();
    // 1) STRICT marker grammar — rejected at the source, BEFORE any env / file lookup. This is
    //    the prompt-trick: the only way to satisfy it is a fresh, hand-invented literary token.
    let bad = [
        ("@trap:foo", "malformed"),                // too short / not the shape
        ("@trap:CrimsonOwlPond", "4 digits"),      // no trailing 4 digits
        ("@trap:HTTPSPROXYGATE4827", "CamelCase"), // ALLCAPS "word" loophole
        ("@trap:GoFooBars4827", "CamelCase"),      // 2-letter "Go" — words need >=3 chars
        ("@trap:DeepRiverStone1234", "trivial"),   // 1234 = trivial digit run
    ];
    for (tok, needle) in bad {
        let out = h.run(&["search", "x", tok]);
        assert!(!out.success, "{tok} should be rejected: {}", out.stdout);
        assert!(
            out.stderr.contains(needle),
            "{tok} → expected `{needle}` in: {}",
            out.stderr
        );
    }
    // The exact loophole the design calls out — an acronym + zeros — is rejected.
    let html = h.run(&["search", "x", "@trap:HTML0000"]);
    assert!(!html.success, "HTML0000 must be rejected: {}", html.stdout);

    // 2) csift-literal guard: a Bash command carrying a VALID marker but NOT running csift must
    //    NOT satisfy the trap (else any echoed token would resolve one).
    let genc = "-Users-testuser-Projects-trapguard";
    let g = Home::new();
    g.write(
        &format!("{genc}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"echo @trap:LonelyCedarMarsh4827"}}]}}"#, "\n",
        ),
    );
    let noncsift = g.run_with_env(
        &["search", "x", "@trap:LonelyCedarMarsh4827"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(!noncsift.success);
    assert!(
        noncsift.stderr.contains("not found"),
        "a non-csift command must not satisfy the trap: {}",
        noncsift.stderr
    );

    // 3) Ambiguous: two subagents carrying the SAME marker in csift commands → hard error.
    let denc = "-Users-testuser-Projects-trapdup";
    let d = Home::new();
    d.write(
        &format!("{denc}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
        ),
    );
    for hex in ["aaaa1111bbbb2222c", "cccc3333dddd4444e"] {
        d.write(
            &format!("{denc}/{SESS}/subagents/agent-{hex}.jsonl"),
            concat!(
                r#"{"type":"user","isSidechain":true,"timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub"}}"#, "\n",
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b","name":"Bash","input":{"command":"csift turns @trap:TwinEchoGrove5291"}}]}}"#, "\n",
            ),
        );
    }
    let amb = d.run_with_env(
        &["search", "x", "@trap:TwinEchoGrove5291"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(!amb.success);
    assert!(
        amb.stderr.contains("AMBIGUOUS"),
        "two carriers → ambiguous: {}",
        amb.stderr
    );
}

#[test]
fn target_at_encoded_dir_resolves() {
    // `@<encoded-dir>` names a project dir by its encoded form directly.
    let h = populated_home();
    let out = h.run(&["agents", &format!("@{ENC}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("builtin-task"),
        "@encoded resolved: {}",
        out.stdout
    );
}
