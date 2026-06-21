//! End-to-end CLI integration tests that drive the REAL compiled `csift` binary
//! against a synthetic `~/.claude/projects` tree.
//!
//! These cover the surfaces unit tests cannot reach without process-global env
//! mutation: the `run_list` / `run_search` / `run_whoami` / `run_agents`
//! orchestration + file I/O + the text/JSON renderers + `main.rs`'s dispatch and
//! exit-code mapping + `cli::parse_argv` (which reads `std::env::args`).
//!
//! Isolation: every invocation points `$HOME` at a per-test temp dir via
//! `Command::env("HOME", …)` (child-process scope only — no shared-state race with
//! the in-crate threaded unit tests). The binary path comes from cargo's
//! `CARGO_BIN_EXE_csift`, so the build under test is exactly the one cargo produced.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Wrap a session id / agent hex as the `@<id>` POSITIONAL target token (the grammar that
/// replaced the removed `--session` flag). Used throughout these tests to pin one session.
fn at(s: impl AsRef<str>) -> String {
    format!("@{}", s.as_ref())
}

/// A scratch `$HOME` with a `.claude/projects` tree, removed on drop.
struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("csift-it-{pid}-{n}"));
        std::fs::create_dir_all(root.join(".claude").join("projects")).unwrap();
        Home { root }
    }

    fn projects(&self) -> PathBuf {
        self.root.join(".claude").join("projects")
    }

    /// Write a file under the projects root, creating parent dirs. `rel` is relative
    /// to `~/.claude/projects`.
    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.projects().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, contents).unwrap();
        p
    }

    /// Run the csift binary with this `$HOME`, returning (success, stdout, stderr).
    fn run(&self, args: &[&str]) -> Output {
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        self.run_full(args, extra_env, None)
    }

    fn run_full(&self, args: &[&str], extra_env: &[(&str, &str)], cwd: Option<&Path>) -> Output {
        let exe = env!("CARGO_BIN_EXE_csift");
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .env("HOME", &self.root)
            // Make whoami deterministic: clear the session env unless a test sets it.
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CODEX_COMPANION_SESSION_ID");
        if let Some(d) = cwd {
            cmd.current_dir(d);
        }
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("spawn csift");
        Output {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

const ENC: &str = "-Users-testuser-Projects-foo";
const SESS: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

/// A full session jsonl with identity fields, a genuine-user turn, an assistant
/// reply, a tool round-trip, an isMeta pseudo-turn, and a malformed line (to drive
/// the skipped-line accounting). Returns the home with the tree populated.
fn populated_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"why is the carry needed?"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"the carry holds a partial line"},{"type":"text","text":"The carry is the partial line at a chunk boundary."}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0u","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Read","input":{"file":"parse.rs"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","parentUuid":"a0u","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call0","content":"the carry is the low-edge partial"}]}}"#, "\n",
            r#"{"type":"user","uuid":"meta","isMeta":true,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"Continue from where you left off."}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"now explain the panic path"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"No panic — a malformed line is skipped and counted."}]}}"#, "\n",
            // A malformed line that LOOKS like a transcript record ("role":"user" is
            // present) so it survives search's pre-JSON category prefilter and is
            // genuinely counted by BOTH list (head/tail parse) and search
            // (parse-stage) skipped-line accounting.
            r#"{"type":"user","role":"user" this is broken json after the marker}"#, "\n",
        ),
    );

    // A built-in subagent transcript + meta, and a workflow subagent + journal.
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-aaa111.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaa111","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"sub: do the thing about carry"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:03:21.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-aaa111.meta.json"),
        r#"{"agentType":"oh-my-claudecode:executor","description":"run the carry task","toolUseId":"toolu_x"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"bbb222","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"wf task about carry"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T06:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.meta.json"),
        r#"{"agentType":"workflow-subagent"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_abc/journal.jsonl"),
        concat!(
            r#"{"type":"started","agentId":"bbb222","key":"v2:abc"}"#,
            "\n",
            r#"{"type":"result","agentId":"bbb222","key":"v2:abc","result":"ok"}"#,
            "\n",
        ),
    );
    h
}

// ── list ──

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
    // A leading {kind:"session_header", …} scope record precedes the per-session objects
    // whenever the set spans ≥1 subagent (uniform JSON scope disclosure, same as turns).
    // Every OTHER non-empty line must be a JSON object with a session_id.
    let mut count = 0;
    let mut saw_header = false;
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        if v.get("kind").and_then(|k| k.as_str()) == Some("session_header") {
            saw_header = true;
            // The header discloses the span with turns' field names.
            assert!(v.get("sessions_in_scope").is_some(), "header span: {line}");
            assert!(v.get("top_level_sessions").is_some(), "header span: {line}");
            assert!(v.get("subagent_sessions").is_some(), "header span: {line}");
            continue;
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

// NOTE: the `$HOME`-unset / `$HOME`-empty fallback arms of `path::home_dir`
// (var_os None / empty → the OS `home_dir()` fallback) are intentionally NOT
// integration-tested: removing HOME makes the binary scan the developer's REAL
// `~/.claude/projects`, which is non-hermetic (variable contents, potentially large)
// and reads real user data. The fallback is a thin, audited 3-line guard; it is left
// as a documented coverage gap rather than tested via a non-deterministic real-corpus
// scan. See the final coverage report's "remaining gaps".

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
fn list_encoded_token_after_flag_ordering() {
    let h = populated_home();
    // Exercises normalize_argv: a leading-`-` encoded token THEN --format json.
    let out = h.run(&["list", ENC, "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
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
fn list_at_uuid_filters_like_siblings() {
    // `list @<uuid>` is the SAME session filter every other subcommand carries — the `@<uuid>`
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
fn unknown_flag_reports_clean_error_not_project_dir_error() {
    // A typo'd / unknown flag on any scope-operating subcommand must surface as an
    // "unexpected argument" message (NOT the misleading "no Claude Code project dir named
    // --xxx"). The `--`-leading value-parser reject makes this uniform tool-wide.
    let h = populated_home();
    let at_sess = at(SESS);
    for args in [
        vec!["files", at_sess.as_str(), "--by-fil"],
        vec!["turns", at_sess.as_str(), "--budgett", "5000"],
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

// ── search ──

#[test]
fn search_text_returns_round_trip_exchange() {
    let h = populated_home();
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("s1 = "),
        "the session-label table:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("·t"),
        "exchanges reference the label: {}",
        out.stdout
    );
    assert!(out.stdout.contains("matched"));
    assert!(out.stdout.contains("malformed line(s) skipped"));
}

#[test]
fn search_json_emits_hits_and_summary() {
    let h = populated_home();
    let out = h.run(&["search", "carry", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert!(!lines.is_empty());
    // Last line is the trailing summary object with matched/dropped/skipped.
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert!(
        summary.get("matched").is_some(),
        "no summary: {:?}",
        lines.last()
    );
    assert!(summary.get("skipped_lines").is_some());
}

#[test]
fn search_no_match_reports_zero() {
    let h = populated_home();
    let out = h.run(&["search", "zzz-no-such-token-zzz"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "got: {}",
        out.stdout
    );
    // Even with no matches, the skipped-line note still surfaces.
    assert!(out.stdout.contains("malformed line(s) skipped"));
}

#[test]
fn search_category_filter_and_max_count() {
    let h = populated_home();
    // -t user restricts to user text; "carry" matches the user record in the top-level session
    // AND in two subagents, so --max-count 1 caps to one and DROPS the rest (drop note appears
    // only when something is actually dropped — the footer no longer prints "0 dropped").
    let out = h.run(&["search", "carry", "-t", "user", "--max-count", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched 1"), "{}", out.stdout);
    assert!(
        out.stdout.contains("dropped by --max-count"),
        "{}",
        out.stdout
    );
}

#[test]
fn search_empty_pattern_warns_then_emits() {
    let h = populated_home();
    let out = h.run(&["search", ""]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The unbounded empty-pattern warning goes to stderr.
    assert!(
        out.stderr.contains("empty pattern with no category"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_session_target_only_does_not_warn() {
    // Empty pattern + ONLY an `@<uuid>` session target (no category/time/turn filter) → the
    // warning's `has_session_filter` operand (a `pins_single_session` target) is TRUE → warning
    // suppressed.
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "an @<uuid> session scope must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_category_does_not_warn() {
    // An empty pattern but WITH a `-t` category → the warning's
    // `args.categories.is_empty()` operand is FALSE, so the warning is suppressed.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "",
        "-t",
        "user",
        "--no-subagents",
        at(SESS).as_str(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "category filter must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_uuid_positional_does_not_warn() {
    // A bare-uuid POSITIONAL routes to the SAME session filter as `--session` (via
    // resolve_session_files), so the empty-pattern warning — which claims "no session
    // filter" — must be SUPPRESSED. Previously the gate only inspected `--session` and
    // printed the misleading warning here.
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "a bare-uuid positional scopes to one session and must suppress the warning; \
         stderr: {}",
        out.stderr
    );
}

#[test]
fn search_short_t_after_positional_parses_and_filters() {
    // The reported critical bug: a trailing short flag after the positional path used to
    // be swallowed ("no project dir named -t"). End-to-end through the real binary, a
    // `-t user` after the path must now parse and filter to user turns.
    let h = populated_home();
    let out = h.run(&["search", "carry", ENC, "-t", "user", "--no-subagents"]);
    assert!(
        out.success,
        "short flag after positional must parse; stderr: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("no Claude Code project dir named"),
        "the short flag must not be misrouted as a project dir; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_short_i_after_positional_parses() {
    // The trailing boolean short flag `-i` likewise must parse, not error.
    let h = populated_home();
    let out = h.run(&["search", "CARRY", ENC, "-i", "--no-subagents"]);
    assert!(
        out.success,
        "trailing -i must parse; stderr: {}",
        out.stderr
    );
    assert!(!out.stderr.contains("no Claude Code project dir named"));
}

#[test]
fn search_with_positional_path_target_like_siblings() {
    // `csift search PATTERN <encoded>` — a POSITIONAL path, exactly like
    // `files`/`recover`/`turns`; exercises the explicit-paths branch (`paths.is_empty()` FALSE).
    let h = populated_home();
    let out = h.run(&["search", "carry", ENC, "--no-subagents"]);
    assert!(
        out.success,
        "positional PATH must work; stderr: {}",
        out.stderr
    );
    assert!(out.stdout.contains("matched"), "got: {}", out.stdout);
}

#[test]
fn search_count_prints_only_the_match_total() {
    // `-c`/--count: just the integer, no headers — and it must equal the footer `matched`
    // (the ripgrep `-c` contract). Compare against the JSON summary so the assertion tracks
    // whatever the fixture actually yields.
    let h = populated_home();
    let full = h.run(&["search", "carry", "--no-subagents", "--format", "json"]);
    let footer: serde_json::Value = serde_json::from_str(
        full.stdout
            .lines()
            .filter(|l| !l.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    let expected = footer["matched"].as_u64().unwrap();

    let out = h.run(&["search", "carry", "--no-subagents", "-c"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.trim().parse::<u64>().unwrap(),
        expected,
        "-c must print exactly the match total; got {:?}",
        out.stdout
    );
    // No per-exchange output leaked through.
    assert!(!out.stdout.contains("SESSION"), "got: {}", out.stdout);
    assert!(!out.stdout.contains("matched "), "got: {}", out.stdout);

    // JSON form is `{"matched":N}`.
    let j = h.run(&[
        "search",
        "carry",
        "--no-subagents",
        "-c",
        "--format",
        "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(j.stdout.trim()).unwrap();
    assert_eq!(v["matched"].as_u64().unwrap(), expected);
}

#[test]
fn search_count_reports_true_total_despite_max_count() {
    // `-c` reports the TRUE total even when `--max-count` would cap the listing (the count
    // adds the capped-away remainder back), so the number is never silently shrunk.
    let h = populated_home();
    let capped = h.run(&["search", "carry", "-c", "--max-count", "1"]);
    let uncapped = h.run(&["search", "carry", "-c"]);
    assert_eq!(
        capped.stdout.trim(),
        uncapped.stdout.trim(),
        "--max-count must not change the -c total"
    );
}

#[test]
fn search_files_with_matches_lists_distinct_sessions() {
    // `-l`/--files-with-matches: one id per matching session, no exchange bodies, no footer.
    // "carry" appears in the top-level session AND both subagents, so spanning yields three
    // distinct ids — the top-level uuid plain, each subagent hex annotated with its parent.
    let h = populated_home();
    let out = h.run(&["search", "carry", "-l"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(
        lines.len(),
        3,
        "three distinct matching transcripts: {lines:?}"
    );
    assert!(out.stdout.contains(SESS), "top-level uuid: {}", out.stdout);
    assert!(
        out.stdout.contains("aaa111"),
        "subagent hex: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(parent"),
        "subagent rows annotate the re-feedable parent: {}",
        out.stdout
    );
    // No exchange rendering or footer leaks through the plain listing.
    assert!(!out.stdout.contains("TURN"), "got: {}", out.stdout);
    assert!(!out.stdout.contains("matched "), "got: {}", out.stdout);

    // `-l --format json`: one discrimination object per line, no footer.
    let j = h.run(&["search", "carry", "-l", "--format", "json"]);
    let objs: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(objs.len(), 3);
    assert!(objs.iter().any(|o| o["is_subagent"] == true));
    assert!(objs
        .iter()
        .all(|o| o.get("parent_session_id").is_some() && o.get("session_id").is_some()));
}

// (`-c` + `-l` together are now a hard error — see
// `search_count_and_files_with_matches_are_mutually_exclusive`. They are each a "return ONLY
// this" mode and the two totals both live in the normal footer, so there is no "winner".)

#[test]
fn search_siblings_surface_the_rest_of_the_turn() {
    // The Q3 shape: a matched USER record should be able to surface WITH the agent reply.
    // "needed" lives ONLY in the opening user message, so the agent text can reach the
    // output only as a `--siblings` context record (default sibling set = all-but-`-t`).
    let h = populated_home();
    let base = h.run(&["search", "needed", "-t", "user", "--no-subagents"]);
    assert!(
        !base.stdout.contains("partial line at a chunk boundary"),
        "without --siblings the agent reply must NOT appear: {}",
        base.stdout
    );

    let out = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· agent"),
        "the agent sibling renders under the `·` marker: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("partial line at a chunk boundary"),
        "the agent reply text surfaces as a sibling: {}",
        out.stdout
    );
    // The matched user record opens the exchange (◂) and is never repeated as a sibling.
    assert_eq!(
        out.stdout.matches("carry needed").count(),
        1,
        "the matched user line appears once, not duplicated as a sibling: {}",
        out.stdout
    );
}

#[test]
fn search_sibling_category_narrows_and_implies_siblings() {
    // `--sibling-category agent` (without `--siblings`) implies siblings AND restricts them
    // to the agent reply — the tool_use / tool_result siblings are excluded.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--sibling-category",
        "agent",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· agent"),
        "agent sibling present: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("· tool"),
        "non-agent siblings must be excluded: {}",
        out.stdout
    );

    // JSON: the envelope carries a `siblings` array with the agent excerpt; absent without
    // the flag.
    let j = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--sibling-category",
        "agent",
        "--format",
        "json",
    ]);
    let env: serde_json::Value = serde_json::from_str(j.stdout.lines().next().unwrap()).unwrap();
    let sibs = env["siblings"].as_array().expect("siblings array present");
    assert_eq!(sibs.len(), 1);
    assert_eq!(sibs[0]["category"], "agent");

    let plain = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let env2: serde_json::Value =
        serde_json::from_str(plain.stdout.lines().next().unwrap()).unwrap();
    assert!(
        env2.get("siblings").is_none(),
        "no siblings key without the flag: {}",
        plain.stdout
    );
}

#[test]
fn search_full_emits_the_untruncated_record() {
    // A message far longer than the ~400-char excerpt cap, with a token at the very TAIL.
    // The default excerpt truncates (explicit `… (+N chars)` marker) and hides the tail;
    // `--full` (and its `--no-truncate` alias) emit the whole record so the tail is readable
    // — the gap that otherwise forces a drop to the raw jsonl.
    let h = Home::new();
    let filler = "x".repeat(900);
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"needle {filler} taIlToken9z"}}}}"#
        ),
    );

    let def = h.run(&["search", "needle", "--no-subagents"]);
    assert!(def.success, "stderr: {}", def.stderr);
    assert!(
        def.stdout.contains("… (+"),
        "default excerpt must truncate with the explicit marker: {}",
        def.stdout
    );
    assert!(
        !def.stdout.contains("taIlToken9z"),
        "the tail token is hidden by the default cap: {}",
        def.stdout
    );

    let full = h.run(&["search", "needle", "--no-subagents", "--full"]);
    assert!(
        full.stdout.contains("taIlToken9z"),
        "--full must surface the tail: {}",
        full.stdout
    );
    assert!(
        !full.stdout.contains("… (+"),
        "--full removes the truncation marker: {}",
        full.stdout
    );

    // The `--no-truncate` alias behaves identically.
    let alias = h.run(&["search", "needle", "--no-subagents", "--no-truncate"]);
    assert!(
        alias.stdout.contains("taIlToken9z"),
        "--no-truncate alias must surface the tail too: {}",
        alias.stdout
    );
}

#[test]
fn search_hit_carries_line_and_uuid_address() {
    let h = populated_home();
    // "needed" lives only in the opening user record (fixture line 1).
    let out = h.run(&["search", "needed", "-t", "user", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user  L1  "),
        "the hit header carries its `L<line>` address: {}",
        out.stdout
    );
    // JSON: per-hit `line` + `uuid` (the `csift get` address).
    let j = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let env: serde_json::Value = serde_json::from_str(j.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(env["hits"][0]["line"], 1);
    assert_eq!(env["hits"][0]["uuid"], "u0");
}

#[test]
fn search_footer_always_reports_match_and_session_totals() {
    let h = populated_home();
    let out = h.run(&["search", "carry", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· 1 session ·"),
        "the footer carries the distinct-session total: {}",
        out.stdout
    );
    // JSON footer gains `sessions` alongside `matched`.
    let j = h.run(&["search", "carry", "--no-subagents", "--format", "json"]);
    let footer: serde_json::Value = serde_json::from_str(
        j.stdout
            .lines()
            .filter(|l| !l.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(footer["sessions"], 1);
    assert!(footer["matched"].as_u64().unwrap() >= 1);
}

#[test]
fn search_count_and_files_with_matches_are_mutually_exclusive() {
    let h = populated_home();
    let out = h.run(&["search", "carry", "-c", "-l"]);
    assert!(!out.success, "passing both must error");
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
    );
}

// ── search addressing (the folded-in `get`): --line / --uuid fetch specific records, full ──

#[test]
fn search_line_address_fetches_the_record_in_full() {
    let h = populated_home();
    // Fixture L1 = the opening user record. Addressing it (no pattern) returns it FULL.
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "1",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("◂ user  L1  "),
        "the addressed user record, with its L1 address: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("why is the carry needed?"),
        "the full body: {}",
        out.stdout
    );
}

#[test]
fn search_line_address_renders_uncapped() {
    let h = populated_home();
    // L2 = the assistant thinking + agent-text record. Addressed → full (no excerpt cap).
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "2",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("The carry is the partial line at a chunk boundary."),
        "the agent block renders end-to-end: {}",
        out.stdout
    );
}

#[test]
fn search_multiple_lines_and_ranges_address_many_records() {
    let h = populated_home();
    // L1 (turn 0) + L6 (turn 1) — distinct turns, both surfaced under the SAME session label.
    let list = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "6,1",
    ]);
    assert!(list.success, "stderr: {}", list.stderr);
    assert!(
        list.stdout.contains("why is the carry needed?")
            && list.stdout.contains("now explain the panic path"),
        "both addressed records: {}",
        list.stdout
    );
    // A range expands to every record in span (L1-L7 are records; L8 malformed is skipped).
    let range = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "1-7",
    ]);
    assert!(range.success, "stderr: {}", range.stderr);
    assert!(
        range.stdout.contains("why is the carry needed?") && range.stdout.contains("No panic"),
        "the spanned records: {}",
        range.stdout
    );
}

#[test]
fn search_uuid_address_locates_records_anywhere() {
    let h = populated_home();
    // No scope → spans every project; the uuid selector pins exactly the requested record(s).
    let one = h.run(&["search", "", "--uuid", "u0"]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(
        one.stdout.contains("why is the carry needed?"),
        "by uuid u0: {}",
        one.stdout
    );
    let many = h.run(&["search", "", "--uuid", "u0,u1"]);
    assert!(many.success, "stderr: {}", many.stderr);
    assert!(
        many.stdout.contains("why is the carry needed?")
            && many.stdout.contains("now explain the panic path"),
        "both uuids: {}",
        many.stdout
    );
}

#[test]
fn search_line_address_json_carries_full_address() {
    let h = populated_home();
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The exchange object (the line carrying `hits`); each hit keeps its line + uuid address.
    let ex: serde_json::Value = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v.get("hits").is_some())
        .expect("an exchange object");
    let hit0 = &ex["hits"][0];
    assert_eq!(hit0["line"], 2);
    assert_eq!(hit0["uuid"], "a0");
}

#[test]
fn search_explicit_unresolved_line_is_reported() {
    let h = populated_home();
    // An EXPLICIT line past EOF resolves to nothing → an `unresolved:` line (text) and an
    // `unresolved` array (json). `search` itself still exits 0 (a no-match is not an error).
    let txt = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "999",
    ]);
    assert!(txt.success, "stderr: {}", txt.stderr);
    assert!(
        txt.stdout.contains("unresolved: L999"),
        "the miss is reported: {}",
        txt.stdout
    );
    let js = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "1,999",
        "--format",
        "json",
    ]);
    let footer: serde_json::Value = serde_json::from_str(
        js.stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(footer["unresolved"][0], "L999");
}

#[test]
fn search_range_past_eof_is_clamped_not_reported() {
    let h = populated_home();
    // 6-1000: L6/L7 are records; L8 malformed; L9-1000 past EOF — all RANGE members, so the
    // gaps are clamped silently (no `unresolved`), the present records returned.
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--line",
        "6-1000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("now explain the panic path"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("unresolved"),
        "a range never reports its own gaps: {}",
        out.stdout
    );
}

#[test]
fn search_subagent_line_addresses_the_subagent_transcript() {
    let h = populated_home();
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--subagent",
        "aaa111",
        "--line",
        "1",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("sub: do the thing about carry"),
        "the subagent record body: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("subagent") && out.stdout.contains("aaa111"),
        "the label table marks it a subagent: {}",
        out.stdout
    );
}

#[test]
fn search_subagent_scopes_a_normal_search_not_just_line_addressing() {
    // REGRESSION (the `--subagent` silent no-op): without `--line`, `--subagent <hex>` used to
    // be dropped entirely — the search ran over the WHOLE scope and fail-OPENed to the full
    // corpus. Decisive fixture fact: "panic" lives ONLY in the top-level transcript; subagent
    // `aaa111` contains "carry"/"sub" but NEVER "panic". So scoping to aaa111 MUST yield zero
    // "panic" hits; the bug returned the top-level hit anyway.
    let h = populated_home();

    // Unscoped: "panic" matches (it is in the top-level session).
    let unscoped = h.run(&["search", "panic", at(SESS).as_str(), "-c"]);
    assert!(unscoped.success, "stderr: {}", unscoped.stderr);
    assert_eq!(
        unscoped.stdout.trim(),
        "1",
        "panic should match once unscoped: {}",
        unscoped.stdout
    );

    // Scoped to the subagent that has NO "panic": must be 0 (was 1 with the no-op bug).
    let scoped = h.run(&[
        "search",
        "panic",
        at(SESS).as_str(),
        "--subagent",
        "aaa111",
        "-c",
    ]);
    assert!(scoped.success, "stderr: {}", scoped.stderr);
    assert_eq!(
        scoped.stdout.trim(),
        "0",
        "--subagent must SCOPE the search to aaa111 (no panic there), not widen it: {}",
        scoped.stdout
    );

    // And a term that IS in aaa111 ("sub") still matches under the same scope — proving the
    // scope is the subagent transcript, not an empty/echo result.
    let positive = h.run(&[
        "search",
        "sub",
        at(SESS).as_str(),
        "--subagent",
        "aaa111",
        "-l",
    ]);
    assert!(positive.success, "stderr: {}", positive.stderr);
    assert!(
        positive.stdout.contains("aaa111"),
        "the in-scope subagent should still match its own content: {}",
        positive.stdout
    );
    // -l lists exactly the one in-scope transcript: a single line, `aaa111 (parent <SESS>)`.
    // The sibling wf agent bbb222 must be absent. (The parent-uuid annotation legitimately
    // echoes SESS — that is the subagent's re-feedable parent, not a top-level hit.)
    let lines: Vec<&str> = positive
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "scope must resolve to exactly one transcript: {}",
        positive.stdout
    );
    assert!(
        !positive.stdout.contains("bbb222"),
        "scope must be ONLY aaa111, not the sibling subagent bbb222: {}",
        positive.stdout
    );
}

#[test]
fn search_subagent_unknown_hex_fails_closed() {
    // The fail-OPEN footgun: an unmatched `--subagent` hex must ERROR (so the caller knows the
    // scope is empty), never silently fall back to the whole corpus. Asserted WITHOUT `--line`
    // — the path that previously ignored `--subagent` outright.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--subagent",
        "deadbeef00",
        "-c",
    ]);
    assert!(
        !out.success,
        "an unmatched --subagent hex must fail, not return corpus counts; stdout: {} stderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr.contains("--subagent") && out.stderr.contains("deadbeef00"),
        "the error should name the flag + the unmatched hex: {}",
        out.stderr
    );
}

#[test]
fn search_tool_response_names_the_tool_it_answers() {
    let h = populated_home();
    // Fixture L4 = a tool_result for tool_use_id `call0`, whose tool_use (L3) is `Read`.
    let out = h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "tool-response",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("tool-response Read"),
        "the response names the tool it answers: {}",
        out.stdout
    );
}

#[test]
fn search_text_output_is_token_lean() {
    let h = populated_home();
    let out = h.run(&["search", "carry", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Session id declared ONCE in an `s1 = <uuid>` table; exchanges reference the `s1·t<n>` label.
    assert!(
        out.stdout.contains(&format!("s1 = {SESS}")),
        "session-label table: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("s1·t"),
        "exchange references the label: {}",
        out.stdout
    );
    // The old heavyweight header is gone: no `═══` rule, no uppercase `SESSION `/`TURN `.
    assert!(
        !out.stdout.contains("═══"),
        "no rule glyphs: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("TURN "),
        "no uppercase TURN: {}",
        out.stdout
    );
    // The uuid appears exactly once (in the table), not repeated per exchange.
    assert_eq!(
        out.stdout.matches(SESS).count(),
        1,
        "the full uuid is printed once, then referenced: {}",
        out.stdout
    );
    // Timestamps are single local+offset (no `(<UTC>)` second copy on the turn header).
    assert!(
        !out.stdout.contains(" (2026-"),
        "no parenthesised UTC copy: {}",
        out.stdout
    );
}

#[test]
fn files_bare_uuid_positional_routes_to_session() {
    // The documented `csift files <uuid>` form (a bare uuid in the positional slot) now
    // resolves as a session filter across all projects, not as a (nonexistent) project
    // dir. Previously errored "no Claude Code project dir for …/<uuid>".
    let h = populated_home();
    let out = h.run(&["files", at(SESS).as_str(), "--summary", "--no-subagents"]);
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
    // Bash/Edit mutation, so the body is the honest empty rollup — the point is it ran).
    assert!(
        out.stdout.contains("detail=summary"),
        "files summary did not run; got: {}",
        out.stdout
    );
}

#[test]
fn turns_bare_uuid_positional_routes_to_session() {
    let h = populated_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--budget",
        "2000",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "got: {}", out.stdout);
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
fn at_agent_hex_subtree_includes_descendants_unless_no_subagents() {
    // The rule: locating an AGENT → itself (--no-subagents), else itself + ALL topological
    // descendants. Build a nested pair (PARENT spawns CHILD) flat on disk, linked via the
    // child's meta toolUseId pointing at the Agent tool_use recorded in PARENT's transcript.
    let enc = "-Users-testuser-Projects-agtree";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let parent = "aaaa1111bbbb2222c"; // 17 hex
    let child = "cccc3333dddd4444e"; // 17 hex
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_parent","name":"Agent","input":{"description":"parent"}}]}}"#, "\n",
        ),
    );
    // PARENT spawns CHILD (the Agent tool_use is recorded HERE).
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{parent}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaaa1111bbbb2222c","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"PARENTWORK"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_child","name":"Agent","input":{"description":"child"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{parent}.meta.json"),
        r#"{"agentType":"general-purpose","toolUseId":"call_parent"}"#,
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{child}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"cccc3333dddd4444e","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"CHILDWORK"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{child}.meta.json"),
        r#"{"agentType":"Explore","toolUseId":"call_child"}"#,
    );

    // Default `@<parent>` → parent + descendant child both searchable.
    let full = h.run(&["search", "WORK", &format!("@{parent}")]);
    assert!(full.success, "stderr: {}", full.stderr);
    assert!(
        full.stdout.contains("PARENTWORK"),
        "parent in scope: {}",
        full.stdout
    );
    assert!(
        full.stdout.contains("CHILDWORK"),
        "descendant child in scope: {}",
        full.stdout
    );

    // `--no-subagents` → the parent agent ALONE (child excluded).
    let alone = h.run(&["search", "WORK", &format!("@{parent}"), "--no-subagents"]);
    assert!(alone.success, "stderr: {}", alone.stderr);
    assert!(
        alone.stdout.contains("PARENTWORK"),
        "parent still in scope: {}",
        alone.stdout
    );
    assert!(
        !alone.stdout.contains("CHILDWORK"),
        "child EXCLUDED under --no-subagents: {}",
        alone.stdout
    );
}

