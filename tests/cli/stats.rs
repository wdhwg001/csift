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
}

#[test]
fn stats_split_compactions_and_physical_lines_exact() {
    // Mutation pins: top = rows - sub (not +), compactions accumulate (2 exactly), and the
    // physical line count adds the torn-final-fragment ONLY when the file lacks a trailing
    // newline (3 records, no trailing newline => lines == 3).
    let enc = "-Users-testuser-Projects-statsplit";
    let sess = "dddd0000-aaaa-4bbb-8ccc-000000000001";
    let h = Home::new();
    let body = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        "\n",
        r#"{"type":"user","uuid":"c1","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"summary one"}}"#,
        "\n",
        r#"{"type":"user","uuid":"c2","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"summary two"}}"#
    );
    h.write(&format!("{enc}/{sess}.jsonl"), body); // NO trailing newline
    h.write(
        &format!("{enc}/{sess}/subagents/agent-e0e1e2e3e4e5e6e7.jsonl"),
        "{\"type\":\"user\",\"isSidechain\":true,\"timestamp\":\"2026-06-07T05:03:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"sub\"}}\n",
    );
    let o = h.run(&["stats", enc, "--format", "json"]);
    assert!(o.success, "stderr: {}", o.stderr);
    assert!(
        o.stdout.contains("\"compactions\":2"),
        "compactions == 2:\n{}",
        o.stdout
    );
    assert!(
        o.stdout.contains("\"lines\":3"),
        "torn-fragment line count == 3:\n{}",
        o.stdout
    );
    assert!(
        o.stdout.contains("\"lines\":1"),
        "trailing-newline subagent file counts exactly 1 line:\n{}",
        o.stdout
    );
    let t = h.run(&["stats", enc]);
    assert!(
        t.stdout.contains("1 top-level") && t.stdout.contains("1 subagent"),
        "exact top/sub split:\n{}",
        t.stdout
    );
}
