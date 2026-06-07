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
    let out = h.run(&["files", "--session", SESS]);
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
    let out = h.run(&["files", "--session", SESS, "--by-file", "--format", "json"]);
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
    let out = h.run(&["files", "--session", SESS, "--timeline"]);
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
    let out = h.run(&["files", "--session", SESS, "--by-dir", "--format", "json"]);
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
    let out = h.run(&["files", "--session", SESS, "--turn-range", "0..0"]);
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
        "--session",
        SESS,
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
        "--session",
        SESS,
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
    let out = h.run(&["files", "--session", SESS, "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no file mutations found"),
        "got: {}",
        out.stdout
    );
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
    let with = h.run(&["files", "--session", SESS]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(
        with.stdout.contains("/tmp"),
        "subagent write spanned: {}",
        with.stdout
    );
    let without = h.run(&["files", "--session", SESS, "--no-subagents"]);
    assert!(without.success, "stderr: {}", without.stderr);
    assert!(
        without.stdout.contains("no file mutations found"),
        "--no-subagents drops the subagent write: {}",
        without.stdout
    );
}

#[test]
fn files_unknown_session_errors() {
    let h = files_scenario_home();
    let out = h.run(&["files", "--session", "00000000-0000-0000-0000-000000000000"]);
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
fn recover_coverage_counts_and_boundary() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", "--session", SESS, "--file", RFILE, "--coverage"]);
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
    let out = h.run(&["recover", "--session", SESS, "--file", RFILE, "--patches"]);
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
        "--session",
        SESS,
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
        "--session",
        SESS,
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
fn recover_plan_restores_latest_with_provenance() {
    let h = recover_scenario_home();
    let out_path = h.root.join("restored-plan.md");
    let out = h.run(&[
        "recover",
        "--session",
        SESS,
        "--plan",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The ExitPlanMode plan is listed as a candidate with Lnnn / turn / ts provenance.
    assert!(out.stdout.contains("ExitPlanMode"), "{}", out.stdout);
    assert!(
        out.stdout.contains("restored from"),
        "provenance line: {}",
        out.stdout
    );
    assert!(out.stdout.contains("jsonl line"), "{}", out.stdout);
    // The restored file equals the plan text verbatim.
    let restored = std::fs::read_to_string(&out_path).expect("restored plan written");
    assert_eq!(restored, "PLAN café🛠\n- step one\n- step two");
}

#[test]
fn recover_json_every_object_has_line_no_and_local_ts() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        "--session",
        SESS,
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
        "--session",
        SESS,
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
    let out = h.run(&["recover", ".", "--file", RFILE, "--coverage", "--plan"]);
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
        "--session",
        SESS,
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
fn recover_file_required_for_patches_optional_for_plan() {
    let h = recover_scenario_home();
    // --patches without --file → error.
    let no_file = h.run(&["recover", "--session", SESS, "--patches"]);
    assert!(!no_file.success);
    assert!(
        no_file.stderr.contains("--file") && no_file.stderr.contains("required"),
        "file-required bail: {}",
        no_file.stderr
    );
    // --plan without --file → OK.
    let plan_ok = h.run(&["recover", "--session", SESS, "--plan"]);
    assert!(plan_ok.success, "plan needs no --file: {}", plan_ok.stderr);
}

#[test]
fn recover_dry_run_alias_works() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", "--session", SESS, "--file", RFILE, "--dry-run"]);
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
        "--session",
        SESS,
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
    // The four mutually-exclusive mode flags and their semantics are documented.
    for needle in [
        "--patches",
        "--at",
        "--coverage",
        "--plan",
        "segmented unified-diff",
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
        "--session",
        SESS,
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
fn recover_patches_json_segments_and_boundary_objects() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        "--session",
        SESS,
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
        "--session",
        SESS,
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
        "--session",
        SESS,
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
fn recover_plan_json_lists_candidates_with_line_no() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", "--session", SESS, "--plan", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    let cand = objs
        .iter()
        .find(|o| o.get("type").and_then(|v| v.as_str()) == Some("plan_candidate"))
        .expect("a plan_candidate object");
    assert!(
        cand.get("line_no").and_then(|v| v.as_u64()).is_some(),
        "{cand}"
    );
    assert!(
        cand.get("source").and_then(|v| v.as_str()).is_some(),
        "{cand}"
    );
    assert!(cand.get("ts_local").is_some(), "local ts present: {cand}");
    assert!(cand.get("is_latest_in_session").is_some(), "{cand}");
    assert!(objs.last().unwrap().get("summary").is_some());
}

#[test]
fn recover_plan_stdout_without_out_prints_body() {
    // No --out → the plan body is printed inline (small plan, no truncation marker).
    let h = recover_scenario_home();
    let out = h.run(&["recover", "--session", SESS, "--plan"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("--- plan body ---"),
        "inline body header: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("PLAN café🛠"),
        "plan content inline: {}",
        out.stdout
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
        "--session",
        SESS,
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
fn recover_plan_json_out_writes_plan_verbatim() {
    let h = recover_scenario_home();
    let out_path = h.root.join("plan.json.md");
    let out = h.run(&[
        "recover",
        "--session",
        SESS,
        "--plan",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let body = std::fs::read_to_string(&out_path).expect("plan --out artifact");
    assert_eq!(
        body, "PLAN café🛠\n- step one\n- step two",
        "verbatim plan text"
    );
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
        "--session",
        SESS,
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
        "--session",
        SESS,
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
fn recover_real_plan_round_trips_to_disk_byte_exact() {
    let Some((enc, sess, _)) = real_fixture() else {
        eprintln!("SKIP recover_real_plan_round_trips_to_disk_byte_exact: real fixture absent");
        return;
    };
    // The motivating use-case: restore the latest plan and compare it to the on-disk plan
    // file `goofy-finding-kettle.md`. The recovered text must match the disk file exactly.
    let disk_plan = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".claude")
        .join("plans")
        .join("goofy-finding-kettle.md");
    if !disk_plan.is_file() {
        eprintln!("SKIP: on-disk plan file absent");
        return;
    }
    let out_dir = std::env::temp_dir().join(format!("csift-real-plan-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).unwrap();
    let restored = out_dir.join("restored.md");
    let out = run_real(&[
        "recover",
        &enc,
        "--session",
        &sess,
        "--plan",
        "--out",
        restored.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Provenance is cited (Lnnn / turn / ts).
    assert!(
        out.stdout.contains("restored from"),
        "provenance: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("jsonl line"),
        "line number cited: {}",
        out.stdout
    );
    let got = std::fs::read_to_string(&restored).expect("restored plan");
    let want = std::fs::read_to_string(&disk_plan).expect("disk plan");
    std::fs::remove_dir_all(&out_dir).ok();
    assert_eq!(
        got, want,
        "restored plan must be BYTE-EXACT to the on-disk plan"
    );
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
        "--session",
        &sess,
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
        "--session",
        &sess,
        "--file",
        disk_plan.to_str().unwrap(),
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let first = out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap();
    let snap: serde_json::Value = serde_json::from_str(first).unwrap();
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
