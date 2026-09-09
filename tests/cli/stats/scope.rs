//! `stats` scope and windowing: the span switches, the row cap, and the turn window.

use crate::harness::*;

#[test]
fn stats_spans_subagents_by_default_and_restricts() {
    // Mutation pin on the span contract (§ subcommand spanning default): `stats` spans the
    // session's subagent transcripts by default; `--no-subagents` restricts to the top level.
    let h = Home::new();
    subagents_only_scenario(&h);
    let span = h.run(&["stats", at(SESS).as_str()]);
    assert!(span.success, "stderr: {}", span.stderr);
    assert!(
        span.stdout.contains("sub111"),
        "stats spans subagents by default: {}",
        span.stdout
    );
    let top = h.run(&["stats", at(SESS).as_str(), "--no-subagents"]);
    assert!(top.success, "stderr: {}", top.stderr);
    assert!(
        !top.stdout.contains("sub111"),
        "--no-subagents restricts stats to the top level: {}",
        top.stdout
    );
}

#[test]
fn stats_cap_arithmetic_and_uncapped_zero() {
    // Mutation pins: dropped = len - n (NOT len / n: 4 sessions, cap 1 => exactly 3), and
    // `--max-count 0` stays UNCAPPED (the n > 0 filter, not n >= 0).
    let h = Home::new();
    for i in 0..4u8 {
        h.write(
            &format!("-Users-testuser-Projects-statcap/cccc000{i}-aaaa-4bbb-8ccc-00000000000{i}.jsonl"),
            &format!("{{\"type\":\"user\",\"uuid\":\"u0\",\"timestamp\":\"2026-06-07T0{i}:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"work {i}\"}}}}\n"),
        );
    }
    let capped = h.run(&[
        "stats",
        "-Users-testuser-Projects-statcap",
        "--max-count",
        "1",
        "--format",
        "json",
    ]);
    assert!(capped.success, "stderr: {}", capped.stderr);
    assert_eq!(
        capped.stdout.matches("\"lines\":").count(),
        1,
        "one row kept:\n{}",
        capped.stdout
    );
    assert!(
        capped.stdout.contains(":3") && capped.stdout.contains("dropped"),
        "exactly 3 dropped disclosed:\n{}",
        capped.stdout
    );
    let uncapped = h.run(&[
        "stats",
        "-Users-testuser-Projects-statcap",
        "--max-count",
        "0",
        "--format",
        "json",
    ]);
    assert_eq!(
        uncapped.stdout.matches("\"lines\":").count(),
        4,
        "uncapped shows all four:\n{}",
        uncapped.stdout
    );
    // The TEXT surface carries the same disclosure, and carries it only when a row was
    // actually dropped: a "0 more session(s) not shown" footer on every run would be a
    // standing lie about a cap that did not fire.
    let capped_text = h.run(&[
        "stats",
        "-Users-testuser-Projects-statcap",
        "--max-count",
        "1",
    ]);
    assert!(
        capped_text.stdout.contains("+3 more session(s) not shown"),
        "the text footer names the drop:\n{}",
        capped_text.stdout
    );
    let uncapped_text = h.run(&["stats", "-Users-testuser-Projects-statcap"]);
    assert!(
        !uncapped_text.stdout.contains("not shown"),
        "and says nothing when nothing dropped:\n{}",
        uncapped_text.stdout
    );
}

#[test]
fn stats_turn_range_windows_the_aggregates() {
    let h = Home::new();
    let enc = "-Users-testuser-Projects-statturn";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            // Turn 0: one Read tool call.
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t0","name":"Read","input":{}}]}}"#, "\n",
            // Turn 1: one Edit tool call.
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"second ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{}}]}}"#, "\n",
        ),
    );
    // Bare-N shorthand: turn 1 only - Edit counted, Read not, turns == 1.
    let out = h.run(&["stats", enc, "--turn", "1", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["turns"], 1, "one turn in window: {}", out.stdout);
    assert!(
        rows[0]["tools"].get("Edit").is_some() && rows[0]["tools"].get("Read").is_none(),
        "only turn 1's tool calls count: {}",
        out.stdout
    );
}
