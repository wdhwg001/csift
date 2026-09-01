//! End-to-end CLI integration tests that drive the REAL compiled `csift` binary
//! against a synthetic `~/.claude/projects` tree.
//!
//! These cover the surfaces unit tests cannot reach without process-global env
//! mutation: the `run_list` / `run_search` / `run_whoami` / `run_agents`
//! orchestration + file I/O + the text/JSON renderers + `main.rs`'s dispatch and
//! exit-code mapping + `cli::parse_argv` (which reads `std::env::args`).
//!
//! Isolation: every invocation points `$HOME` at a per-test temp dir via
//! `Command::env("HOME", …)` (child-process scope only - no shared-state race with
//! the in-crate threaded unit tests). The binary path comes from cargo's
//! `CARGO_BIN_EXE_csift`, so the build under test is exactly the one cargo produced.

pub(crate) use std::path::{Path, PathBuf};

pub(crate) use std::process::Command;

pub(crate) use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Wrap a session id / agent hex as the `@<id>` POSITIONAL target token (the grammar that
/// replaced the removed `--session` flag). Used throughout these tests to pin one session.
pub(crate) fn at(s: impl AsRef<str>) -> String {
    format!("@{}", s.as_ref())
}

/// A scratch `$HOME` with a `.claude/projects` tree, removed on drop.
pub(crate) struct Home {
    pub(crate) root: PathBuf,
}

impl Home {
    pub(crate) fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("csift-it-{pid}-{n}"));
        std::fs::create_dir_all(root.join(".claude").join("projects")).unwrap();
        Home { root }
    }

    pub(crate) fn projects(&self) -> PathBuf {
        self.root.join(".claude").join("projects")
    }

    /// Write a file under the projects root, creating parent dirs. `rel` is relative
    /// to `~/.claude/projects`.
    pub(crate) fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.projects().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, contents).unwrap();
        p
    }

    /// Write a session-registry fixture row at `<root>/.claude/sessions/<pid>.json`
    /// (the live-truth surfaces read it through the same claude_home resolution).
    pub(crate) fn write_session_registry(&self, pid: u32, json: &str) -> PathBuf {
        let dir = self.root.join(".claude").join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{pid}.json"));
        std::fs::write(&p, json).unwrap();
        p
    }

    /// Write a fixture under `<root>/.claude/<rel>` (tasks and other beside-projects
    /// surfaces resolve through the same claude_home).
    pub(crate) fn write_claude(&self, rel: &str, contents: &str) -> PathBuf {
        let p = self.root.join(".claude").join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, contents).unwrap();
        p
    }

    /// Spawn csift with piped stdio and this `$HOME`, returning the Child - the `wait`
    /// tests block on its readiness stderr line before appending their trigger.
    pub(crate) fn spawn(&self, args: &[&str]) -> std::process::Child {
        let exe = env!("CARGO_BIN_EXE_csift");
        Command::new(exe)
            .args(args)
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CODEX_COMPANION_SESSION_ID")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn csift")
    }

    /// Run the csift binary with this `$HOME`, returning (success, stdout, stderr).
    pub(crate) fn run(&self, args: &[&str]) -> Output {
        self.run_with_env(args, &[])
    }

    pub(crate) fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        self.run_full(args, extra_env, None)
    }

    /// Like `run`, but feeding `input` on stdin (`--sessions-from -`).
    pub(crate) fn run_with_stdin(&self, args: &[&str], input: &str) -> Output {
        use std::io::Write as _;
        let exe = env!("CARGO_BIN_EXE_csift");
        let mut child = Command::new(exe)
            .args(args)
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root) // the Windows home var - same relocation there
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CODEX_COMPANION_SESSION_ID")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn csift");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().expect("wait csift");
        Output {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    pub(crate) fn run_full(
        &self,
        args: &[&str],
        extra_env: &[(&str, &str)],
        cwd: Option<&Path>,
    ) -> Output {
        let exe = env!("CARGO_BIN_EXE_csift");
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root) // the Windows home var - same relocation there
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

pub(crate) struct Output {
    pub(crate) success: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) const ENC: &str = "-Users-testuser-Projects-foo";

/// A filesystem path as a JSON-string-safe fragment (Windows backslashes escaped).
pub(crate) fn jpath(s: &str) -> String {
    s.replace('\\', "\\\\")
}

pub(crate) const SESS: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

/// envelope v2: parse a JSON stream and keep the rows of one `kind`.
pub(crate) fn json_rows(stdout: &str, kind: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("valid json line"))
        .filter(|v| v["kind"] == kind)
        .collect()
}

