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

#[test]
fn stats_aggregates_records_turns_tools_and_tokens() {
    let h = Home::new();
    let enc = "-Users-testuser-Projects-statsy";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":100,"cache_creation_input_tokens":5},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:09.000Z","message":{"role":"assistant","model":"claude-opus-4-8","usage":{"input_tokens":1,"output_tokens":2},"content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &format!("@{sess}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("turns 2"), "{}", out.stdout);
    assert!(out.stdout.contains("Bash×1"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("claude-opus-4-8: in 11 · out 22 · cache-read 100 · cache-write 5"),
        "token sums: {}",
        out.stdout
    );

    let j = h.run(&["stats", &format!("@{sess}"), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let row = json_rows(&j.stdout, "session").remove(0);
    assert_eq!(row["turns"], 2);
    assert_eq!(row["user_records"], 3); // 2 genuine + 1 tool_result carrier
    assert_eq!(row["assistant_records"], 2);
    assert_eq!(row["tools"]["Bash"], 1);
    assert_eq!(row["tokens"]["claude-opus-4-8"]["output"], 22);
    let sum = json_summary(&j.stdout);
    assert_eq!(sum["turns"], 2);

    // --since bounds the counted records (only the second turn's records remain).
    let win = h.run(&[
        "stats",
        &format!("@{sess}"),
        "--since",
        "2026-06-07T05:30:00Z",
        "--format",
        "json",
    ]);
    let row = json_rows(&win.stdout, "session").remove(0);
    assert_eq!(row["turns"], 1, "window admits only the later turn");
    assert_eq!(row["tokens"]["claude-opus-4-8"]["output"], 2);
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

#[test]
fn stats_aggregates_are_exact() {
    // Mutation pin on the stats aggregation core: token sums per model, tool CALL
    // counts, the span label, and the JSON id trio must carry REAL values - an emptied
    // merge map or a += degraded to *= zeroed them with no test on the numbers.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "8899aabb-ccdd-4000-8000-00000000000d";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","model":"relay-model-x","usage":{"input_tokens":111,"output_tokens":44,"cache_read_input_tokens":5,"cache_creation_input_tokens":4},"content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/a.md","content":"x"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:40.000Z","toolUseResult":{"type":"create","filePath":"/p/a.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","model":"relay-model-x","usage":{"input_tokens":222,"output_tokens":55},"content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    // A subagent transcript so the MERGED (multi-transcript) rollup path renders too -
    // the scoped --iterate verification showed merged_tools/merged_tokens survived a
    // single-transcript fixture (the merge path was never invoked).
    h.write(
        &format!("{enc}/{sess}/subagents/agent-aggr222.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aggr222","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"sub work"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"relay-model-x","usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":7,"cache_creation_input_tokens":3},"content":[{"type":"tool_use","id":"sw1","name":"Write","input":{"file_path":"/s/b.md","content":"y"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &format!("@{sess}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("relay-model-x"),
        "model row present: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("333") && out.stdout.contains("99"),
        "token sums 111+222=333 in / 44+55=99 out: {}",
        out.stdout
    );
    assert!(out.stdout.contains("Write"), "tool tally: {}", out.stdout);
    assert!(
        out.stdout.contains("1m05s"),
        "span label for the 65s session: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("1333") && out.stdout.contains("599"),
        "MERGED token sums 333+1000 / 99+500 across the two transcripts: {}",
        out.stdout
    );
    let js = h.run(&["stats", &format!("@{sess}"), "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    assert!(
        js.stdout
            .contains(&format!(r#""parent_session_id":"{sess}""#)),
        "the JSON id trio is populated: {}",
        js.stdout
    );
    assert!(
        js.stdout.contains(r#""Write":2"#),
        "MERGED tool calls across the two transcripts: {}",
        js.stdout
    );
    // The asserted merged values are UNIQUE to the merged object (5+7 / 4+3 - no single
    // row carries them): a per-row field must never be able to satisfy the merge pin.
    assert!(
        js.stdout.contains(r#""cache_read":12"#) && js.stdout.contains(r#""cache_creation":7"#),
        "the cache accumulators merge too: {}",
        js.stdout
    );
    assert!(
        js.stdout.contains(r#""is_subagent":true"#),
        "the subagent row's id-domain discriminator is real, never defaulted: {}",
        js.stdout
    );
}

#[test]
fn stats_line_type_census_counts_every_physical_line() {
    // Every physical line lands in the `types` census by its top-level `type`; a
    // {…}-framed line with an INVALID interior is counted malformed (the probe upgrades
    // the O(1) shape check to an exact whole-file census on stats).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the harbor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"att1","attachment":{"type":"edited_text_file","filePath":"/p/x.rs","snippet":"fn y() {}"}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1"}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-06-07T05:00:02.000Z","compactMetadata":{"trigger":"auto","preTokens":10,"postTokens":2,"durationMs":5}}"#, "\n",
            r#"{"type":"attachment","broken":}"#, "\n",
        ),
    );
    let out = h.run(&["stats", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("types"),
        "census line present:\n{}",
        out.stdout
    );
    for key in [
        "user×1",
        "assistant×1",
        "attachment×1",
        "file-history-snapshot×1",
        "system×1",
    ] {
        assert!(out.stdout.contains(key), "missing {key}:\n{}", out.stdout);
    }
    assert!(
        out.stdout.contains("1 malformed line(s) skipped"),
        "framed-invalid interior on a non-candidate line is COUNTED:\n{}",
        out.stdout
    );

    let outj = h.run(&["stats", at(SESS).as_str(), "--format", "json"]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let session = rows
        .iter()
        .find(|r| r["kind"] == "session")
        .expect("session row");
    assert_eq!(session["lines"], 6, "physical line count");
    assert_eq!(session["line_types"]["user"], 1);
    assert_eq!(session["line_types"]["attachment"], 1);
    assert_eq!(session["line_types"]["file-history-snapshot"], 1);
    assert_eq!(session["line_types"]["system"], 1);
    assert_eq!(session["skipped_lines"], 1);
    let summary = rows.last().unwrap();
    assert_eq!(
        summary["line_types"]["assistant"], 1,
        "summary merges the census: {}",
        outj.stdout
    );
}

const SESS2: &str = "b2c3d4e5-f6a7-4b8c-9d0e-1f2a3b4c5d6e";

#[test]
fn stats_total_block_merges_census_and_books_malformed_notes_exactly() {
    // Session 1 carries one tool call and one shape-malformed line; session 2 is clean
    // with OUT-OF-ORDER timestamps. Pins: the TOTAL block appears only for >1 session
    // and merges the types census + tools; the malformed note prints per-session AND as
    // the scope total (exactly twice, never a "(0 malformed" line); first/last span
    // derives from timestamp COMPARISON, not file order; the duration label is exact.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#, "\n",
            "not json here\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS2}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"mid"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"earliest, recorded late"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u2","timestamp":"2026-06-07T06:30:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"latest"}]}}"#, "\n",
        ),
    );
    let both = h.run(&["stats", ENC]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert!(both.stdout.contains("TOTAL"), "{}", both.stdout);
    let total_tail = &both.stdout[both.stdout.find("TOTAL").unwrap()..];
    assert!(
        total_tail.contains("types") && total_tail.contains("user×3"),
        "TOTAL merges the census:\n{}",
        both.stdout
    );
    assert!(
        total_tail.contains("tools") && total_tail.contains("Read×1"),
        "TOTAL merges tools:\n{}",
        both.stdout
    );
    assert_eq!(
        both.stdout.matches("malformed line(s) skipped").count(),
        2,
        "per-session note + scope total, exactly twice:\n{}",
        both.stdout
    );
    assert!(
        !both.stdout.contains("(0 malformed"),
        "a clean session never prints a zero note:\n{}",
        both.stdout
    );

    // Single session: no TOTAL block, and a clean file prints no malformed note at all.
    let one = h.run(&["stats", at(SESS2).as_str()]);
    assert!(!one.stdout.contains("TOTAL"), "{}", one.stdout);
    assert!(!one.stdout.contains("malformed"), "{}", one.stdout);
    // Span is comparison-derived: earliest 05:00Z (recorded second), latest 06:30Z. The
    // JSON pair is raw UTC (tz-independent; text renders local).
    let onej = h.run(&["stats", at(SESS2).as_str(), "--format", "json"]);
    let row: serde_json::Value = onej
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "session")
        .expect("session row");
    assert_eq!(
        row["first_utc"], "2026-06-07T05:00:00.000Z",
        "first from comparison, not file order: {}",
        onej.stdout
    );
    assert_eq!(
        row["last_utc"], "2026-06-07T06:30:00.000Z",
        "{}",
        onej.stdout
    );
    assert!(
        one.stdout.contains("(1h30m"),
        "exact duration:\n{}",
        one.stdout
    );
}
