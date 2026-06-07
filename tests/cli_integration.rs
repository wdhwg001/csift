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
    // Every non-empty line must be a JSON object with a session_id.
    let mut count = 0;
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        assert!(v.get("session_id").is_some(), "missing session_id: {line}");
        count += 1;
    }
    assert!(count >= 1, "expected at least the top-level session");
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
    assert!(out.stderr.contains("--session"), "stderr: {}", out.stderr);
}

// ── search ──

#[test]
fn search_text_returns_round_trip_exchange() {
    let h = populated_home();
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "no header:\n{}", out.stdout);
    assert!(out.stdout.contains("TURN"));
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
    // -t agent restricts to agent text; --max-count 1 forces the cap + drop note.
    let out = h.run(&["search", "carry", "-t", "agent", "--max-count", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched 1"));
    // With more than one agent hit across turns/subagents, the cap drops some.
    assert!(out.stdout.contains("dropped"));
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
fn search_empty_pattern_with_session_only_does_not_warn() {
    // Empty pattern + ONLY `--session` (no category/time/turn filter) → the warning
    // chain reaches `args.session.is_none()` and it is FALSE → warning suppressed.
    // This is the last operand of the chain, so the earlier operands are all true.
    let h = populated_home();
    let out = h.run(&["search", "", "--session", SESS, "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "a --session scope must suppress the warning; stderr: {}",
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
        "--session",
        SESS,
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "category filter must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_with_explicit_path_target() {
    // `--path <encoded>` exercises resolve_search_targets' explicit-paths branch
    // (`paths.is_empty()` FALSE).
    let h = populated_home();
    let out = h.run(&["search", "carry", "--path", ENC, "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched"), "got: {}", out.stdout);
}

#[test]
fn search_session_filter_and_turn_range() {
    let h = populated_home();
    // --session selects the parent; --turn-range picks turn 1 only.
    let out = h.run(&[
        "search",
        "",
        "--session",
        SESS,
        "--turn-range",
        "1..1",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("TURN 1"));
    assert!(!out.stdout.contains("TURN 0"));
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
        "--session",
        "00000000-0000-0000-0000-000000000000",
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
        "--session",
        SESS,
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("TURN 1"));
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
fn whoami_path_flag_when_not_found() {
    let h = Home::new(); // empty projects → the session id won't resolve to a file
    let out = h.run_with_env(
        &["whoami", "--path"],
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
    assert!(out.stderr.contains("--session"), "stderr: {}", out.stderr);
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
    let out = h.run_with_env(&["whoami", "--path"], &[("CLAUDE_CODE_SESSION_ID", sid)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("-ZZZ-second"),
        "found in the second dir: {}",
        out.stdout
    );
}

#[test]
fn whoami_text_no_path_flag_when_not_found_is_silent() {
    // The `None => {}` render arm: session resolved but file not found AND --path NOT
    // given → only the session line prints, no path note.
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
        !out.stdout.contains("not found"),
        "no path note without --path"
    );
    assert!(
        !out.stdout.to_lowercase().contains("path"),
        "no path line at all"
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
fn agents_text_lists_lifecycle_rows() {
    let h = populated_home();
    let out = h.run(&["agents", "--session", SESS]);
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
fn agents_json_rows() {
    let h = populated_home();
    let out = h.run(&["agents", "--session", SESS, "--format", "json"]);
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
    let out = h.run(&["agents", "--session", SESS, "--kind", "workflow"]);
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
        "--session",
        SESS,
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
    let out = h.run(&["agents", "--session", SESS]);
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
        "--session",
        "deadbeef-0000-0000-0000-000000000000",
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
    let out = h.run(&["agents", "--session", SESS]);
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
    let out = h.run(&["agents", "--session", SESS]);
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
