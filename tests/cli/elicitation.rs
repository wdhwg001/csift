use crate::harness::*;

#[test]
fn sidecar_schema_skewed_marker_is_counted_never_invisible() {
    // R12 §2: a sentinel-bearing sidecar line the CURRENT schema cannot read (a
    // pre-release fossil: `phase`/`kind`/`key` instead of `csiftPhase`/…) used to be
    // fully invisible — correctly never merged, but not counted either. It now moves
    // `skipped_lines` on every sidecar-merging surface (valid-JSON-ness ≠ silence).
    let h = Home::new();
    let enc = "-Users-test-Projects-fossil";
    let sess = "eeeeeeee-aaaa-4bbb-8ccc-00000000f055";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"q"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
            "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/elicitations.jsonl"),
        concat!(
            r#"{"type":"csift-elicitation","csift":"elicitation-marker-v1","phase":"pending","kind":"AskUserQuestion","key":"toolu_fossil","sessionId":"eeeeeeee-aaaa-4bbb-8ccc-00000000f055"}"#,
            "\n",
        ),
    );
    let at = format!("@{sess}");
    let l = h.run(&["list", &at, "--no-subagents", "--format", "json"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert_eq!(
        json_summary(&l.stdout)["skipped_lines"],
        1,
        "the fossil marker must move the counter: {}",
        l.stdout
    );
    let rows = json_rows(&l.stdout, "session");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["sidecar_present"], true, "{rows:?}");
    assert_eq!(
        rows[0]["pending_elicitations"].as_array().map(Vec::len),
        Some(0),
        "a fossil never merges as pending: {rows:?}"
    );
    let s = h.run(&["search", "", &at, "--no-subagents", "--format", "json"]);
    assert!(s.success, "stderr: {}", s.stderr);
    let sum = json_summary(&s.stdout);
    assert_eq!(
        sum["skipped_lines"], 1,
        "search folds the sidecar skip in: {}",
        s.stdout
    );
    assert_eq!(
        sum["with_elicitation_sidecar"], false,
        "nothing merged — only counted: {}",
        s.stdout
    );
}

#[test]
fn auq_answer_opens_a_turn_and_surfaces_clean_answer() {
    let h = holes_home();
    // search -t user for the answer prose: it must surface under `user`.
    let out = h.run(&[
        "search",
        "option A is fine",
        "-t",
        "user",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hit_line = out
        .stdout
        .lines()
        .find(|l| l.contains("option A is fine"))
        .unwrap_or_else(|| panic!("AUQ answer not surfaced under user:\n{}", out.stdout));
    let v: serde_json::Value = serde_json::from_str(hit_line).unwrap();
    // It is a genuine-user turn boundary now → turn_index 1 (after the "start" opener).
    assert_eq!(
        v.get("turn_index").and_then(serde_json::Value::as_u64),
        Some(1),
        "AUQ answer must open turn 1: {hit_line}"
    );
}

#[test]
fn ghost_guard_is_structural_and_mcp_exempt() {
    // Mutation pins on the ghost guard: (a) a key natively closed by a REAL tool_use id is
    // dropped; (b) a key merely QUOTED in prose is NOT closed (structural, not substring);
    // (c) an MCP pending is EXEMPT even when its key matches a native tool id.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the prosek01 token appears in prose only, and closedk01 is also quoted here first"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"closedk01","name":"AskUserQuestion","input":{"questions":[{"question":"native q"}]}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"mcpk01","name":"Bash","input":{"command":"echo hi"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n{}\n{}\n",
            auq_pending_line("closedk01", "2026-06-07T05:01:00.000Z", "zzclosed question"),
            auq_pending_line("prosek01", "2026-06-07T05:02:00.000Z", "zzprose question"),
            mcp_pending_line(
                "mcpk01",
                "2026-06-07T05:03:00.000Z",
                "gdrive",
                "authorize zzmcp"
            )
        ),
    );
    let prose = h.run(&["search", "zzprose", &at(SESS)]);
    assert!(
        prose.stdout.contains("zzprose"),
        "prose-quoted key stays pending (structural guard):\n{}",
        prose.stdout
    );
    let closed = h.run(&["search", "zzclosed", &at(SESS)]);
    assert!(
        closed.stdout.contains("no matching exchanges"),
        "natively-closed key is dropped:\n{}",
        closed.stdout
    );
    let mcp = h.run(&["search", "zzmcp", &at(SESS)]);
    assert!(
        mcp.stdout.contains("zzmcp"),
        "MCP pending is exempt from the native guard:\n{}",
        mcp.stdout
    );
}

