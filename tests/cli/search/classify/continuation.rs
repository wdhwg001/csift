//! The CLASSIFIER-STOP shape end to end: the three records the harness writes when a
//! safety classifier stops a response mid-turn, and the leaf the continuation carries.
//!
//! One event, three records in this order: the errored result of every call that had not
//! returned, a `type:"system"` notice, and the isMeta user record telling the model how to
//! carry on. Only the last is new to the taxonomy - the notice is `harness.system.notice`
//! by its subtype and the refused assistant turn classifies on its own blocks.

use crate::harness::*;

/// The session this module's fixture writes, and the project directory under it.
const SESS: &str = "11111111-2222-4333-8444-555555555501";
const PROJ: &str = "-Users-dev-stopped";

/// The continuation's whole text, as the harness writes it: the plain classifier-stop
/// form, em dash included.
const STOPPED: &str = "Your response above was stopped by a safety classifier \u{2014} this is not a tool or API error. The rest of it was withheld, and tool calls in it that had not finished did not run. Do not produce that content again, even reworded.";

/// The whole event on disk. The refusal record carries a thinking block and nothing else,
/// which is what the API returns when the classifier stops the response before any text.
fn home() -> Home {
    let h = Home::new();
    let injected = serde_json::json!({
        "type": "user",
        "uuid": "c0",
        "parentUuid": "b0",
        "isMeta": true,
        "turnCompanion": true,
        "promptId": "p1",
        "timestamp": "2026-06-07T05:00:40.000Z",
        "message": {"role": "user", "content": STOPPED},
    })
    .to_string();
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"11111111-2222-4333-8444-555555555501","cwd":"/Users/dev/stopped","version":"2.1.278","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"summarise the zzstop report"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/Users/dev/stopped/report.md"}}]}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"a0","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"withheld"},"content":[{"type":"thinking","thinking":"weighing the request","signature":"c2ln"}]}}"#,
        r#"{"type":"user","uuid":"r0","parentUuid":"a1","timestamp":"2026-06-07T05:00:30.000Z","toolDenialKind":"interrupted","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"Interrupted: the response that made this tool call was stopped by a safety classifier while the call was running; it may have partially or fully completed."}]}}"#,
        r#"{"type":"system","uuid":"b0","parentUuid":"r0","subtype":"informational","level":"notice","isMeta":false,"timestamp":"2026-06-07T05:00:35.000Z","content":"the model's safeguards stopped the response above · continuing once with that noted"}"#,
        &injected,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"c0","timestamp":"2026-06-07T05:00:50.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Understood, carrying on."}]}}"#,
    ];
    h.write(&format!("{PROJ}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

#[test]
fn the_classifier_stop_continuation_is_enforce_retry_on_every_selector_that_names_it() {
    let h = home();
    // The LEAF, and the two coarse faces above it. The model RECEIVED this message, so a
    // bare `-t harness` and a bare `-t harness.enforce` - which show only the visible
    // face of their role and category - both have to reach it.
    for sel in ["harness.enforce.retry", "harness.enforce", "harness"] {
        let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents", "-t", sel]);
        assert!(out.success, "{sel}: {}", out.stderr);
        assert!(
            out.stdout.contains("harness.enforce.retry"),
            "-t {sel} reaches the continuation: {}",
            out.stdout
        );
        assert!(
            out.stdout
                .contains("Your response above was stopped by a safety classifier"),
            "-t {sel} renders its text: {}",
            out.stdout
        );
    }
}

#[test]
fn a_pattern_scan_finds_the_continuation_and_the_census_counts_it_once() {
    let h = home();
    // A FLAGLESS pattern scan: the record rides a `type:"user"` line, so no gate stands
    // between a reader and its words.
    let out = h.run(&[
        "search",
        "stopped by a safety classifier",
        at(SESS).as_str(),
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.enforce.retry"),
        "a pattern scan reaches it: {}",
        out.stdout
    );
    // ONE label, ONE record: the continuation carries a single leaf, so the census counts
    // it under `harness.enforce.retry` and nowhere else.
    let c = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--count-by",
        "label",
    ]);
    assert!(c.success, "stderr: {}", c.stderr);
    let row = c
        .stdout
        .lines()
        .find(|l| l.contains("harness.enforce.retry"))
        .unwrap_or_else(|| panic!("no enforce.retry census row: {}", c.stdout));
    assert!(
        row.split_whitespace().any(|t| t == "1"),
        "counted exactly once: {row}"
    );
}

#[test]
fn the_notice_and_the_refused_turn_keep_their_own_leaves() {
    let h = home();
    // The notice is decided by its SUBTYPE, not by the event around it, and it is gated:
    // a selector has to name it.
    let n = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "harness.system.notice",
    ]);
    assert!(n.success, "stderr: {}", n.stderr);
    assert!(
        n.stdout.contains("harness.system.notice"),
        "the notice keeps its subtype leaf: {}",
        n.stdout
    );
    // The refused assistant turn classifies on its own blocks - a thinking block here -
    // and the refusal itself is a FACT on that unit rather than a leaf.
    let t = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "agent.thinking.summary",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains("agent.thinking.summary"),
        "the refused turn keeps its block leaf: {}",
        t.stdout
    );
}

#[test]
fn show_renders_the_continuation_at_its_own_line() {
    let h = home();
    // The 1:1 law from the other side: the record now carries a leaf, so an address
    // renders it under that leaf rather than as an unmodeled isMeta unit.
    let out = h.run(&["show", at(SESS).as_str(), "--line", "6"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.enforce.retry"),
        "the address renders the leaf: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("unmodeled"),
        "no longer the residue: {}",
        out.stdout
    );
}
