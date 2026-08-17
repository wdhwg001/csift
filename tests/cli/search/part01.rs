use crate::harness::*;

#[test]
fn list_encoded_token_after_flag_ordering() {
    let h = populated_home();
    // Exercises normalize_argv: a leading-`-` encoded token THEN --format json.
    let out = h.run(&["list", ENC, "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));
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
fn search_text_returns_round_trip_exchange() {
    let h = populated_home();
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}·t", &SESS[..8])),
        "the id-prefix header token:\n{}",
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
fn search_count_by_label_censuses_the_scope() {
    let h = populated_home();
    // Empty pattern + --count-by label = "what record-types are here?" — the exploration
    // on-ramp so an empty `-t <leaf>` result is never mistaken for a typo.
    let out = h.run(&["search", "", "--no-subagents", "--count-by", "label"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for leaf in [
        "user.message",
        "agent.thinking",
        "agent.message",
        "agent.tool.use",
        "agent.tool.result",
    ] {
        assert!(
            out.stdout.contains(leaf),
            "census missing {leaf}:\n{}",
            out.stdout
        );
    }
    // JSON: census rows (axis/key/records) + a summary carrying the totals.
    let out = h.run(&[
        "search",
        "",
        "--no-subagents",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let rows = json_rows(&out.stdout, "census");
    assert!(!rows.is_empty(), "no census rows:\n{}", out.stdout);
    assert!(
        rows.iter()
            .all(|r| r["axis"] == "label" && r["key"].is_string() && r["records"].is_u64()),
        "census row shape:\n{}",
        out.stdout
    );
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["axis"], "label", "summary: {summary}");
    assert!(
        summary["matched_records"].as_u64().unwrap() >= 5,
        "summary: {summary}"
    );
    assert!(
        summary["distinct_keys"].as_u64().unwrap() >= 5,
        "summary: {summary}"
    );
    assert_eq!(
        summary["excluded_records"], 0,
        "label axis excludes nothing"
    );
}

#[test]
fn search_census_counts_records_not_sections_and_pairing_rides_comm_views() {
    // Two census laws in one fixture. (1) A record that emits SEVERAL section hits (here an
    // assistant record carrying a text block AND a tool_use block) is ONE record to every
    // census — a leaf's tally must equal what `-t <leaf>` surfaces, never drift above it by
    // the multi-section overlap. (2) Pairing is a property of the underlying tool block, so
    // it rides the communication view too: a SendMessage with no tool_result is `pending`
    // even though its richest view is agent.communication.sent — the "anything stuck?"
    // census needs no `-t` at all.
    let enc = "-Users-testuser-Projects-census";
    let sess = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-26T09:00:00.000Z","message":{"role":"user","content":"go"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-26T09:01:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"scanning now"},{"type":"tool_use","id":"toolu_c1","name":"Bash","input":{"command":"ls"}}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"u2","timestamp":"2026-06-26T09:02:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_c1","content":"ok"}]}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-26T09:03:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_c2","name":"SendMessage","input":{"to":"peer","message":{"type":"message","content":"ping"}}}]}}"#,
            "\n",
        ),
    );
    let target = format!("@{sess}");
    let census = |args: &[&str]| -> (Vec<serde_json::Value>, serde_json::Value) {
        let mut full = vec!["search", "", target.as_str()];
        full.extend_from_slice(args);
        full.extend_from_slice(&["--format", "json"]);
        let out = h.run(&full);
        assert!(out.success, "stderr: {}", out.stderr);
        (json_rows(&out.stdout, "census"), json_summary(&out.stdout))
    };

    // Label census: the text+tool_use record counts ONCE under each of its leaves.
    let (rows, summary) = census(&["--count-by", "label"]);
    let count = |key: &str| -> u64 {
        rows.iter()
            .find(|r| r["key"] == key)
            .map_or(0, |r| r["records"].as_u64().unwrap())
    };
    assert_eq!(count("agent.message"), 1, "rows: {rows:?}");
    // The text+tool record AND the SendMessage record both carry agent.tool.use.
    assert_eq!(count("agent.tool.use"), 2, "rows: {rows:?}");
    assert_eq!(count("agent.communication.sent"), 1, "rows: {rows:?}");
    // 4 RECORDS in scope (opener, text+tool, result, send) — the multi-section record
    // must not inflate the total.
    assert_eq!(summary["matched_records"], 4, "summary: {summary}");

    // Pairing census, NO -t: the returned Bash is paired, the unreturned SendMessage is
    // pending (via its comm view), the genuine opener is outside the axis and reported.
    let (rows, summary) = census(&["--count-by", "pairing"]);
    let count = |key: &str| -> u64 {
        rows.iter()
            .find(|r| r["key"] == key)
            .map_or(0, |r| r["records"].as_u64().unwrap())
    };
    assert_eq!(count("paired"), 2, "use+result both paired: {rows:?}");
    assert_eq!(count("pending"), 1, "the frozen SendMessage: {rows:?}");
    assert_eq!(summary["excluded_records"], 1, "the opener: {summary}");

    // The comm selector agrees — the send is IN the pairing domain now, not excluded.
    let (rows, summary) = census(&["-t", "agent.communication.sent", "--count-by", "pairing"]);
    assert_eq!(rows.len(), 1, "rows: {rows:?}");
    assert_eq!(rows[0]["key"], "pending");
    assert_eq!(summary["excluded_records"], 0, "summary: {summary}");
}