#[test]
fn epm_pending_renders_kind_and_plan_body() {
    // Mutation pins on pending_text: the ExitPlanMode arm renders "ExitPlanMode: <plan>",
    // an EMPTY plan renders the bare kind (the !b.is_empty() guard), and plan_text reads
    // the REAL input.plan (never a fabricated body).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
        ),
    );
    let epm = |key: &str, ts: &str, plan: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"e-{key}","timestamp":"{ts}","sessionId":"{SESS}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"{key}","name":"ExitPlanMode","input":{{"plan":"{plan}"}}}}]}},"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"ExitPlanMode","csiftKey":"{key}"}}"#
        )
    };
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n{}\n",
            epm("epmk01", "2026-06-07T05:01:00.000Z", "Beacon rollout plan"),
            epm("epmk02", "2026-06-07T05:02:00.000Z", "")
        ),
    );
    let o = h.run(&["verbatim", &at(SESS)]);
    assert!(o.success, "stderr: {}", o.stderr);
    assert!(
        o.stdout.contains("ExitPlanMode: Beacon rollout plan"),
        "plan body rendered:\n{}",
        o.stdout
    );
    assert_eq!(
        o.stdout.matches("ExitPlanMode:").count(),
        1,
        "empty plan renders the bare kind (no colon):\n{}",
        o.stdout
    );
}

#[test]
fn elicitation_ghost_pending_dropped_when_natively_closed() {
    // R7 §3 (the ghost-pending guard): Claude Code fires NO PostToolUse for a REJECTED
    // AUQ/ExitPlanMode, so the hook never writes its `resolved` marker — sidecar-internal
    // pairing alone would report the elicitation pending FOREVER while the native transcript
    // long since holds the flushed tool_use + rejection tool_result. The native record
    // outranks the sidecar: the ghost is dropped like a resolved pair (and never duplicated
    // beside the native record in search).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_GHOST","name":"AskUserQuestion","input":{"questions":[{"question":"Deploy now?"}]}}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a1","timestamp":"2026-06-07T05:01:10.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_GHOST","content":"The user doesn't want to proceed with this tool use. The tool use was rejected.","is_error":true}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line("toolu_GHOST", "2026-06-07T05:00:55.000Z", "Deploy now?")
        ),
    );

    // search: the native record surfaces; the sidecar ghost does NOT (no duplicate).
    let j = h.run(&[
        "search",
        "",
        &at(SESS),
        "--no-subagents",
        "-t",
        "agent.tool.use",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let hits: Vec<serde_json::Value> = json_rows(&j.stdout, "exchange")
        .into_iter()
        .flat_map(|ex| ex["hits"].as_array().unwrap().clone())
        .collect();
    assert!(
        hits.iter()
            .any(|h| h["tool_use_id"] == "toolu_GHOST" && h["source"].is_null()),
        "the native record must surface: {hits:?}"
    );
    assert!(
        !hits.iter().any(|h| h["source"] == "elicitation-sidecar"),
        "the natively-closed ghost must be dropped, never merged as a duplicate: {hits:?}"
    );

    // list: sidecar detected, but NOTHING reported pending.
    let lj = h.run(&["list", &at(SESS), "--no-subagents", "--format", "json"]);
    assert!(lj.success, "stderr: {}", lj.stderr);
    let row = json_rows(&lj.stdout, "session").remove(0);
    assert_eq!(row["sidecar_present"], true);
    assert!(
        row["pending_elicitations"].as_array().unwrap().is_empty(),
        "a natively-closed elicitation must not report as pending: {row}"
    );
}

#[test]
fn elicitation_pending_kept_when_key_only_quoted_in_prose() {
    // The ghost guard is STRUCTURAL: the key appearing inside another record's TEXT (a Bash
    // command grepping for it) is not closure evidence — a genuinely-open elicitation whose
    // id someone merely quoted must stay pending.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_OTHER","name":"Bash","input":{"command":"grep toolu_STILLOPEN session.jsonl"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_STILLOPEN",
                "2026-06-07T05:02:00.000Z",
                "Which branch?"
            )
        ),
    );
    let lj = h.run(&["list", &at(SESS), "--no-subagents", "--format", "json"]);
    assert!(lj.success, "stderr: {}", lj.stderr);
    let row = json_rows(&lj.stdout, "session").remove(0);
    assert_eq!(
        row["pending_elicitations"].as_array().unwrap().len(),
        1,
        "a prose quote of the key is NOT closure — must stay pending: {row}"
    );
}