/// envelope v2: the closing `{"kind":"summary",…}` line (always the last line).
pub(crate) fn json_summary(stdout: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(
        stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .next_back()
            .expect("a summary line"),
    )
    .expect("valid json line");
    assert_eq!(v["kind"], "summary", "last line must be the summary: {v}");
    v
}

/// A full session jsonl with identity fields, a genuine-user turn, an assistant
/// reply, a tool round-trip, an isMeta pseudo-turn, and a malformed line (to drive
/// the skipped-line accounting). Returns the home with the tree populated.
pub(crate) fn populated_home() -> Home {
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

// A fixture with ONE subagent whose bare hex is a realistic >=12-char id.
pub(crate) fn show_subagent_home() -> (Home, &'static str, &'static str) {
    let enc = "-Users-testuser-Projects-linehex";
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
            r#"{"type":"user","isSidechain":true,"agentId":"aaa111bbb222ccc33","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub: do the thing about carry"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub done"}]}}"#, "\n",
        ),
    );
    (h, sess, hex)
}

/// Three top-level sessions for the collision tests: two ids share their first 8 chars, the
/// third does not. Each carries one searchable round-trip seeded with `SEEDWORD`.
pub(crate) fn header_collision_scenario(h: &Home) -> (&'static str, &'static str, &'static str) {
    let enc = "-Users-dev-example-project";
    let c1 = "aaaabbbb-1111-4000-8000-000000000001";
    let c2 = "aaaabbbb-2222-4000-8000-000000000002";
    let solo = "ccccdddd-3333-4000-8000-000000000003";
    // Distinct per-session hours so the chronological order is unambiguous:
    // COLLIDEONE (05h) < COLLIDETWO (06h) < SOLOWORD (07h).
    for (i, (sess, word)) in [(c1, "COLLIDEONE"), (c2, "COLLIDETWO"), (solo, "SOLOWORD")]
        .into_iter()
        .enumerate()
    {
        let hour = 5 + i;
        h.write(
            &format!("{enc}/{sess}.jsonl"),
            &format!(
                "{}\n{}\n",
                format_args!(
                    r#"{{"type":"user","timestamp":"2026-06-07T0{hour}:00:00.000Z","message":{{"role":"user","content":"SEEDWORD {word}"}}}}"#
                ),
                format_args!(
                    r#"{{"type":"assistant","timestamp":"2026-06-07T0{hour}:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"ack {word}"}}]}}}}"#
                ),
            ),
        );
    }
    (c1, c2, solo)
}

/// A session owning two subagents whose 16-hex ids share their first 8 chars, plus one
/// distinct-prefix subagent - the agent-side prefix-resolution fixtures.
pub(crate) fn agent_prefix_scenario(h: &Home) -> (&'static str, &'static str, &'static str) {
    let enc = "-Users-dev-example-project";
    let sess = "11112222-3333-4000-8000-000000000004";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"lead"}}"#, "\n",
        ),
    );
    let alpha = "e1e2a3b4c5d6f708";
    let beta = "e1e2a3b4ffff0001";
    let gamma = "f0a1b2c3d4e5f607";
    for (agent, word) in [
        (alpha, "AGENTALPHA"),
        (beta, "AGENTBETA"),
        (gamma, "AGENTGAMMA"),
    ] {
        h.write(
            &format!("{enc}/{sess}/subagents/agent-{agent}.jsonl"),
            &format!(
                "{}\n",
                format_args!(
                    r#"{{"type":"user","isSidechain":true,"agentId":"{agent}","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"user","content":"seed {word}"}}}}"#
                ),
            ),
        );
    }
    (alpha, beta, gamma)
}

