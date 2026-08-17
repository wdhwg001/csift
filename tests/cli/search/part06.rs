use crate::harness::*;

#[test]
fn search_subagent_scope_spawn_lookup_resolves_in_subagent_spawn_and_return() {
    // P3a subagent-scope spawn lookup: when scanning a SUBAGENT transcript, the spawn lookup is
    // built from its PARENT session (whose sidecar holds the flat set of ALL subagents), so an
    // IN-SUBAGENT spawn resolves `self ⇨ <grandchild>` and an in-subagent Task-return resolves to
    // `agent.communication.inbox <grandchild> ⇨ self`. (Before the fix the lookup was top-level
    // only → both degraded.)
    let enc = "-Users-x-nest";
    let sess = "15151515-2626-3737-4848-595959595959";
    let agent_a = "aaaa1111bbbb2222";
    let agent_b = "bbbb3333cccc4444";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"15151515-2626-3737-4848-595959595959","cwd":"/Users/x/nest","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"delegate"}}"#, "\n",
        ),
    );
    // Subagent A: spawns a Task (id=toolu_inner) and later receives its return (same id).
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_a}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaaa1111bbbb2222","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"do the parent-of-nest work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_inner","name":"Task","input":{"description":"zzspawn grandchild","subagent_type":"executor"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sr0","timestamp":"2026-06-07T05:30:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_inner","content":"zzreturn grandchild done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_a}.meta.json"),
        r#"{"agentType":"executor","toolUseId":"toolu_outerA"}"#,
    );
    // Grandchild B: discovered under the SAME parent dir; its spawn `toolUseId` is the IN-SUBAGENT
    // Task tool_use id, so the lookup (built from the parent) joins toolu_inner → B.
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_b}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"bbbb3333cccc4444","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":"grandchild seed"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_b}.meta.json"),
        r#"{"agentType":"executor","toolUseId":"toolu_inner"}"#,
    );

    // The in-subagent SPAWN resolves `self ⇨ <grandchild>` (scan only A via --no-subagents).
    let sent = h.run(&[
        "search",
        "zzspawn",
        "-t",
        "agent.communication.sent",
        at(agent_a).as_str(),
        "--no-subagents",
    ]);
    assert!(sent.success, "stderr: {}", sent.stderr);
    assert!(
        sent.stdout.contains(&format!("self ⇨ {agent_b}")),
        "in-subagent spawn must resolve self ⇨ grandchild; got: {}",
        sent.stdout
    );

    // The in-subagent Task-RETURN resolves to `agent.communication.inbox <grandchild> ⇨ self`.
    let inbox = h.run(&[
        "search",
        "zzreturn",
        "-t",
        "agent.communication.inbox",
        at(agent_a).as_str(),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("agent.communication.inbox"),
        "in-subagent Task-return must surface as inbox; got: {}",
        inbox.stdout
    );
    assert!(
        inbox.stdout.contains(&format!("{agent_b} ⇨ self")),
        "the return resolves grandchild ⇨ self; got: {}",
        inbox.stdout
    );
}

#[test]
fn search_rejects_old_flat_category_selector() {
    // 0 back-compat (GOLD §6): the old flat `-t tool-response` is a HARD clap error that lists the
    // valid selectors; the dotted form works.
    let h = populated_home();
    let bad = h.run(&["search", "carry", "-t", "tool-response"]);
    assert!(!bad.success, "old flat -t must error; got:\n{}", bad.stdout);
    assert!(
        bad.stderr.contains("agent.tool.result"),
        "the error lists the valid selectors; stderr: {}",
        bad.stderr
    );
}

