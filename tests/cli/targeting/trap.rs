//! The @trap self-resolution marker: strict grammar, flush timing, shell tools.

use crate::harness::*;

#[test]
fn trap_resolves_a_powershell_shell_command() {
    // On Windows without Git-for-Windows bash, CC's shell tool is a SEPARATE tool named
    // `PowerShell` (same `input.command` field - extracted from the 2.1.228 binary). A
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
    // 1) STRICT marker grammar - rejected at the source, BEFORE any env / file lookup. This is
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
    // The exact loophole the design calls out - an acronym + zeros - is rejected.
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