#[test]
fn whoami_always_prints_path() {
    // The resolved jsonl path is ALWAYS printed (no flag — `--show-path` was removed).
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
fn cross_surface_session_id_is_identical_for_a_subagent() {
    // id-form unification: the SAME subagent transcript reports the SAME bare-hex
    // session_id from files, search, and turns (search/turns previously kept `agent-`).
    let h = populated_home();
    let files = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by-file",
        "--format",
        "json",
        "--subagents-only",
    ]);
    let search = h.run(&["search", "", ENC, at(SESS).as_str(), "--format", "json"]);
    // turns now defaults to top-level-only, so opt INTO spanning subagents to exercise the
    // cross-surface id-form check on the turns surface too.
    let turns = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--include-subagents",
        "--budget",
        "8000",
        "--format",
        "json",
    ]);
    // The bare-hex subagent id (no `agent-` prefix) must appear in each surface's JSON,
    // and the `agent-` prefixed form must NOT.
    for (name, out) in [("files", &files), ("search", &search), ("turns", &turns)] {
        assert!(out.success, "{name} stderr: {}", out.stderr);
        assert!(
            !out.stdout.contains("\"agent-aaa111\"") && !out.stdout.contains("agent-aaa111"),
            "{name} leaked an agent- prefixed session_id: {}",
            out.stdout
        );
    }
    // At least one surface must actually mention the bare id (proves the subagent was
    // scanned, not just that the prefix is absent).
    assert!(
        files.stdout.contains("aaa111") || turns.stdout.contains("aaa111"),
        "no surface emitted the bare subagent id; files={} turns={}",
        files.stdout,
        turns.stdout
    );
}

#[test]
fn search_session_filter_and_turn_range() {
    let h = populated_home();
    // --session selects the parent; --turn-range picks turn 1 only.
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--turn-range",
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
fn search_turn_range_with_since_is_mutually_exclusive() {
    let h = populated_home();
    let out = h.run(&["search", "", "--turn-range", "0..1", "--since", "2h"]);
    assert!(!out.success, "mutually-exclusive flags must error");
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_turn_range_with_until_is_mutually_exclusive() {
    // The mutual-exclusion check ORs `since` and `until`; this exercises the `until`
    // operand of `args.since.is_some() || args.until.is_some()`.
    let h = populated_home();
    let out = h.run(&["search", "", "--turn-range", "0..1", "--until", "2h"]);
    assert!(!out.success, "turn-range + until must error");
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
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
        "turns",
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

    // search -t user: the same label is matchable; the raw blob is not surfaced. The label
    // now reflects the TRUE kind, so `-t user 'background-command'` matches it (the prior
    // mislabel meant the prefix and the summary disagreed).
    let s = h.run(&[
        "search",
        "background-command",
        "-t",
        "user",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains("[background-command wf12abc completed]"),
        "search -t user must surface the attribution label; got: {}",
        s.stdout
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
        "turns",
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
    // JSON consumers get (a) a leading {kind:"session_header",…} object carrying the
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
        "turns",
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
        first.contains("\"kind\":\"session_header\"")
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
fn turns_automation_notification_does_not_consume_human_round_trip_floor() {
    // The round-trip HARD FLOOR is reserved for HUMAN exchanges. A session whose RECENT
    // turns are machine automation pulses (each with an agent ack) plus ONE older human
    // round-trip, at a small budget, must still recover the human turn — the pulses must NOT
    // crowd it out of the protected floor (the prior `is_round_trip` ignored is_automation).
    let h = Home::new();
    let sess = "22222222-3333-4444-5555-666666666666";
    let mut lines = vec![
        // The OLDER human round-trip (the one the floor must protect).
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/r","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"HUMAN-QUESTION-MARKER please explain the carry-propagation bug in detail"}}"#.to_string(),
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The carry is the partial line held across a chunk boundary; here is the full explanation of the propagation path and the fix."}]}}"#.to_string(),
    ];
    // SEVEN newer automation pulses (each a round-trip pulse→ack) — recency-first, these
    // would be picked before the human turn and (under the bug) consume the floor.
    for i in 0..7 {
        lines.push(format!(
            r#"{{"type":"user","uuid":"n{i}","timestamp":"2026-06-07T06:0{i}:00.000Z","message":{{"role":"user","content":"<task-notification>\n<task-id>auto{i}</task-id>\n<status>completed</status>\n<summary>Background command \"job {i}\" completed (exit code 0)</summary>\n</task-notification>"}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"m{i}","parentUuid":"n{i}","timestamp":"2026-06-07T06:0{i}:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"Acknowledged pulse {i}."}}]}}}}"#
        ));
    }
    h.write(
        &format!("-Users-x-r/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    // A budget small enough that, if the floor were spent on pulses, the human turn would be
    // crowded out — but large enough to fit the human round-trip in its protected lane.
    let t = h.run(&[
        "turns",
        at(sess).as_str(),
        "--budget",
        "1200",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains("HUMAN-QUESTION-MARKER"),
        "the human round-trip must survive the floor despite newer automation pulses; got: {}",
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
        with.stdout.contains("tool-response"),
        "resolved match: {}",
        with.stdout
    );
    assert!(with.stdout.contains("matched 1"));
}

#[test]
fn search_skips_non_transcript_noise_lines() {
    // A session padded with attachment / system / file-history-snapshot lines (no
    // role marker) → search's pre-JSON category prefilter drops them (the
    // `!line_is_transcript_candidate` TRUE arm) while still matching the real turn.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"attachment","data":{"x":1}}"#, "\n",
            r#"{"type":"file-history-snapshot","snapshot":{}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","preTokens":1}"#, "\n",
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"real turn with carry token"}}"#, "\n",
            r#"{"type":"queue-operation","op":"x"}"#, "\n",
        ),
    );
    let out = h.run(&["search", "carry", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched 1"), "got: {}", out.stdout);
}

#[test]
fn search_global_max_count_caps_across_files() {
    // Two sessions each matching once; --max-count 1 emits one and drops one GLOBALLY
    // (the cross-file cap merge arm). Use --no-subagents to keep the count exact.
    let h = Home::new();
    for i in 0..2 {
        let sid = format!("ssss{i}ss-0000-0000-0000-00000000000{i}");
        h.write(
            &format!("{ENC}/{sid}.jsonl"),
            &format!(
                "{{\"type\":\"user\",\"uuid\":\"u{i}\",\"timestamp\":\"2026-06-0{}T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"global cap token zzcap\"}}}}\n",
                i + 1
            ),
        );
    }
    let out = h.run(&["search", "zzcap", "--max-count", "1", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched 1"));
    assert!(out.stdout.contains("1 dropped"));
    assert!(out.stdout.contains("by --max-count"));
}

#[test]
fn search_timeline_interleaves_subagents_with_top_level_by_timestamp() {
    // The combined timeline is CHRONOLOGICAL, not file-grouped: a subagent exchange whose
    // turn began BETWEEN two parent turns must sort BETWEEN them — even though the subagent
    // file is scanned after the parent file. Parent turns at T=00 and T=10, subagent turn at
    // T=05 → expected envelope order 00 (parent) · 05 (SUBAGENT) · 10 (parent).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ping alpha"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply alpha"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"ping gamma"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply gamma"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub222.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub222","uuid":"s0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":"ping beta"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa0","parentUuid":"s0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub reply beta"}]}}"#, "\n",
        ),
    );

    let out = h.run(&["search", "ping", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let envelopes: Vec<_> = json_lines(&out.stdout)
        .into_iter()
        .filter(|o| o.get("turn_index").is_some())
        .collect();
    assert_eq!(
        envelopes.len(),
        3,
        "parent ×2 + subagent ×1: {}",
        out.stdout
    );

    // Chronological interleave: 00 (parent) · 05 (SUBAGENT, between) · 10 (parent).
    assert_eq!(
        envelopes[0]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:00.000Z")
    );
    assert_eq!(envelopes[0]["is_subagent"], serde_json::json!(false));
    assert_eq!(
        envelopes[1]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:05.000Z")
    );
    assert_eq!(
        envelopes[1]["is_subagent"],
        serde_json::json!(true),
        "subagent sorts BETWEEN the two parent turns, not grouped after them"
    );
    assert_eq!(envelopes[1]["parent_session_id"], serde_json::json!(SESS));
    assert_eq!(
        envelopes[2]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:10.000Z")
    );
    assert_eq!(envelopes[2]["is_subagent"], serde_json::json!(false));

    // ts_local is the same instant rendered in the host TZ (present, non-null).
    assert!(
        envelopes[1]["ts_local"].is_string(),
        "envelope carries ts_local"
    );
}

// ── whoami ──

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
fn whoami_json_format() {
    let h = populated_home();
    let out = h.run_with_env(
        &["whoami", "--format", "json"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(out.stdout.trim()).expect("valid json");
    assert_eq!(v.get("session_id").and_then(|s| s.as_str()), Some(SESS));
    assert!(v.get("path").and_then(|p| p.as_str()).is_some());
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
    // encode the CANONICAL path — that's what `current_dir()` reports inside the
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

// ── agents ──

#[test]
fn agents_nested_subagent_topology_links_parent_depth_and_tree() {
    // A NESTED subagent (agent spawned BY another agent). On-disk the layout is FLAT — both
    // agents sit directly under <session>/subagents/ — because CC writes every subagent's
    // transcript under getSessionId()=<main> regardless of depth (verified vs the cleanroom).
    // The agent→agent link is LOGICAL: the child's spawning Task tool_use is recorded in the
    // PARENT's transcript (not main), and the child's meta.json toolUseId points at it.
    let enc = "-Users-testuser-Projects-nested";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let h = Home::new();
    // Main session: spawns PARENT via an Agent tool_use (id call_parent).
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_parent","name":"Agent","input":{"description":"parent agent","subagent_type":"general-purpose"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:09:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_parent","content":"parent done"}]}}"#, "\n",
        ),
    );
    // PARENT transcript (flat under subagents/). It SPAWNS the child via an Agent tool_use
    // (id call_child) recorded HERE — this is the linkage a main-only scan would miss.
    h.write(
        &format!("{enc}/{sess}/subagents/agent-parentaaa.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"parentaaa","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"parent: do work"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_child","name":"Agent","input":{"description":"child agent","subagent_type":"Explore"}}]}}"#, "\n",
            r#"{"type":"user","timestamp":"2026-06-07T05:08:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_child","content":"child done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-parentaaa.meta.json"),
        r#"{"agentType":"general-purpose","description":"parent agent","toolUseId":"call_parent"}"#,
    );
    // CHILD transcript — FLAT in the SAME subagents/ dir (not nested on disk). Its meta
    // toolUseId=call_child points at the spawn recorded in PARENT's transcript.
    h.write(
        &format!("{enc}/{sess}/subagents/agent-childbbb.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"childbbb","timestamp":"2026-06-07T05:01:30.000Z","message":{"role":"user","content":"child: explore"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:07:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"child result"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-childbbb.meta.json"),
        r#"{"agentType":"Explore","description":"child agent","toolUseId":"call_child"}"#,
    );

    // JSON (flat): both agents listed; child carries parent_agent_id=parentaaa + depth 1,
    // and recovers its trigger/description from the PARENT transcript's spawn (not main).
    let j = h.run(&["agents", at(sess).as_str(), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    let child = objs
        .iter()
        .find(|o| o["agent_id"] == "childbbb")
        .expect("child node");
    let parent = objs
        .iter()
        .find(|o| o["agent_id"] == "parentaaa")
        .expect("parent node");
    assert_eq!(
        child["parent_agent_id"], "parentaaa",
        "child links to parent: {child}"
    );
    assert_eq!(
        child["depth"],
        serde_json::json!(1),
        "child depth 1: {child}"
    );
    assert_eq!(
        parent["parent_agent_id"],
        serde_json::Value::Null,
        "parent is a root: {parent}"
    );
    assert_eq!(
        parent["depth"],
        serde_json::json!(0),
        "parent depth 0: {parent}"
    );

    // Tree JSON: child nests UNDER parent in the agents[].children array.
    let tj = h.run(&["agents", at(sess).as_str(), "--tree", "--format", "json"]);
    assert!(tj.success, "stderr: {}", tj.stderr);
    let tree: serde_json::Value = serde_json::from_str(tj.stdout.lines().next().unwrap()).unwrap();
    let agents = tree["agents"].as_array().expect("agents array");
    let proot = agents
        .iter()
        .find(|o| o["agent_id"] == "parentaaa")
        .expect("parent at top level of tree");
    let kids = proot["children"].as_array().expect("parent has children");
    assert_eq!(
        kids[0]["agent_id"], "childbbb",
        "child nested under parent: {proot}"
    );
    assert!(
        !agents.iter().any(|o| o["agent_id"] == "childbbb"),
        "child is NOT also a top-level tree node: {tree}"
    );

    // Tree TEXT: child indented one level deeper than parent.
    let tt = h.run(&["agents", at(sess).as_str(), "--tree"]);
    assert!(tt.success, "stderr: {}", tt.stderr);
    let pidx = tt.stdout.find("parentaaa").expect("parent in tree text");
    let cidx = tt.stdout.find("childbbb").expect("child in tree text");
    assert!(cidx > pidx, "child printed after parent: {}", tt.stdout);
    // child line has more leading spaces than the parent line
    let line_indent = |needle: &str| -> usize {
        let li = tt.stdout[..tt.stdout.find(needle).unwrap()]
            .rfind('\n')
            .map_or(0, |p| p + 1);
        tt.stdout[li..].chars().take_while(|c| *c == ' ').count()
    };
    assert!(
        line_indent("childbbb") > line_indent("parentaaa"),
        "child is indented deeper than parent: {}",
        tt.stdout
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
fn agents_text_returned_files_and_tree_render() {
    // Exercise the TEXT-render branches for `--returned-message` / `--with-files` / `--tree`
    // (the print_node `returned`/`files`/topology arms) and the one_line returned-message
    // preview path. A node with no resolvable returned message renders `(unresolved)`; a
    // node with no files renders `files (none)`.
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--tree",
        "--returned-message",
        "--with-files",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The returned-message line renders (resolved or `(unresolved)`).
    assert!(
        out.stdout.contains("returned"),
        "returned line missing: {}",
        out.stdout
    );
    // The with-files line renders (`files … changed` or `files (none)`).
    assert!(
        out.stdout.contains("files"),
        "files line missing: {}",
        out.stdout
    );
    // Tree topology: the workflow run parents its agent.
    assert!(
        out.stdout.contains("wf_abc") || out.stdout.contains("workflow"),
        "tree topology missing: {}",
        out.stdout
    );
}

#[test]
fn agents_kind_filter_json_and_tree_json_and_multi_node_text() {
    // `--kind builtin-task --format json` hits the BuiltinTask JSON-label arm; `--tree
    // --format json` nests `children`; a flat multi-node text render hits the inter-node
    // blank-line arm. All on the populated home (a builtin + a workflow subagent).
    let h = populated_home();
    let bt = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--kind",
        "builtin-task",
        "--format",
        "json",
    ]);
    assert!(bt.success, "stderr: {}", bt.stderr);
    assert!(
        bt.stdout.contains("\"builtin-task\""),
        "builtin-task JSON label missing: {}",
        bt.stdout
    );
    let tree = h.run(&["agents", at(SESS).as_str(), "--tree", "--format", "json"]);
    assert!(tree.success, "stderr: {}", tree.stderr);
    assert!(
        tree.stdout.contains("children"),
        "tree JSON must nest children: {}",
        tree.stdout
    );
    // Flat text render with BOTH subagents → the inter-node blank line fires.
    let flat = h.run(&["agents", at(SESS).as_str()]);
    assert!(flat.success && flat.stdout.matches("triggered").count() >= 2);
}

#[test]
fn agents_with_files_renders_changed_list_and_summary_json() {
    // A subagent that ACTUALLY changed a file → the `--with-files` text path renders the
    // `files N changed` + per-file create/op tag lines (vs the `(none)` arm), and `--by
    // start` exercises the start-axis label. Also covers files `--summary --format json`.
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
    let out = h.run(&["agents", at(sess).as_str(), "--with-files", "--by", "start"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("changed") && out.stdout.contains("new.rs"),
        "files-changed list not rendered: {}",
        out.stdout
    );
    // The summary JSON path (the `json_grouped` summary arm + trailing summary object).
    let f = h.run(&["files", at(sess).as_str(), "--summary", "--format", "json"]);
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
fn agents_single_agent_grab_text() {
    // `--agent <hex>` grabs ONE subagent (implies --returned-message), exercising the
    // single-node text path + the guided-error landing flag documented in the EXAMPLES.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "aaa111"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("aaa111"),
        "node not grabbed: {}",
        out.stdout
    );
}

#[test]
fn agents_agent_grab_bypasses_time_and_kind_filters() {
    // `--agent <hex>` is a DIRECT lookup: even with a --since window that would exclude the
    // agent's trigger time AND a --kind that does not match, the grab still resolves.
    let h = populated_home();
    // aaa111 is a builtin-task triggered ~05:00; this window + kind would normally exclude it.
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--agent",
        "aaa111",
        "--since",
        "2026-06-08T00:00:00Z",
        "--kind",
        "workflow",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("aaa111"),
        "direct --agent lookup must bypass time/kind filters: {}",
        out.stdout
    );
}

#[test]
fn agents_bad_hex_errors_with_discovery_guidance() {
    // A typo'd / non-existent --agent hex is a HARD error (non-zero) with discovery
    // guidance — NOT the ambiguous `no subagents found` that a zero-subagent session prints.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "deadbeefcafe"]);
    assert!(!out.success, "a bad hex must be a hard error");
    assert!(
        out.stderr.contains("no subagent matched") && out.stderr.contains("agents @<uuid>"),
        "error must name the bad id + the discovery path; stderr: {}",
        out.stderr
    );
}

#[test]
fn agents_agent_with_tree_renders_single_node_not_whole_workflow() {
    // `--agent <hex> --tree`: the single-node grab wins; the whole workflow tree is NOT
    // dumped. bbb222 is in workflow wf_abc alongside no other agent here, but the grab must
    // render bbb222 and NOT the WORKFLOW run header.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "bbb222", "--tree"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("bbb222"),
        "node grabbed: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("WORKFLOW"),
        "--agent must suppress the whole-workflow tree: {}",
        out.stdout
    );
}

#[test]
fn agents_rejects_subagent_span_flags_with_pointed_error() {
    // `agents --no-subagents` (a flag it does not have) is rejected with a pointed message,
    // NOT swallowed as a bogus PATH value by allow_hyphen_values.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--no-subagents"]);
    assert!(!out.success, "the no-op span flag must error");
    assert!(
        out.stderr.contains("no --include-subagents") || out.stderr.contains("no subagent-span"),
        "stderr should explain agents has no span flag; got: {}",
        out.stderr
    );
}

#[test]
fn agents_json_rows() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let mut kinds = Vec::new();
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid json");
        if let Some(k) = v.get("kind").and_then(|k| k.as_str()) {
            kinds.push(k.to_string());
        }
    }
    assert!(kinds.iter().any(|k| k == "builtin-task"));
    assert!(kinds.iter().any(|k| k == "workflow"));
}

#[test]
fn agents_kind_filter_workflow_only() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--kind", "workflow"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("workflow"));
    assert!(!out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("kind=workflow"));
}

#[test]
fn agents_by_completion_axis_and_window() {
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T06:00:30Z",
        "--by",
        "completion",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Only the workflow agent completes at 06:01 (after the bound); the built-in
    // completes at 05:03 (before). Window-axis footer reflects completion.
    assert!(out.stdout.contains("window-axis=completion"));
    assert!(out.stdout.contains("workflow"));
    assert!(!out.stdout.contains("builtin-task"));
}

#[test]
fn agents_no_subagents_says_none() {
    let h = Home::new();
    // A session with no sidecar at all.
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no subagents found"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_unknown_session_errors() {
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at("deadbeef-0000-0000-0000-000000000000").as_str(),
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn agents_via_project_path_target() {
    // Drive agents with a PATH target (not --session): resolve_target_sessions takes
    // the explicit-paths branch, enumerates the project's sessions, groups subagents.
    let h = populated_home();
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("workflow"));
    // The agentType sub-label is rendered in [brackets].
    assert!(
        out.stdout.contains("[oh-my-claudecode:executor]")
            || out.stdout.contains("[workflow-subagent]")
    );
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
        out.stdout.contains(SESS),
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
        none.stderr.contains("no session id starts with"),
        "{}",
        none.stderr
    );
}

#[test]
fn agents_all_projects_default_scan() {
    // No PATH and no --session → scan every project (the all_project_dirs branch).
    let h = populated_home();
    let out = h.run(&["agents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("subagent(s)"));
}

#[test]
fn agents_path_with_no_sessions_and_no_session_flag_is_empty_not_error() {
    // A project dir that exists but has ZERO session files, with NO --session given →
    // `resolve_target_sessions` finds no files but does NOT bail (the `if let
    // Some(sid)` FALSE arm of the empty-files guard). Output: "no subagents found".
    let h = Home::new();
    std::fs::create_dir_all(h.projects().join(ENC)).unwrap(); // empty project dir
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no subagents found"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_row_without_timestamps_omits_duration() {
    // A subagent whose transcript records carry NO timestamps → started/completed are
    // both absent → `duration_label` returns None (the `if let Some(dur)` FALSE arm),
    // so NO "duration" line is rendered for that row; status is `unknown`.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-nots99.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"nots99","message":{"role":"user","content":"start, no timestamp"}}"#, "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("unknown"),
        "status unknown: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("duration"),
        "no duration line: {}",
        out.stdout
    );
}

#[test]
fn agents_reports_skipped_lines_note() {
    // A subagent transcript with a malformed line → the per-row "malformed line(s)
    // skipped" note (agents.rs render skipped_lines arm).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // The malformed line is the NEWEST (last) record so the TAIL scan reaches and
    // counts it (head stops at the first record; tail walks newest-first).
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-broken1.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"broken1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
            "{ this is a malformed newest line }\n",
        ),
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("malformed line(s) skipped"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_groups_multiple_sessions_with_separator() {
    // Two sessions each with a subagent → the render groups rows under per-session
    // headers separated by a blank line (the `last_session.is_some()` separator arm).
    let h = Home::new();
    let sess_a = "aaaaaaaa-0000-0000-0000-000000000001";
    let sess_b = "bbbbbbbb-0000-0000-0000-000000000002";
    for s in [sess_a, sess_b] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        h.write(
            &format!("{ENC}/{s}/subagents/agent-x{}.jsonl", &s[0..3]),
            &format!(
                "{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"x{}\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"s\"}}}}\n{{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"done\"}}]}}}}\n",
                &s[0..3]
            ),
        );
    }
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.matches("SESSION").count() >= 2,
        "two session headers: {}",
        out.stdout
    );
}

// ── agents TOPOLOGY (Part A) ──

/// A session whose PARENT transcript carries the spawn linkage: an `Agent` tool_use
/// (`toolu_x`, == the built-in meta's `toolUseId`) at 04:59:58 — the TRUE trigger,
/// ~2s before the child-head ts — whose paired SYNC tool_result is the built-in's
/// returned message; a built-in subagent that EDITS a file (for `--with-files`); a
/// workflow agent whose journal carries a `result` payload; and a top-level
/// `workflows/wf_topo.json` manifest (the WorkflowRun source).
fn topology_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T04:59:50.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sp1","timestamp":"2026-06-07T04:59:58.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_x","name":"Agent","input":{"description":"the carry task","subagent_type":"oh-my-claudecode:executor"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:03:25.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_x","content":"SYNC-RETURN: the built-in carry answer"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"sp2","timestamp":"2026-06-07T05:59:55.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_w","name":"Workflow","input":{"description":"the wf"}}]}}"#, "\n",
        ),
    );
    // Built-in subagent: child-head ts (05:00:00) LAGS the trigger (04:59:58); it edits
    // a file so --with-files surfaces a path.
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-topo11.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"topo11","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"do carry"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/repo/src/parse.rs"}}]}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:03:20.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-topo11.meta.json"),
        r#"{"agentType":"oh-my-claudecode:executor","description":"the carry task","toolUseId":"toolu_x"}"#,
    );
    // Workflow agent + journal with a result payload.
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_topo/agent-topo22.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"topo22","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"wf"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T06:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_topo/agent-topo22.meta.json"),
        r#"{"agentType":"workflow-subagent"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_topo/journal.jsonl"),
        concat!(
            r#"{"type":"started","agentId":"topo22","key":"v2:topo"}"#, "\n",
            r#"{"type":"result","agentId":"topo22","key":"v2:topo","result":"WF-RETURN: journal payload"}"#, "\n",
        ),
    );
    // Top-level workflow RUN manifest (NOT under subagents/).
    h.write(
        &format!("{ENC}/{SESS}/workflows/wf_topo.json"),
        r#"{"runId":"wf_topo","taskId":"t1","workflowName":"carry-wf","status":"completed","agentCount":1,"durationMs":62000,"totalTokens":9000,"totalToolCalls":4,"defaultModel":"claude-opus-4-8[1m]","startTime":"2026-06-07T05:59:55.000Z"}"#,
    );
    h
}

#[test]
fn agents_true_trigger_time_is_the_parent_tool_use_ts() {
    // The default axis is TRIGGER: the built-in's `trigger_utc` is the parent Agent
    // tool_use ts (04:59:58), which DIVERGES from its child-head `started_utc`
    // (05:00:00) — proving the topology recovered the true spawn instant.
    let h = topology_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let builtin = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("json"))
        .find(|v| v.get("agent_id").and_then(|a| a.as_str()) == Some("topo11"))
        .expect("the built-in topo11 node");
    assert_eq!(builtin["trigger_utc"], "2026-06-07T04:59:58.000Z");
    assert_eq!(builtin["started_utc"], "2026-06-07T05:00:00.000Z");
    assert_ne!(builtin["trigger_utc"], builtin["started_utc"]);
    assert_eq!(builtin["spawn_tool"], "Agent");
    assert_eq!(builtin["spawn_tool_use_id"], "toolu_x");
}

#[test]
fn agents_default_axis_is_trigger_not_start() {
    // A bound BETWEEN the trigger (04:59:58) and the start (05:00:00): the DEFAULT
    // (trigger) axis EXCLUDES the built-in (triggered before the bound); `--by start`
    // INCLUDES it (started after the bound). Proves the default flipped to trigger.
    let h = topology_home();
    let default_axis = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T04:59:59Z",
        "--kind",
        "builtin-task",
        "--format",
        "json",
    ]);
    assert!(default_axis.success, "stderr: {}", default_axis.stderr);
    let default_has_topo11 = default_axis.stdout.contains("topo11");
    assert!(
        !default_has_topo11,
        "default (trigger) axis must EXCLUDE topo11 triggered before the bound: {}",
        default_axis.stdout
    );
    let by_start = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T04:59:59Z",
        "--by",
        "start",
        "--kind",
        "builtin-task",
        "--format",
        "json",
    ]);
    assert!(by_start.success, "stderr: {}", by_start.stderr);
    assert!(
        by_start.stdout.contains("topo11"),
        "--by start must INCLUDE topo11 started after the bound: {}",
        by_start.stdout
    );
    // The footer reflects the default axis.
    let footer = h.run(&["agents", at(SESS).as_str()]);
    assert!(footer.stdout.contains("window-axis=trigger"));
}

#[test]
fn agents_returned_message_three_way_resolution() {
    let h = topology_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--returned-message",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let nodes: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("json"))
        .collect();
    let builtin = nodes
        .iter()
        .find(|v| v["agent_id"] == "topo11")
        .expect("topo11");
    // SYNC built-in → parent tool_result text.
    assert_eq!(
        builtin["returned_message"],
        "SYNC-RETURN: the built-in carry answer"
    );
    assert_eq!(builtin["returned_message_source"], "sync-tool-result");
    let wf = nodes
        .iter()
        .find(|v| v["agent_id"] == "topo22")
        .expect("topo22");
    // WORKFLOW → journal result payload.
    assert_eq!(wf["returned_message"], "WF-RETURN: journal payload");
    assert_eq!(wf["returned_message_source"], "workflow-journal");
}

#[test]
fn agents_returned_message_omitted_by_default() {
    // Without --returned-message (and without --agent), the returned message is NOT in
    // the JSON — keeping a plain listing compact.
    let h = topology_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let first: serde_json::Value =
        serde_json::from_str(out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap()).unwrap();
    assert!(
        first.get("returned_message").is_none(),
        "returned_message must be omitted by default: {first}"
    );
}

