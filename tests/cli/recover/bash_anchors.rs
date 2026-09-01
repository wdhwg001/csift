//! Bash content anchors end to end: heredoc-only history restores, appends land on
//! complete buffers, windows merge, and the gates hold through the CLI.

use crate::harness::*;

const BSESS: &str = "bbbbcccc-1111-4222-8333-444455556666";

#[test]
fn recover_restores_a_file_written_only_by_bash_heredoc() {
    // The shell-first session shape: no structured Write ever ran - the whole
    // history is one quoted-delimiter heredoc. Before v0.9.4 this was "no
    // recoverable history"; now the body is a first-class anchor.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{BSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"write the config"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cat > conf/app.toml <<'EOF'\nname = \"harbor\"\nport = 8080\nEOF"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(BSESS).as_str(),
        "--file",
        "/work/proj/conf/app.toml",
        "--at",
        "@line:99999999",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("name = \"harbor\"") && out.stdout.contains("port = 8080"),
        "heredoc body recovered:\n{}",
        out.stdout
    );
    let cov = h.run(&[
        "recover",
        at(BSESS).as_str(),
        "--file",
        "/work/proj/conf/app.toml",
        "--coverage",
    ]);
    assert!(
        cov.stdout.contains("bash-write-anchor"),
        "coverage discloses the anchor class:\n{}",
        cov.stdout
    );
}

#[test]
fn bash_append_lands_after_a_structured_write() {
    // Structured Write (complete buffer) then `echo >> `: the appended line is
    // placed, no boundary, and the coverage counts disclose the anchor class.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{BSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"log it"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/w/run.log","content":"started\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/w/run.log","content":"started\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /w/run.log"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/w","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"echo finished >> run.log"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r2","timestamp":"2026-06-07T05:00:04.000Z","toolUseResult":{"stdout":"","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(BSESS).as_str(),
        "--file",
        "/w/run.log",
        "--at",
        "@line:99999999",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let started = out.stdout.find("started");
    let finished = out.stdout.find("finished");
    assert!(
        started.is_some() && finished.is_some() && started < finished,
        "append placed after the write:\n{}",
        out.stdout
    );
    let cov = h.run(&[
        "recover",
        at(BSESS).as_str(),
        "--file",
        "/w/run.log",
        "--coverage",
    ]);
    assert!(
        cov.stdout.contains("bash-write-anchor"),
        "coverage discloses the anchor count:\n{}",
        cov.stdout
    );
}

#[test]
fn ssh_heredoc_and_variable_targets_stay_boundaries() {
    // An ssh-fed heredoc writes REMOTELY; a $VAR target is unknowable. Neither may
    // anchor - the heuristic disclosure lanes keep them.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{BSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"provision"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"ssh relay tee /w/notes.md <<'EOF'\nremote body\nEOF"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"remote body\n","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"remote body\n"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(BSESS).as_str(),
        "--file",
        "/w/notes.md",
        "--coverage",
    ]);
    assert!(
        !out.stdout.contains("bash-write-anchor") && !out.stdout.contains("bash-heredoc"),
        "an ssh heredoc must never anchor local content:\n{}",
        out.stdout
    );
}
