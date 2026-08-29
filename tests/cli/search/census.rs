//! search terminal modes: -c, -l, and the --count-by census axes.

use crate::harness::*;

#[test]
fn search_count_by_label_censuses_the_scope() {
    let h = populated_home();
    // Empty pattern + --count-by label = "what record-types are here?" - the exploration
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
    // census - a leaf's tally must equal what `-t <leaf>` surfaces, never drift above it by
    // the multi-section overlap. (2) Pairing is a property of the underlying tool block, so
    // it rides the communication view too: a SendMessage with no tool_result is `pending`
    // even though its richest view is agent.communication.sent - the "anything stuck?"
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
    // 4 RECORDS in scope (opener, text+tool, result, send) - the multi-section record
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

    // The comm selector agrees - the send is IN the pairing domain now, not excluded.
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
fn search_count_prints_only_the_match_total() {
    // `-c`/--count: just the integer, no headers - and it must equal the footer `matched`
    // (the ripgrep `-c` contract). Compare against the JSON summary so the assertion tracks
    // whatever the fixture actually yields.
    let h = populated_home();
    let full = h.run(&["search", "carry", "--no-subagents", "--format", "json"]);
    let footer: serde_json::Value = serde_json::from_str(
        full.stdout
            .lines()
            .filter(|l| !l.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    let expected = footer["matched"].as_u64().unwrap();

    let out = h.run(&["search", "carry", "--no-subagents", "-c"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.trim().parse::<u64>().unwrap(),
        expected,
        "-c must print exactly the match total; got {:?}",
        out.stdout
    );
    // No per-exchange output leaked through.
    assert!(!out.stdout.contains("SESSION"), "got: {}", out.stdout);
    assert!(!out.stdout.contains("matched "), "got: {}", out.stdout);

    // JSON form is `{"matched":N}`.
    let j = h.run(&[
        "search",
        "carry",
        "--no-subagents",
        "-c",
        "--format",
        "json",
    ]);
    let v = json_summary(&j.stdout);
    assert_eq!(v["matched"].as_u64().unwrap(), expected);
}

#[test]
fn search_count_reports_true_total_despite_max_count() {
    // `-c` reports the TRUE total even when `--max-count` would cap the listing (the count
    // adds the capped-away remainder back), so the number is never silently shrunk.
    let h = populated_home();
    let capped = h.run(&["search", "carry", "-c", "--max-count", "1"]);
    let uncapped = h.run(&["search", "carry", "-c"]);
    assert_eq!(
        capped.stdout.trim(),
        uncapped.stdout.trim(),
        "--max-count must not change the -c total"
    );
}

#[test]
fn sessions_with_matches_pipes_into_sessions_from_and_refetch_round_trips() {
    let h = populated_home();
    // `-l`: bare ids, one per line - WHICH sessions matched.
    let l = h.run(&["search", "", "-l"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert!(
        l.stdout.lines().any(|s| s.trim() == SESS),
        "lists the matching session: {}",
        l.stdout
    );
    // The id stream pipes STRAIGHT into `--sessions-from -` (the composition loop closes
    // inside csift - no jq/sed re-quoting).
    let piped = h.run_with_stdin(
        &["stats", "--sessions-from", "-", "--format", "json"],
        &l.stdout,
    );
    assert!(piped.success, "stderr: {}", piped.stderr);
    assert!(
        piped.stdout.contains(SESS),
        "piped scope reached stats: {}",
        piped.stdout
    );
    // `-l --format json` is a pointed error (JSON readers use the summary's transcript_ids).
    let j = h.run(&["search", "", "-l", "--format", "json"]);
    assert!(!j.success);
    // Every JSON hit carries `refetch` - a ready-to-run `csift show` addressed at the hit's
    // OWN transcript - and the command actually round-trips.
    let js = h.run(&["search", "", &format!("@{SESS}"), "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    let ex_rows = json_rows(&js.stdout, "exchange");
    let refetch = ex_rows[0]["hits"][0]["refetch"]
        .as_str()
        .expect("refetch is a string");
    assert!(refetch.starts_with("csift show @"), "got: {refetch}");
    let parts: Vec<&str> = refetch.split_whitespace().skip(1).collect();
    let rf = h.run(&parts);
    assert!(rf.success, "the refetch command round-trips: {}", rf.stderr);
}

#[test]
fn count_by_label_census_respects_label_filters() {
    // R7 §2.3: `-t`/`-T` decide which records ENTER the census, and the label-axis KEYS pass
    // the same predicate - a dual-labeled record (an AUQ answer = user.answer +
    // agent.tool.result) must not leak its filtered-out twin into the census keys.
    let h = Home::new();
    let sess = "44444444-5555-6666-7777-888888888888";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"44444444-5555-6666-7777-888888888888","cwd":"/Users/x/auq","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the zzopt option\". You can now continue."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-auq/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    // Filtered: only the SURVIVING label keys appear.
    let out = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "--no-subagents",
        "-t",
        "user",
        "-T",
        "user.message",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let keys: Vec<String> = json_rows(&out.stdout, "census")
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        keys,
        vec!["user.answer".to_string()],
        "census keys must pass the -t/-T predicate: {}",
        out.stdout
    );

    // Unfiltered: the FULL label set is still censused (both leaves of the dual record).
    let full = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "--no-subagents",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let full_keys: Vec<String> = json_rows(&full.stdout, "census")
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert!(
        full_keys.contains(&"user.answer".to_string())
            && full_keys.contains(&"agent.tool.result".to_string()),
        "no filter ⇒ full label sets: {}",
        full.stdout
    );
}

#[test]
fn count_by_tool_reports_exact_record_counts() {
    // Mutation pin: the per-axis counters must actually COUNT (a `+=` degraded to a
    // no-op leaves every tally at zero and the excluded total frozen) - pin exact
    // numbers on a fixed fixture: parent Write (tool_use + result carrier = 2 records)
    // + subagent Write (tool_use only = 1 record).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str(), "--count-by", "tool"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("3  Write"),
        "Write must tally 3 records: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains("excluded") || out.stderr.contains("no tool"),
        "records outside the tool axis are reported: {}",
        out.stderr
    );
}

#[test]
fn sessions_with_matches_disclosed_cap_drop_on_stderr() {
    // Mutation pin: the -l + --max-count drop note fires exactly when dropped_by_cap > 0.
    let enc = "-Users-testuser-Projects-lcap";
    let h = Home::new();
    for i in 0..2u8 {
        h.write(
            &format!("{enc}/ee00000{i}-aaaa-4bbb-8ccc-00000000000{i}.jsonl"),
            &format!("{{\"type\":\"user\",\"uuid\":\"u0\",\"timestamp\":\"2026-06-07T0{i}:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"zzlcap hit\"}}}}\n"),
        );
    }
    let capped = h.run(&["search", "zzlcap", enc, "-l", "--max-count", "1"]);
    assert!(capped.success, "stderr: {}", capped.stderr);
    assert!(
        capped.stderr.contains("dropped by --max-count"),
        "cap drop disclosed on stderr: {}",
        capped.stderr
    );
    let full = h.run(&["search", "zzlcap", enc, "-l"]);
    assert!(
        !full.stderr.contains("dropped by --max-count"),
        "no note without a drop: {}",
        full.stderr
    );
}

#[test]
fn count_by_version_censuses_the_cc_version_stamp() {
    // A session that upgraded mid-flight: two records on the old version, one on the new,
    // one metadata record with no stamp (excluded + disclosed).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","version":"2.0.100","message":{"role":"user","content":"probe the relay"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","version":"2.0.100","message":{"role":"assistant","content":[{"type":"text","text":"relay checked"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","timestamp":"2026-06-07T06:00:00.000Z","version":"2.0.200","message":{"role":"user","content":"probe again"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"second pass"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "", at(SESS).as_str(), "--count-by", "version"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("2.0.100") && out.stdout.contains("2.0.200"),
        "per-version keys:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("1 record(s) have no version"),
        "the stampless record is excluded AND disclosed:\n{}",
        out.stderr
    );
    let outj = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--count-by",
        "version",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        rows.iter().any(|r| r["kind"] == "census"
            && r["axis"] == "version"
            && r["key"] == "2.0.100"
            && r["records"] == 2),
        "2.0.100 counts twice: {}",
        outj.stdout
    );
    assert!(
        rows.iter()
            .any(|r| r["kind"] == "census" && r["key"] == "2.0.200" && r["records"] == 1),
        "2.0.200 counts once: {}",
        outj.stdout
    );
    assert_eq!(
        rows.last().unwrap()["excluded_records"],
        1,
        "summary excluded: {}",
        outj.stdout
    );
}

#[test]
fn count_by_result_separates_errored_results_and_marks_them_inline() {
    // C-13: an errored result is still `paired` (a result came back), so the pairing
    // census is structurally blind to "any failed reads?". The result axis answers it,
    // the errored row says so inline, and JSON carries is_error.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read the ledgers"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/ok.md"}},{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/p/gone.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"fine content"}]}}"#, "\n",
            r#"{"type":"user","uuid":"r2","parentUuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"<tool_use_error>File does not exist.</tool_use_error>","is_error":true}]}}"#, "\n",
        ),
    );
    // The axis: 1 ok, 1 error; non-result records excluded and disclosed.
    let out = h.run(&["search", "", at(SESS).as_str(), "--count-by", "result"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("ok") && out.stdout.contains("error"),
        "both buckets:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("2 record(s) have no result"),
        "non-result records are excluded AND disclosed:\n{}",
        out.stderr
    );
    let outj = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--count-by",
        "result",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for (key, n) in [("ok", 1), ("error", 1)] {
        assert!(
            rows.iter().any(|r| r["kind"] == "census"
                && r["axis"] == "result"
                && r["key"] == key
                && r["records"] == n),
            "{key} bucket: {}",
            outj.stdout
        );
    }
    // Pairing census UNCHANGED: both calls are paired (the axes answer different questions).
    let pairing = h.run(&["search", "", at(SESS).as_str(), "--count-by", "pairing"]);
    assert!(
        pairing.stdout.contains("3  paired"),
        "errored results still pair (1 use-carrier + 2 result-carriers = 3 records):\n{}",
        pairing.stdout
    );
    // Text render: the errored result row carries [error]; the clean one does not.
    let txt = h.run(&["search", "", at(SESS).as_str(), "-t", "agent.tool.result"]);
    assert!(
        txt.stdout.contains("agent.tool.result [error]")
            || txt.stdout.contains("agent.tool.result\u{200b}[error]")
            || txt.stdout.contains("▹ agent.tool.result [error]"),
        "inline error marker:\n{}",
        txt.stdout
    );
    assert_eq!(
        txt.stdout.matches("[error]").count(),
        1,
        "only the errored row is marked:\n{}",
        txt.stdout
    );
    // JSON hits carry is_error on both sides of the truth.
    let hj = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "agent.tool.result",
        "--format",
        "json",
    ]);
    assert!(
        hj.stdout.contains(r#""is_error":true"#) && hj.stdout.contains(r#""is_error":false"#),
        "explicit error state on every result hit: {}",
        hj.stdout
    );
}