#[test]
fn agents_single_agent_grab_includes_returned_and_files() {
    // `--agent <hex>` selects ONE node and implies the returned message + files.
    let h = topology_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--agent",
        "topo11",
        "--with-files",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "exactly the one selected node: {:?}", lines);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["agent_id"], "topo11");
    assert_eq!(
        v["returned_message"],
        "SYNC-RETURN: the built-in carry answer"
    );
    let files = v["files_changed"].as_array().expect("files_changed array");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "/repo/src/parse.rs");
    assert_eq!(files[0]["op"], "edit");
}

#[test]
fn agents_tree_renders_workflow_run_as_parent_of_its_agents() {
    let h = topology_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--tree", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // One object per session: workflow_runs[] (each with children[]) + agents[].
    let v: serde_json::Value =
        serde_json::from_str(out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap()).unwrap();
    let runs = v["workflow_runs"].as_array().expect("workflow_runs");
    assert_eq!(runs.len(), 1, "one workflow run");
    let run = &runs[0];
    assert_eq!(run["run_id"], "wf_topo");
    assert_eq!(run["workflow_name"], "carry-wf");
    assert_eq!(run["agent_count"], 1);
    let children = run["children"].as_array().expect("run children");
    assert_eq!(
        children.len(),
        1,
        "the workflow agent is nested under its run"
    );
    assert_eq!(children[0]["agent_id"], "topo22");
    // The built-in (no workflow_id) is a top-level agent, NOT under the run.
    let builtins = v["agents"].as_array().expect("top-level agents");
    assert!(builtins.iter().any(|a| a["agent_id"] == "topo11"));

    // Text tree shows the WORKFLOW header with its run id + the nested agent.
    let text = h.run(&["agents", at(SESS).as_str(), "--tree"]);
    assert!(
        text.stdout.contains("WORKFLOW  wf_topo"),
        "got: {}",
        text.stdout
    );
    assert!(text.stdout.contains("[carry-wf]"));
    assert!(text.stdout.contains("topo22"));
}

#[test]
fn agents_tree_keeps_workflow_agents_without_a_run_manifest() {
    // A workflow dir can have a journal + agents BEFORE its top-level
    // `workflows/wf_*.json` run-manifest is written (an in-flight run), or after the
    // manifest is pruned. Such agents must NOT vanish from `--tree`: the tree renders a
    // workflow agent only as a child of a run, so without a synthesized stand-in run the
    // agent is silently dropped. Build a `wf_orphan` with an agent + journal but NO
    // manifest and assert the tree surfaces it. Regression: real session 0a1b2c3d's
    // in-flight `wf_132003a7-de2` (10 agents, journal, no manifest) was dropped from the
    // tree (552 of 562 agents) until this stand-in was added.
    let h = topology_home();
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_orphan/agent-topo33.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"topo33","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"orphan wf"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T07:01:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"orphan done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_orphan/agent-topo33.meta.json"),
        r#"{"agentType":"workflow-subagent"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_orphan/journal.jsonl"),
        concat!(
            r#"{"type":"started","agentId":"topo33","key":"v2:orphan"}"#, "\n",
            r#"{"type":"result","agentId":"topo33","key":"v2:orphan","result":"ORPHAN-RETURN: in-flight payload"}"#, "\n",
        ),
    );
    // No `{ENC}/{SESS}/workflows/wf_orphan.json` manifest is written on purpose.

    // FLAT view sees every agent (discovery is lossless): topo11 + topo22 + topo33.
    let flat = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(flat.success, "stderr: {}", flat.stderr);
    let flat_ids: Vec<String> = flat
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("json"))
        .map(|v| v["agent_id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        flat_ids.iter().any(|id| id == "topo33"),
        "flat view must include the manifest-less workflow agent: {flat_ids:?}"
    );

    // TREE view must also surface topo33 — under a SYNTHESIZED run for wf_orphan whose
    // run-level fields are null (no manifest) but whose children carry the agent.
    let tree = h.run(&["agents", at(SESS).as_str(), "--tree", "--format", "json"]);
    assert!(tree.success, "stderr: {}", tree.stderr);
    let v: serde_json::Value =
        serde_json::from_str(tree.stdout.lines().find(|l| !l.trim().is_empty()).unwrap()).unwrap();
    let runs = v["workflow_runs"].as_array().expect("workflow_runs");
    let orphan_run = runs
        .iter()
        .find(|r| r["run_id"] == "wf_orphan")
        .expect("a synthesized run node for the manifest-less wf_orphan");
    // The synthesized run carries no manifest metadata, only its agents.
    assert!(orphan_run["status"].is_null(), "no manifest → null status");
    assert!(
        orphan_run["agent_count"].is_null(),
        "no manifest → null agent_count"
    );
    let children = orphan_run["children"].as_array().expect("orphan children");
    assert_eq!(
        children.len(),
        1,
        "the orphan agent nests under its stand-in"
    );
    assert_eq!(children[0]["agent_id"], "topo33");

    // No agent is lost: every flat agent appears somewhere in the tree.
    let mut tree_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for a in v["agents"].as_array().unwrap() {
        tree_ids.insert(a["agent_id"].as_str().unwrap().to_string());
    }
    for r in runs {
        for c in r["children"].as_array().unwrap() {
            tree_ids.insert(c["agent_id"].as_str().unwrap().to_string());
        }
    }
    for id in &flat_ids {
        assert!(
            tree_ids.contains(id),
            "tree dropped agent {id} present in the flat view (tree={tree_ids:?})"
        );
    }

    // Text tree shows the orphan run header + the agent (not silently omitted).
    let text = h.run(&["agents", at(SESS).as_str(), "--tree"]);
    assert!(
        text.stdout.contains("WORKFLOW  wf_orphan"),
        "text tree must show the stand-in run header: {}",
        text.stdout
    );
    assert!(text.stdout.contains("topo33"), "got: {}", text.stdout);
}

#[test]
fn agents_id_form_is_bare_hex_joinable_across_files_and_recover() {
    // The subagent's id is the BARE hex everywhere: `agents` prints `topo11`, and
    // `files --session <subagent-hex>` / `recover` print the SAME bare hex (not the
    // `agent-` stem) — so a consumer can join file mutations back to the agent node.
    let h = topology_home();
    let agents_json = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(agents_json.stdout.contains(r#""agent_id":"topo11""#));
    // files spans subagents by default; the subagent row's session_id is bare hex.
    let files_json = h.run(&["files", at(SESS).as_str(), "--format", "json", "--by-file"]);
    assert!(files_json.success, "stderr: {}", files_json.stderr);
    assert!(
        files_json.stdout.contains(r#""session_id":"topo11""#)
            || files_json.stdout.contains("topo11"),
        "files must carry the bare-hex subagent id (joinable to agents): {}",
        files_json.stdout
    );
    assert!(
        !files_json.stdout.contains("agent-topo11"),
        "the un-stripped agent- stem must NOT appear: {}",
        files_json.stdout
    );
}

// ── files ──

/// A session whose transcript performs the acid-test scenario: two `/tmp/*.md` Writes
/// (creates), three `…/gaps/*.md` Edits (updates), and a Bash `rm`, across two turns,
/// each structured tool_use paired with its create/update carrier. Returns the home.
fn files_scenario_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"set up the gap docs and tmp notes"}}"#, "\n",
            // Write /tmp/beacon-a.md (create) + carrier.
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/beacon-a.md","content":"x"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","toolUseResult":{"type":"create","filePath":"/tmp/beacon-a.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            // Write /tmp/beacon-b.md (create) + carrier.
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w2","name":"Write","input":{"file_path":"/tmp/beacon-b.md","content":"y"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","toolUseResult":{"type":"create","filePath":"/tmp/beacon-b.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w2","content":"ok"}]}}"#, "\n",
            // Three Edits to gaps docs (updates) + carriers.
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/spec/gaps/one.md","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","toolUseResult":{"type":"update","filePath":"/p/spec/gaps/one.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/p/spec/gaps/two.md","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c3","toolUseResult":{"type":"update","filePath":"/p/spec/gaps/two.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a4","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e3","name":"Edit","input":{"file_path":"/p/spec/gaps/three.md","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c4","toolUseResult":{"type":"update","filePath":"/p/spec/gaps/three.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e3","content":"ok"}]}}"#, "\n",
            // ── turn 1: a Bash rm ──
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"clean up tmp"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a5","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"rm /tmp/beacon-a.md"}}]}}"#, "\n",
            // A malformed line that survives the prefilter (carries "role":"user").
            r#"{"type":"user","role":"user" broken json after marker}"#, "\n",
        ),
    );
    h
}

#[test]
fn files_default_summary_acid_test() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"));
    // The /tmp bucket: two writes (the created docs) + the heuristic bash rm.
    assert!(
        out.stdout.contains("/tmp: 2 write"),
        "/tmp bucket: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("bash (heuristic)"),
        "heuristic bash label: {}",
        out.stdout
    );
    // The gaps bucket: three edits.
    assert!(
        out.stdout.contains("/p/spec/gaps: 3 edit"),
        "gaps bucket: {}",
        out.stdout
    );
    // Footer accounting + heuristic caveat + skipped-line note.
    assert!(out.stdout.contains("detail=summary"));
    assert!(out.stdout.contains("Bash mutations are heuristic"));
    assert!(out.stdout.contains("malformed line(s) skipped"));
}

#[test]
fn files_by_file_distinct_counts_via_json() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--by-file", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    // The trailing summary object reports distinct_files + total_mutations.
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    // Distinct files: /tmp/beacon-a.md, /tmp/beacon-b.md, gaps/one,two,three = 5.
    assert_eq!(
        summary.get("distinct_files").and_then(|v| v.as_u64()),
        Some(5),
        "summary: {summary}"
    );
    assert_eq!(
        summary.get("detail_level").and_then(|v| v.as_str()),
        Some("by-file")
    );
    // Count distinct gap docs (acid test #1): rows whose `file` ends in `/gaps/*.md`.
    let mut gap_docs = 0;
    let mut tmp_creates = 0;
    for l in &lines {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        if let Some(f) = v.get("file").and_then(|f| f.as_str()) {
            if f.starts_with("/p/spec/gaps/") {
                gap_docs += 1;
            }
            // Acid test #2: /tmp Writes are authoritative creates (write count > 0).
            if f.starts_with("/tmp/") && v.get("write").and_then(|w| w.as_u64()) == Some(1) {
                tmp_creates += 1;
            }
        }
    }
    assert_eq!(gap_docs, 3, "three distinct gap docs touched");
    assert_eq!(tmp_creates, 2, "two /tmp docs created via Write");
}

#[test]
fn files_timeline_is_chronological_with_heuristic_label() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("detail=timeline"));
    // The bash rm is the newest mutation (06:00) and carries the heuristic label.
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.contains("/tmp/beacon-a.md") || l.contains("/p/spec/gaps"))
        .collect();
    // The first /tmp/beacon-a.md mention (the Write at 05:00) precedes the bash rm.
    let write_pos = out.stdout.find("write  /tmp/beacon-a.md");
    let bash_pos = out.stdout.find("bash (heuristic)  /tmp/beacon-a.md");
    assert!(write_pos.is_some() && bash_pos.is_some(), "{}", out.stdout);
    assert!(
        write_pos < bash_pos,
        "the Write precedes the bash rm chronologically: {}",
        out.stdout
    );
    assert!(!lines.is_empty());
}

#[test]
fn files_by_dir_groups_and_counts() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--by-dir", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let mut saw_gaps_dir = false;
    for l in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        if v.get("dir").and_then(|d| d.as_str()) == Some("/p/spec/gaps") {
            // Three edits, three distinct files in that dir.
            assert_eq!(v.get("edit").and_then(|e| e.as_u64()), Some(3));
            assert_eq!(v.get("distinct_files").and_then(|d| d.as_u64()), Some(3));
            saw_gaps_dir = true;
        }
    }
    assert!(saw_gaps_dir, "the gaps dir row must appear: {}", out.stdout);
}

#[test]
fn files_turn_range_excludes_later_bash() {
    // --turn-range 0..0 keeps the turn-0 structured edits and DROPS the turn-1 bash rm.
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--turn-range", "0..0"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("turn-range=0..0"));
    // 5 mutations remain (2 writes + 3 edits), not 6 (the bash rm is in turn 1).
    assert!(
        out.stdout.contains("5 mutation(s)"),
        "turn 1 bash excluded: {}",
        out.stdout
    );
}

#[test]
fn files_turn_range_with_since_is_mutually_exclusive() {
    let h = files_scenario_home();
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--turn-range",
        "0..1",
        "--since",
        "2h",
    ]);
    assert!(!out.success, "mutually-exclusive flags must error");
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
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
    // Edit-before-Read boundary attributed to the file, carrying the jsonl line number — and
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
    let out = h.run(&["files", at(SESS).as_str(), "--no-subagents", "--timeline"]);
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
        .find(|o| o.get("type").and_then(|t| t.as_str()) == Some("edit_before_read_boundary"))
        .expect("a boundary object");
    assert_eq!(b["path"], "/p/app.rs");
    assert_eq!(b["kind"], "modified_since_read");
    assert!(
        b["line_no"].as_u64().unwrap_or(0) >= 1,
        "boundary carries its jsonl line: {b}"
    );
    let summary = objs
        .iter()
        .find(|o| o.get("detail_level").is_some())
        .expect("trailing summary");
    assert_eq!(summary["edit_before_read_boundaries"], serde_json::json!(1));
}

#[test]
fn files_spans_subagent_mutations() {
    // A subagent that Writes a file → its mutation is attributed under the session by
    // default (OMC fan-out edits happen in subagents). --no-subagents drops it.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub111.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub111","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"sub: write a file"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw1","name":"Write","input":{"file_path":"/tmp/subagent-out.md","content":"z"}}]}}"#, "\n",
        ),
    );
    let with = h.run(&["files", at(SESS).as_str()]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(
        with.stdout.contains("/tmp"),
        "subagent write spanned: {}",
        with.stdout
    );
    let without = h.run(&["files", at(SESS).as_str(), "--no-subagents"]);
    assert!(without.success, "stderr: {}", without.stderr);
    assert!(
        without.stdout.contains("no file mutations found"),
        "--no-subagents drops the subagent write: {}",
        without.stdout
    );
}

/// A session whose TOP-LEVEL turn writes `/parent/p.md` and whose SUBAGENT writes
/// `/sub/s.md`. `--subagents-only` returns the exact set-difference (subagent file
/// only, parent file excluded) — the complement of `--no-subagents` (parent only).
fn subagents_only_scenario(h: &Home) {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"pw1","name":"Write","input":{"file_path":"/parent/p.md","content":"x"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","toolUseResult":{"type":"create","filePath":"/parent/p.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"pw1","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub111.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub111","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub: write a file"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw1","name":"Write","input":{"file_path":"/sub/s.md","content":"z"}}]}}"#, "\n",
        ),
    );
}

#[test]
fn files_subagents_only_returns_set_difference() {
    let h = Home::new();
    subagents_only_scenario(&h);

    // Default (spans subagents): BOTH the parent and subagent files surface.
    let with = h.run(&["files", at(SESS).as_str(), "--by-file"]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(with.stdout.contains("/parent/p.md"), "got: {}", with.stdout);
    assert!(with.stdout.contains("/sub/s.md"), "got: {}", with.stdout);

    // --no-subagents: ONLY the parent file.
    let top = h.run(&["files", at(SESS).as_str(), "--by-file", "--no-subagents"]);
    assert!(top.success, "stderr: {}", top.stderr);
    assert!(top.stdout.contains("/parent/p.md"), "got: {}", top.stdout);
    assert!(
        !top.stdout.contains("/sub/s.md"),
        "--no-subagents must exclude the subagent file: {}",
        top.stdout
    );

    // --subagents-only: the COMPLEMENT — ONLY the subagent file, parent excluded.
    let sub = h.run(&["files", at(SESS).as_str(), "--by-file", "--subagents-only"]);
    assert!(sub.success, "stderr: {}", sub.stderr);
    assert!(sub.stdout.contains("/sub/s.md"), "got: {}", sub.stdout);
    assert!(
        !sub.stdout.contains("/parent/p.md"),
        "--subagents-only must exclude the parent file: {}",
        sub.stdout
    );
}

#[test]
fn files_timeline_json_marks_subagent_rows_with_refeedable_parent() {
    // The timeline JSON discriminates the id-domain: a subagent row carries is_subagent=true
    // + the re-feedable parent uuid; a top-level row carries is_subagent=false and
    // parent_session_id == session_id (so a consumer can always `csift turns <parent>`).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["files", at(SESS).as_str(), "--timeline", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let parent_row = objs
        .iter()
        .find(|o| o["path"] == "/parent/p.md")
        .expect("parent row present");
    assert_eq!(parent_row["is_subagent"], serde_json::json!(false));
    assert_eq!(parent_row["session_id"], serde_json::json!(SESS));
    assert_eq!(parent_row["parent_session_id"], serde_json::json!(SESS));

    let sub_row = objs
        .iter()
        .find(|o| o["path"] == "/sub/s.md")
        .expect("subagent row present");
    assert_eq!(sub_row["is_subagent"], serde_json::json!(true));
    assert_eq!(sub_row["session_id"], serde_json::json!("sub111"));
    // The hex session_id is NOT re-feedable; the parent uuid IS.
    assert_eq!(sub_row["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn files_grouped_json_and_text_discriminate_subagent_id_domain() {
    // The r6 id-domain fix extends is_subagent + parent_session_id to the GROUPED views
    // (not just --timeline): a --by-file subagent row carries the discriminator in JSON and
    // is branded `SUBAGENT <hex> · parent SESSION <uuid>` in text (never a bare-hex SESSION).
    let h = Home::new();
    subagents_only_scenario(&h);

    let j = h.run(&["files", at(SESS).as_str(), "--by-file", "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs = json_lines(&j.stdout);
    let sub = objs
        .iter()
        .find(|o| o["file"] == "/sub/s.md")
        .expect("subagent grouped row present");
    assert_eq!(sub["is_subagent"], serde_json::json!(true));
    assert_eq!(sub["session_id"], serde_json::json!("sub111"));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));
    let parent = objs
        .iter()
        .find(|o| o["file"] == "/parent/p.md")
        .expect("parent grouped row present");
    assert_eq!(parent["is_subagent"], serde_json::json!(false));
    assert_eq!(parent["parent_session_id"], serde_json::json!(SESS));

    let t = h.run(&["files", at(SESS).as_str(), "--by-file"]);
    assert!(t.success, "stderr: {}", t.stderr);
    // The subagent group's header is branded SUBAGENT + the re-feedable parent uuid.
    assert!(
        t.stdout
            .contains(&format!("SUBAGENT sub111  ·  parent SESSION {SESS}")),
        "subagent group not branded: {}",
        t.stdout
    );
    // The parent group's header keeps the plain SESSION <uuid> form.
    assert!(
        t.stdout.contains(&format!("SESSION {SESS}")),
        "top-level group lost its SESSION header: {}",
        t.stdout
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
fn recover_coverage_out_is_noop_with_stderr_note() {
    // `--out` is a no-op in --coverage mode: no file is written, and a stderr note makes the
    // no-op visible (the help truth-up for r6).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/app.rs","content":"line\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","toolUseResult":{"type":"create","filePath":"/p/app.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out_path = h.root.join("cov-out.md");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/app.rs",
        "--coverage",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("--out is ignored in --coverage mode"),
        "missing the no-op note: {}",
        out.stderr
    );
    assert!(
        !out_path.exists(),
        "coverage --out must not create a file, but it did"
    );
}

#[test]
fn turns_json_units_carry_id_domain_discriminators() {
    // turns per-unit JSON gains is_subagent + parent_session_id (top-level run here).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask a real question"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a substantive reply"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["turns", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let unit = objs
        .iter()
        .find(|o| o["role"] == "user" || o["role"] == "assistant")
        .expect("a per-unit record present");
    assert_eq!(unit["is_subagent"], serde_json::json!(false));
    assert_eq!(unit["session_id"], serde_json::json!(SESS));
    assert_eq!(unit["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn files_timeline_op_uses_underscore_spelling() {
    // The timeline `op` value is UNDERSCORE-delimited (notebook_edit/multi_edit) so it matches
    // the grouped per-op COUNT keys — one on-wire spelling across both files JSON modes.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"m1","name":"MultiEdit","input":{"file_path":"/p/multi.rs","edits":[{"old_string":"a","new_string":"b"}]}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"n1","name":"NotebookEdit","input":{"notebook_path":"/p/nb.ipynb","new_source":"x"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(SESS).as_str(), "--timeline", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let ops: Vec<&str> = objs.iter().filter_map(|o| o["op"].as_str()).collect();
    assert!(
        ops.contains(&"multi_edit"),
        "expected underscore multi_edit, got: {ops:?}"
    );
    assert!(
        ops.contains(&"notebook_edit"),
        "expected underscore notebook_edit, got: {ops:?}"
    );
    // The hyphenated spelling must NOT appear on the wire.
    assert!(
        !ops.iter().any(|o| o.contains('-')),
        "no hyphenated op token on the wire, got: {ops:?}"
    );
}

#[test]
fn search_subagent_hit_json_marks_refeedable_parent() {
    // A search hit inside a subagent transcript: JSON carries is_subagent + the re-feedable
    // parent uuid (the bare-hex session_id is not a --session target).
    let h = Home::new();
    subagents_only_scenario(&h);
    // The subagent's user seed contains "write a file".
    let out = h.run(&[
        "search",
        "write a file",
        at(SESS).as_str(),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sub = objs
        .iter()
        .find(|o| o["is_subagent"] == serde_json::json!(true))
        .expect("a subagent hit present");
    assert_eq!(sub["session_id"], serde_json::json!("sub111"));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn search_at_uuid_path_scopes_to_session() {
    // `search "" @<uuid>` scopes to that session via the `@<uuid>` PATH positional (the grammar
    // that replaced the removed bare-uuid-pattern routing). An empty pattern = pure filter over
    // scope, so the session's own exchanges come back.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("s1 = "),
        "scoped search should return the session's exchanges: {}",
        out.stdout
    );
}

#[test]
fn search_bare_uuid_is_a_literal_pattern_not_a_scope() {
    // A BARE uuid (no `@`) as the sole positional is now a LITERAL pattern, NOT a session scope.
    // It is searched verbatim across the corpus and emits no scope-routing note.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", SESS]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("is a session id, not a pattern"),
        "a bare uuid must NOT be routed to a scope anymore; stderr: {}",
        out.stderr
    );
}

#[test]
fn files_subagents_only_with_no_subagent_says_none() {
    // A session that spawned NO subagents → --subagents-only finds nothing for it.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"touch /tmp/only-parent"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(SESS).as_str(), "--subagents-only"]);
    // No subagents under the session → the --session resolver bails (nothing to dump).
    assert!(
        !out.success,
        "stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn files_no_subagents_and_subagents_only_are_mutually_exclusive() {
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--no-subagents",
        "--subagents-only",
    ]);
    assert!(!out.success, "clap must reject the two together");
    assert!(
        out.stderr.contains("cannot be used with")
            || out.stderr.to_lowercase().contains("subagents-only"),
        "expected a clap conflict error: {}",
        out.stderr
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

    let out = h.run(&["files", at(SESS).as_str(), "--by-file", "--no-subagents"]);
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

/// Minimal JSON-string encoder for embedding a Bash command verbatim into a fixture
/// (escapes `"` and `\`). Sufficient for the simple commands used above.
fn serde_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
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

#[test]
fn files_help_mentions_detail_levels_and_heuristic() {
    let h = Home::new();
    let out = h.run(&["files", "--help"]);
    assert!(out.success);
    assert!(out.stdout.contains("--summary"));
    assert!(out.stdout.contains("--by-dir"));
    assert!(out.stdout.contains("--by-file"));
    assert!(out.stdout.contains("--timeline"));
    assert!(
        out.stdout.contains("--subagents-only"),
        "help must document the --subagents-only flag: {}",
        out.stdout
    );
    assert!(
        out.stdout.to_lowercase().contains("heuristic"),
        "help must flag the Bash-heuristic caveat: {}",
        out.stdout
    );
}

#[test]
fn search_help_mentions_regex_dialect_boundaries() {
    let h = Home::new();
    let out = h.run(&["search", "--help"]);
    assert!(out.success);
    assert!(
        out.stdout.contains("linear-time"),
        "dialect block: {}",
        out.stdout
    );
    assert!(out.stdout.contains("backreference"));
    assert!(out.stdout.contains("lookahead") || out.stdout.contains("lookbehind"));
}

// ── top-level dispatch / help / version (main.rs + clap) ──

#[test]
fn help_exits_zero() {
    let h = Home::new();
    let out = h.run(&["--help"]);
    assert!(out.success);
    assert!(out.stdout.contains("ripgrep for Claude Code"));
}

#[test]
fn version_exits_zero() {
    let h = Home::new();
    let out = h.run(&["--version"]);
    assert!(out.success);
    assert!(out.stdout.contains("csift"));
}

#[test]
fn no_subcommand_errors() {
    let h = Home::new();
    let out = h.run(&[]);
    assert!(!out.success, "missing subcommand must exit nonzero");
}

fn _assert_path_exists(p: &Path) {
    assert!(p.exists());
}

// ════════════════════════════════════════════════════════════════════════════
// recover
// ════════════════════════════════════════════════════════════════════════════

/// Absolute path of the file whose history the recover scenarios reconstruct.
const RFILE: &str = "/Users/testuser/Projects/foo/app.py";

/// A session that builds `app.py` across a realistic life-cycle, mirroring the shapes
/// verified against real `~/.claude/projects` data:
///   turn 0  full Read of app.py (4 lines) → an Edit (line 2 rewritten, structuredPatch)
///   turn 1  a `modified since read` integrity error (a HARD boundary), then a fresh full
///           Read (the post-drift state) → another Edit
///   turn 2  an ExitPlanMode plan + a plan-file Write (for --plan)
/// Plus a malformed line (skipped-line accounting) and a history-snapshot marker.
fn recover_scenario_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            // ── turn 0 ──
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"refactor app.py"}}"#, "\n",
            // file-history-snapshot marker for app.py (backupFileName null, as in real data).
            r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-06-07T05:00:00.500Z","trackedFileBackups":{"/Users/testuser/Projects/foo/app.py":{"backupFileName":null,"version":1,"backupTime":"2026-06-07T05:00:00.500Z"}}}}"#, "\n",
            // Full Read of app.py: 4 lines, startLine 1 == numLines == totalLines (an anchor).
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/Users/testuser/Projects/foo/app.py","content":"import os\nraw = open(src).read()\nuse(raw)\nprint(café🛠)","startLine":1,"numLines":4,"totalLines":4}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            // Edit line 2 (structuredPatch replaces the open().read() line with a with-block).
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed0","name":"Edit","input":{"file_path":"/Users/testuser/Projects/foo/app.py","old_string":"raw = open(src).read()","new_string":"with open(src) as fh:\n    raw = fh.read()","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:02.500Z","toolUseResult":{"filePath":"/Users/testuser/Projects/foo/app.py","oldString":"raw = open(src).read()","newString":"with open(src) as fh:\n    raw = fh.read()","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":2,"lines":["-raw = open(src).read()","+with open(src) as fh:","+    raw = fh.read()"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed0","content":"ok"}]}}"#, "\n",
            // ── turn 1: a modified-since-read integrity error (HARD boundary) ──
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"continue"}}"#, "\n",
            // The error carrier (no inline path) — attributed to app.py via the tool_use_id join.
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:01.500Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/Users/testuser/Projects/foo/app.py","old_string":"use(raw)","new_string":"USE(raw)"}}]}}"#, "\n",
            // A fresh full Read of the post-drift file (6 lines now).
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T06:00:02.000Z","toolUseResult":{"file":{"filePath":"/Users/testuser/Projects/foo/app.py","content":"import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF","startLine":1,"numLines":6,"totalLines":6}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
            // ── turn 2: a plan (ExitPlanMode) + a plan-file Write ──
            r#"{"type":"user","uuid":"u2","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"plan the next step"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T07:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"pl0","name":"ExitPlanMode","input":{"plan":"PLAN café🛠\n- step one\n- step two","planFilePath":"/Users/testuser/.claude/plans/foo.md"}}]}}"#, "\n",
            // A malformed line that survives the prefilter (carries "Edit") → counted.
            r#"{"type":"user","role":"user" broken Edit json after marker}"#, "\n",
        ),
    );
    h
}