#[test]
fn search_count_by_other_axes() {
    let h = populated_home();

    // `tool`: per tool name; non-tool records are excluded AND the exclusion is reported.
    let out = h.run(&[
        "search",
        "",
        "--no-subagents",
        "--count-by",
        "tool",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "census");
    assert!(!rows.is_empty(), "tool census rows:\n{}", out.stdout);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["axis"], "tool");
    assert!(
        summary["excluded_records"].as_u64().unwrap() > 0,
        "non-tool records must be counted as excluded: {summary}"
    );

    // `pairing`: the fixtures carry paired tool traffic → a `paired` key exists.
    let out = h.run(&[
        "search",
        "",
        "--no-subagents",
        "--count-by",
        "pairing",
        "--format",
        "json",
    ]);
    let rows = json_rows(&out.stdout, "census");
    assert!(
        rows.iter().any(|r| r["key"] == "paired"),
        "pairing census must show paired:\n{}",
        out.stdout
    );

    // `turn`: an ascending histogram, keys `t<N>` (single transcript in scope).
    let out = h.run(&[
        "search",
        "",
        "--no-subagents",
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    let rows = json_rows(&out.stdout, "census");
    let keys: Vec<String> = rows
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert!(
        keys.iter().all(|k| k.starts_with('t')),
        "turn keys: {keys:?}"
    );
    let mut sorted = keys.clone();
    sorted.sort_by_key(|k| k[1..].parse::<usize>().unwrap_or(usize::MAX));
    assert_eq!(keys, sorted, "turn axis must be ascending: {keys:?}");

    // `session`: one key per transcript.
    let out = h.run(&[
        "search",
        "",
        "--no-subagents",
        "--count-by",
        "session",
        "--format",
        "json",
    ]);
    let summary = json_summary(&out.stdout);
    assert!(summary["distinct_keys"].as_u64().unwrap() >= 1);

    // An unknown axis is a clap parse error naming the closed set.
    let out = h.run(&["search", "", "--count-by", "bogus"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("possible values"),
        "stderr: {}",
        out.stderr
    );

    // The old v0.4 spelling is gone (zero-BC): unknown argument + tip at the new one.
    let out = h.run(&["search", "", "--count-by-label"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("'--count-by'"),
        "tip names the new flag: {}",
        out.stderr
    );
}

#[test]
fn search_empty_diagnosis_names_the_excluding_label() {
    let h = populated_home();
    // "low-edge" occurs ONLY under agent.tool.result (record c0). Searching it under
    // `-t user.message` yields zero — the exact L74681 trap. The zero-result diagnosis must
    // NAME the excluding label so a model self-corrects instead of assuming a syntax error.
    let out = h.run(&["search", "low-edge", "--no-subagents", "-t", "user.message"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("no matching exchanges"));
    assert!(
        out.stderr.contains("DEFINITIVE absence"),
        "stderr: {}",
        out.stderr
    );
    assert!(out.stderr.contains("DOES occur"), "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("agent.tool.result"),
        "stderr: {}",
        out.stderr
    );
    // JSON summary carries the machine-legible diagnosis.
    let out = h.run(&[
        "search",
        "low-edge",
        "--no-subagents",
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["definitive_absence"], serde_json::json!(true));
    assert_eq!(
        summary["active_filters"],
        serde_json::json!("-t user.message")
    );
    assert_eq!(
        summary["excluded_by_label"]["by_label"]["agent.tool.result"],
        serde_json::json!(1)
    );
}

#[test]
fn search_empty_diagnosis_reports_genuine_absence() {
    let h = populated_home();
    // A token absent even WITHOUT the label filter → say so plainly (not a label mistake).
    let out = h.run(&[
        "search",
        "zzz-absent-zzz",
        "--no-subagents",
        "-t",
        "agent.message",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("DEFINITIVE absence"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("genuinely absent"),
        "stderr: {}",
        out.stderr
    );
    let out = h.run(&[
        "search",
        "zzz-absent-zzz",
        "--no-subagents",
        "-t",
        "agent.message",
        "--format",
        "json",
    ]);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["definitive_absence"], serde_json::json!(true));
    assert_eq!(summary["excluded_by_label"], serde_json::Value::Null);
}

#[test]
fn search_text_subagent_hit_carries_exact_refetch() {
    let h = populated_home();
    // "carry" occurs in the SUBAGENT transcripts (agent-aaa111 / agent-bbb222). A subagent
    // hit's line number is per-FILE, so the fetch MUST use the subagent's own id, never the
    // parent uuid. Text mode now prints the ready-to-run command so a model never derives it.
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("↳ csift show @aaa111 --line")
            || out.stdout.contains("↳ csift show @bbb222 --line"),
        "a subagent hit must print its exact refetch with the AGENT id:\n{}",
        out.stdout
    );
    // The refetch NEVER addresses a subagent line at the parent uuid (the silent-wrong-record
    // hazard the pointer closes).
    assert!(
        !out.stdout.contains(&format!("csift show @{SESS} --line")),
        "a subagent refetch must not use the parent uuid:\n{}",
        out.stdout
    );
}

#[test]
fn list_and_stats_max_count_cap_and_report() {
    let h = populated_home(); // 1 top-level + 2 subagent = 3 rows
                              // list: cap to 2, drop 1 — reported in the JSON summary AND the text footer (never silent).
    let lj = h.run(&["list", "--max-count", "2", "--format", "json"]);
    assert!(lj.success, "stderr: {}", lj.stderr);
    assert_eq!(
        json_rows(&lj.stdout, "session").len(),
        2,
        "list capped to 2"
    );
    assert_eq!(
        json_summary(&lj.stdout)["dropped_by_cap"],
        serde_json::json!(1),
        "list drop reported"
    );
    let lt = h.run(&["list", "--max-count", "1"]);
    assert!(
        lt.stdout.contains("more session(s) not shown"),
        "list drop footer: {}",
        lt.stdout
    );
    assert!(
        lt.stdout.contains("--max-count"),
        "the guidance names the override"
    );
    // stats: cap to 2, drop 1.
    let sj = h.run(&["stats", "--max-count", "2", "--format", "json"]);
    assert!(sj.success, "stderr: {}", sj.stderr);
    assert_eq!(
        json_summary(&sj.stdout)["dropped_by_cap"],
        serde_json::json!(1),
        "stats drop reported: {}",
        sj.stdout
    );
}

#[test]
fn search_truncated_excerpt_emits_reader_caution() {
    let h = Home::new();
    let enc = "-Users-test-Projects-trunc";
    let sess = "bbbbbbbb-cccc-4ddd-8eee-ffffffffffff";
    // A long assistant message (well past the 400-char excerpt cap) whose OPENING contradicts
    // the deep match — the exact "trusting the truncated head misreads the whole record" failure
    // the caution guards against.
    let long = format!(
        "{}NEEDLEXYZ the real intent is the OPPOSITE of the opening {}",
        "opening padding ".repeat(40),
        "trailing padding ".repeat(40),
    );
    let body = format!(
        concat!(
            r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","sessionId":"{sess}","cwd":"/Users/test/Projects/trunc","message":{{"role":"user","content":"go"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:05.000Z","sessionId":"{sess}","message":{{"role":"assistant","content":[{{"type":"text","text":"{long}"}}]}}}}"#,
            "\n",
        ),
        sess = sess,
        long = long,
    );
    h.write(&format!("{enc}/{sess}.jsonl"), &body);
    let at = format!("@{sess}");

    // Default (truncating): the caution appears with all three pieces (what it is + --no-truncate +
    // --line/--uuid).
    let out = h.run(&["search", "NEEDLEXYZ", &at]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("TRUNCATED"),
        "no caution:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("--no-truncate"),
        "no --no-truncate hint:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("--line") && out.stdout.contains("--uuid"),
        "no per-record fetch hint:\n{}",
        out.stdout
    );

    // --no-truncate lifts the cap → no truncation → NO caution, and the whole text is shown.
    let full = h.run(&["search", "NEEDLEXYZ", &at, "--no-truncate"]);
    assert!(full.success, "stderr: {}", full.stderr);
    assert!(
        !full.stdout.contains("TRUNCATED"),
        "caution must be suppressed under --no-truncate:\n{}",
        full.stdout
    );
    assert!(
        full.stdout.contains("OPPOSITE of the opening"),
        "full text not shown:\n{}",
        full.stdout
    );

    // JSON summary carries the machine echo `excerpts_truncated`.
    let json = h.run(&["search", "NEEDLEXYZ", &at, "--format", "json"]);
    let last = json
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .unwrap();
    let summary: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(summary["excerpts_truncated"], serde_json::Value::Bool(true));

    // And under --no-truncate the flag flips false.
    let json_full = h.run(&[
        "search",
        "NEEDLEXYZ",
        &at,
        "--no-truncate",
        "--format",
        "json",
    ]);
    let last_full = json_full
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .unwrap();
    let summary_full: serde_json::Value = serde_json::from_str(last_full).unwrap();
    assert_eq!(
        summary_full["excerpts_truncated"],
        serde_json::Value::Bool(false)
    );
}
