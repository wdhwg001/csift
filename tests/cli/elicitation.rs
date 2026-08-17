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