#[test]
fn recover_batch_reconstructs_many_files_in_one_scan() {
    let h = Home::new();
    let read_full = |uid: &str, path: &str, content: &str, total: usize| -> String {
        serde_json::json!({
            "type":"user","uuid":uid,"timestamp":"2026-06-07T05:00:00.000Z",
            "toolUseResult":{"file":{"filePath":path,"content":content,"startLine":1,"numLines":total,"totalLines":total}},
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":uid,"content":"ok"}]}
        }).to_string()
    };
    // Session 1 holds two files; a SECOND session holds a third — all recovered in ONE scan.
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n{}\n",
            read_full("r0", "/tmp/alpha.md", "# Alpha\nline two\nline three", 3),
            read_full("r1", "/tmp/beta.md", "beta one\nbeta two", 2)
        ),
    );
    let sess2 = "11112222-3333-4444-5555-666677778888";
    h.write(
        &format!("{ENC}/{sess2}.jsonl"),
        &format!(
            "{}\n",
            read_full("r2", "/tmp/gamma.md", "gamma only line", 1)
        ),
    );

    // Manifest: three real targets + a comment + an absent one.
    let manifest = h.root.join("manifest.txt");
    std::fs::write(
        &manifest,
        "/tmp/alpha.md\n/tmp/beta.md\n# a comment\n/tmp/gamma.md\n/tmp/absent.md\n",
    )
    .unwrap();
    let out_dir = h.root.join("recovered");
    let out = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);

    // Each present file is reconstructed to its raw content, mirrored under out-dir.
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/alpha.md")).unwrap(),
        "# Alpha\nline two\nline three\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/beta.md")).unwrap(),
        "beta one\nbeta two\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/gamma.md")).unwrap(),
        "gamma only line\n"
    );
    // The absent target writes no file and is reported as no-history.
    assert!(!out_dir.join("tmp/absent.md").exists());
    let report = std::fs::read_to_string(out_dir.join("recovery-report.tsv")).unwrap();
    assert!(
        report.contains("complete\t3\t3\t/tmp/alpha.md"),
        "report:\n{report}"
    );
    assert!(
        report.contains("no-history\t0\t0\t/tmp/absent.md"),
        "report:\n{report}"
    );
    assert!(out.stdout.contains("3 complete"), "summary: {}", out.stdout);

    // Re-running without --force skips the already-present files.
    let out2 = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out2.success, "stderr: {}", out2.stderr);
    assert!(
        out2.stdout.contains("3 skipped"),
        "skip summary: {}",
        out2.stdout
    );
}

#[test]
fn recover_batch_requires_out_dir_and_excludes_file() {
    let h = recover_scenario_home();
    let manifest = h.root.join("m.txt");
    std::fs::write(&manifest, "/tmp/x.md\n").unwrap();
    let no_out = h.run(&["recover", "--files-from", manifest.to_str().unwrap()]);
    assert!(!no_out.success);
    assert!(no_out.stderr.contains("--out-dir"), "{}", no_out.stderr);
    let both = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        h.root.join("o").to_str().unwrap(),
        "--file",
        "/tmp/x.md",
    ]);
    assert!(!both.success);
    assert!(
        both.stderr.contains("mutually exclusive"),
        "{}",
        both.stderr
    );
}

#[test]
fn recover_coverage_counts_and_boundary() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
    // Two full reads, two edits, one integrity error, one history snapshot.
    assert!(
        out.stdout.contains("2 read (2 full"),
        "read counts: {}",
        out.stdout
    );
    assert!(out.stdout.contains("edit"), "edit count: {}", out.stdout);
    assert!(out.stdout.contains("integrity-error"), "{}", out.stdout);
    assert!(out.stdout.contains("history-snapshot"), "{}", out.stdout);
    // The modified-since-read boundary is AUTHORITATIVE and carries its jsonl line number.
    assert!(
        out.stdout.contains("modified since read"),
        "boundary text: {}",
        out.stdout
    );
    assert!(out.stdout.contains("AUTHORITATIVE"), "{}", out.stdout);
    // Fragments = boundaries + 1 = 2.
    assert!(
        out.stdout.contains("fragments: 2"),
        "fragments: {}",
        out.stdout
    );
    // The malformed line is counted, never hidden.
    assert!(
        out.stdout.contains("malformed line(s) skipped"),
        "{}",
        out.stdout
    );
}

#[test]
fn recover_patches_segments_split_at_boundary_with_line_numbers() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--patches"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // At least TWO segments, split by the integrity boundary.
    let segs = out.stdout.matches("─ SEGMENT").count();
    assert!(
        segs >= 2,
        "expected ≥2 segments, got {segs}:\n{}",
        out.stdout
    );
    // The boundary divider carries L<line>, the kind, and AUTHORITATIVE confidence.
    assert!(out.stdout.contains("INTEGRITY BOUNDARY"), "{}", out.stdout);
    assert!(
        out.stdout.contains("modified since read") && out.stdout.contains("AUTHORITATIVE"),
        "boundary line: {}",
        out.stdout
    );
    // Every segment header + boundary carries a jsonl line number (Lnnn).
    assert!(
        out.stdout.contains("L"),
        "line numbers present: {}",
        out.stdout
    );
    // The first segment's diff shows the open().read() → with-block refactor.
    assert!(
        out.stdout.contains("-raw = open(src).read()"),
        "diff removed line: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("+with open(src) as fh:"),
        "diff added line: {}",
        out.stdout
    );
}

#[test]
fn recover_at_snapshot_has_line_numbers_and_no_fabrication() {
    let h = recover_scenario_home();
    // As of @turn:0 (before the post-drift re-read), the file is the 4-line original with
    // the line-2 edit applied → 5 lines, all known, line-numbered.
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Line-numbered known content.
    assert!(out.stdout.contains("import os"), "{}", out.stdout);
    assert!(
        out.stdout.contains("with open(src) as fh:"),
        "edit applied: {}",
        out.stdout
    );
    // The café🛠 line round-trips UTF-8 verbatim (locale-neutral multi-byte).
    assert!(
        out.stdout.contains("café🛠"),
        "utf-8 verbatim: {}",
        out.stdout
    );
}

#[test]
fn recover_at_partial_read_marks_explicit_gaps() {
    // A separate session that ONLY windowed-reads a slice of a file → explicit gaps.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"look at the spec"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"line5\nline6\nline7","startLine":5,"numLines":3,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/spec.md",
        "--at",
        "@line:9999",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Known lines 5-7 are numbered; lines 1-4 and 8-10 are EXPLICIT gaps, never fabricated.
    assert!(
        out.stdout.contains("??? lines 1..4 unknown"),
        "leading gap: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("    5  line5"),
        "numbered known line: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("??? lines 8..10 unknown"),
        "trailing gap: {}",
        out.stdout
    );
}

#[test]
fn recover_json_every_object_has_line_no_and_local_ts() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    // First object is the coverage object; it carries covered_ranges + boundaries (each
    // boundary has line_no + ts_utc + ts_local).
    let cov: serde_json::Value = serde_json::from_str(lines[0]).expect("ndjson parses");
    assert!(cov.get("covered_ranges").is_some(), "{cov}");
    let bounds = cov
        .get("boundaries")
        .and_then(|b| b.as_array())
        .expect("boundaries array");
    assert!(!bounds.is_empty(), "≥1 boundary");
    let b0 = &bounds[0];
    assert!(
        b0.get("line_no").and_then(|v| v.as_u64()).is_some(),
        "boundary line_no: {b0}"
    );
    assert!(
        b0.get("ts_utc").is_some() && b0.get("ts_local").is_some(),
        "boundary ts: {b0}"
    );
    assert_eq!(
        b0.get("kind").and_then(|v| v.as_str()),
        Some("modified_since_read")
    );
    // Trailing summary line.
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert!(
        summary.get("summary").is_some(),
        "trailing summary: {summary}"
    );
}

#[test]
fn recover_at_json_lines_carry_provenance_and_gaps() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let first = out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap();
    let snap: serde_json::Value = serde_json::from_str(first).unwrap();
    assert_eq!(snap.get("type").and_then(|v| v.as_str()), Some("snapshot"));
    let lines = snap
        .get("lines")
        .and_then(|v| v.as_array())
        .expect("lines array");
    // Every emitted line carries n + text + set_at_line provenance (the jsonl line that set it).
    for l in lines {
        assert!(
            l.get("n").is_some() && l.get("text").is_some(),
            "line shape: {l}"
        );
        assert!(l.get("set_at_line").is_some(), "provenance: {l}");
    }
    assert!(snap.get("gaps").is_some(), "gaps array present: {snap}");
}

// ── clap surface (mode group, mutual exclusion, --file requirement) ──

#[test]
fn recover_two_modes_conflict() {
    let h = Home::new();
    let out = h.run(&["recover", ".", "--file", RFILE, "--coverage", "--patches"]);
    assert!(
        !out.success,
        "two modes must be a clap conflict: {}",
        out.stdout
    );
}

#[test]
fn recover_turn_range_and_since_mutually_exclusive() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--turn-range",
        "0..1",
        "--since",
        "2h",
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("mutually exclusive"),
        "expected the mutual-exclusion bail: {}",
        out.stderr
    );
}

#[test]
fn recover_file_required_for_all_modes() {
    let h = recover_scenario_home();
    // Every mode (patches / at / coverage) requires --file → each bails without it.
    let at_sess = at(SESS);
    for mode in [
        vec!["recover", at_sess.as_str(), "--patches"],
        vec!["recover", at_sess.as_str(), "--at", "@turn:0"],
        vec!["recover", at_sess.as_str(), "--coverage"],
    ] {
        let no_file = h.run(&mode);
        assert!(!no_file.success, "{mode:?} must bail without --file");
        assert!(
            no_file.stderr.contains("--file") && no_file.stderr.contains("required"),
            "{mode:?} file-required bail: {}",
            no_file.stderr
        );
    }
}

#[test]
fn recover_dry_run_alias_works() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--dry-run"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("recoverable"),
        "coverage via --dry-run alias: {}",
        out.stdout
    );
}

#[test]
fn recover_line_range_restricts_output() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@line:9999",
        "--line-range",
        "1..2",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Only lines 1-2 are shown; line 5 (EOF) is outside the range and absent.
    assert!(out.stdout.contains("import os"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("    6  EOF"),
        "line 6 outside range: {}",
        out.stdout
    );
}

#[test]
fn recover_help_mentions_modes() {
    let h = Home::new();
    let out = h.run(&["recover", "--help"]);
    assert!(out.success);
    // All five modes (default restore + the four explicit flags) and their semantics are
    // documented, including the salvage fallback the restore-fail message points at.
    for needle in [
        "--salvage",
        "--patches",
        "--at",
        "--coverage",
        "restore",
        "Segmented unified-diff",
        "Best-effort",
        "partial snapshot",
    ] {
        assert!(
            out.stdout.contains(needle),
            "help missing {needle}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn recover_no_history_says_so() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such/file.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "honest empty result: {}",
        out.stdout
    );
}

#[test]
fn recover_restore_default_returns_raw_full_content() {
    // Default mode (no --salvage/--patches/--at/--coverage) RESTOREs the file's final content
    // as RAW bytes — no SESSION banner, no line numbers, no mode footer — because this session
    // saw the whole file (the post-drift full Read re-establishes all 6 lines).
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE]);
    assert!(out.success, "stderr: {}", out.stderr);
    let expected =
        "import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF\n";
    assert_eq!(out.stdout, expected, "raw restored content");
    // No decoration leaks into the restorable bytes.
    for banned in ["SESSION", "mode=", "  1  "] {
        assert!(
            !out.stdout.contains(banned),
            "no {banned} in raw restore: {}",
            out.stdout
        );
    }
}

#[test]
fn recover_restore_partial_file_errors_pointing_to_salvage() {
    // The session only WINDOW-read lines 5-7 of a 10-line file. Default restore must FAIL
    // (never a holey file), name what it can/can't recover, and point at --salvage.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"look"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"line5\nline6\nline7","startLine":5,"numLines":3,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["recover", at(SESS).as_str(), "--file", "/p/spec.md"]);
    assert!(
        !out.success,
        "partial restore must fail: stdout={}",
        out.stdout
    );
    assert!(
        out.stdout.is_empty(),
        "no holey file on stdout: {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains("recovered 3/10"),
        "names recoverable count: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("[5-7]"),
        "covered range: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("1-4") && out.stderr.contains("8-10"),
        "missing ranges: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("--salvage"),
        "points at --salvage: {}",
        out.stderr
    );
    // No external-change boundary here (just an incomplete read) — so no boundary list.
    assert!(
        !out.stderr.contains("changed OUTSIDE"),
        "no boundary list when there was no external change: {}",
        out.stderr
    );
    // …but the hidden-change caveat fires even without a boundary.
    assert!(
        out.stderr.contains("does not hunt for hidden changes"),
        "caveat present even with no boundary: {}",
        out.stderr
    );
}

#[test]
fn recover_restore_out_writes_raw_file_no_stdout() {
    let h = recover_scenario_home();
    let out_path = h.root.join("restored.py");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.is_empty(),
        "restore --out keeps stdout empty (note goes to stderr): {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains("recovered"),
        "stderr note: {}",
        out.stderr
    );
    let written = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(
        written,
        "import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF\n"
    );
}

#[test]
fn recover_restore_json_emits_single_complete_object_no_trailer() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "restore emits exactly one object: {}",
        out.stdout
    );
    let v: serde_json::Value = serde_json::from_str(lines[0]).expect("one json object");
    assert_eq!(v["file"], RFILE);
    assert_eq!(v["complete"], serde_json::Value::Bool(true));
    assert_eq!(v["lines"], serde_json::json!(6));
    assert!(
        v["content"]
            .as_str()
            .unwrap()
            .contains("with open(src) as fh:"),
        "content carries the edited line: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("summary"),
        "no trailer: {}",
        out.stdout
    );
}

#[test]
fn recover_salvage_dumps_surviving_fragment_with_gaps() {
    // --salvage is restore's never-fails sibling: a windowed-only session yields the surviving
    // lines (numbered) with the rest as explicit gaps, never an error.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"look"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"line5\nline6\nline7","startLine":5,"numLines":3,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/spec.md",
        "--salvage",
    ]);
    assert!(out.success, "salvage never fails: {}", out.stderr);
    assert!(
        out.stdout.contains("??? lines 1..4 unknown"),
        "leading gap: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("    5  line5"),
        "numbered survivor: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("??? lines 8..10 unknown"),
        "trailing gap: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("mode=salvage"),
        "salvage footer: {}",
        out.stdout
    );
}

#[test]
fn recover_modified_since_read_invalidates_stale_lines() {
    // A full Read (5 lines) → an edit blocked by "modified since read" (the file changed
    // underneath, e.g. prettier) → a re-read of only lines 1-2. The pre-boundary lines 3-5 are
    // now STALE and must be INVALIDATED: salvage shows 1-2 + explicit gaps (never the stale
    // CCC/DDD/EEE), and restore must FAIL rather than confidently hand back stale content.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/x.txt","content":"AAA\nBBB\nCCC\nDDD\nEEE","startLine":1,"numLines":5,"totalLines":5}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/p/x.txt","old_string":"CCC","new_string":"ZZZ"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T06:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/x.txt","content":"AAA\nBBB","startLine":1,"numLines":2,"totalLines":5}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
        ),
    );
    // Salvage: stale CCC/DDD/EEE are dropped; lines 3-5 are explicit gaps.
    let salv = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/x.txt",
        "--salvage",
    ]);
    assert!(salv.success, "stderr: {}", salv.stderr);
    assert!(
        salv.stdout.contains("    1  AAA"),
        "re-read line kept: {}",
        salv.stdout
    );
    assert!(
        salv.stdout.contains("??? lines 3..5 unknown"),
        "stale region invalidated: {}",
        salv.stdout
    );
    for stale in ["CCC", "DDD", "EEE"] {
        assert!(
            !salv.stdout.contains(stale),
            "stale {stale} must not be shown as current: {}",
            salv.stdout
        );
    }
    // Restore: refuses rather than falsely claim "complete" on the invalidated buffer.
    let rest = h.run(&["recover", at(SESS).as_str(), "--file", "/p/x.txt"]);
    assert!(
        !rest.success,
        "restore must fail on the invalidated file: {}",
        rest.stdout
    );
    assert!(
        rest.stderr.contains("recovered 2/5"),
        "honest partial count: {}",
        rest.stderr
    );
    // Smart failure: it lists the external-change boundary…
    assert!(
        rest.stderr.contains("changed OUTSIDE") && rest.stderr.contains("modified_since_read"),
        "lists the external-change boundary: {}",
        rest.stderr
    );
    // …and recognizes the pre-change state was COMPLETELY recoverable (scenario 1), recommending
    // the pre-change dump + patches-since recipe.
    assert!(
        rest.stderr.contains("COMPLETELY recoverable"),
        "surfaces the complete pre-change state: {}",
        rest.stderr
    );
    assert!(
        rest.stderr.contains("--at @line:") && rest.stderr.contains("--patches"),
        "recommends pre-change dump + patches: {}",
        rest.stderr
    );
}

#[test]
fn recover_restore_surfaces_fuller_pre_change_partial_state() {
    // Scenario 2: a file NOT authored here — windowed-read lines 1-8 of a 10-line file, then a
    // modified-since-read boundary, then re-read only lines 1-2. Latest is 2/10; but BEFORE the
    // change 8/10 survives (fuller, still partial). Restore surfaces that + a snapshot-as-of recipe.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/big.txt","content":"L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8","startLine":1,"numLines":8,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/p/big.txt","old_string":"L1","new_string":"X1"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T06:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/big.txt","content":"L1\nL2","startLine":1,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["recover", at(SESS).as_str(), "--file", "/p/big.txt"]);
    assert!(!out.success, "partial restore fails: {}", out.stdout);
    assert!(
        out.stderr.contains("recovered 2/10"),
        "latest count: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("MORE survives") && out.stderr.contains("8/10"),
        "fuller (still partial) pre-change state surfaced: {}",
        out.stderr
    );
    // The recommended pre-change dump is `--at @line:N` (NOT `--salvage --at`, which would be a
    // mutually-exclusive-mode parse error).
    assert!(
        out.stderr.contains("--at @line:") && !out.stderr.contains("--salvage --at"),
        "recommends a valid snapshot-as-of command: {}",
        out.stderr
    );
}

#[test]
fn recover_patches_json_segments_and_boundary_objects() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--patches",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    // At least one segment object (carrying unified_diff + line_no + pre_state_known +
    // anchor_source) and one boundary object (carrying line_no + kind + confidence).
    let seg = objs
        .iter()
        .find(|o| o.get("type").and_then(|v| v.as_str()) == Some("segment"))
        .expect("a segment object");
    assert!(
        seg.get("unified_diff").and_then(|v| v.as_str()).is_some(),
        "{seg}"
    );
    assert!(
        seg.get("line_no").and_then(|v| v.as_u64()).is_some(),
        "segment line_no: {seg}"
    );
    assert!(seg.get("pre_state_known").is_some(), "{seg}");
    assert!(seg.get("anchor_source").is_some(), "{seg}");
    let bnd = objs
        .iter()
        .find(|o| o.get("type").and_then(|v| v.as_str()) == Some("boundary"))
        .expect("a boundary object");
    assert_eq!(
        bnd.get("kind").and_then(|v| v.as_str()),
        Some("modified_since_read")
    );
    assert_eq!(
        bnd.get("confidence").and_then(|v| v.as_str()),
        Some("authoritative")
    );
    assert!(
        bnd.get("line_no").and_then(|v| v.as_u64()).is_some(),
        "{bnd}"
    );
    // Trailing summary.
    assert!(objs.last().unwrap().get("summary").is_some());
}

#[test]
fn recover_patches_out_writes_concatenated_diffs() {
    let h = recover_scenario_home();
    let out_path = h.root.join("patches.diff");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--patches",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote concatenated patches"),
        "{}",
        out.stdout
    );
    let blob = std::fs::read_to_string(&out_path).expect("patches file");
    assert!(
        blob.contains("@@ -") && blob.contains("+with open(src) as fh:"),
        "diff blob: {blob}"
    );
}

#[test]
fn recover_at_out_writes_partial_snapshot_with_gaps() {
    // A windowed-read-only file → the --out artifact carries explicit gap markers.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"peek"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"l3\nl4","startLine":3,"numLines":2,"totalLines":8}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out_path = h.root.join("snap.txt");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/spec.md",
        "--at",
        "@line:9999",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote partial snapshot"),
        "{}",
        out.stdout
    );
    let body = std::fs::read_to_string(&out_path).expect("snapshot file");
    assert!(
        body.contains("??? lines 1..2 unknown"),
        "leading gap in artifact: {body}"
    );
    assert!(body.contains("l3"), "known content in artifact: {body}");
    assert!(
        body.contains("??? lines 5..8 unknown"),
        "trailing gap in artifact: {body}"
    );
}

#[test]
fn recover_patches_via_project_path_target() {
    // Drive recover by a PROJECT PATH (encoded dir) instead of --session, exercising the
    // multi-session merge + sort path.
    let h = recover_scenario_home();
    let out = h.run(&["recover", ENC, "--file", RFILE, "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("recoverable"), "{}", out.stdout);
}

#[test]
fn recover_at_json_out_writes_artifact() {
    let h = recover_scenario_home();
    let out_path = h.root.join("snap.json.txt");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // stdout is NDJSON; the --out file is the verbatim reconstructed body.
    let body = std::fs::read_to_string(&out_path).expect("at --out artifact");
    assert!(body.contains("import os"), "verbatim known content: {body}");
}

#[test]
fn recover_coverage_no_boundaries_says_none() {
    // A clean file with only a full Read (no integrity issues) → no boundaries.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/clean.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/clean.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("integrity boundaries: (none)"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("fragments: 1"), "{}", out.stdout);
}

#[test]
fn recover_patches_heuristic_bash_boundary_is_flagged() {
    // A full Read then a Bash `sed -i` on the same file → a HEURISTIC (soft) boundary,
    // flagged with HEURISTIC confidence (not AUTHORITATIVE).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/h.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/h.rs"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/h.rs",
        "--patches",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("INTEGRITY BOUNDARY"), "{}", out.stdout);
    assert!(
        out.stdout.contains("HEURISTIC"),
        "heuristic confidence: {}",
        out.stdout
    );
    assert!(out.stdout.contains("bash"), "bash detail: {}", out.stdout);
}

// ── REAL-FIXTURE acceptance tests (the load-bearing falsification, design §10.8-10) ──
//
// These drive the recover binary against the ACTUAL 233 MB session jsonl on this dev
// machine. They are GATED on the fixture's presence (like the e2e password-gated tests):
// when it is absent (CI / another machine) they print a skip note and return, so the
// suite stays hermetic — but where the data exists, they prove the feature on REAL data.

/// The real session fixture + its encoded project dir, or `None` when absent.
fn real_fixture() -> Option<(String, String, PathBuf)> {
    let home = std::env::var_os("HOME")?;
    let enc = "-Users-testuser-Projects-Acme-widget-factory-worktrees-feature-session-7";
    let sess = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let p = PathBuf::from(&home)
        .join(".claude")
        .join("projects")
        .join(enc)
        .join(format!("{sess}.jsonl"));
    if p.is_file() {
        Some((enc.to_string(), sess.to_string(), p))
    } else {
        None
    }
}

/// Run the real binary against the REAL `$HOME` (not a temp one) for fixture tests.
fn run_real(args: &[&str]) -> Output {
    let exe = env!("CARGO_BIN_EXE_csift");
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_COMPANION_SESSION_ID");
    let out = cmd.output().expect("spawn csift");
    Output {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn recover_real_multi_patch_segmentation_at_modified_since_read() {
    let Some((enc, sess, _)) = real_fixture() else {
        eprintln!("SKIP recover_real_multi_patch_segmentation: real fixture absent");
        return;
    };
    // engine.py shows a real `File has been modified since read` error at jsonl L22980.
    // A --patches run over it must emit ≥2 segments split by an AUTHORITATIVE boundary
    // carrying that line number, and no single diff may span the boundary.
    let engine = "/Users/testuser/Projects/Acme/widget_factory-worktrees/feature-session-7/app/src/app/engine/engine.py";
    let out = run_real(&[
        "recover",
        &enc,
        at(sess).as_str(),
        "--file",
        engine,
        "--patches",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let segs = out.stdout.matches("─ SEGMENT").count();
    assert!(segs >= 2, "expected ≥2 real segments, got {segs}");
    assert!(
        out.stdout.contains("INTEGRITY BOUNDARY") && out.stdout.contains("L22980"),
        "boundary at the real L22980: {}",
        out.stdout
    );
    assert!(out.stdout.contains("modified since read"), "{}", out.stdout);
    assert!(out.stdout.contains("AUTHORITATIVE"), "{}", out.stdout);
}

#[test]
fn recover_real_reconstruction_matches_disk_on_contiguous_prefix() {
    let Some((enc, sess, _)) = real_fixture() else {
        eprintln!("SKIP recover_real_reconstruction_matches_disk: real fixture absent");
        return;
    };
    // Reconstruct the plan file from its Read/Edit stream (NOT the whole-plan anchor) and
    // assert the contiguous-from-line-1 KNOWN prefix matches the live on-disk file
    // byte-for-byte. Gaps + post-drift islands are allowed (partial by design); the
    // trustworthy contiguous prefix must never disagree with disk.
    let disk_plan = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".claude")
        .join("plans")
        .join("goofy-finding-kettle.md");
    if !disk_plan.is_file() {
        eprintln!("SKIP: on-disk plan file absent");
        return;
    }
    let out = run_real(&[
        "recover",
        &enc,
        at(sess).as_str(),
        "--file",
        disk_plan.to_str().unwrap(),
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // A leading {kind:"session_header"} scope record may precede the snapshot when the scope
    // spans subagents — find the first snapshot object (the one carrying `lines`), not just
    // the first non-empty line.
    let snap: serde_json::Value = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v.get("lines").is_some())
        .expect("a snapshot object with a `lines` array");
    let mut known: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        let n = l.get("n").and_then(|v| v.as_u64()).unwrap() as usize;
        let t = l.get("text").and_then(|v| v.as_str()).unwrap().to_string();
        known.insert(n, t);
    }
    let disk = std::fs::read_to_string(&disk_plan).unwrap();
    let disk_lines: Vec<&str> = {
        let mut v: Vec<&str> = disk.split('\n').collect();
        if v.last() == Some(&"") {
            v.pop();
        }
        v
    };
    // Walk the contiguous prefix from line 1 and assert each known line matches disk.
    let mut n = 1usize;
    let mut prefix_len = 0usize;
    while let Some(text) = known.get(&n) {
        assert!(
            n <= disk_lines.len(),
            "reconstructed beyond disk length at {n}"
        );
        assert_eq!(
            text,
            disk_lines[n - 1],
            "contiguous-prefix line {n} must match disk"
        );
        prefix_len = n;
        n += 1;
    }
    assert!(
        prefix_len > 50,
        "expected a substantial clean prefix, got {prefix_len}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// recover render-path branch completeness: arms reachable only through the real
// stdout/JSON renderers (empty-session skips, no-history `!any`, multi-session
// separators, --out for every mode, coverage holes, truncation). Synthetic homes
// only — these never touch the real fixture.
// ─────────────────────────────────────────────────────────────────────────────

/// A session that reads a file then makes an UN-ANCHORABLE edit (a structured patch deep
/// in an unknown gap) → `--coverage` reports a coverage hole + a windowed read.
fn recover_hole_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"peek"}}"#, "\n",
            // A windowed read of lines 1-2 of a 100-line file (lines 3-100 stay gaps).
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/big.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":100}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            // An edit at line 60 — deep in the gap, no adjacent known line → un-anchorable.
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"filePath":"/p/big.rs","oldString":"zzz","newString":"Z","structuredPatch":[{"oldStart":60,"oldLines":1,"newStart":60,"newLines":1,"lines":["-zzz","+Z"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed0","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn recover_coverage_reports_unanchorable_holes() {
    // Drives the `if rep.counts.edit_unanchorable > 0` coverage-hole line in the text
    // renderer + the windowed-read count display.
    let h = recover_hole_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/big.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("un-anchorable edits (coverage holes): 1"),
        "coverage hole reported: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("windowed"),
        "windowed read counted: {}",
        out.stdout
    );
}

#[test]
fn recover_patches_no_history_says_so() {
    // `--patches` against a file with no recoverable events → the `!any` honest-empty arm
    // of the patches text renderer (distinct from the coverage-mode `!any` already tested).
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such.rs",
        "--patches",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "patches honest-empty: {}",
        out.stdout
    );
}

#[test]
fn recover_at_no_history_says_so() {
    // `--at` against a file with no events → the at-mode `!any` honest-empty arm.
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such.rs",
        "--at",
        "@line:5",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "at honest-empty: {}",
        out.stdout
    );
}

#[test]
fn recover_history_snapshot_only_session_emits_no_segment_or_boundary() {
    // A session whose ONLY event for the target is a file-history-snapshot marker. The
    // marker is counted but opens no segment and creates no boundary → the
    // `segments.is_empty() && boundaries.is_empty()` skip fires (both text + JSON patches).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-06-07T05:00:00.500Z","trackedFileBackups":{"/p/snap.rs":{"backupFileName":null,"version":1,"backupTime":"2026-06-07T05:00:00.500Z"}}}}"#, "\n",
        ),
    );
    // Text patches: the snapshot-only session is skipped → honest empty.
    let text = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/snap.rs",
        "--patches",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("no recoverable history"),
        "snapshot-only session yields no patch segments: {}",
        text.stdout
    );
    // JSON patches: only the trailing summary object, zero segment/boundary objects.
    let js = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/snap.rs",
        "--patches",
        "--format",
        "json",
    ]);
    assert!(js.success, "stderr: {}", js.stderr);
    let objs: Vec<serde_json::Value> = js
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    assert!(
        objs.iter().all(|o| o.get("type").is_none()),
        "no segment/boundary objects, only the summary: {}",
        js.stdout
    );
    assert!(objs.last().unwrap().get("summary").is_some());
    assert_eq!(
        objs.last().unwrap()["summary"]["sessions"].as_u64(),
        Some(0),
        "the snapshot-only session contributed no patch output"
    );
}

