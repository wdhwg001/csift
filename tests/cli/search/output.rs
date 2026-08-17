//! search output shapes: excerpts, siblings, --raw, refetch addresses, id trios.

use crate::harness::*;

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
fn search_truncated_excerpt_emits_reader_caution() {
    let h = Home::new();
    let enc = "-Users-test-Projects-trunc";
    let sess = "bbbbbbbb-cccc-4ddd-8eee-ffffffffffff";
    // A long assistant message (well past the 400-char excerpt cap) whose OPENING contradicts
    // the deep match - the exact "trusting the truncated head misreads the whole record" failure
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

#[test]
fn search_short_match_has_no_truncation_caution() {
    // Every "carry" match in the canonical fixture fits the cap → nothing clipped → no caution.
    let h = populated_home();
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("TRUNCATED"),
        "no truncation expected for short matches:\n{}",
        out.stdout
    );
}

#[test]
fn search_siblings_surface_the_rest_of_the_turn() {
    // The Q3 shape: a matched USER record should be able to surface WITH the agent reply.
    // "needed" lives ONLY in the opening user message, so the agent text can reach the
    // output only as a `--siblings` context record (default sibling set = all-but-`-t`).
    let h = populated_home();
    let base = h.run(&["search", "needed", "-t", "user", "--no-subagents"]);
    assert!(
        !base.stdout.contains("partial line at a chunk boundary"),
        "without --siblings the agent reply must NOT appear: {}",
        base.stdout
    );

    // `--siblings` (zero-arg): the fixed policy renders the turn's other side.
    let out = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· agent"),
        "the agent sibling renders under the `·` marker: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("partial line at a chunk boundary"),
        "the agent reply text surfaces as a sibling: {}",
        out.stdout
    );
    // The matched user record opens the exchange (◂) and is never repeated as a sibling.
    assert_eq!(
        out.stdout.matches("carry needed").count(),
        1,
        "the matched user line appears once, not duplicated as a sibling: {}",
        out.stdout
    );
}

#[test]
fn search_siblings_fixed_policy_renders_turn_and_json_carries_array() {
    // `--siblings` (zero-arg, FIXED policy): message units always render; the fixture
    // turn's few tool units fall inside the per-leaf caps, so the whole back-and-forth
    // surfaces. JSON carries the `siblings` array; absent without the flag.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· agent.message"),
        "agent.message sibling present: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("· agent.tool.use"),
        "tool siblings render under the fixed caps: {}",
        out.stdout
    );

    let j = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
        "--format",
        "json",
    ]);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    let sibs = env["siblings"].as_array().expect("siblings array present");
    assert!(
        sibs.iter().any(|s| s["label"] == "agent.message"),
        "{sibs:?}"
    );
    assert_eq!(
        env["siblings_hidden"], 0,
        "nothing capped on this small turn"
    );

    let plain = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let env2 = json_rows(&plain.stdout, "exchange").remove(0);
    assert!(
        env2.get("siblings").is_none(),
        "no siblings key without the flag: {env2}"
    );
}

#[test]
fn search_no_truncate_emits_the_untruncated_record() {
    // A message far longer than the ~400-char excerpt cap, with a token at the very TAIL.
    // The default excerpt truncates (explicit `… (+N chars)` marker) and hides the tail;
    // `--no-truncate` emits the whole record so the tail is readable - the gap that otherwise
    // forces a drop to the raw jsonl.
    let h = Home::new();
    let filler = "x".repeat(900);
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"needle {filler} taIlToken9z"}}}}"#
        ),
    );

    let def = h.run(&["search", "needle", "--no-subagents"]);
    assert!(def.success, "stderr: {}", def.stderr);
    assert!(
        def.stdout.contains("… (+"),
        "default excerpt must truncate with the explicit marker: {}",
        def.stdout
    );
    assert!(
        !def.stdout.contains("taIlToken9z"),
        "the tail token is hidden by the default cap: {}",
        def.stdout
    );

    let full = h.run(&["search", "needle", "--no-subagents", "--no-truncate"]);
    assert!(
        full.stdout.contains("taIlToken9z"),
        "--no-truncate must surface the tail: {}",
        full.stdout
    );
    assert!(
        !full.stdout.contains("… (+"),
        "--no-truncate removes the truncation marker: {}",
        full.stdout
    );

    // Zero back-compat: the old `--full` spelling is GONE - it must ERROR (unknown argument),
    // never silently work, so existing users are forced onto the unambiguous `--no-truncate`.
    let removed = h.run(&["search", "needle", "--no-subagents", "--full"]);
    assert!(
        !removed.success,
        "--full was removed and must be rejected, got success:\n{}",
        removed.stdout
    );
}