#[test]
fn search_skips_non_transcript_noise_lines() {
    // A session padded with attachment / file-history-snapshot / queue-operation lines (no
    // role marker) → search's pre-JSON category prefilter drops them (the
    // `!line_is_transcript_candidate` TRUE arm) while still matching the real turn. (The
    // `compact_boundary` line IS kept by the prefilter now (D7), but carries no `carry` literal
    // and no compactMetadata here, so it produces no spurious hit.)
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"attachment","data":{"x":1}}"#, "\n",
            r#"{"type":"file-history-snapshot","snapshot":{}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","preTokens":1}"#, "\n",
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"real turn with carry token"}}"#, "\n",
            r#"{"type":"queue-operation","op":"x"}"#, "\n",
        ),
    );
    let out = h.run(&["search", "carry", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched 1"), "got: {}", out.stdout);
}

#[test]
fn search_quoted_tags_mid_prose_stay_user_message() {
    // FINDING-1: a genuine user message that merely QUOTES `<task-notification>` /
    // `<teammate-message>` mid-prose stays `user.message` — it is NOT reclassified
    // `harness.notification` / `agent.communication.inbox` (this bit csift's OWN dev sessions,
    // which quote these tags constantly).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"In csift the <task-notification> pulse and the <teammate-message peer form both route through classify zzquoted."}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ack"}]}}"#, "\n",
        ),
    );
    // Found under user.message …
    let um = h.run(&[
        "search",
        "zzquoted",
        "-t",
        "user.message",
        &at(SESS),
        "--no-subagents",
    ]);
    assert!(um.success, "stderr: {}", um.stderr);
    assert!(
        um.stdout.contains("user.message"),
        "FINDING-1: quoted tags stay user.message:\n{}",
        um.stdout
    );
    // … and NOT under harness.notification …
    let notif = h.run(&[
        "search",
        "zzquoted",
        "-t",
        "harness.notification",
        &at(SESS),
        "--no-subagents",
    ]);
    assert!(notif.success, "stderr: {}", notif.stderr);
    assert!(
        notif.stdout.contains("no matching exchanges"),
        "FINDING-1: a quoted <task-notification> is not a notification:\n{}",
        notif.stdout
    );
    // … and NOT under agent.communication.inbox.
    let inbox = h.run(&[
        "search",
        "zzquoted",
        "-t",
        "agent.communication.inbox",
        &at(SESS),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("no matching exchanges"),
        "FINDING-1: a quoted <teammate-message> is not inbox:\n{}",
        inbox.stdout
    );
}