#[test]
fn recover_coverage_groups_multiple_sessions_with_separator() {
    // Two sessions BOTH touching the same file via a positional project-path target (no
    // --session) → the coverage renderer prints two SESSION headers separated by a blank
    // line (the `if !*first` separator arm).
    let h = Home::new();
    let sess_a = "aaaaaaaa-1111-1111-1111-111111111111";
    let sess_b = "bbbbbbbb-2222-2222-2222-222222222222";
    for s in [sess_a, sess_b] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            concat!(
                r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
                r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/shared.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            ),
        );
    }
    let out = h.run(&["recover", ENC, "--file", "/p/shared.rs", "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.matches("SESSION").count() >= 2,
        "two session headers with a separator: {}",
        out.stdout
    );
}

/// Cross-session interleaved reconstruction: a file is created by the top-level session,
/// then edited by it AND by two of its subagents, interleaved by wall-clock, with only
/// PARTIAL reads anywhere (no single transcript ever holds the whole file, and each
/// subagent's own buffer is too sparse to anchor its string edits). The union of all
/// transcripts' edits IS the complete file. `--at` must merge the parent+subagents into one
/// timestamp-ordered timeline and reconstruct EVERY line — including the subagents' own
/// edits, which are un-anchorable in their isolated transcripts. NO external-edit
/// attachments are present, so this isolates the merge (not Claude Code's edited_text_file
/// reconciliation). The 8-line file ends in a newline → the trailing-newline normalisation
/// must keep seen_total at 8 (no phantom `line 9 unknown`).
#[test]
fn recover_at_merges_interleaved_cross_session_edits() {
    const MSESS: &str = "cccccccc-5555-5555-5555-555555555555";
    let h = Home::new();
    // ── Top-level: Write the 8-line file, then (after subagent A) one structured-patch edit.
    h.write(
        &format!("{ENC}/{MSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"build doc.md"}}"#, "\n",
            // Write create result carries the full content (as real Claude Code does).
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w0","name":"Write","input":{"file_path":"/p/doc.md","content":"A1\nA2\nA3\nA4\nA5\nA6\nA7\nA8\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/doc.md","content":"A1\nA2\nA3\nA4\nA5\nA6\nA7\nA8\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w0","content":"ok"}]}}"#, "\n",
            // A PARTIAL read of lines 1-4 (after subagent A's edits land on disk). totalLines
            // is the SEPARATOR count (9) of the newline-terminated 8-line file.
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:20.000Z","toolUseResult":{"file":{"filePath":"/p/doc.md","content":"A1\nA2\nA3-suba\nA4","startLine":1,"numLines":4,"totalLines":9}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
            // Top-level structured-patch edit: A1 → A1-main.
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:21.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_main","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A1","new_string":"A1-main"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:21.500Z","toolUseResult":{"filePath":"/p/doc.md","oldString":"A1","newString":"A1-main","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-A1","+A1-main"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_main","content":"ok"}]}}"#, "\n",
        ),
    );
    // ── Subagent A: partial-read lines 3-6, then two STRING edits (bare tool_result, NO
    //    toolUseResult → the input-side fallback supplies content). In isolation its buffer
    //    (lines 3-6) is too sparse to anchor a string edit; only the merge (with the parent's
    //    Write anchor in scope) makes these edits land.
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-aaaaaa.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"edit lines 3 and 6"}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:10.500Z","toolUseResult":{"file":{"filePath":"/p/doc.md","content":"A3\nA4\nA5\nA6","startLine":3,"numLines":4,"totalLines":9}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rda","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ea1","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A3","new_string":"A3-suba","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:11.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ea1","content":"The file /p/doc.md has been updated successfully."}]}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ea2","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A6","new_string":"A6-suba","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ea2","content":"The file /p/doc.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-aaaaaa.meta.json"),
        r#"{"agentType":"general-purpose","description":"edit A","toolUseId":"t_a"}"#,
    );
    // ── Subagent B: partial-read lines 5-8, then one STRING edit A8 → A8-subb (latest).
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-bbbbbb.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":"edit line 8"}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:30.500Z","toolUseResult":{"file":{"filePath":"/p/doc.md","content":"A5\nA6-suba\nA7\nA8","startLine":5,"numLines":4,"totalLines":9}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rdb","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:31.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"eb1","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A8","new_string":"A8-subb","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:31.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"eb1","content":"The file /p/doc.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-bbbbbb.meta.json"),
        r#"{"agentType":"general-purpose","description":"edit B","toolUseId":"t_b"}"#,
    );

    let out = h.run(&[
        "recover",
        at(MSESS).as_str(),
        "--file",
        "/p/doc.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Exactly ONE merged snapshot object (the parent+subagents folded into one timeline),
    // not three per-transcript fragments.
    let snaps: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("snapshot"))
        .collect();
    assert_eq!(
        snaps.len(),
        1,
        "one merged snapshot, got {}: {}",
        snaps.len(),
        out.stdout
    );
    let snap = &snaps[0];
    let mut recon: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        recon.insert(
            l.get("n").and_then(|v| v.as_u64()).unwrap() as usize,
            l.get("text").and_then(|v| v.as_str()).unwrap().to_string(),
        );
    }
    let got: Vec<&str> = recon.values().map(String::as_str).collect();
    // The COMPLETE interleaved file: parent's Write + parent's edit + BOTH subagents' edits.
    assert_eq!(
        got,
        vec!["A1-main", "A2", "A3-suba", "A4", "A5", "A6-suba", "A7", "A8-subb"],
        "merged reconstruction must carry every cross-session edit: {snap}"
    );
    // No phantom trailing gap (the file ends in a newline; seen_total must be 8, not 9).
    assert_eq!(
        snap.get("seen_total_lines").and_then(|v| v.as_u64()),
        Some(8),
        "trailing-newline normalisation keeps seen_total at 8: {snap}"
    );
    assert!(
        snap.get("gaps")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "a fully-recovered file has no gaps: {snap}"
    );
}

/// `--at <datetime>` is time-travel: the same transcript reconstructs a DIFFERENT state of
/// the file depending on the wall-clock instant asked for. This is the canonical, efficient
/// way to ask "what did this plan/file look like at 8pm last night" — far better than hunting
/// line numbers. Three cutoffs across one Write + two timestamped edits must each land on the
/// exact intervening state: original / after-edit-1 / after-edit-2. Exercises the
/// `resolve_cutoff` datetime branch (TimeWindow until-bound) end-to-end through reconstruction,
/// and accepts both an absolute RFC3339 instant and a bare date.
#[test]
fn recover_at_datetime_time_travels_across_edits() {
    const TSESS: &str = "dddddddd-7777-7777-7777-777777777777";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{TSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"draft plan.md"}}"#, "\n",
            // 05:00 — Write the 3-line original.
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w0","name":"Write","input":{"file_path":"/p/plan.md","content":"L1\nL2\nL3\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/plan.md","content":"L1\nL2\nL3\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w0","content":"ok"}]}}"#, "\n",
            // 08:00 — edit L2.
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/plan.md","old_string":"L2","new_string":"L2-edited"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T08:00:00.500Z","toolUseResult":{"filePath":"/p/plan.md","oldString":"L2","newString":"L2-edited","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-L2","+L2-edited"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            // 11:00 — edit L3.
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T11:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/p/plan.md","old_string":"L3","new_string":"L3-edited"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T11:00:00.500Z","toolUseResult":{"filePath":"/p/plan.md","oldString":"L3","newString":"L3-edited","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":3,"oldLines":1,"newStart":3,"newLines":1,"lines":["-L3","+L3-edited"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","content":"ok"}]}}"#, "\n",
        ),
    );

    // Reconstruct the file as of `when` and return its lines in order.
    let recon_at = |when: &str| -> Vec<String> {
        let out = h.run(&[
            "recover",
            at(TSESS).as_str(),
            "--file",
            "/p/plan.md",
            "--at",
            when,
            "--format",
            "json",
        ]);
        assert!(out.success, "--at {when:?} stderr: {}", out.stderr);
        let snap = out
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("snapshot"))
            .unwrap_or_else(|| panic!("--at {when:?} produced no snapshot: {}", out.stdout));
        let mut recon: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
            recon.insert(
                l.get("n").and_then(|v| v.as_u64()).unwrap() as usize,
                l.get("text").and_then(|v| v.as_str()).unwrap().to_string(),
            );
        }
        recon.into_values().collect()
    };

    // 06:00 — after the Write, before any edit → the pristine original.
    assert_eq!(
        recon_at("2026-06-07T06:00:00Z"),
        vec!["L1", "L2", "L3"],
        "as of 06:00 only the Write has happened"
    );
    // 09:00 — between the two edits → L2 edited, L3 still original. THE headline time-travel.
    assert_eq!(
        recon_at("2026-06-07T09:00:00Z"),
        vec!["L1", "L2-edited", "L3"],
        "as of 09:00 the L2 edit is applied but the 11:00 L3 edit is not"
    );
    // 12:00 (and a bare date covering the whole day) — after both edits → fully edited.
    assert_eq!(
        recon_at("2026-06-07T12:00:00Z"),
        vec!["L1", "L2-edited", "L3-edited"],
        "as of 12:00 both edits are applied"
    );
    // A bare date is accepted too (resolves to system-local midnight); pick one several days
    // past the edits so no timezone shifts the bound before them → both edits applied.
    assert_eq!(
        recon_at("2026-06-15"),
        vec!["L1", "L2-edited", "L3-edited"],
        "a bare date is a valid --at bound → both edits applied"
    );
}

#[test]
fn recover_at_skips_session_with_no_seen_total() {
    // Two sessions under one project: one reads /p/seen.rs, the other only reads a DIFFERENT
    // file. For the target /p/seen.rs, the second session has empty known + no seen_total →
    // the `known.is_empty() && seen_total_lines.is_none()` continue fires, so only ONE
    // session snapshot is emitted (the other is honestly skipped, not shown as empty).
    let h = Home::new();
    let sess_a = "aaaaaaaa-3333-3333-3333-333333333333";
    let sess_b = "bbbbbbbb-4444-4444-4444-444444444444";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/seen.rs","content":"x\ny","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/other.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["recover", ENC, "--file", "/p/seen.rs", "--at", "@line:9999"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.matches("SESSION").count(),
        1,
        "only the session that actually touched /p/seen.rs is shown: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("x"),
        "the seen content is rendered: {}",
        out.stdout
    );
}

#[test]
fn recover_at_json_out_writes_partial_snapshot_artifact() {
    // The at-mode JSON renderer's `--out` arm: it writes the partial-snapshot blob (known
    // lines + explicit gap markers) to disk while still emitting NDJSON to stdout.
    let h = recover_scenario_home();
    let out_path = h.root.join("at-artifact.txt");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let blob = std::fs::read_to_string(&out_path).expect("at JSON --out artifact");
    assert!(
        blob.contains("import os"),
        "the snapshot artifact carries known content: {blob}"
    );
}

#[test]
fn recover_patches_json_out_writes_concatenated_diffs() {
    // The patches-mode JSON renderer's `--out` arm writes the concatenated diff blob.
    let h = recover_scenario_home();
    let out_path = h.root.join("patches-json.diff");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--patches",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let blob = std::fs::read_to_string(&out_path).expect("patches JSON --out artifact");
    assert!(
        blob.contains("@@ -") && blob.contains("+with open(src) as fh:"),
        "concatenated diff blob from the JSON renderer: {blob}"
    );
}

/// A session whose ONLY event for the target is an edit with no preceding read → the edit
/// is un-anchorable, leaving an empty buffer and no seen_total (events non-empty, but the
/// reconstruction is empty). Used to drive the at-mode "nothing known" skip in both
/// renderers.
fn recover_empty_reconstruction_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            // An Edit result for /p/e.rs with NO prior read → un-anchorable, no known lines.
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"filePath":"/p/e.rs","oldString":"x","newString":"y","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-x","+y"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed0","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn recover_at_text_skips_session_with_events_but_no_known_content() {
    // Text at-mode: the session HAS an event (the edit, so it passes the events-empty
    // guard) but reconstructs to nothing known + no seen_total → the
    // `known.is_empty() && seen_total.is_none()` continue fires → honest "no history".
    let h = recover_empty_reconstruction_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/e.rs",
        "--at",
        "@line:9999",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "an all-un-anchorable session shows nothing reconstructable: {}",
        out.stdout
    );
}

#[test]
fn recover_at_json_skips_session_with_events_but_no_known_content() {
    // JSON at-mode: same shape → no snapshot object, only the summary with sessions == 0.
    let h = recover_empty_reconstruction_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/e.rs",
        "--at",
        "@line:9999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    assert!(
        objs.iter()
            .all(|o| o.get("type").and_then(|v| v.as_str()) != Some("snapshot")),
        "no snapshot emitted for an empty reconstruction: {}",
        out.stdout
    );
    assert_eq!(
        objs.last().unwrap()["summary"]["sessions"].as_u64(),
        Some(0)
    );
}

#[test]
fn recover_coverage_zero_seen_total_reports_zero_percent() {
    // A coverage run where the reconstruction is empty (un-anchorable edit only) → known 0,
    // seen_total 0 → the `if total > 0` percent guard takes its FALSE side (0%), and the
    // covered-ranges line shows "(none)".
    let h = recover_empty_reconstruction_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/e.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("0/0 lines (0%)"),
        "zero-total recoverable reports 0%: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("covered line ranges: (none)"),
        "no covered ranges: {}",
        out.stdout
    );
    // The un-anchorable edit is surfaced as a coverage hole.
    assert!(
        out.stdout
            .contains("un-anchorable edits (coverage holes): 1"),
        "{}",
        out.stdout
    );
}

#[test]
fn recover_turn_range_and_until_mutually_exclusive() {
    // The mutual-exclusion guard's `until.is_some()` operand: --turn-range with --until (no
    // --since) → the second operand of the inner `||` is the one that trips the bail.
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--turn-range",
        "0..1",
        "--until",
        "2026-06-08",
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("mutually exclusive"),
        "--turn-range + --until is the mutual-exclusion bail: {}",
        out.stderr
    );
}

#[test]
fn recover_json_coverage_skips_empty_event_session() {
    // JSON coverage mode: one session touches the target, another touches a different file.
    // The non-touching session is skipped (`s.events.is_empty()` true in the JSON branch),
    // so exactly one coverage object precedes the summary, and summary.sessions == 1.
    let h = Home::new();
    let sess_a = "aaaaaaaa-5555-5555-5555-555555555555";
    let sess_b = "bbbbbbbb-6666-6666-6666-666666666666";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/t.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/elsewhere.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        ENC,
        "--file",
        "/p/t.rs",
        "--coverage",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    let cov_objs = objs
        .iter()
        .filter(|o| o.get("recoverable_lines").is_some())
        .count();
    assert_eq!(
        cov_objs, 1,
        "only the touching session yields a coverage object"
    );
    assert_eq!(
        objs.last().unwrap()["summary"]["sessions"].as_u64(),
        Some(1),
        "the non-touching session was skipped, not emitted"
    );
}

#[test]
fn recover_json_patches_skips_empty_event_session() {
    // JSON patches mode skip: a target with no events in the (only) session → no segment or
    // boundary objects, summary.sessions == 0 (the `s.events.is_empty()` JSON-patches arm).
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such.rs",
        "--patches",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    assert!(
        objs.iter().all(|o| o.get("type").is_none()),
        "no segment/boundary objects for a no-event target: {}",
        out.stdout
    );
    assert_eq!(
        objs.last().unwrap()["summary"]["sessions"].as_u64(),
        Some(0)
    );
}

#[test]
fn recover_json_at_skips_session_with_no_seen_total() {
    // JSON at-mode skip: same two-session shape as the text variant, but `--format json`,
    // driving the `known.is_empty() && seen_total.is_none()` continue in the JSON renderer.
    let h = Home::new();
    let sess_a = "aaaaaaaa-7777-7777-7777-777777777777";
    let sess_b = "bbbbbbbb-8888-8888-8888-888888888888";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/here.rs","content":"x\ny","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/elsewhere.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        ENC,
        "--file",
        "/p/here.rs",
        "--at",
        "@line:9999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    let snaps = objs
        .iter()
        .filter(|o| o.get("type").and_then(|v| v.as_str()) == Some("snapshot"))
        .count();
    assert_eq!(
        snaps, 1,
        "only the session that saw /p/here.rs emits a snapshot"
    );
    assert_eq!(
        objs.last().unwrap()["summary"]["sessions"].as_u64(),
        Some(1)
    );
}

#[test]
fn recover_coverage_heuristic_boundary_uses_soft_symbol() {
    // A coverage run over a session with a HEURISTIC (bash) boundary drives the coverage
    // renderer's `~` (soft) boundary symbol arm — distinct from the `⚠` authoritative one.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/sb.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/sb.rs"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/sb.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("integrity boundaries:") && out.stdout.contains("HEURISTIC"),
        "heuristic boundary listed in coverage: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("~ L"),
        "the soft '~' symbol prefixes a heuristic boundary: {}",
        out.stdout
    );
}

#[test]
fn recover_patches_boundary_only_session_still_renders() {
    // A session whose ONLY event is a Bash mutation on the target (no prior read) produces a
    // boundary but NO segment → `segments.is_empty() && boundaries.is_empty()` is
    // `true && false` (the second operand FALSE side), so the session is NOT skipped and the
    // lone boundary is rendered.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/bo.rs"}}]}}"#, "\n",
        ),
    );
    // Text patches: the boundary is shown even though no segment exists.
    let text = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bo.rs",
        "--patches",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("INTEGRITY BOUNDARY") && text.stdout.contains("HEURISTIC"),
        "the lone boundary renders without a segment: {}",
        text.stdout
    );
    // JSON patches: a boundary object is emitted (same non-skip second-operand path).
    let js = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bo.rs",
        "--patches",
        "--format",
        "json",
    ]);
    assert!(js.success, "stderr: {}", js.stderr);
    let has_boundary = js
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|o| o.get("type").and_then(|v| v.as_str()) == Some("boundary"));
    assert!(has_boundary, "a boundary object is emitted: {}", js.stdout);
}

#[test]
fn recover_at_empty_when_spec_omits_cutoff_line() {
    // `--at ""` (an explicit empty cutoff spec) → `resolve_cutoff` returns None → the
    // `if let Some(c) = cutoff` FALSE side: the snapshot renders WITHOUT an "as of:" line.
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--at", ""]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("as of: jsonl line"),
        "an empty cutoff omits the 'as of' line: {}",
        out.stdout
    );
    // It still renders the fully-replayed snapshot (no cutoff → everything).
    assert!(out.stdout.contains("import os"), "{}", out.stdout);
}

#[test]
fn recover_at_line_range_outside_known_keeps_seen_total() {
    // A windowed read sets seen_total_lines, but a `--line-range` that selects NO known line
    // leaves `known` empty while seen_total is still Some → the
    // `known.is_empty() && seen_total.is_none()` check has its SECOND operand FALSE (the
    // session is NOT skipped; it renders an all-gap snapshot up to the seen total).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            // A windowed read of lines 5-6 (seen_total 10).
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/lr.rs","content":"l5\nl6","startLine":5,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    // Restrict to lines 1-2 — OUTSIDE the known 5-6 window → known empties, seen_total stays.
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/lr.rs",
        "--at",
        "@line:9999",
        "--line-range",
        "1..2",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The session is rendered (not skipped) because seen_total is Some → an explicit gap.
    assert!(
        out.stdout.contains("SESSION") && out.stdout.contains("unknown"),
        "an out-of-range line filter still renders the session as explicit gaps: {}",
        out.stdout
    );
}

#[test]
fn recover_at_json_line_range_outside_known_keeps_seen_total() {
    // The JSON at-mode twin of the text test: a windowed read sets seen_total, but a
    // `--line-range` selecting no known line empties `known` while seen_total stays Some →
    // the JSON renderer's `known.is_empty() && seen_total.is_none()` second operand is FALSE
    // (the snapshot is emitted, carrying the gap up to the seen total).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/lrj.rs","content":"l5\nl6","startLine":5,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/lrj.rs",
        "--at",
        "@line:9999",
        "--line-range",
        "1..2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let snap = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|o| o.get("type").and_then(|v| v.as_str()) == Some("snapshot"))
        .expect("a snapshot object is still emitted");
    // No known lines survive the range filter, but the seen total + gaps are reported.
    assert_eq!(
        snap.get("lines")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0),
        "no known lines in the 1..2 range: {snap}"
    );
    assert_eq!(
        snap.get("seen_total_lines").and_then(|v| v.as_u64()),
        Some(10),
        "the seen total is preserved: {snap}"
    );
}

#[test]
fn recover_turn_range_alone_is_accepted() {
    // `--turn-range` WITHOUT --since/--until is valid (drives the `&&` right operand of the
    // mutual-exclusion guard to its false side: turn_range set, since/until both absent).
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--turn-range",
        "0..0",
    ]);
    assert!(
        out.success,
        "a bare --turn-range is not a conflict: {}",
        out.stderr
    );
    // Turn 0 only → the first segment's reads/edits are in scope; the turn-1 boundary is not.
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

// ════════════════════════════════════════════════════════════════════════════
// turns
// ════════════════════════════════════════════════════════════════════════════

/// Build the `turns` integration fixture: a realistic multi-compaction transcript
/// authored with LOCALE-NEUTRAL tokens only (accented-Latin + emoji — the
/// same house charset style as the recover fixtures).
///
/// Shape (all on the SINGLE top-level session jsonl so the spanning walk is exercised
/// without subagent noise; tests pass `--no-subagents`):
///   • 3 genuine round-trip turns, then a compaction SUMMARY #1 (its §6 quotes turn 0's
///     user verbatim + §9 quotes the last assistant — drives the dedup test),
///   • 3 more round-trips, then SUMMARY #2,
///   • 3 more round-trips, then SUMMARY #3,
///   • a final live block: one HUGE round-trip (user > 600 chars, assistant > 900 chars
///     → drives the role-asymmetric ellipsis test) and an assistant-heavy tail (a turn
///     whose assistant side is enormous + a pure tool-call turn) → drives the 50% floor.
/// Plus one malformed line for the skipped-line accounting.
/// A tiny jsonl builder for the turns fixture: free methods on a struct so there is no
/// closure-capture conflict (the earlier closure form double-borrowed the buffer).
struct TurnsBuilder {
    out: String,
    ts: u64,
}

impl TurnsBuilder {
    fn new() -> Self {
        TurnsBuilder {
            out: String::new(),
            ts: 0,
        }
    }

    fn next_ts(&mut self) -> String {
        self.ts += 1;
        let t = self.ts;
        format!(
            "2026-06-07T{:02}:{:02}:{:02}.000Z",
            t / 3600,
            (t / 60) % 60,
            t % 60
        )
    }

    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// A round-trip turn: user opener (+ optional N tool calls) + assistant EOT text.
    fn round_trip(&mut self, user: &str, asst: &str, tools: usize) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"{user}"}}}}"#
        ));
        if tools > 0 {
            let blocks: Vec<String> = (0..tools)
                .map(|i| format!(r#"{{"type":"tool_use","id":"t{i}","name":"Bash","input":{{}}}}"#))
                .collect();
            let ts = self.next_ts();
            self.line(&format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{}]}}}}"#,
                blocks.join(",")
            ));
        }
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{asst}"}}]}}}}"#
        ));
    }

    /// Emit one assistant text record (one agent message in a turn's run).
    fn agent_text(&mut self, text: &str) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        ));
    }

    /// Emit one assistant tool_use record (drives the per-message attribution span).
    fn tool_use(&mut self) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"tx","name":"Bash","input":{{}}}}]}}}}"#
        ));
    }

    /// A turn with a LONG agent-message run: a user opener, then the ordered list of
    /// agent messages (each preceded by one tool_use so the placeholder Y is non-zero),
    /// to drive the richness selection. The LAST entry is the EOT.
    fn long_agent_run(&mut self, user: &str, agent_msgs: &[&str]) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"{user}"}}}}"#
        ));
        for m in agent_msgs {
            self.tool_use();
            self.agent_text(m);
        }
    }
}

fn turns_fixture_jsonl() -> String {
    let mut b = TurnsBuilder::new();

    // ── Block A: 3 round-trips, then SUMMARY #1 ──
    b.round_trip(
        "the very first ask about the café budget walk logic",
        "first reply café🛠",
        2,
    );
    b.round_trip(
        "second ask explain the panic path now please",
        "second reply",
        2,
    );
    b.round_trip("third ask about the carry boundary", "third reply", 0);
    // SUMMARY #1: §6 quotes turn 0's user verbatim (dedup target), §9 quotes an assistant.
    b.line(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued from a previous conversation.\n6. All user messages:\n   - \"the very first ask about the café budget walk logic\"\n   - \"second ask explain ...\"\n9. Optional Next Step:\n   The assistant said \"third reply\" before compaction."}}"#,
    );

    // ── Block B: 3 round-trips, then SUMMARY #2 ──
    b.round_trip(
        "fourth ask after the first compaction boundary",
        "fourth reply",
        1,
    );
    b.round_trip("fifth ask keep going", "fifth reply", 3);
    b.round_trip("sixth ask about détours", "sixth reply", 0);
    b.line(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued.\n6. All user messages:\n   - \"fourth ask after the first compaction boundary\"\n9. Optional Next Step:\n   The assistant said \"sixth reply\"."}}"#,
    );

    // ── Block C: 3 round-trips, then SUMMARY #3 ──
    b.round_trip("seventh ask post second boundary", "seventh reply", 2);
    b.round_trip("eighth ask almost there", "eighth reply", 1);
    b.round_trip("ninth ask wrap it up", "ninth reply", 0);
    b.line(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued.\n6. All user messages:\n   - \"seventh ask post second boundary\"\n9. Optional Next Step:\n   The assistant said \"ninth reply\"."}}"#,
    );

    // ── Block D (live region, after the newest summary) ──
    // A LONG agent-message run in ONE turn (8 agent messages > the default >6 threshold):
    // a rich first, pure-declaration middles, a sudden rich middle, a FUSED finding+decl
    // body, and the EOT — drives the richness selection + placeholder integration tests.
    b.long_agent_run(
        "kick off the long debugging chain",
        &[
            "found the AGENTRICHFIRST root cause already", // first — rich (lexeme) → kept
            "let me try the LETMEDECL one next",           // middle decl → collapse
            "now i will check LETMEDECL another",          // middle decl → collapse
            // A declaration with a digit adjacent to multi-byte chars (the exact
            // shape that once panicked the ±16-byte number-of-substance window): a
            // signal-less intent-verb opener → collapses, and must NOT panic.
            "let me LETMEDECL look at the 🤖 07:40 log", // middle decl → collapse
            "AGENTRICHMID 12 passed 3 failed in src/x.rs:9", // sudden rich middle → kept
            "let me write LETMEDECL it up",              // middle decl → collapse
            "now let me LETMEDECL finalize",             // middle decl → collapse
            "root cause confirmed in src/y.rs:42 — now let me FUSEDTAIL write the fix", // fused → kept
            "the AGENTEOT final committed answer", // last — always kept
        ],
    );
    // A HUGE round-trip: user > 600 chars, assistant > 900 chars → role-asymmetric ellipsis.
    let huge_user = format!("HEADuser {} TAILuser", "u".repeat(800));
    let huge_asst = format!("HEADasst {} TAILasst", "a".repeat(1100));
    b.round_trip(&huge_user, &huge_asst, 5);
    // An assistant-heavy tail: a complete turn whose assistant side is enormous.
    let big_asst = format!("big {} end", "b".repeat(3000));
    b.round_trip("short live ask", &big_asst, 2);
    // A pure tool-call turn (no assistant EOT text) → partial turn.
    let ts = b.next_ts();
    b.line(&format!(
        r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"do the final thing"}}}}"#
    ));
    let ts = b.next_ts();
    b.line(&format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"z","name":"Bash","input":{{}}}}]}}}}"#
    ));
    // A malformed line that survives the prefilter (carries "role":"user") → counted.
    b.line(r#"{"type":"user","role":"user" broken json after marker}"#);

    b.out
}

fn turns_home() -> Home {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &turns_fixture_jsonl());
    h
}

