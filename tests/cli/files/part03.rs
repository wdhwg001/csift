use crate::harness::*;

#[test]
fn recover_patches_boundary_only_session_still_renders() {
    // A session whose ONLY event is a Bash mutation on the target (no prior read) produces a
    // boundary but NO segment → `segments.is_empty() && boundaries.is_empty()` is
    // `true && false` (the second operand FALSE side), so the session is NOT skipped and the
    // lone boundary is rendered.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/bo.rs"}}]}}"#, "\n",
        ),
    );
    // Text patches: the boundary is shown even though no segment exists.
    let text = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bo.rs",
        "--patches",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("INTEGRITY BOUNDARY") && text.stdout.contains("HEURISTIC"),
        "the lone boundary renders without a segment: {}",
        text.stdout
    );
    // JSON patches: a boundary object is emitted (same non-skip second-operand path).
    let js = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bo.rs",
        "--patches",
        "--format",
        "json",
    ]);
    assert!(js.success, "stderr: {}", js.stderr);
    let has_boundary = js
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|o| o.get("kind").and_then(|v| v.as_str()) == Some("boundary"));
    assert!(has_boundary, "a boundary object is emitted: {}", js.stdout);
}

#[test]
fn turns_spans_at_least_two_compaction_boundaries() {
    // THE HEADLINE: a 40K budget over the 3-summary fixture must span >= 2 boundaries,
    // and at least one selected unit must come from before the 2nd-newest summary
    // (compactions_before >= 2). Asserted on the compiled binary's JSON over real-shaped
    // committed data.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let boundaries = objs
        .iter()
        .filter(|o| o["kind"] == "compaction_boundary")
        .count();
    assert!(
        boundaries >= 2,
        "must span >=2 compaction boundaries, got {boundaries}"
    );
    let deep = objs
        .iter()
        .filter(|o| o.get("role").is_some())
        .any(|o| o["compactions_before"].as_u64().unwrap_or(0) >= 2);
    assert!(
        deep,
        "at least one unit must predate the 2nd-newest summary"
    );
    // Each boundary record carries a line_no + summary_chars.
    for o in objs.iter().filter(|o| o["kind"] == "compaction_boundary") {
        assert!(o["line"].as_u64().unwrap() > 0);
        assert!(o["summary_chars"].as_u64().unwrap() > 0);
    }
}