#[test]
fn search_hit_carries_line_and_uuid_address() {
    let h = populated_home();
    // "needed" lives only in the opening user record (fixture line 1).
    let out = h.run(&["search", "needed", "-t", "user", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.message  L1  "),
        "the hit header carries its `L<line>` address: {}",
        out.stdout
    );
    // JSON: per-hit `line` + `uuid` (the `csift get` address).
    let j = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    assert_eq!(env["hits"][0]["line"], 1);
    assert_eq!(env["hits"][0]["uuid"], "u0");
}

#[test]
fn search_raw_emits_verbatim_lines_on_a_pure_stdout() {
    // `--raw` = show's escape hatch on search's filter surface: the matched records'
    // VERBATIM jsonl lines, byte-identical to the file, stdout pure (notes → stderr).
    let (h, sess, _hex) = show_subagent_home();
    let out = h.run(&[
        "search",
        "go",
        &format!("@{sess}"),
        "--no-subagents",
        "--raw",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Every stdout line parses as JSON AND is byte-identical to a line of the transcript.
    let disk = std::fs::read_to_string(
        h.projects()
            .join("-Users-testuser-Projects-linehex")
            .join(format!("{sess}.jsonl")),
    )
    .unwrap();
    let disk_lines: Vec<&str> = disk.lines().collect();
    let mut n = 0;
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "stdout is a pure jsonl stream: {line}"
        );
        assert!(
            disk_lines.contains(&line),
            "verbatim (byte-identical to the file): {line}"
        );
        n += 1;
    }
    assert!(n >= 1, "at least the matching record emits: {}", out.stdout);
    // The filter surface still applies: an excluding -T yields an empty (exit 0) stream.
    let none = h.run(&[
        "search",
        "go",
        &format!("@{sess}"),
        "--no-subagents",
        "--raw",
        "-T",
        "user",
    ]);
    assert!(none.success, "stderr: {}", none.stderr);
    assert!(
        none.stdout.trim().is_empty(),
        "user-record hit excluded: {}",
        none.stdout
    );
    // Conflicts: --raw excludes the rendered-surface modes.
    for extra in [["--siblings"], ["-c"], ["-l"]] {
        let mut args = vec!["search", "go", "--raw"];
        args.extend_from_slice(&extra);
        let bad = h.run(&args);
        assert!(!bad.success, "--raw + {extra:?} must conflict");
    }
    let badjson = h.run(&["search", "go", "--raw", "--format", "json"]);
    assert!(!badjson.success, "--raw + --format json must error");
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
        jpath(&target.to_string_lossy())
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
        with.stdout.contains("agent.tool.result"),
        "resolved match: {}",
        with.stdout
    );
    assert!(with.stdout.contains("matched 1"));
}

#[test]
fn search_hit_rows_carry_the_id_trio() {
    // R9: bare `.hits[]` flattening is the most natural jq idiom against the most-piped
    // command; the trio now rides every hit row (matching the exchange row's copy), so the
    // idiom yields real ids instead of silent nulls.
    let h = populated_home();
    let out = h.run(&["search", "carry", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for ex in json_rows(&out.stdout, "exchange") {
        for hit in ex["hits"].as_array().unwrap() {
            assert_eq!(hit["session_id"], ex["session_id"], "hit trio: {hit}");
            assert_eq!(hit["is_subagent"], ex["is_subagent"]);
            assert_eq!(hit["parent_session_id"], ex["parent_session_id"]);
        }
    }
}

#[test]
fn json_hits_carry_pairing_and_refetch() {
    // Mutation pin: the JSON hit objects' `pairing` (a real "paired" value, not a
    // defaulted null) and the ready-to-run `refetch` command must actually be emitted.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#""pairing":"paired""#),
        "the Write use/result pair carries pairing=paired: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""refetch":"csift show @"#),
        "every hit carries the ready-to-run refetch: {}",
        out.stdout
    );
}

#[test]
fn siblings_hidden_count_is_exact() {
    // Mutation pin: the fixed sibling policy's capped-away remainder is COUNTED exactly
    // (thinking cap = 2; five thinking siblings -> 3 hidden), and the overflow pointer
    // carries that number - a degraded += froze it with no test on the value.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "778899aa-bbcc-4000-8000-00000000000c";
    let mut jsonl = String::from(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"SIBSEED question"}}"#,
    );
    jsonl.push('\n');
    for i in 0..5 {
        jsonl.push_str(&format!(
            r#"{{"type":"assistant","uuid":"t{i}","timestamp":"2026-06-07T05:00:0{}.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"step {i}"}}]}}}}"#,
            i + 1
        ));
        jsonl.push('\n');
    }
    h.write(&format!("{enc}/{sess}.jsonl"), &jsonl);
    let out = h.run(&["search", "SIBSEED", &format!("@{sess}"), "--siblings"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(+3 more"),
        "exactly 3 thinking siblings capped away: {}",
        out.stdout
    );
}

#[test]
fn search_subagent_hit_json_marks_refeedable_parent() {
    // A search hit inside a subagent transcript: JSON carries is_subagent + the re-feedable
    // parent uuid (the bare-hex session_id is not a --session target).
    let h = Home::new();
    subagents_only_scenario(&h);
    // The subagent's user seed contains "write a file".
    let out = h.run(&[
        "search",
        "write a file",
        at(SESS).as_str(),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sub = objs
        .iter()
        .find(|o| o["is_subagent"] == serde_json::json!(true))
        .expect("a subagent hit present");
    assert_eq!(sub["session_id"], serde_json::json!("sub111"));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn search_surfaces_extractable_image_ids_on_a_hit() {
    // A `search` hit on a message that carries images must expose the SAME extractable ids as
    // `turns`/`image` - so a search result feeds straight into `csift image --id` with no
    // manual L+i assembly. r2 ("two more") carries a jpeg + a png at line 3 → L3i1, L3i2.
    let h = image_home();
    let out = h.run(&["search", "two more", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("[2 images: L3i1, L3i2]"),
        "image-id suffix on the hit line:\n{}",
        out.stdout
    );
    // The JSON envelope carries the ids array on the hit too.
    let j = h.run(&[
        "search",
        "two more",
        at(SESS).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let hit = j
        .stdout
        .lines()
        .find(|l| l.contains("\"image_ids\"") && l.contains("L3i1"))
        .expect("a hit object carrying image_ids");
    assert!(hit.contains("L3i2"), "both ids present: {hit}");
}