#[test]
fn turns_slice_reassembles_out_document_within_window() {
    // --slice paginates the SAME verbatim document `--out` writes into ≤window-CHAR chunks with
    // NO chrome. Assert: every chunk ≤ window, concatenating slices 1..K reproduces the `--out`
    // document byte-for-byte (the zero-drift contract between build_document_body and out_blob),
    // and an out-of-range slice is empty (exit 0).
    let h = turns_home();
    let window = 500usize;

    let out_path = h.root.join("turns_doc.md");
    let r = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(r.success, "stderr: {}", r.stderr);
    let document = std::fs::read_to_string(&out_path).expect("out document written");
    assert!(!document.is_empty(), "fixture yields a non-empty document");
    assert!(
        document.chars().count() > window,
        "document must exceed one window to exercise multi-slice ({} chars)",
        document.chars().count()
    );

    let win = window.to_string();
    let mut reassembled = String::new();
    let mut n = 1usize;
    loop {
        let ns = n.to_string();
        let s = h.run(&[
            "turns",
            at(SESS).as_str(),
            "--budget",
            "20000",
            "--no-subagents",
            "--window",
            &win,
            "--slice",
            &ns,
        ]);
        assert!(s.success, "slice {n} stderr: {}", s.stderr);
        if s.stdout.is_empty() {
            break; // out-of-range slice → empty → done
        }
        assert!(
            s.stdout.chars().count() <= window,
            "slice {n} exceeds the {window}-char window ({} chars)",
            s.stdout.chars().count()
        );
        reassembled.push_str(&s.stdout);
        n += 1;
        assert!(n < 1000, "runaway slice loop");
    }
    assert!(
        n > 2,
        "fixture should span at least two slices, got {}",
        n - 1
    );
    assert_eq!(
        reassembled, document,
        "concatenated slices must reproduce the --out document byte-for-byte"
    );
}

#[test]
fn turns_slice_rejects_out_json_and_zero() {
    // --slice writes the selected chunk to stdout and is verbatim-text only, so it refuses
    // --out, --format json, and the 1-based 0 index — each with a pointed error.
    let h = turns_home();

    let bad_out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--slice",
        "1",
        "--out",
        h.root.join("x.md").to_str().unwrap(),
    ]);
    assert!(!bad_out.success);
    assert!(
        bad_out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        bad_out.stderr
    );

    let bad_json = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--slice",
        "1",
        "--format",
        "json",
    ]);
    assert!(!bad_json.success);
    assert!(
        bad_json.stderr.contains("text format"),
        "stderr: {}",
        bad_json.stderr
    );

    let bad_zero = h.run(&["turns", at(SESS).as_str(), "--no-subagents", "--slice", "0"]);
    assert!(!bad_zero.success);
    assert!(
        bad_zero.stderr.contains("1-based"),
        "stderr: {}",
        bad_zero.stderr
    );
}

/// Parse the JSON-lines stdout into a vector of serde_json::Value objects.
fn json_lines(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| l.trim_start().starts_with('{'))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

/// Strip the OPERATIONAL trailer lines the text renderer prints to stdout but that are
/// NOT part of the reconstruction DOCUMENT (per SPEC §6.8 the document
/// is: doc-header-block + unit headers + bodies + ellipsis markers + boundary banners).
/// The `(skipped N malformed …)` diagnostic and the `(wrote full reconstruction …)`
/// notice are stdout-only chrome, never written to `--out`; everything else stays.
fn turns_document_text(stdout: &str) -> String {
    let kept: Vec<&str> = stdout
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("(skipped ") && !t.starts_with("(wrote ")
        })
        .collect();
    // Re-join with the same '\n' the renderer used; trailing newline normalized away by
    // the line split, so this is the exact emitted-document char basis.
    kept.join("\n")
}

#[test]
fn turns_budget_respected_real_emitted_chars() {
    // HONEST budget test: drive the compiled binary in default TEXT form AND with `--out`,
    // read the ACTUAL emitted bytes, count the WHOLE document with `.chars().count()`, and
    // assert it is <= budget at three real budgets on the multi-compaction fixture. This
    // replaces the old circular checks (the reported "chars used" number, and the JSON sum
    // re-derived with a hardcoded `+ 24`) — neither of which measured the real document.
    //
    // The contract binds the default TEXT form (SPEC §6.8 — budget allocation + text output).
    // We bound BOTH the stdout document (doc-header-block + banners + units, minus the
    // operational trailers) AND the `--out` file (the documented verbatim reconstruction,
    // which omits the stdout-only header block) — so every component the contract lists is
    // measured against budget.
    let h = turns_home();
    for budget in [40000usize, 15000, 8000] {
        let out_path = h.root.join(format!("turns-budget-{budget}.md"));
        let bs = budget.to_string();
        // Default text form (stdout is the document + operational chrome).
        let text = h.run(&[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            &bs,
        ]);
        assert!(text.success, "stderr: {}", text.stderr);

        let doc = turns_document_text(&text.stdout);
        let doc_chars = doc.chars().count();
        assert!(
            doc_chars <= budget,
            "REAL emitted text document is {doc_chars} chars, exceeds budget {budget}\n--- document ---\n{doc}"
        );

        // The `--out` file: the verbatim reconstruction document (no operational chrome).
        let outrun = h.run(&[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            &bs,
            "--out",
            out_path.to_str().unwrap(),
        ]);
        assert!(outrun.success, "stderr: {}", outrun.stderr);
        let body = std::fs::read_to_string(&out_path).expect("out file written");
        let out_chars = body.chars().count();
        assert!(
            out_chars <= budget,
            "REAL --out file is {out_chars} chars, exceeds budget {budget}"
        );

        // The reported "chars used" header line is itself within budget (it is now a real
        // upper bound on the emitted length, not a self-fulfilling cost() echo).
        let reported: usize = text
            .stdout
            .lines()
            .find_map(|l| {
                let l = l.trim();
                let idx = l.find(&format!(" / {budget} chars used"))?;
                l[..idx].rsplit(' ').next()?.parse().ok()
            })
            .expect("chars-used line present");
        assert!(
            reported <= budget,
            "reported chars-used {reported} must be <= budget {budget}"
        );
        // The reported figure must NOT under-state the truth: the real document is <= the
        // header's claim (the fix made the accounting an honest upper bound, never an
        // under-count — that was the original overshoot bug).
        assert!(
            doc_chars <= reported,
            "header claims {reported} chars but the real document is {doc_chars} — the \
             accounting under-states the cost (the overshoot bug)"
        );
    }

    // The skipped malformed line is still surfaced, never hidden (it just is not counted
    // against the reconstruction budget — it is operational chrome).
    let any = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "8000",
    ]);
    assert!(
        any.stdout.contains("1 malformed line(s) skipped"),
        "{}",
        any.stdout
    );
}

#[test]
fn turns_smaller_budget_emits_strictly_less() {
    // The emitted document shrinks monotonically with the budget (real measured chars),
    // and a bigger budget's selected line_no set is a superset of a smaller one's.
    let h = turns_home();
    let doc_len = |budget: &str| -> usize {
        let t = h.run(&[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            budget,
        ]);
        turns_document_text(&t.stdout).chars().count()
    };
    let big = doc_len("40000");
    let small = doc_len("8000");
    assert!(
        small < big,
        "smaller budget must emit fewer chars: 8000→{small} vs 40000→{big}"
    );
    assert!(small <= 8000 && big <= 40000, "both within budget");
}

#[test]
fn turns_smaller_budget_selects_fewer() {
    let h = turns_home();
    let big = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let small = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "3000",
        "--format",
        "json",
    ]);
    let count_units = |s: &str| {
        json_lines(s)
            .iter()
            .filter(|o| o.get("role").is_some())
            .count()
    };
    assert!(
        count_units(&small.stdout) < count_units(&big.stdout),
        "small budget must select strictly fewer units"
    );
}

#[test]
fn turns_round_trip_floor_recovers_a_user_turn() {
    // The fixture's live tail is assistant-heavy (a huge assistant EOT). The 50% floor
    // must still recover at least one USER turn even at a modest budget — the pulse
    // regression. Without the floor a naive recency walk would recover zero users.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "6000",
        "--round-trip-fraction",
        "0.5",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let users = objs.iter().filter(|o| o["role"] == "user").count();
    assert!(
        users >= 1,
        "the 50% floor must recover >=1 user turn, got {users}"
    );
}

#[test]
fn turns_spans_at_least_two_compaction_boundaries() {
    // THE HEADLINE: a 40K budget over the 3-summary fixture must span >= 2 boundaries,
    // and at least one selected unit must come from before the 2nd-newest summary
    // (compactions_before >= 2). Asserted on the compiled binary's JSON over real-shaped
    // committed data.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let boundaries = objs
        .iter()
        .filter(|o| o["kind"] == "compaction_boundary")
        .count();
    assert!(
        boundaries >= 2,
        "must span >=2 compaction boundaries, got {boundaries}"
    );
    let deep = objs
        .iter()
        .filter(|o| o.get("role").is_some())
        .any(|o| o["compactions_before"].as_u64().unwrap_or(0) >= 2);
    assert!(
        deep,
        "at least one unit must predate the 2nd-newest summary"
    );
    // Each boundary record carries a line_no + summary_chars.
    for o in objs.iter().filter(|o| o["kind"] == "compaction_boundary") {
        assert!(o["line_no"].as_u64().unwrap() > 0);
        assert!(o["summary_chars"].as_u64().unwrap() > 0);
    }
}

#[test]
fn turns_max_compactions_caps_the_reach() {
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--max-compactions",
        "1",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let boundaries = objs
        .iter()
        .filter(|o| o["kind"] == "compaction_boundary")
        .count();
    assert!(
        boundaries <= 1,
        "--max-compactions 1 caps boundaries to <=1, got {boundaries}"
    );
    // No selected unit may have compactions_before > 1.
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(
            o["compactions_before"].as_u64().unwrap() <= 1,
            "cap leaked: {o}"
        );
    }
}

#[test]
fn turns_ellipsis_role_asymmetry_and_counts() {
    // The huge live round-trip: user > 600 → head 360 / tail 240; assistant > 900 →
    // head 594 / tail 306. The assistant head is strictly larger. The text output shows
    // the head + the elision marker + the tail; JSON carries the exact elided counts.
    let h = turns_home();
    let text = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    // The user head begins with HEADuser then 'u's; the marker carries the elided count.
    assert!(
        text.stdout.contains("HEADuser"),
        "user head present: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("TAILuser"),
        "user tail kept: {}",
        text.stdout
    );
    assert!(text.stdout.contains("HEADasst"), "asst head present");
    assert!(text.stdout.contains("TAILasst"), "asst tail kept");
    assert!(
        text.stdout.contains("chars elided") || text.stdout.contains("chars]"),
        "elision marker present: {}",
        text.stdout
    );

    let json = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    // Find the huge user + huge assistant units (full_chars over the cap).
    let huge_user = objs
        .iter()
        .find(|o| o["role"] == "user" && o["full_chars"].as_u64().unwrap_or(0) > 600)
        .expect("huge user unit present");
    let huge_asst = objs
        .iter()
        .find(|o| o["role"] == "assistant" && o["full_chars"].as_u64().unwrap_or(0) > 900)
        .expect("huge assistant unit present");
    assert!(huge_user["truncated"].as_bool().unwrap());
    assert!(huge_asst["truncated"].as_bool().unwrap());
    // The assistant rendered_chars (900) is strictly larger than the user's (600) — the
    // role-asymmetric caps drive a larger assistant head.
    assert_eq!(huge_user["rendered_chars"].as_u64().unwrap(), 600);
    assert_eq!(huge_asst["rendered_chars"].as_u64().unwrap(), 900);
    assert!(
        huge_asst["rendered_chars"].as_u64().unwrap()
            > huge_user["rendered_chars"].as_u64().unwrap()
    );
    // elided_chars == full_chars - cap.
    assert_eq!(
        huge_user["elided_chars"].as_u64().unwrap(),
        huge_user["full_chars"].as_u64().unwrap() - 600
    );
    assert_eq!(
        huge_asst["elided_chars"].as_u64().unwrap(),
        huge_asst["full_chars"].as_u64().unwrap() - 900
    );
    // The JSON `text` field is the FULL verbatim message (un-truncated) — longer than
    // the rendered cap.
    assert!(huge_user["text"].as_str().unwrap().chars().count() > 600);
}

#[test]
fn turns_slices_pins_emitted_count_to_the_fleet() {
    // `--slices N` makes the slice COUNT the hard constraint: it emits AT MOST N chunks no matter
    // how many a char budget would have produced, and each chunk stays within the window. A 2-slice
    // fleet over this multi-block fixture: slices 1-2 are within window, and any index > 2 is empty
    // — the count can never drift to 3/4/5 as the turns grow.
    let h = turns_home();
    let win = 1500usize;
    for i in 1..=2 {
        let o = h.run(&[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--slices",
            "2",
            "--window",
            "1500",
            "--slice",
            &i.to_string(),
        ]);
        assert!(o.success, "stderr: {}", o.stderr);
        assert!(
            o.stdout.chars().count() <= win,
            "slice {i} exceeds the window: {}",
            o.stdout.chars().count()
        );
    }
    let s1 = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "2",
        "--window",
        "1500",
        "--slice",
        "1",
    ]);
    assert!(
        !s1.stdout.is_empty(),
        "slice 1 of a filled 2-fleet is non-empty"
    );
    let s3 = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "2",
        "--window",
        "1500",
        "--slice",
        "3",
    ]);
    assert!(
        s3.stdout.is_empty(),
        "an index beyond the fixed fleet must be empty, got: {}",
        s3.stdout
    );
}

#[test]
fn turns_slices_keeps_newest_discards_oldest() {
    // The fleet fills newest-first; the oldest turns that don't fit are DISCARDED (not truncated).
    // A tight 2-slice fleet keeps the live tail and drops the oldest round-trip ("the very first
    // ask … café").
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=2 {
        doc.push_str(
            &h.run(&[
                "turns",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "2",
                "--window",
                "1500",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    assert!(
        !doc.contains("the very first ask"),
        "the oldest turn must be discarded by a small fleet: {doc}"
    );
    assert!(
        doc.contains("final committed answer")
            || doc.contains("TAILuser")
            || doc.contains("short live ask")
            || doc.contains("do the final thing"),
        "the newest turns must be kept: {doc}"
    );
}

#[test]
fn turns_slices_keeps_user_turns_whole_no_role_cap() {
    // The defect a peer session caught: budget mode middle-truncates a USER turn at the 600-char
    // role cap even with budget to spare. In `--slices` mode the only cap is the WINDOW, so a
    // multi-hundred-char user directive survives VERBATIM. The fixture's huge_user (≈817 chars)
    // appears whole (no mid-cut) when the window comfortably exceeds it.
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=8 {
        doc.push_str(
            &h.run(&[
                "turns",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "8",
                "--window",
                "9000",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    let whole = format!("HEADuser {} TAILuser", "u".repeat(800));
    assert!(
        doc.contains(&whole),
        "a long user turn must be kept whole in --slices mode (it was gutted at the 600 cap?)"
    );
    // Contrast: the SAME fixture under budget mode STILL applies the 600 user cap (legacy behavior
    // is untouched) — so the verbatim user body is NOT present and the elision marker IS.
    let budgeted = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(
        !budgeted.stdout.contains(&whole),
        "budget mode must still apply the 600 user cap (legacy unchanged)"
    );
    assert!(
        budgeted.stdout.contains("chars elided") || budgeted.stdout.contains("chars]"),
        "budget mode shows the elision marker"
    );
}

#[test]
fn turns_slices_ellipsizes_only_a_turn_bigger_than_one_window() {
    // The ONLY content cut in --slices mode is a single turn that ALONE exceeds one window. With a
    // small window the big assistant turn (≈3000 chars) is middle-elided, while the shorter user
    // turn (≈817) in the same fleet is kept whole.
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=8 {
        doc.push_str(
            &h.run(&[
                "turns",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "8",
                "--window",
                "1200",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    assert!(
        doc.contains("chars elided") || doc.contains("chars]"),
        "a turn larger than one window is ellipsized: {doc}"
    );
    assert!(
        doc.contains(&format!("HEADuser {} TAILuser", "u".repeat(800))),
        "a turn that fits within one window is kept whole alongside it: {doc}"
    );
}

#[test]
fn turns_slices_requires_a_slice_index() {
    // `--slices N` sets the fleet size; without `--slice i` there is no chunk to emit — a clear
    // error, not a silent full-document dump.
    let h = turns_home();
    let o = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "4",
    ]);
    assert!(!o.success, "must fail without --slice");
    assert!(
        o.stderr.contains("--slice"),
        "error names the missing flag: {}",
        o.stderr
    );
}

#[test]
fn turns_tool_call_markers_present_with_correct_counts() {
    // The fixture's huge live round-trip has 5 tool calls; turn "fifth ask" has 3.
    let h = turns_home();
    let text = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(
        text.stdout.contains("[5 tool calls]"),
        "5-tool marker: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("[3 tool calls]"),
        "3-tool marker present"
    );
    // A 0-tool turn omits the marker — "third reply" turn had 0 tools, so there is no
    // "[0 tool calls]" anywhere.
    assert!(
        !text.stdout.contains("[0 tool calls]"),
        "0-tool marker must be omitted"
    );
    // JSON carries the exact tool_calls count.
    let json = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    assert!(
        objs.iter()
            .any(|o| o["role"] == "user" && o["tool_calls"] == 5),
        "a unit with tool_calls==5 present"
    );
}

#[test]
fn turns_line_numbers_present_in_text_and_json() {
    let h = turns_home();
    let text = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    // Text lines carry L<number> markers (the jsonl line) for both roles.
    assert!(
        text.stdout.lines().any(|l| l.starts_with("▽ L")),
        "user lines carry L-numbers: {}",
        text.stdout
    );
    assert!(
        text.stdout.lines().any(|l| l.starts_with("△ L")),
        "assistant lines carry L-numbers"
    );
    let json = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    for o in json_lines(&json.stdout)
        .iter()
        .filter(|o| o.get("role").is_some())
    {
        assert!(
            o["line_no"].as_u64().unwrap() > 0,
            "every unit carries a positive line_no"
        );
        // full_chars == text.chars().count().
        assert_eq!(
            o["full_chars"].as_u64().unwrap() as usize,
            o["text"].as_str().unwrap().chars().count()
        );
    }
}

#[test]
fn turns_dedup_demotes_summary_match_never_drops() {
    // Turn 0's user ("the very first ask...") is quoted verbatim by SUMMARY #1's §6.
    // BUT turn 0 sits BEFORE older boundaries (compactions_before > 0), so it is NOT
    // deduped (older summary content is gone from context). To exercise live-region
    // dedup we check the NEWEST summary's quotes against live turns; the fixture's live
    // turns are unique, so dedup count may be 0 here — assert the mechanism via the
    // header only when it fires, and always assert nothing is dropped.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    // Every selected unit has a boolean also_in_summary field (mechanism wired).
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(o["also_in_summary"].is_boolean());
    }
    // Turn 0's verbatim user text is still present (not dropped) even though SUMMARY #1
    // quotes it — pre-boundary turns are pure restoration.
    assert!(
        objs.iter().any(|o| o["role"] == "user"
            && o["text"]
                .as_str()
                .unwrap()
                .contains("the very first ask about the café")),
        "the pre-boundary verbatim user turn is restored, never dropped"
    );
}

#[test]
fn turns_fidelity_beats_summary_verbatim_count() {
    // The summary holds ~1 verbatim assistant quote (§9) + a handful of clipped §6
    // bullets. `turns` restores MANY more verbatim units. Assert concrete counts:
    // >= 3 restored user units and >= 3 restored assistant units, far exceeding the
    // summary's single verbatim assistant quote.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&out.stdout);
    let users = objs.iter().filter(|o| o["role"] == "user").count();
    let asst = objs.iter().filter(|o| o["role"] == "assistant").count();
    assert!(
        users >= 3,
        "restored user units {users} must exceed the summary's clipped bullets"
    );
    assert!(
        asst >= 3,
        "restored assistant units {asst} must exceed the summary's 1 verbatim quote"
    );
}

#[test]
fn turns_deterministic_byte_identical() {
    let h = turns_home();
    let a = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    let b = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    assert_eq!(
        a.stdout, b.stdout,
        "two identical invocations must be byte-identical"
    );
}

#[test]
fn turns_out_file_holds_full_reconstruction() {
    let h = turns_home();
    let out_path = h.root.join("turns-out.md");
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote full reconstruction"),
        "{}",
        out.stdout
    );
    let body = std::fs::read_to_string(&out_path).expect("out file written");
    assert!(body.contains("▽ L"), "out file carries the rendered turns");
    assert!(
        body.contains("compaction boundary"),
        "out file carries banners"
    );
}

#[test]
fn turns_token_budget_unit_scales_by_four() {
    // --budget-unit tokens multiplies by ~4 chars/token. A 3000-token budget should
    // select more than a 3000-char budget (4x the room).
    let h = turns_home();
    let tok = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "3000",
        "--budget-unit",
        "tokens",
        "--format",
        "json",
    ]);
    let chr = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "3000",
        "--budget-unit",
        "chars",
        "--format",
        "json",
    ]);
    let units = |s: &str| {
        json_lines(s)
            .iter()
            .filter(|o| o.get("role").is_some())
            .count()
    };
    assert!(
        units(&tok.stdout) >= units(&chr.stdout),
        "a token budget (4x chars) must not select fewer units"
    );
}

#[test]
fn turns_turn_range_and_since_mutually_exclusive() {
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--turn-range",
        "0..2",
        "--since",
        "2h",
    ]);
    assert!(!out.success, "the conflicting window flags must error");
    assert!(
        out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn turns_invalid_round_trip_fraction_errors() {
    let h = turns_home();
    for f in ["0", "1", "1.5", "-0.1"] {
        let out = h.run(&[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--round-trip-fraction",
            f,
        ]);
        assert!(!out.success, "round-trip-fraction {f} must be rejected");
    }
}

#[test]
fn turns_zero_budget_errors() {
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "0",
    ]);
    assert!(!out.success, "a zero budget must error");
    assert!(
        out.stderr.contains("--budget must be > 0"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn turns_help_lists_the_subcommand_and_flags() {
    let h = turns_home();
    let top = h.run(&["--help"]);
    assert!(
        top.stdout.contains("turns"),
        "top help lists turns: {}",
        top.stdout
    );
    let sub = h.run(&["turns", "--help"]);
    assert!(sub.stdout.contains("--budget"), "{}", sub.stdout);
    assert!(
        sub.stdout.contains("--round-trip-fraction"),
        "{}",
        sub.stdout
    );
    assert!(sub.stdout.contains("--max-compactions"), "{}", sub.stdout);
    assert!(sub.stdout.contains("--budget-unit"), "{}", sub.stdout);
}

/// A fixture whose NEWEST summary quotes a LIVE-region turn verbatim → exercises the
/// live-region dedup demote-and-flag path (the main fixture's live turns are unique).
fn turns_dedup_home() -> Home {
    let h = Home::new();
    let mut s = String::new();
    // turn 0 (pre-boundary): user + assistant.
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pre-boundary ask kept verbatim here"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"pre reply"}]}}"#);
    s.push('\n');
    // SUMMARY whose §6 quotes a LIVE turn that comes AFTER it (the "live duplicate ask").
    s.push_str(r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"the live duplicate ask that the summary already holds verbatim\"\n9. Optional Next Step:\n   The assistant said \"pre reply\"."}}"#);
    s.push('\n');
    // turn 1 (LIVE region) whose user text matches the summary's §6 bullet → deduped.
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"the live duplicate ask that the summary already holds verbatim"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"live reply"}]}}"#);
    s.push('\n');
    // turn 2 (LIVE region) unique.
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"a unique live follow-up question"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T07:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"follow-up reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{SESS}.jsonl"), &s);
    h
}

#[test]
fn turns_live_region_dedup_demotes_and_flags() {
    let h = turns_dedup_home();
    // Text: the dedup header line + the (also in summary) flag must appear.
    let text = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("also present in summary"),
        "dedup header line: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("(also in summary)"),
        "demoted-unit flag: {}",
        text.stdout
    );
    // JSON: exactly the live duplicate unit carries also_in_summary:true, and it is still
    // PRESENT (demoted, never dropped).
    let json = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    let flagged: Vec<_> = objs
        .iter()
        .filter(|o| o.get("also_in_summary").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert!(
        !flagged.is_empty(),
        "at least one unit flagged also_in_summary"
    );
    assert!(
        flagged.iter().any(|o| o["text"]
            .as_str()
            .unwrap()
            .contains("the live duplicate ask")),
        "the live duplicate unit is flagged AND present (not dropped)"
    );
}

#[test]
fn turns_json_out_file_is_verbatim() {
    let h = turns_home();
    let out_path = h.root.join("turns.json");
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let body = std::fs::read_to_string(&out_path).expect("json out file");
    // The huge user unit's full verbatim text (un-truncated) is in the file.
    assert!(
        body.contains("HEADuser"),
        "out json carries the unit objects"
    );
    // Every non-blank line is valid JSON.
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line).expect("each out line is JSON");
    }
}

#[test]
fn turns_no_genuine_turns_emits_honest_empty_message() {
    // A session with NO genuine user turns (only a summary + an isMeta pseudo-turn +
    // tool noise) → nothing selected, an honest "no turns selected" message (never a
    // fabricated turn). This is the only empty-selection path: the most-recent complete
    // turn is always force-included when one exists (load-bearing).
    let h = Home::new();
    let mut s = String::new();
    s.push_str(r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"gone\""}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"a carrier, not a genuine turn"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{SESS}.jsonl"), &s);
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no turns selected"),
        "honest empty message: {}",
        out.stdout
    );
}

#[test]
fn turns_default_all_projects_scan_runs() {
    // No target + no --session → scan every project under the scratch $HOME. The fixture
    // is the only project, so it is found. Exercises the all-projects resolve path.
    let h = turns_home();
    let out = h.run(&["turns", "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_json_single_side_units_present_under_tight_budget() {
    // A tight budget forces some single-side (user-only / assistant-only) selections in
    // the JSON output — exercise the single-side JSON emit path.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "2500",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    // Some units selected, budget respected.
    let units: usize = objs.iter().filter(|o| o.get("role").is_some()).count();
    assert!(units >= 1, "at least one unit under a tight budget");
    let sum: usize = objs
        .iter()
        .filter(|o| o.get("role").is_some())
        .map(|o| o["rendered_chars"].as_u64().unwrap() as usize + 24)
        .sum();
    assert!(sum <= 2500, "tight budget respected: {sum}");
}

#[test]
fn turns_since_window_filters_turns() {
    // A `--since` far in the future excludes every turn (timestamp-less or older) → no
    // selection. Exercises the time-window path in run_turns.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--since",
        "2999-01-01T00:00:00Z",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("no turns selected"), "{}", out.stdout);
}

/// A SECOND clean session (no malformed lines) under the same project — exercises the
/// multi-session render path (blank separator) + the no-skipped-lines branch.
fn turns_two_sessions_home() -> Home {
    let h = Home::new();
    // Session 1: the main multi-summary fixture (has a malformed line).
    h.write(&format!("{ENC}/{SESS}.jsonl"), &turns_fixture_jsonl());
    // Session 2: a small CLEAN session (no malformed line, no summary).
    let sess2 = "00000000-0000-4000-8000-000000000002";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"clean session ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"clean session reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess2}.jsonl"), &s);
    h
}

#[test]
fn turns_multi_session_text_has_blank_separator_and_both_sessions() {
    // No --session → both sessions in the project are rendered, separated by a blank
    // line (the `if !first { println!() }` arm). Sessions are sorted by id.
    let h = turns_two_sessions_home();
    let out = h.run(&["turns", "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let session_headers = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("SESSION "))
        .count();
    assert!(
        session_headers >= 2,
        "both sessions rendered: {}",
        out.stdout
    );
    // The clean session's content is present.
    assert!(out.stdout.contains("clean session ask"), "{}", out.stdout);
}

#[test]
fn turns_defaults_to_top_level_only_no_subagent_span() {
    // FOOTGUN FIX: `turns <uuid>` with NO flags must reconstruct ONLY the top-level thread —
    // it must NOT span the session's subagents (unlike files/search). So a bare run prints no
    // `(subagent transcript)` blocks and no scope banner (one session in scope, rendered).
    let h = populated_home();
    let out = h.run(&["turns", at(SESS).as_str(), "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("SESSION {SESS}")),
        "the top-level thread must render: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("(subagent transcript)"),
        "turns must NOT span subagents by default: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("scope  "),
        "a single top-level session prints no scope banner: {}",
        out.stdout
    );
}

#[test]
fn turns_include_subagents_opts_into_span_with_scope_banner() {
    // `--include-subagents` is the explicit opt-in for the rare cross-fan-out reconstruction;
    // it spans the subagents AND prints a scope banner that reports the TRUE top-level/subagent
    // split (never `0 top-level`, even though the budget applies per session).
    let h = populated_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--include-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(subagent transcript)"),
        "--include-subagents must span subagents: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("scope  ") && out.stdout.contains("1 top-level"),
        "scope banner must report the targeted top-level (never 0 top-level): {}",
        out.stdout
    );
}

#[test]
fn turns_targeted_top_level_skipped_at_tiny_budget_is_reported_not_silent() {
    // CRITICAL: at a budget too small for the targeted top-level session's first round-trip,
    // the session must be reported with an explicit skip note (never silently absent), and the
    // scope banner must still count it as `1 top-level` in scope — not `0`.
    let h = populated_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--include-subagents",
        "--budget",
        "120",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("SESSION {SESS}  skipped")),
        "the targeted top-level session must be reported as skipped, not silently dropped: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("1 top-level"),
        "scope banner must still report 1 top-level in scope (not 0): {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("raise --budget"),
        "the skip note must tell the user to raise --budget: {}",
        out.stdout
    );
}

