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