#[test]
fn search_global_max_count_caps_across_files() {
    // Two sessions each matching once; --max-count 1 emits one and drops one GLOBALLY
    // (the cross-file cap merge arm). Use --no-subagents to keep the count exact.
    let h = Home::new();
    for i in 0..2 {
        let sid = format!("ssss{i}ss-0000-0000-0000-00000000000{i}");
        h.write(
            &format!("{ENC}/{sid}.jsonl"),
            &format!(
                "{{\"type\":\"user\",\"uuid\":\"u{i}\",\"timestamp\":\"2026-06-0{}T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"global cap token zzcap\"}}}}\n",
                i + 1
            ),
        );
    }
    let out = h.run(&["search", "zzcap", "--max-count", "1", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // TRUE total at both ends; the emitted window + global drop are disclosed.
    assert!(out.stdout.contains("matched 2"), "{}", out.stdout);
    assert!(out.stdout.contains("showing earliest 1"), "{}", out.stdout);
    assert!(out.stdout.contains("1 later dropped"), "{}", out.stdout);
    assert!(out.stdout.contains("by --max-count"));
}

#[test]
fn search_timeline_interleaves_subagents_with_top_level_by_timestamp() {
    // The combined timeline is CHRONOLOGICAL, not file-grouped: a subagent exchange whose
    // turn began BETWEEN two parent turns must sort BETWEEN them — even though the subagent
    // file is scanned after the parent file. Parent turns at T=00 and T=10, subagent turn at
    // T=05 → expected envelope order 00 (parent) · 05 (SUBAGENT) · 10 (parent).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ping alpha"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply alpha"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"ping gamma"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply gamma"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub222.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub222","uuid":"s0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":"ping beta"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa0","parentUuid":"s0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub reply beta"}]}}"#, "\n",
        ),
    );

    let out = h.run(&["search", "ping", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let envelopes: Vec<_> = json_lines(&out.stdout)
        .into_iter()
        .filter(|o| o.get("turn_index").is_some())
        .collect();
    assert_eq!(
        envelopes.len(),
        3,
        "parent ×2 + subagent ×1: {}",
        out.stdout
    );

    // Chronological interleave: 00 (parent) · 05 (SUBAGENT, between) · 10 (parent).
    assert_eq!(
        envelopes[0]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:00.000Z")
    );
    assert_eq!(envelopes[0]["is_subagent"], serde_json::json!(false));
    assert_eq!(
        envelopes[1]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:05.000Z")
    );
    assert_eq!(
        envelopes[1]["is_subagent"],
        serde_json::json!(true),
        "subagent sorts BETWEEN the two parent turns, not grouped after them"
    );
    assert_eq!(envelopes[1]["parent_session_id"], serde_json::json!(SESS));
    assert_eq!(
        envelopes[2]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:10.000Z")
    );
    assert_eq!(envelopes[2]["is_subagent"], serde_json::json!(false));

    // ts_local is the same instant rendered in the host TZ (present, non-null).
    assert!(
        envelopes[1]["ts_local"].is_string(),
        "envelope carries ts_local"
    );
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
fn list_skipped_lines_is_a_window_census_stats_is_the_authority() {
    // R12 §1 disclosure pin: a malformed line OUTSIDE list's head/tail windows is
    // invisible to `list` BY DESIGN (§7: list never scans the middle — full coverage
    // measured ~4× its unscoped runtime), while `stats` (a full scan) is the
    // corruption-census authority over the same bytes. Pinning BOTH numbers keeps the
    // divergence a documented contract instead of silent drift.
    let h = Home::new();
    let enc = "-Users-test-Projects-midtear";
    let sess = "eeeeeeee-aaaa-4bbb-8ccc-000000000150";
    let mut body = String::new();
    for i in 0..20 {
        if i == 9 {
            body.push_str("MID-FILE TEAR not json\n");
            continue;
        }
        let (ty, role) = if i % 2 == 0 {
            ("user", "user")
        } else {
            ("assistant", "assistant")
        };
        body.push_str(&format!(
            r#"{{"type":"{ty}","uuid":"m{i}","timestamp":"2026-06-07T05:00:{i:02}.000Z","message":{{"role":"{role}","content":[{{"type":"text","text":"msg {i}"}}]}}}}"#
        ));
        body.push('\n');
    }
    h.write(&format!("{enc}/{sess}.jsonl"), &body);
    let at = format!("@{sess}");
    let l = h.run(&["list", &at, "--no-subagents", "--format", "json"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert_eq!(
        json_summary(&l.stdout)["skipped_lines"],
        0,
        "the mid-file tear sits outside list's windows (disclosed design): {}",
        l.stdout
    );
    let s = h.run(&["stats", &at, "--no-subagents", "--format", "json"]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert_eq!(
        json_summary(&s.stdout)["skipped_lines"],
        1,
        "stats full-scans and must see the tear: {}",
        s.stdout
    );
}

#[test]
fn verbatim_header_carries_budget_accounting_in_json_and_spanned_of_total_in_text() {
    // R10: `spanned N compaction boundaries` read as a TRANSCRIPT property when it is a
    // QUERY property (budget-window-relative) — the text now prints `spanned K of N … in
    // scope`, and the JSON header carries the full budget accounting the text header
    // shows (the machine format must never be thinner than the human one).
    let h = populated_home();
    let out = h.run(&["verbatim", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let header: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next().unwrap()).unwrap();
    for key in [
        "budget_chars",
        "round_trip_fraction",
        "chars_used",
        "boundaries_spanned",
        "boundaries_total",
        "selected_user",
        "selected_assistant",
    ] {
        assert!(!header[key].is_null(), "header must carry {key}: {header}");
    }
}

#[test]
fn stats_aggregates_are_exact() {
    // Mutation pin on the stats aggregation core: token sums per model, tool CALL
    // counts, the span label, and the JSON id trio must carry REAL values — an emptied
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
    // A subagent transcript so the MERGED (multi-transcript) rollup path renders too —
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
    // The asserted merged values are UNIQUE to the merged object (5+7 / 4+3 — no single
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
    // carries that number — a degraded += froze it with no test on the value.
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
fn gated_no_match_still_counts_malformed_exactly() {
    // Mutation pin on the malformed law's GATE path: a no-match literal query lets the
    // whole-file gate close every file WITHOUT building records — the gated accounting
    // must still report the exact malformed count (a degraded `+=` would drift it).
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "66778899-aabb-4000-8000-00000000000b";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"clean line"}}"#, "\n",
            r#"{"type":"user","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"torn"#, "\n", // crash-truncated candidate
            r#"free text garbage line"#, "\n", // non-candidate garbage
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"also clean"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzgatedmiss",
        &format!("@{sess}"),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let last: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next_back().unwrap()).unwrap();
    assert_eq!(last["matched"], serde_json::json!(0));
    assert_eq!(
        last["skipped_lines"],
        serde_json::json!(2),
        "exactly the torn candidate + the garbage line: {}",
        out.stdout
    );
}