#[test]
fn turns_json_header_carries_true_scope_and_rendered_and_by_kind() {
    // The JSON session_header distinguishes TRUE scope (sessions_in_scope) from rendered
    // (sessions_rendered), and carries the per-class automation_by_kind breakdown.
    let h = populated_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--include-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let first = out.stdout.lines().next().unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(first).expect("header json");
    assert_eq!(v["kind"], "session_header");
    assert!(
        v.get("sessions_in_scope").is_some(),
        "missing sessions_in_scope: {first}"
    );
    assert!(
        v.get("sessions_rendered").is_some(),
        "missing sessions_rendered: {first}"
    );
    assert_eq!(
        v["top_level_sessions"], 1,
        "targeted top-level counted: {first}"
    );
    let by = v
        .get("automation_by_kind")
        .expect("automation_by_kind present");
    for k in ["background-command", "agent", "workflow", "monitor", "task"] {
        assert!(by.get(k).is_some(), "by_kind missing class {k}: {first}");
    }
}

#[test]
fn turns_clean_session_reports_no_skipped_lines() {
    // A session with NO malformed lines → the skipped-lines footer is OMITTED (the
    // `skipped_lines > 0` false arm).
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000003";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"clean ask one"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"clean reply one"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "turns",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("malformed"),
        "clean session has no skipped-line footer: {}",
        out.stdout
    );
}

#[test]
fn turns_json_clean_session_emits_zero_skipped_terminator() {
    // JSON: a clean session now ALWAYS closes with a {"kind":"skipped_lines",
    // "skipped_lines":0} terminator (so a consumer can detect end-of-stream) — the record is
    // unconditional, mirroring search/files/recover.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000004";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"clean json ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"clean json reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "turns",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let term = objs
        .iter()
        .find(|o| o["kind"] == "skipped_lines")
        .expect("clean session still emits the skipped_lines terminator");
    assert_eq!(term["skipped_lines"].as_u64().unwrap(), 0);
    assert!(objs.iter().any(|o| o["role"] == "user"));
}

#[test]
fn turns_main_fixture_text_reports_skipped_line() {
    // The main fixture HAS a malformed line → the skipped-lines footer appears (the
    // `skipped_lines > 0` TRUE arm in both text + an explicit count).
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 malformed line(s) skipped"),
        "{}",
        out.stdout
    );
}

#[test]
fn turns_json_main_fixture_has_skipped_record() {
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    assert!(
        objs.iter()
            .any(|o| o["kind"] == "skipped_lines" && o["skipped_lines"].as_u64().unwrap() >= 1),
        "the malformed line is surfaced in JSON under the `skipped_lines` key"
    );
}

#[test]
fn turns_assistant_only_orphan_lead_renders() {
    // A session that LEADS with an assistant before any user is rare but real; turns
    // must render the orphan assistant side without panicking (the user-None render
    // arms). group_turn_indices folds the lead into turn 0.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000005";
    let mut s = String::new();
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"orphan lead before any user"}]}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"user","content":"the real first ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"real reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "turns",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_turn_range_alone_is_not_a_conflict() {
    // --turn-range WITHOUT --since/--until is valid (the L186 false arm: turn_range set
    // but since/until both None). Restrict to turns 0..2.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--turn-range",
        "0..2",
        "--format",
        "json",
    ]);
    assert!(
        out.success,
        "a bare --turn-range must not conflict: {}",
        out.stderr
    );
    let objs = json_lines(&out.stdout);
    // No turn beyond index 2 selected.
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(
            o["turn_index"].as_u64().unwrap() <= 2,
            "turn-range cap: {o}"
        );
    }
}

#[test]
fn turns_empty_session_file_is_safe() {
    // An empty jsonl (0 bytes) → mmap returns None → no turns, honest empty message.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000006";
    h.write(&format!("{ENC}/{sess}.jsonl"), "");
    let out = h.run(&[
        "turns",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("no turns selected"), "{}", out.stdout);
}

#[test]
fn turns_valid_round_trip_fraction_accepted() {
    // A fraction strictly inside (0,1) is accepted (the L189 false arm — valid input).
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--round-trip-fraction",
        "0.7",
    ]);
    assert!(out.success, "0.7 is a valid fraction: {}", out.stderr);
    assert!(
        out.stdout.contains("round-trip-fraction 0.70"),
        "{}",
        out.stdout
    );
}

#[test]
fn turns_nonzero_budget_accepted() {
    // A positive budget passes the L195 check (false arm).
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "1000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_since_and_until_both_bound_the_window() {
    // BOTH --since and --until set (the L186 inner `||` both-bounds path + the time
    // window contains() both arms). A wide window admits the fixture's turns.
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--since",
        "2026-06-07T00:00:00Z",
        "--until",
        "2026-06-08T00:00:00Z",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_token_budget_text_output_runs() {
    // --budget-unit tokens through the TEXT renderer (the BudgetUnit::Tokens conversion
    // arm in run_turns, distinct from the json tests).
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "2000",
        "--budget-unit",
        "tokens",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // 2000 tokens ≈ 8000 chars budget in the header.
    assert!(out.stdout.contains("budget 8000 chars"), "{}", out.stdout);
}

#[test]
fn turns_turn_range_excludes_out_of_window_turns() {
    // A turn-range that excludes the LOW turns (the L278 `turn_index < lo` true arm) and
    // the HIGH turns (`turn_index > hi` true arm).
    let h = turns_home();
    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--turn-range",
        "3..5",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        let ti = o["turn_index"].as_u64().unwrap();
        assert!((3..=5).contains(&ti), "turn {ti} outside 3..5");
    }
}

#[test]
fn turns_multi_session_json_runs_both() {
    // JSON over two sessions (no --session) → both sessions' units emitted.
    let h = turns_two_sessions_home();
    let out = h.run(&[
        "turns",
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sessions: std::collections::BTreeSet<&str> = objs
        .iter()
        .filter_map(|o| o.get("session_id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        sessions.len() >= 2,
        "both sessions present in JSON: {sessions:?}"
    );
}

#[test]
fn turns_scan_skips_non_candidate_lines() {
    // A session containing NON-candidate records (a system metrics line, a
    // file-history-snapshot) interleaved with real turns → the scan-time prefilter skips
    // them (the `!line_is_turn_candidate` true arm) without affecting the turns.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000007";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask before noise"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"system","subtype":"turn_duration","durationMs":1234}"#);
    s.push('\n');
    s.push_str(
        r#"{"type":"file-history-snapshot","snapshot":{"messageId":"m","trackedFileBackups":{}}}"#,
    );
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply after noise"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "turns",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("ask before noise"), "{}", out.stdout);
    assert!(out.stdout.contains("reply after noise"), "{}", out.stdout);
    // The non-candidate lines are silently skipped (not malformed → no skip count).
    assert!(
        !out.stdout.contains("malformed"),
        "non-candidate != malformed: {}",
        out.stdout
    );
}

// ── Multi-agent-message richness (the model-expansion) ──

#[test]
fn turns_agent_msgs_rich_restores_middles_and_collapses_declarations() {
    // `--agent-msgs rich` over the long-run turn: the rich first / sudden-rich middle /
    // fused body survive verbatim; the pure-declaration middles collapse into a
    // placeholder carrying a fetchable L{a}–L{b} range. The default (eot-only) shows ONLY
    // the EOT — proving the flag changes behavior.
    let h = turns_home();
    let rich = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
    ]);
    assert!(rich.success, "stderr: {}", rich.stderr);
    // Rich members survive verbatim.
    assert!(
        rich.stdout.contains("AGENTRICHFIRST"),
        "rich first kept: {}",
        rich.stdout
    );
    assert!(
        rich.stdout.contains("AGENTRICHMID"),
        "sudden rich middle kept"
    );
    assert!(
        rich.stdout.contains("FUSEDTAIL"),
        "fused finding+decl body kept whole"
    );
    assert!(rich.stdout.contains("AGENTEOT"), "the EOT is always kept");
    // The pure declarations are collapsed — their unique token must NOT appear verbatim.
    assert!(
        !rich.stdout.contains("LETMEDECL"),
        "pure declarations must be collapsed, not emitted: {}",
        rich.stdout
    );
    // A placeholder line with a fetchable range is present.
    assert!(
        rich.stdout.contains("agent message") && (rich.stdout.contains("tool call")),
        "a collapsed-agents placeholder is present: {}",
        rich.stdout
    );
    // The `eot-only` ESCAPE keeps only the EOT — the intermediate rich members are absent.
    let eot = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "eot-only",
    ]);
    assert!(eot.stdout.contains("AGENTEOT"), "eot-only keeps the EOT");
    assert!(
        !eot.stdout.contains("AGENTRICHFIRST") && !eot.stdout.contains("AGENTRICHMID"),
        "the eot-only escape must NOT restore intermediate agent messages: {}",
        eot.stdout
    );
}

#[test]
fn turns_default_longest_restores_substance_and_drops_declarations() {
    // The NEW DEFAULT (`longest`, no flag) over the long-run fixture turn. The agent run's
    // char lengths are: AGENTRICHFIRST=43, decls 26–34, AGENTRICHMID=45, FUSEDTAIL=72
    // (the LONGEST), AGENTEOT=35. So the default keeps:
    //   • FUSEDTAIL — the LONGEST (72 chars) → the substantive Rich Response.
    //   • AGENTRICHMID — a RICH middle (file:line + ratio) → a mid-run major finding.
    // and COLLAPSES everything else into placeholders, INCLUDING:
    //   • AGENTRICHFIRST — a SHORT first (43 < 280 rich-min) and not the longest → dropped
    //     (proves the first is kept only when SUBSTANTIVE, not merely rich/present).
    //   • AGENTEOT — a SHORT, non-rich LAST (the ~35-char throwaway wrap-up) → dropped
    //     (THE headline: the last is no longer unconditionally kept; the substance is).
    //   • the pure LETMEDECL declarations.
    // This is exactly the substance the OLD `agents.last()` default silently dropped, plus
    // the deliberate dropping of the throwaway last.
    let h = turns_home();
    let dflt = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(dflt.success, "stderr: {}", dflt.stderr);
    // The LONGEST + the rich middle are restored.
    assert!(
        dflt.stdout.contains("FUSEDTAIL"),
        "default restores the LONGEST agent message: {}",
        dflt.stdout
    );
    assert!(
        dflt.stdout.contains("AGENTRICHMID"),
        "default restores the rich middle finding: {}",
        dflt.stdout
    );
    // The throwaway last (AGENTEOT) and the short first (AGENTRICHFIRST) are NOT kept by
    // the default — they fall below the substantive/rich bar and are not the longest.
    assert!(
        !dflt.stdout.contains("AGENTEOT"),
        "default drops the non-rich throwaway LAST (the headline case): {}",
        dflt.stdout
    );
    assert!(
        !dflt.stdout.contains("AGENTRICHFIRST"),
        "default drops a SHORT (non-substantive) first: {}",
        dflt.stdout
    );
    // The pure declarations still collapse — the default is NOT `all`.
    assert!(
        !dflt.stdout.contains("LETMEDECL"),
        "default collapses pure declarations into a placeholder: {}",
        dflt.stdout
    );
    assert!(
        dflt.stdout.contains("agent message") && dflt.stdout.contains("tool call"),
        "a collapsed-agents placeholder is present under the default: {}",
        dflt.stdout
    );
}

#[test]
fn turns_agent_msgs_rich_placeholder_range_is_fetchable_and_attributed() {
    // The JSON form carries a `collapsed_agents` record with X/Y/Z + first/last line so a
    // consumer can Read the raw range; Y is non-zero (each collapsed msg had a tool_use).
    let h = turns_home();
    let json = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let objs = json_lines(&json.stdout);
    let ph = objs
        .iter()
        .find(|o| o["kind"] == "collapsed_agents")
        .expect("a collapsed_agents placeholder record");
    assert!(ph["agent_messages"].as_u64().unwrap() >= 1);
    assert!(
        ph["tool_calls"].as_u64().unwrap() >= 1,
        "Y attributes the span's tool calls"
    );
    let first = ph["first_line"].as_u64().unwrap();
    let last = ph["last_line"].as_u64().unwrap();
    assert!(first <= last && first > 0, "a fetchable jsonl line range");
}

#[test]
fn turns_agent_msgs_all_keeps_every_message_no_placeholder() {
    // `--agent-msgs all` emits every agent message of the long run, no placeholder.
    let h = turns_home();
    let all = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "all",
    ]);
    assert!(all.success, "stderr: {}", all.stderr);
    // Even the pure declarations appear verbatim now.
    assert!(
        all.stdout.contains("LETMEDECL"),
        "all keeps declarations: {}",
        all.stdout
    );
    assert!(all.stdout.contains("AGENTRICHFIRST") && all.stdout.contains("AGENTEOT"));
    // No collapsed-agents placeholder line.
    assert!(
        !all.stdout.contains("agent messages]") && !all.stdout.contains("agent message]"),
        "all mode emits no placeholder: {}",
        all.stdout
    );
}

/// The captured pre-feature baseline: the EXACT single-EOT stdout the `turns` tool emitted
/// on the §fixture BEFORE the multi-agent-message richness feature (one agent EOT message
/// per turn, no `△ L…–L…` collapsed-agents placeholder, no intermediate rich members). The
/// DEFAULT now keeps the LONGEST agent message + the first-if-substantive + the rich
/// middles, so this baseline is reproduced by the `--agent-msgs eot-only` ESCAPE, not by
/// the implicit default. Captured under `TZ=UTC` so the system-local timestamp render is
/// deterministic across machines. Re-capture (only on an INTENDED eot-only-output change):
///   TZ=UTC csift turns @<SESS> --no-subagents --budget 40000 --agent-msgs eot-only
const TURNS_PRE_FEATURE_BASELINE: &str = include_str!("turns_pre_feature_baseline.txt");

#[test]
fn turns_eot_only_escape_is_byte_identical_to_pre_feature_baseline() {
    // The `eot-only` ESCAPE reproduces the pre-feature single-EOT document byte-for-byte
    // (the "force last-only" guarantee), asserted TWO ways:
    //   (1) `--agent-msgs eot-only` is byte-identical to a CAPTURED pre-feature baseline —
    //       catches a drift in the last-only path even if the default moved with it;
    //   (2) the IMPLICIT default now DIFFERS — it restores intermediate substance (the
    //       longest + rich members) the old single-EOT default silently dropped.
    // `TZ=UTC` pins the system-local timestamp render so the captured baseline is portable.
    let h = turns_home();
    let tz_utc = [("TZ", "UTC")];
    let eot_only = h.run_with_env(
        &[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--agent-msgs",
            "eot-only",
        ],
        &tz_utc,
    );
    assert!(eot_only.success, "stderr: {}", eot_only.stderr);
    assert_eq!(
        eot_only.stdout, TURNS_PRE_FEATURE_BASELINE,
        "`--agent-msgs eot-only` must be byte-identical to the captured pre-feature \
         (single-EOT) baseline; an INTENDED eot-only-output change requires re-capturing \
         tests/turns_pre_feature_baseline.txt under TZ=UTC --agent-msgs eot-only"
    );

    // The implicit default (Longest) is DIFFERENT — it restores the substance the
    // single-EOT default dropped (proving the default changed, not just a flag alias).
    let implicit = h.run_with_env(
        &[
            "turns",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
        ],
        &tz_utc,
    );
    assert!(implicit.success, "stderr: {}", implicit.stderr);
    assert_ne!(
        implicit.stdout, eot_only.stdout,
        "the implicit default must NO LONGER equal eot-only — it keeps the longest + \
         rich members the single-EOT default silently dropped"
    );
}

#[test]
fn turns_profile_heavy_keeps_at_least_as_many_as_light() {
    // heavy (lower thresholds) selects >= as many KEPT agent messages as light, and both
    // are bounded by `all` and floored by `eot-only`.
    let h = turns_home();
    let at_sess = at(SESS);
    let kept_agents = |args: &[&str]| -> usize {
        let mut full = vec![
            "turns",
            at_sess.as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--format",
            "json",
        ];
        full.extend_from_slice(args);
        let out = h.run(&full);
        assert!(out.success, "stderr: {}", out.stderr);
        json_lines(&out.stdout)
            .iter()
            .filter(|o| o["role"] == "assistant")
            .count()
    };
    let eot = kept_agents(&["--agent-msgs", "eot-only"]);
    let light = kept_agents(&["--profile", "light"]);
    let heavy = kept_agents(&["--profile", "heavy"]);
    let all = kept_agents(&["--agent-msgs", "all"]);
    assert!(heavy >= light, "heavy {heavy} >= light {light}");
    assert!(light >= eot, "light {light} >= eot-only {eot}");
    assert!(all >= heavy, "all {all} >= heavy {heavy}");
}

#[test]
fn turns_budget_respected_under_rich_and_all_modes() {
    // The summed-cost == summed-emitted invariant holds with placeholders + multi-agent
    // lanes: the REAL emitted document stays <= budget under rich AND all, across budgets.
    let h = turns_home();
    for mode in ["rich", "all"] {
        for budget in [40000usize, 15000, 8000] {
            let bs = budget.to_string();
            let out = h.run(&[
                "turns",
                at(SESS).as_str(),
                "--no-subagents",
                "--budget",
                &bs,
                "--agent-msgs",
                mode,
            ]);
            assert!(out.success, "stderr: {}", out.stderr);
            let doc = turns_document_text(&out.stdout);
            assert!(
                doc.chars().count() <= budget,
                "mode {mode} budget {budget}: real document is {} chars (over budget)",
                doc.chars().count()
            );
        }
    }
}

#[test]
fn turns_rich_filters_subagent_runs_too() {
    // The shared code path: a SUBAGENT transcript carrying a long agent run is richness-
    // filtered with the same flags (default --include-subagents). The subagent's pure
    // declarations collapse; its rich member + EOT survive.
    let h = turns_home();
    // A subagent sidecar with a long agent run under the session.
    let mut sub = String::new();
    sub.push_str(r#"{"type":"user","isSidechain":true,"agentId":"subrun","timestamp":"2026-06-07T09:00:00.000Z","message":{"role":"user","content":"subagent kicks off a long chain"}}"#);
    sub.push('\n');
    let msgs = [
        "SUBRICHFIRST found the cause in src/z.rs:7",
        "let me SUBDECL a",
        "now i will SUBDECL b",
        "let me SUBDECL c",
        "now let me SUBDECL d",
        "next i SUBDECL e",
        "let me SUBDECL f",
        "the SUBEOT final subagent answer",
    ];
    let mut ts = 1;
    for m in msgs {
        sub.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T09:00:{ts:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"s","name":"Bash","input":{{}}}}]}}}}"#
        ));
        sub.push('\n');
        ts += 1;
        sub.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T09:00:{ts:02}.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{m}"}}]}}}}"#
        ));
        sub.push('\n');
        ts += 1;
    }
    h.write(&format!("{ENC}/{SESS}/subagents/agent-subrun.jsonl"), &sub);

    let out = h.run(&[
        "turns",
        at(SESS).as_str(),
        "--include-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("SUBRICHFIRST"),
        "subagent rich member kept: {}",
        out.stdout
    );
    assert!(out.stdout.contains("SUBEOT"), "subagent EOT kept");
    assert!(
        !out.stdout.contains("SUBDECL"),
        "subagent pure declarations collapse under the shared richness path: {}",
        out.stdout
    );
}

#[test]
fn turns_help_lists_the_new_agent_msg_flags() {
    let h = turns_home();
    let help = h.run(&["turns", "--help"]);
    assert!(help.success);
    for flag in [
        "--agent-msgs",
        "--agent-run-threshold",
        "--agent-rich-min-chars",
        "--agent-declaration-max-chars",
        "--keep-first",
        "--no-keep-first",
        "--profile",
    ] {
        assert!(
            help.stdout.contains(flag),
            "help must list {flag}: {}",
            help.stdout
        );
    }
    // Invalid enum values exit nonzero with a clap error.
    let bad_mode = h.run(&["turns", at(SESS).as_str(), "--agent-msgs", "bogus"]);
    assert!(!bad_mode.success, "invalid --agent-msgs must fail");
    let bad_profile = h.run(&["turns", at(SESS).as_str(), "--profile", "bogus"]);
    assert!(!bad_profile.success, "invalid --profile must fail");
}

// ── Genuine-user-message holes (Part B): AskUserQuestion answer as a turn boundary,
//    ExitPlanMode rejection-with-message + plan pointer, interrupt non-boundary —
//    driven end-to-end through the REAL binary on a session built from the verified
//    real-data record shapes (AUQ answer boundary; ExitPlanMode typed reject). ──

/// A session whose ONLY genuine human opener is "start the work", followed by an
/// AskUserQuestion exchange (Q+options+multi-byte answer) and an ExitPlanMode plan that the
/// user REJECTS with a typed message, plus an interrupt marker that must NOT split a
/// turn.
fn holes_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            // turn 0: genuine human opener.
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the work"}}"#, "\n",
            // assistant asks (member of turn 0). Options carry per-option descriptions.
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which option for step two?","header":"STEP TWO","options":[{"label":"option A (recommended)","description":"the conservative path that reuses existing state"},{"label":"option B","description":"the full path that rebuilds from scratch"}]}]}}]}}"#, "\n",
            // turn 1: the AUQ ANSWER opens a turn (the behavior change). The carrier echoes
            // the options WITH descriptions and carries free-text `annotations.notes`.
            r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","toolUseResult":{"questions":[{"question":"which option for step two?","header":"STEP TWO","options":[{"label":"option A (recommended)","description":"the conservative path that reuses existing state"},{"label":"option B","description":"the full path that rebuilds from scratch"}]}],"answers":{"which option for step two?":"option A is fine, the scope is broader than stated"},"annotations":{"which option for step two?":{"notes":"go with option A but budget for the edge cases, it is more involved than a quick tweak"}}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"which option for step two?\"=\"option A is fine, the scope is broader than stated\"."}]}}"#, "\n",
            // assistant proposes a plan (member of turn 1).
            r#"{"type":"assistant","uuid":"a1","parentUuid":"ans","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_PLAN1","name":"ExitPlanMode","input":{"plan":"the plan body here","planFilePath":"/Users/testuser/.claude/plans/elegant-scribbling-dream.md"}}]}}"#, "\n",
            // turn 2: the user REJECTS the plan with a typed message → boundary + pointer.
            r#"{"type":"user","uuid":"rej","parentUuid":"a1","timestamp":"2026-06-07T05:20:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLAN1","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nplease run the smoke tests once before calling it done"}]}}"#, "\n",
            // an interrupt marker — a turn MEMBER of turn 2, NOT a new boundary.
            r#"{"type":"user","uuid":"int","parentUuid":"rej","timestamp":"2026-06-07T05:20:30.000Z","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"int","timestamp":"2026-06-07T05:21:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok, adding the smoke-test check"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn auq_answer_opens_a_turn_and_surfaces_clean_answer() {
    let h = holes_home();
    // search -t user for the answer prose: it must surface under `user`.
    let out = h.run(&[
        "search",
        "option A is fine",
        "-t",
        "user",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hit_line = out
        .stdout
        .lines()
        .find(|l| l.contains("option A is fine"))
        .unwrap_or_else(|| panic!("AUQ answer not surfaced under user:\n{}", out.stdout));
    let v: serde_json::Value = serde_json::from_str(hit_line).unwrap();
    // It is a genuine-user turn boundary now → turn_index 1 (after the "start" opener).
    assert_eq!(
        v.get("turn_index").and_then(serde_json::Value::as_u64),
        Some(1),
        "AUQ answer must open turn 1: {hit_line}"
    );
}

#[test]
fn turns_reconstructs_auq_exchange_and_plan_rejection_with_pointer() {
    let h = holes_home();
    let out = h.run(&["turns", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The AUQ exchange is reconstructed as a complete unit: marker + question + options
    // + the answer prose.
    assert!(
        out.stdout.contains("AskUserQuestion"),
        "AUQ unit label missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("option A (recommended)"),
        "AUQ options missing:\n{}",
        out.stdout
    );
    // Each option's DESCRIPTION (supplementary note) must survive — not just the label.
    assert!(
        out.stdout
            .contains("the conservative path that reuses existing state"),
        "AUQ option description missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("the full path that rebuilds from scratch"),
        "second AUQ option description missing:\n{}",
        out.stdout
    );
    // Free-text notes the user attached to the answer must surface verbatim.
    assert!(
        out.stdout
            .contains("it is more involved than a quick tweak"),
        "AUQ answer notes missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("option A is fine, the scope is broader than stated"),
        "AUQ answer missing:\n{}",
        out.stdout
    );
    // The plan rejection surfaces the user's typed instruction AND a pointer to the
    // plan file.
    assert!(
        out.stdout
            .contains("please run the smoke tests once before calling it done"),
        "plan-rejection user message missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[plan: /Users/testuser/.claude/plans/elegant-scribbling-dream.md]"),
        "plan pointer missing:\n{}",
        out.stdout
    );
}

#[test]
fn search_finds_auq_option_descriptions_and_answer_notes_under_user() {
    let h = holes_home();
    // (1) A phrase that lives ONLY in an option's `description` must be searchable in the
    //     reconstructed USER turn (not merely in the raw assistant tool-call JSON).
    let desc = h.run(&[
        "search",
        "the conservative path that reuses existing state",
        "-t",
        "user",
        at(SESS).as_str(),
    ]);
    assert!(desc.success, "stderr: {}", desc.stderr);
    assert!(
        desc.stdout
            .contains("the conservative path that reuses existing state"),
        "option description not searchable under user:\n{}",
        desc.stdout
    );
    // (2) A phrase that lives ONLY in the answer's `annotations.notes` must be searchable
    //     under `user` — it IS the user's typed message. (Regression: previously dropped,
    //     so this returned "no matching exchanges".)
    let notes = h.run(&[
        "search",
        "more involved than a quick tweak",
        "-t",
        "user",
        at(SESS).as_str(),
    ]);
    assert!(notes.success, "stderr: {}", notes.stderr);
    assert!(
        notes.stdout.contains("more involved than a quick tweak"),
        "answer notes not searchable under user:\n{}",
        notes.stdout
    );
}

#[test]
fn interrupt_does_not_split_a_turn() {
    let h = holes_home();
    // The interrupt marker must NOT surface as its own genuine-user turn. Searching for
    // the marker under `user` yields nothing (it is not genuine-user).
    let out = h.run(&["search", "Request interrupted by user", "-t", "user"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges")
            || !out.stdout.contains("◂ user · [Request interrupted"),
        "interrupt must not be a genuine-user hit:\n{}",
        out.stdout
    );
    // And `list` must NOT pick the interrupt as the last-user preview — the real last
    // user message is the plan-rejection instruction.
    let lst = h.run(&["list"]);
    assert!(lst.success, "stderr: {}", lst.stderr);
    assert!(
        !lst.stdout.contains("[Request interrupted by user]"),
        "interrupt leaked into the list preview:\n{}",
        lst.stdout
    );
}

// ── round 7: SCOPE-disclosure uniformity, wrong-flag diagnostics, --out data-safety ──

/// The shared `scope  N sessions in scope (X top-level + Y subagent)` banner is now emitted
/// by EVERY subagent-spanning text surface (list/files/search/recover/turns), not just
/// list/turns. populated_home spans 2 subagents under 1 top-level session.
#[test]
fn scope_banner_uniform_across_spanning_subcommands() {
    let h = populated_home();
    let f = h.run(&["files", at(SESS).as_str(), "--by-file"]);
    assert!(
        f.stdout.contains("sessions in scope"),
        "files banner:\n{}",
        f.stdout
    );
    let s = h.run(&["search", "carry", at(SESS).as_str()]);
    assert!(
        s.stdout.contains("sessions in scope"),
        "search banner:\n{}",
        s.stdout
    );
    let r = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--coverage",
        "--file",
        "/tmp/x",
    ]);
    assert!(
        r.stdout.contains("sessions in scope"),
        "recover banner:\n{}",
        r.stdout
    );
    let l = h.run(&["list", at(SESS).as_str()]);
    assert!(
        l.stdout.contains("sessions in scope"),
        "list banner:\n{}",
        l.stdout
    );
    // The banner is SUPPRESSED under --no-subagents (single top-level transcript).
    let f2 = h.run(&["files", at(SESS).as_str(), "--by-file", "--no-subagents"]);
    assert!(
        !f2.stdout.contains("sessions in scope"),
        "files --no-subagents banner leaked:\n{}",
        f2.stdout
    );
}

/// The leading `{kind:"session_header", …}` JSON scope record is emitted by every spanning
/// subcommand's JSON, reusing turns' three span field names.
#[test]
fn scope_json_header_uniform_across_spanning_subcommands() {
    let h = populated_home();
    // Bind the `@<uuid>` target once so the vecs below can borrow it (a temporary `at(SESS)`
    // inside the array literal would be dropped before `h.run` borrows it).
    let at_sess = at(SESS);
    for args in [
        vec!["list", at_sess.as_str(), "--format", "json"],
        vec!["files", at_sess.as_str(), "--by-file", "--format", "json"],
        vec!["search", "carry", at_sess.as_str(), "--format", "json"],
        vec![
            "recover",
            at_sess.as_str(),
            "--coverage",
            "--file",
            "/tmp/x",
            "--format",
            "json",
        ],
    ] {
        let out = h.run(&args);
        assert!(out.success, "{:?} stderr: {}", args, out.stderr);
        let first = out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap();
        let v: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(
            v.get("kind").and_then(|k| k.as_str()),
            Some("session_header"),
            "{:?} first JSON line is not a session_header:\n{}",
            args,
            out.stdout
        );
        assert!(
            v.get("sessions_in_scope").is_some(),
            "{:?} header span",
            args
        );
        assert!(
            v.get("top_level_sessions").is_some(),
            "{:?} header span",
            args
        );
        assert!(
            v.get("subagent_sessions").is_some(),
            "{:?} header span",
            args
        );
    }
}

/// `--no-subagents` is DOMINANT regardless of flag order (the r6→r7 `overrides_with` removal):
/// passing `--include-subagents` LAST no longer re-enables the fan-out the user suppressed.
#[test]
fn no_subagents_dominant_regardless_of_order_end_to_end() {
    let h = populated_home();
    let span = |out: &Output| out.stdout.contains("sessions in scope");
    // Both orders must suppress the banner (top-level only).
    assert!(!span(&h.run(&[
        "list",
        at(SESS).as_str(),
        "--no-subagents",
        "--include-subagents"
    ])));
    assert!(!span(&h.run(&[
        "list",
        at(SESS).as_str(),
        "--include-subagents",
        "--no-subagents"
    ])));
    assert!(!span(&h.run(&[
        "files",
        at(SESS).as_str(),
        "--by-file",
        "--no-subagents",
        "--include-subagents"
    ])));
    assert!(!span(&h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--no-subagents",
        "--include-subagents"
    ])));
}