/// A session whose PARENT transcript carries the spawn linkage: an `Agent` tool_use
/// (`toolu_x`, == the built-in meta's `toolUseId`) at 04:59:58 - the TRUE trigger,
/// ~2s before the child-head ts - whose paired SYNC tool_result is the built-in's
/// returned message; a built-in subagent that EDITS a file (for `--with-files`); a
/// workflow agent whose journal carries a `result` payload; and a top-level
/// `workflows/wf_topo.json` manifest (the WorkflowRun source).
pub(crate) fn topology_home() -> Home {
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

/// A session whose transcript performs the acid-test scenario: two `/tmp/*.md` Writes
/// (creates), three `…/gaps/*.md` Edits (updates), and a Bash `rm`, across two turns,
/// each structured tool_use paired with its create/update carrier. Returns the home.
pub(crate) fn files_scenario_home() -> Home {
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

/// Fixture with three distinct mutated full paths (a `.rs` under `src/`, a `.md` under
/// `docs/`, a top-level `.txt`) plus an Edit-before-Read boundary on the `.rs` file - so the
/// `--regex`/`--glob` full-path filters can be exercised against a varied set, including the
/// boundary section.
pub(crate) fn path_filter_scenario(h: &Home) {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/testuser/Projects/foo/src/lib.rs","content":"x\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/Users/testuser/Projects/foo/src/lib.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w2","name":"Write","input":{"file_path":"/Users/testuser/Projects/foo/docs/readme.md","content":"y\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:02.500Z","toolUseResult":{"type":"create","filePath":"/Users/testuser/Projects/foo/docs/readme.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w2","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w3","name":"Write","input":{"file_path":"/Users/testuser/Projects/foo/notes.txt","content":"z\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c3","timestamp":"2026-06-07T05:00:03.500Z","toolUseResult":{"type":"create","filePath":"/Users/testuser/Projects/foo/notes.txt"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w3","content":"ok"}]}}"#, "\n",
            // Edit-before-Read boundary on the .rs file.
            r#"{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/Users/testuser/Projects/foo/src/lib.rs","old_string":"x","new_string":"X"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
        ),
    );
}

/// A session whose TOP-LEVEL turn writes `/parent/p.md` and whose SUBAGENT writes
/// `/sub/s.md` - the fixture for span-scope tests: the default spans both files, while
/// `--no-subagents` keeps only the parent file.
pub(crate) fn subagents_only_scenario(h: &Home) {
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

/// Minimal JSON-string encoder for embedding a Bash command verbatim into a fixture
/// (escapes `"` and `\`). Sufficient for the simple commands used above.
pub(crate) fn serde_json_string(s: &str) -> String {
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

pub(crate) fn _assert_path_exists(p: &Path) {
    assert!(p.exists());
}

/// Absolute path of the file whose history the recover scenarios reconstruct.
pub(crate) const RFILE: &str = "/Users/testuser/Projects/foo/app.py";

/// A session that builds `app.py` across a realistic life-cycle, mirroring the shapes
/// verified against real `~/.claude/projects` data:
///   turn 0  full Read of app.py (4 lines) → an Edit (line 2 rewritten, structuredPatch)
///   turn 1  a `modified since read` integrity error (a HARD boundary), then a fresh full
///           Read (the post-drift state) → another Edit
///   turn 2  an ExitPlanMode plan + a plan-file Write (for --plan)
/// Plus a malformed line (skipped-line accounting) and a history-snapshot marker.
pub(crate) fn recover_scenario_home() -> Home {
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
            // The error carrier (no inline path) - attributed to app.py via the tool_use_id join.
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

/// The real session fixture + its encoded project dir, or `None` when absent.
pub(crate) fn real_fixture() -> Option<(String, String, PathBuf)> {
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
pub(crate) fn run_real(args: &[&str]) -> Output {
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

/// A session that reads a file then makes an UN-ANCHORABLE edit (a structured patch deep
/// in an unknown gap) → `--coverage` reports a coverage hole + a windowed read.
pub(crate) fn recover_hole_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"peek"}}"#, "\n",
            // A windowed read of lines 1-2 of a 100-line file (lines 3-100 stay gaps).
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/big.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":100}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            // An edit at line 60 - deep in the gap, no adjacent known line → un-anchorable.
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"filePath":"/p/big.rs","oldString":"zzz","newString":"Z","structuredPatch":[{"oldStart":60,"oldLines":1,"newStart":60,"newLines":1,"lines":["-zzz","+Z"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed0","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}