#[test]
fn turns_includes_pending_askuserquestion() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Pick a deployment target"
            )
        ),
    );
    let out = h.run(&["verbatim", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("AskUserQuestion: Pick a deployment target"),
        "turns must include the pending AUQ as a unit:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(elicitation sidecar)"),
        "the pending unit's locator must be `(elicitation sidecar)`:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "the merged-records note must appear:\n{}",
        out.stdout
    );

    // JSON header flags it; the unit carries source + null line_no.
    let j = h.run(&["verbatim", &at(SESS), "--format", "json"]);
    let header: serde_json::Value = serde_json::from_str(j.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(header["with_elicitation_sidecar"], true);
    let unit = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["source"] == "elicitation-sidecar")
        .expect("a sidecar unit object");
    assert!(
        unit["line"].is_null(),
        "sidecar unit has null line_no: {unit}"
    );
}

#[test]
fn mcp_pending_is_merged_into_turns() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            mcp_pending_line(
                "el-9",
                "2026-06-27T01:10:00.000Z",
                "gdrive",
                "Authorize Google Drive access"
            )
        ),
    );
    let out = h.run(&["verbatim", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("mcp-elicitation: [gdrive]"),
        "turns must include the pending MCP elicitation:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "the merged-records note must appear:\n{}",
        out.stdout
    );
}