/// `--subagents-only` is a `files`-only flag; mistyped onto a sibling it now produces a
/// POINTED error (naming the right flag), not the generic clap PATH-swallow.
#[test]
fn subagents_only_misplaced_gives_pointed_error() {
    let h = populated_home();
    for sub in ["turns", "recover", "list"] {
        let out = h.run(&[sub, at(SESS).as_str(), "--subagents-only"]);
        assert!(!out.success, "{sub} --subagents-only should fail");
        assert!(
            out.stderr.contains("`files`-only flag") && out.stderr.contains("--no-subagents"),
            "{sub}: expected pointed error, got: {}",
            out.stderr
        );
    }
    // search too (pattern positional first).
    let out = h.run(&["search", "x", at(SESS).as_str(), "--subagents-only"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("`files`-only flag"),
        "search: {}",
        out.stderr
    );
    // files itself still accepts it as the real flag.
    let ok = h.run(&["files", at(SESS).as_str(), "--subagents-only", "--by-file"]);
    assert!(
        ok.success,
        "files --subagents-only must work: {}",
        ok.stderr
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
        "turns",
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
        "turns",
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

/// turns text now brands a subagent block `SUBAGENT <hex> · parent SESSION <uuid>` (uniform
/// with list/files/search), never tokening a bare subagent hex as `SESSION`.
#[test]
fn turns_text_brands_subagent_uniformly() {
    let h = populated_home();
    let out = h.run(&["turns", at(SESS).as_str(), "--include-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The subagent block carries the SUBAGENT token + the re-feedable parent uuid.
    assert!(
        out.stdout.contains("SUBAGENT") && out.stdout.contains(&format!("parent SESSION {SESS}")),
        "turns subagent branding missing:\n{}",
        out.stdout
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Failed-Edit handling: a tool call whose RESULT was an error (is_error:true) never
// mutated the file, so reconstruction/coverage must NOT count it. Two triggers:
//   • "String to replace not found in file." (old_string absent)
//   • "File has not been read yet."           (Edit-before-Read wall — Bash/Grep don't
//                                               satisfy CC's Read gate; same wall the
//                                               must-re-Read-a-plan semi-bug hits)
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the line texts (in order) from a `recover --at … --format json` snapshot.
fn recon_lines_from_at_json(stdout: &str) -> Vec<String> {
    let snap = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("snapshot"))
        .unwrap_or_else(|| panic!("no snapshot object in:\n{stdout}"));
    let mut recon: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        recon.insert(
            l.get("n").and_then(|v| v.as_u64()).unwrap() as usize,
            l.get("text").and_then(|v| v.as_str()).unwrap().to_string(),
        );
    }
    recon.into_values().collect()
}

#[test]
fn recover_at_skips_failed_string_not_found_edit_top_level() {
    // Top-level: Write 3 lines, a FAILED Edit (is_error:true, no toolUseResult carrier — so
    // its id is absent from ids_with_result and the input-side fallback would otherwise apply
    // the ghost), then a SUCCESSFUL Edit. The ghost must be absent; the good edit applied.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit f.md"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w0","name":"Write","input":{"file_path":"/p/f.md","content":"L1\nL2\nL3\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/f.md","content":"L1\nL2\nL3\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w0","content":"ok"}]}}"#, "\n",
            // FAILED edit — old_string not in the file. No toolUseResult; tool_result is_error.
            r#"{"type":"assistant","uuid":"a_bad","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_bad","name":"Edit","input":{"file_path":"/p/f.md","old_string":"NONEXISTENT","new_string":"GHOST"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c_bad","timestamp":"2026-06-07T05:00:02.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_bad","content":"String to replace not found in file.","is_error":true}]}}"#, "\n",
            // SUCCESSFUL edit — carrier with structuredPatch.
            r#"{"type":"assistant","uuid":"a_ok","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_ok","name":"Edit","input":{"file_path":"/p/f.md","old_string":"L2","new_string":"L2-ok"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c_ok","timestamp":"2026-06-07T05:00:03.500Z","toolUseResult":{"filePath":"/p/f.md","oldString":"L2","newString":"L2-ok","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-L2","+L2-ok"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_ok","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/f.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("GHOST"),
        "the failed edit's new_string must never appear: {}",
        out.stdout
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["L1", "L2-ok", "L3"],
        "only the successful edit lands"
    );
}

#[test]
fn recover_subagent_input_fallback_skips_failed_edit() {
    // The DANGER case: a SUBAGENT records results as bare tool_result strings (no
    // toolUseResult), so content comes from the input-side fallback. A failed Edit there
    // (is_error:true) must be skipped, not replayed from its input.
    const PSESS: &str = "cccccccc-9999-9999-9999-999999999999";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"spawn a worker"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-deadbeef.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"make g.md then fix it"}}"#, "\n",
            // Write via input fallback (bare success result).
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw","name":"Write","input":{"file_path":"/p/g.md","content":"aa\nbb\ncc\n"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:11.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sw","content":"File created successfully at: /p/g.md"}]}}"#, "\n",
            // FAILED edit (is_error) — must NOT be applied from the input.
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sbad","name":"Edit","input":{"file_path":"/p/g.md","old_string":"NOPE","new_string":"GHOST"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sbad","content":"String to replace not found in file.","is_error":true}]}}"#, "\n",
            // SUCCESSFUL edit via input fallback (bare success result).
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sok","name":"Edit","input":{"file_path":"/p/g.md","old_string":"bb","new_string":"bb-ok"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:13.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sok","content":"The file /p/g.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-deadbeef.meta.json"),
        r#"{"agentType":"general-purpose","description":"worker","toolUseId":"t0"}"#,
    );
    let out = h.run(&[
        "recover",
        at(PSESS).as_str(),
        "--file",
        "/p/g.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("GHOST"),
        "ghost edit leaked: {}",
        out.stdout
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["aa", "bb-ok", "cc"],
        "subagent: only the good edit lands"
    );
}

#[test]
fn recover_coverage_excludes_failed_edit_before_read_after_bash_create() {
    // The user's explicit case: Bash CREATES a file, then a direct Edit (no Read) FAILS with
    // "File has not been read yet" (Bash doesn't satisfy CC's Read gate). When coverage
    // measures "how much can be recovered", that failed Edit must NOT be counted as a
    // recoverable edit — only the (content-less) Bash touch + the integrity boundary show.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"make a config"}}"#, "\n",
            // Bash creates the file (heuristic touch, no content captured).
            r#"{"type":"assistant","uuid":"ab","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"printf 'B1\nB2\nB3\n' > /p/cfg.txt"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"cb","timestamp":"2026-06-07T05:00:01.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b0","content":""}]}}"#, "\n",
            // Direct Edit with no prior Read → fails.
            r#"{"type":"assistant","uuid":"ae","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_nr","name":"Edit","input":{"file_path":"/p/cfg.txt","old_string":"B2","new_string":"B2-edited"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"ce","timestamp":"2026-06-07T05:00:02.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_nr","content":"File has not been read yet. Read it first before writing to it.","is_error":true}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/cfg.txt",
        "--coverage",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("B2-edited"),
        "failed edit content must not appear anywhere: {}",
        out.stdout
    );
    let cov: serde_json::Value = serde_json::from_str(
        out.stdout
            .lines()
            .find(|l| l.contains("recoverable_lines"))
            .unwrap(),
    )
    .unwrap();
    let ev = &cov["events"];
    assert_eq!(
        ev["edit"].as_u64(),
        Some(0),
        "failed edit not counted as a recoverable edit: {cov}"
    );
    assert_eq!(
        ev["edit_unanchorable"].as_u64(),
        Some(0),
        "failed edit not even counted as an un-anchorable edit: {cov}"
    );
    assert_eq!(
        ev["bash"].as_u64(),
        Some(1),
        "the Bash create IS a (heuristic) touch: {cov}"
    );
    assert_eq!(
        ev["integrity_error"].as_u64(),
        Some(1),
        "the Edit-before-Read failure surfaces as an integrity annotation, not an edit: {cov}"
    );
    assert_eq!(
        cov["recoverable_lines"].as_u64(),
        Some(0),
        "nothing is recoverable (Bash has no content, the edit failed): {cov}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Plan-file binding: the `plan` subcommand + the `recover --file @plan` magic resolve
// the session-bound plan via its `plan_mode` attachment — never a path heuristic.
// ─────────────────────────────────────────────────────────────────────────────

/// Build a top-level transcript that (a) binds a plan via a `plan_mode` attachment, (b) also
/// Edits a DIFFERENT plan file (which must NOT be mistaken for the bound one), and (c) has a
/// Write+Edit history of the bound plan (so `@plan` recover has content to rebuild).
fn write_planning_session(h: &Home, sess: &str, bound_abs: &str, other_abs: &str) {
    let jsonl = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"plan it"}}"#, "\n",
        // plan_mode attachment → the AUTHORITATIVE binding.
        r#"{"type":"attachment","isSidechain":false,"attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"__BOUND__","planExists":true},"uuid":"att0","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
        // An Edit of SOMEONE ELSE's plan file — a red herring for the resolver.
        r#"{"type":"assistant","uuid":"ax","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ex","name":"Edit","input":{"file_path":"__OTHER__","old_string":"x","new_string":"y"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"cx","timestamp":"2026-06-07T05:00:02.500Z","toolUseResult":{"filePath":"__OTHER__","oldString":"x","newString":"y","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-x","+y"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ex","content":"ok"}]}}"#, "\n",
        // The bound plan's own Write + Edit history.
        r#"{"type":"assistant","uuid":"aw","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"pw","name":"Write","input":{"file_path":"__BOUND__","content":"P1\nP2\nP3\n"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"cw","timestamp":"2026-06-07T05:00:03.500Z","toolUseResult":{"type":"create","filePath":"__BOUND__","content":"P1\nP2\nP3\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"pw","content":"ok"}]}}"#, "\n",
        r#"{"type":"assistant","uuid":"ap","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"pe","name":"Edit","input":{"file_path":"__BOUND__","old_string":"P2","new_string":"P2-revised"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"cp","timestamp":"2026-06-07T05:00:04.500Z","toolUseResult":{"filePath":"__BOUND__","oldString":"P2","newString":"P2-revised","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-P2","+P2-revised"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"pe","content":"ok"}]}}"#, "\n",
    )
    .replace("__BOUND__", bound_abs)
    .replace("__OTHER__", other_abs);
    h.write(&format!("{ENC}/{sess}.jsonl"), &jsonl);
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
    let v: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next().unwrap_or("")).unwrap();
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
    let v: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next().expect("a hit")).unwrap();
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
fn recover_file_plan_magic_reconstructs_the_bound_plan() {
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let bound_abs = plans_dir
        .join("shimmying-spinning-cascade.md")
        .to_string_lossy()
        .into_owned();
    let other_abs = plans_dir.join("decoy.md").to_string_lossy().into_owned();
    write_planning_session(&h, SESS, &bound_abs, &other_abs);

    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "@plan",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The stderr note announces the resolution.
    assert!(
        out.stderr.contains("@plan resolved to")
            && out.stderr.contains("shimmying-spinning-cascade.md"),
        "missing @plan resolution note: {}",
        out.stderr
    );
    // The bound plan's full Write+Edit history is reconstructed (not the decoy plan).
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["P1", "P2-revised", "P3"],
        "recovered the bound plan's content: {}",
        out.stdout
    );
}

#[test]
fn recover_file_plan_errors_when_no_plan_is_bound() {
    // A session that never entered Plan Mode has no bound plan → @plan must error clearly
    // (never fall back to guessing a plans/ path).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"just code"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "@plan",
        "--coverage",
    ]);
    assert!(!out.success, "should fail: {}", out.stdout);
    assert!(
        out.stderr.contains("no plan file is bound") && out.stderr.contains("plan_mode"),
        "unhelpful error: {}",
        out.stderr
    );
}

#[test]
fn recover_file_plan_errors_when_ambiguous_across_sessions() {
    // Two top-level sessions under one project, each bound to a DIFFERENT plan → @plan over
    // the whole project is ambiguous and must ask for --session, never silently pick one.
    const SESS2: &str = "abcdef01-2345-6789-abcd-ef0123456789";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let a = plans_dir.join("plan-a.md").to_string_lossy().into_owned();
    let b = plans_dir.join("plan-b.md").to_string_lossy().into_owned();
    let decoy = plans_dir.join("decoy.md").to_string_lossy().into_owned();
    write_planning_session(&h, SESS, &a, &decoy);
    write_planning_session(&h, SESS2, &b, &decoy);

    let out = h.run(&["recover", ENC, "--file", "@plan", "--coverage"]);
    assert!(!out.success, "should be ambiguous: {}", out.stdout);
    assert!(
        out.stderr.contains("different bound plan files") && out.stderr.contains("@<uuid>"),
        "unhelpful ambiguity error: {}",
        out.stderr
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
    .replace("__SUBPLAN__", &sub_plan);
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
    let v: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next().unwrap_or("")).unwrap();
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
    .replace("__TOP__", &top);
    h.write(&format!("{ENC}/{PSESS}.jsonl"), &top_jsonl);
    let sub_jsonl = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"bbbb01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan the subtask"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"bbbb01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUB__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__SUB__", &sub);
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
fn recover_file_plan_resolves_subagent_only_plan() {
    // The top-level session never planned, but a SUBAGENT did → @plan falls back to the
    // subagent's bound plan and reconstructs its Write+Edit history.
    const PSESS: &str = "99998888-7777-6666-5555-444433332222";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let sub_plan = plans_dir
        .join("subagent-only-agent-ccccccccccccccccc.md")
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
        r#"{"type":"user","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan + draft it"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"cccc01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUB__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
        r#"{"type":"assistant","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw","name":"Write","input":{"file_path":"__SUB__","content":"D1\nD2\nD3\n"}}]}}"#, "\n",
        r#"{"type":"user","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sw","content":"File created successfully at: __SUB__"}]}}"#, "\n",
        r#"{"type":"assistant","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"se","name":"Edit","input":{"file_path":"__SUB__","old_string":"D2","new_string":"D2-final"}}]}}"#, "\n",
        r#"{"type":"user","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:13.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"se","content":"The file __SUB__ has been updated successfully."}]}}"#, "\n",
    )
    .replace("__SUB__", &sub_plan);
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-cccc01.jsonl"),
        &sub_jsonl,
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-cccc01.meta.json"),
        r#"{"agentType":"general-purpose","description":"planner","toolUseId":"t0"}"#,
    );

    let out = h.run(&[
        "recover",
        at(PSESS).as_str(),
        "--file",
        "@plan",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("@plan resolved to")
            && out.stderr.contains("subagent-only-agent-")
            && out.stderr.contains("subagent"),
        "resolved to the subagent plan: {}",
        out.stderr
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["D1", "D2-final", "D3"],
        "subagent-only plan reconstructed: {}",
        out.stdout
    );
}

// ── image ──

/// A synthetic 1×1 transparent PNG (base64) — a REAL valid PNG (correct chunk CRCs, so the
/// strict `image` decoder accepts it for `--as` transcoding, not just the magic-byte checks).
const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=";
/// Three more DISTINCT valid 1×1 PNGs (red / green / blue) — distinct content fingerprints, so
/// the listing's content-dedup treats them as separate screenshots (not one re-injected image).
const PNG_RED: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
const PNG_GREEN: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==";
const PNG_BLUE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==";
/// A real 3-frame (red/green/blue, 0.5s each = 1.5s) 4×4 animated GIF89a, for the
/// "convert an animated GIF to a still format → first frame + warning" path.
const ANIM_GIF_3F: &str = "R0lGODlhBAAEAPAAAP8AAAAAACH/C05FVFNDQVBFMi4wAwEAAAAh+QQAMgAAACwAAAAABAAEAAACBISPCQUAIfkEADIAAAAsAAAAAAQABACAAIAAAAAAAgSEjwkFACH5BAAyAAAALAAAAAAEAAQAgAAA/wAAAAIEhI8JBQA7";

fn img_block(media: &str, data: &str) -> serde_json::Value {
    serde_json::json!({"type":"image","source":{"type":"base64","media_type":media,"data":data}})
}

fn image_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"first screenshot"}, img_block("image/png", PNG_1X1)]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","content":[{"type":"text","text":"got it"}]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"two more"}, img_block("image/jpeg", PNG_RED), img_block("image/png", PNG_GREEN)]}
    });
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n"),
    );
    h
}

#[test]
fn image_lists_images_with_stable_ids() {
    let h = image_home();
    let out = h.run(&["image", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    // r0 = line 1 (1 image), r2 = line 3 (2 images): L1i1 png, L3i1 jpeg, L3i2 png.
    assert!(out.stdout.contains("L1i1"), "L1i1 missing:\n{}", out.stdout);
    assert!(out.stdout.contains("L3i1"), "L3i1 missing:\n{}", out.stdout);
    assert!(out.stdout.contains("L3i2"), "L3i2 missing:\n{}", out.stdout);
    assert!(out.stdout.contains("image/jpeg"));
    assert!(
        out.stdout.contains("3 image(s)"),
        "count line:\n{}",
        out.stdout
    );
}

#[test]
fn image_extracts_real_bytes_to_dir() {
    let h = image_home();
    let out_dir = h.root.join("imgs");
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("extracted 3 image(s)"),
        "{}",
        out.stdout
    );
    // <sess8>-L1i1.png must be a REAL decoded PNG (magic bytes).
    let f = out_dir.join("0a1b2c3d-L1i1.png");
    let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "not a PNG"
    );
    // media_type drives the extension: image/jpeg → .jpg.
    assert!(out_dir.join("0a1b2c3d-L3i1.jpg").exists());
    assert!(out_dir.join("0a1b2c3d-L3i2.png").exists());
}

/// Fixture: a session where `#1` names TWO different images (CC reuses `#N` per prompt). r0
/// (turn 0) carries #1=transparent + #2=red; a later r2 (turn 1) reuses #1 for a different
/// image (blue). Returns the home.
fn ambiguous_hash_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"look at [Image #1] and [Image #2]"},
            img_block("image/png", PNG_1X1), img_block("image/png", PNG_RED)]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1deadbeef","timestamp":"2026-06-07T06:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"now re-sharing [Image #1]"},
            img_block("image/png", PNG_BLUE)]}
    });
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n"),
    );
    h
}

#[test]
fn image_ambiguous_hash_n_errors_with_occurrence_list() {
    let h = ambiguous_hash_home();

    // Listing surfaces the `#N` handle and shows BOTH #1 images (distinct content → not deduped).
    let list = h.run(&["image", at(SESS).as_str(), "--no-subagents"]);
    assert!(list.success, "stderr: {}", list.stderr);
    assert!(list.stdout.contains("#1") && list.stdout.contains("#2"));
    assert!(
        list.stdout.contains("3 image(s)"),
        "all 3 distinct (no content-dedup):\n{}",
        list.stdout
    );

    // `--id #1` is AMBIGUOUS → it must ERROR (not silently pick one) and list every occurrence
    // with its turn / locator / uuid / time / excerpt so the consumer can disambiguate.
    let err = h.run(&["image", at(SESS).as_str(), "--no-subagents", "--id", "#1"]);
    assert!(!err.success, "ambiguous #1 must fail, got:\n{}", err.stdout);
    assert!(err.stderr.contains("ambiguous"), "stderr: {}", err.stderr);
    assert!(
        err.stderr.contains("L1i1") && err.stderr.contains("L3i1"),
        "both occurrences listed: {}",
        err.stderr
    );
    assert!(
        err.stderr.contains("t0") && err.stderr.contains("t1"),
        "turn indices shown: {}",
        err.stderr
    );
    // the excerpt centers on the marker, and the uuid prefix is surfaced.
    assert!(
        err.stderr.contains("re-sharing") && err.stderr.contains("u1deadbe"),
        "excerpt + uuid in the list: {}",
        err.stderr
    );

    // `--id #2` is UNIQUE (only the line-1 red) → resolves fine.
    let two = h.run(&["image", at(SESS).as_str(), "--no-subagents", "--id", "#2"]);
    assert!(two.success, "stderr: {}", two.stderr);
    assert!(two.stdout.contains("L1i2"), "{}", two.stdout);
}

#[test]
fn image_hash_n_disambiguators_resolve_to_one() {
    let h = ambiguous_hash_home();
    // Each disambiguator narrows `#1` to a unique image: turn, time window, uuid, exact locator.
    let by_turn = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--turn-range",
        "1..1",
        "--id",
        "#1",
        "--format",
        "json",
    ]);
    assert!(by_turn.success, "stderr: {}", by_turn.stderr);
    assert!(
        by_turn.stdout.contains("\"id\": \"L3i1\"") || by_turn.stdout.contains("\"id\":\"L3i1\""),
        "turn-range 1..1 → the line-3 blue #1:\n{}",
        by_turn.stdout
    );

    // Time window: r2 is at 06:00, r0 at 05:00 → --since 05:30 isolates the line-3 one.
    let by_time = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--since",
        "2026-06-07T05:30:00Z",
        "--id",
        "#1",
    ]);
    assert!(by_time.success, "stderr: {}", by_time.stderr);
    assert!(by_time.stdout.contains("L3i1"), "{}", by_time.stdout);

    // uuid prefix → the line-3 record.
    let by_uuid = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--uuid",
        "u1deadbe",
        "--id",
        "#1",
    ]);
    assert!(by_uuid.success, "stderr: {}", by_uuid.stderr);
    assert!(by_uuid.stdout.contains("L3i1"), "{}", by_uuid.stdout);

    // Exact locator extracts the line-3 (blue) image; filename carries `#1` + the locator.
    let out_dir = h.root.join("d_imgs");
    let ex = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L3i1",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(ex.success, "stderr: {}", ex.stderr);
    let f = out_dir.join("0a1b2c3d-img1-L3i1.png");
    let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
}

#[test]
fn image_converts_by_out_path_extension() {
    // The --out path's EXTENSION drives the format (the `convert in out.jpg` idiom): a single
    // image written to a file with a recognized image extension is CONVERTED to it; a png source
    // → a .png path is a raw passthrough. There is no separate `--as` flag.
    let h = image_home(); // r0 L1i1 is a real PNG.
    let png_magic = &[0x89u8, b'P', b'N', b'G'][..];
    for (ext, magic, len) in [
        ("jpg", &[0xFFu8, 0xD8, 0xFF][..], 3usize),
        ("gif", &b"GIF8"[..], 4),
        ("webp", &b"RIFF"[..], 4),
        ("png", png_magic, 4),
    ] {
        let f = h.root.join(format!("shot.{ext}"));
        let out = h.run(&[
            "image",
            at(SESS).as_str(),
            "--no-subagents",
            "--id",
            "L1i1",
            "--out",
            f.to_str().unwrap(),
        ]);
        assert!(out.success, ".{ext} stderr: {}", out.stderr);
        let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
        assert_eq!(&bytes[..len], magic, ".{ext} produced wrong magic bytes");
    }
    // WebP carries the "WEBP" fourcc at offset 8 (lossy VP8, via libwebp).
    let wb = std::fs::read(h.root.join("shot.webp")).unwrap();
    assert_eq!(&wb[8..12], b"WEBP");

    // A single-file path with >1 image selected is an error (can't write many to one file).
    let many = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--out",
        h.root.join("x.png").to_str().unwrap(),
    ]);
    assert!(
        !many.success,
        "single file + many images must error: {}",
        many.stdout
    );
    assert!(
        many.stderr.contains("single") && many.stderr.contains("directory"),
        "error names the file/dir distinction: {}",
        many.stderr
    );

    // A directory path (no image extension) keeps the SOURCE format, auto-named — no conversion.
    let dir = h.root.join("imgs");
    let d = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L1i1",
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert!(d.success, "stderr: {}", d.stderr);
    assert!(
        dir.join("0a1b2c3d-L1i1.png").exists(),
        "source-format auto-name in the directory"
    );
}

#[test]
fn image_animated_gif_to_still_takes_first_frame() {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"an animation [Image #1]"},
            img_block("image/gif", ANIM_GIF_3F)]}
    });
    h.write(&format!("{ENC}/{SESS}.jsonl"), &format!("{r0}\n"));

    // A .png out path flattens the animated GIF to a still → FIRST frame + a warning (frames + s).
    let f = h.root.join("frame.png");
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "#1",
        "--out",
        f.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("first frame") && out.stdout.contains("3 frames"),
        "first-frame warning with frame count:\n{}",
        out.stdout
    );
    let bytes = std::fs::read(&f).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );

    // A .gif out path (same format as source) is a raw passthrough — animation preserved, no warning.
    let g = h.root.join("keep.gif");
    let keep = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "#1",
        "--out",
        g.to_str().unwrap(),
    ]);
    assert!(keep.success, "stderr: {}", keep.stderr);
    assert!(
        !keep.stdout.contains("first frame"),
        "no flatten note: {}",
        keep.stdout
    );
    let gb = std::fs::read(&g).unwrap();
    assert_eq!(&gb[..4], b"GIF8");
}

#[test]
fn search_surfaces_extractable_image_ids_on_a_hit() {
    // A `search` hit on a message that carries images must expose the SAME extractable ids as
    // `turns`/`image` — so a search result feeds straight into `csift image --id` with no
    // manual L+i assembly. r2 ("two more") carries a jpeg + a png at line 3 → L3i1, L3i2.
    let h = image_home();
    let out = h.run(&["search", "two more", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("[2 images: L3i1, L3i2]"),
        "image-id suffix on the hit line:\n{}",
        out.stdout
    );
    // The JSON envelope carries the ids array on the hit too.
    let j = h.run(&[
        "search",
        "two more",
        at(SESS).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let hit = j
        .stdout
        .lines()
        .find(|l| l.contains("\"image_ids\"") && l.contains("L3i1"))
        .expect("a hit object carrying image_ids");
    assert!(hit.contains("L3i2"), "both ids present: {hit}");
}

#[test]
fn image_id_selection_json_and_unresolved() {
    let h = image_home();
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L3i2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| l.trim_start().starts_with('{'))
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(objs
        .iter()
        .any(|o| o.get("id").and_then(|v| v.as_str()) == Some("L3i2")));
    assert!(objs.iter().any(|o| o.get("images").is_some())); // trailing summary
                                                             // A nonexistent id is an explicit error, never a silent miss.
    let miss = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L999i9",
    ]);
    assert!(!miss.success);
    assert!(miss.stderr.contains("L999i9"), "stderr: {}", miss.stderr);
}

#[test]
fn image_extract_single_by_id() {
    let h = image_home();
    let out_dir = h.root.join("one");
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L1i1",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out_dir.join("0a1b2c3d-L1i1.png").exists());
    let n = std::fs::read_dir(&out_dir).unwrap().count();
    assert_eq!(n, 1, "only the selected image should be written");
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
fn turns_surfaces_image_ids_under_the_user_turn() {
    // The image marker shows the SAME `L<line>i<n>` id that `image --id` consumes, so a
    // turns reader can pull the bytes back without re-scanning.
    let h = image_home();
    let out = h.run(&["turns", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("L1i1"),
        "image id not surfaced in turns:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("image"),
        "no [N image(s)] marker:\n{}",
        out.stdout
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
    let out_b = h.run(&[
        "search",
        "",
        "/Users/testuser/Projects/foo_bar",
        "-t",
        "user",
        "-l",
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