#[test]
fn verbatim_no_compaction_note_and_list_sidecar_tristate() {
    // W2-8: `verbatim` on a session with ZERO compactions self-diagnoses (stderr) and
    // points at `show --turn` — the tail-peek misuse correction; --slice (the hook path)
    // stays quiet. W2-9: list rows carry the sidecar TRI-STATE (`sidecar_present`).
    let h = populated_home();
    let at = format!("@{SESS}");

    let out = h.run(&["verbatim", &at]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("has no compaction"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("csift show @"),
        "the note names the correct command: {}",
        out.stderr
    );

    let out = h.run(&["verbatim", &at, "--slice", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("has no compaction"),
        "--slice must stay quiet (hook path): {}",
        out.stderr
    );

    // Tri-state ①: no sidecar file → present:false (hook unknown — cannot conclude).
    let out = h.run(&["list", &at, "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert!(
        rows.iter().all(|r| r["sidecar_present"] == false),
        "{rows:?}"
    );

    // Tri-state ②: a sidecar with only a RESOLVED pair (nothing pending) → present:true,
    // with_elicitation_sidecar:false — "hook installed AND not blocked" is now assertable.
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        concat!(
            r#"{"type":"csift-elicitation-resolved","uuid":"r1","timestamp":"2026-06-07T05:00:10.000Z","#,
            r#""sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","csift":"elicitation-marker-v1","#,
            r#""csiftPhase":"resolved","csiftKind":"AskUserQuestion","csiftKey":"k1"}"#,
            "\n",
        ),
    );
    let out = h.run(&["list", &at, "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    let top: Vec<_> = rows.iter().filter(|r| r["is_subagent"] == false).collect();
    assert!(!top.is_empty());
    assert!(top.iter().all(|r| r["sidecar_present"] == true), "{rows:?}");
    assert!(
        top.iter().all(|r| r["with_elicitation_sidecar"] == false),
        "resolved-only sidecar has nothing pending: {rows:?}"
    );
}

#[test]
fn search_finds_unresolved_askuserquestion_via_sidecar() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Which branch should I target?"
            )
        ),
    );

    // TEXT — the pending AUQ is found and marked `(elicitation sidecar)` (no fake Lnnnn).
    let out = h.run(&["search", "Which branch should I target", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(elicitation sidecar)"),
        "a sidecar hit must render `(elicitation sidecar)`, not Lnnnn:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "the merged-records note must appear:\n{}",
        out.stdout
    );

    // JSON — the hit carries source:"elicitation-sidecar", null line; summary flags it.
    let j = h.run(&[
        "search",
        "Which branch should I target",
        &at(SESS),
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let lines: Vec<&str> = j.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    let ex = json_rows(&j.stdout, "exchange").remove(0);
    let hit = &ex["hits"][0];
    assert_eq!(hit["source"], "elicitation-sidecar");
    assert!(
        hit["line"].is_null(),
        "no fabricated line for a sidecar hit: {hit}"
    );
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(summary["with_elicitation_sidecar"], true);
}

#[test]
fn malformed_sidecar_line_is_skipped_and_counted_in_search() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "BROKEN {{ not json\n{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Which branch should I target?"
            )
        ),
    );
    let j = h.run(&[
        "search",
        "Which branch should I target",
        &at(SESS),
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let summary: serde_json::Value = serde_json::from_str(
        j.stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        summary["skipped_lines"], 1,
        "a malformed sidecar line is counted, never silent"
    );
    assert_eq!(summary["matched"], 1, "the valid pending still merges");
}

#[test]
fn acceptance_mcp_elicitation_searchable_under_tool_use() {
    // §G8 — an MCP elicitation (UNATTESTED in the corpus → synthetic sidecar). The pending marker is a
    // `type:"system"` record with no tool_use block; the guarded §3.10 arm classifies it
    // `agent.tool.use` and matches its content, so `-t agent.tool.use` finds it, rendered
    // `(elicitation sidecar)` (no fabricated L) with the `with elicitation sidecar` note.
    let h = Home::new();
    h.write(
        &format!("{ACC_ENC}/{ACC_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/Users/x/acc","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the mcp flow"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"working"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ACC_ENC}/{ACC_SESS}/elicitations.jsonl"),
        concat!(
            r#"{"type":"system","subtype":"mcp_elicitation","uuid":"m-mcp1","timestamp":"2026-06-07T06:00:00.000Z","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","isSidechain":false,"content":"MCP elicitation [github] (url): zzmcp confirm the action","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"mcp1","csiftMcpServer":"github","hookInput":{}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "zzmcp", "-t", "agent.tool.use", &at(ACC_SESS)]);
    assert!(out.success, "G8: stderr {}", out.stderr);
    assert!(
        out.stdout.contains("agent.tool.use"),
        "G8 MCP pending marker → agent.tool.use:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(elicitation sidecar)")
            && out.stdout.contains("with elicitation sidecar"),
        "G8 must render the sidecar provenance:\n{}",
        out.stdout
    );
}

#[test]
fn list_shows_with_elicitation_sidecar() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Confirm the migration?"
            )
        ),
    );
    let out = h.run(&["list", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "list must annotate the pending session:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("AskUserQuestion: Confirm the migration?"),
        "list surfaces the pending kind:\n{}",
        out.stdout
    );

    let j = h.run(&["list", &at(SESS), "--format", "json"]);
    let row = json_rows(&j.stdout, "session").remove(0);
    assert_eq!(row["with_elicitation_sidecar"], true);
    assert!(row["pending_elicitations"].as_array().unwrap().len() == 1);
}
