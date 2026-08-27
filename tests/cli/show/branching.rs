//! show --branch-points forks + the compaction-boundary logicalParent excerpt.

use crate::harness::*;

#[test]
fn branch_points_reports_forks_and_excludes_tool_result_carriers() {
    // a1 has FOUR children by parentUuid: two parallel tool_result carriers (must not
    // count), one prompt 10s later, and one rewind re-attach 2h later. The one branch
    // point is a1 with 2 conversation children, widest gap 1h59m50s.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"working"},{"type":"tool_use","id":"t1","name":"Read","input":{}},{"type":"tool_use","id":"t2","name":"Read","input":{}}]}}"#, "\n",
            r#"{"type":"user","uuid":"tr1","parentUuid":"a1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"one"}]}}"#, "\n",
            r#"{"type":"user","uuid":"tr2","parentUuid":"a1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"two"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"first path"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"text","text":"down the first path"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"a1","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"rewound here"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1b","parentUuid":"u1","timestamp":"2026-06-07T05:00:50.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a retried opening"}]}}"#, "\n",
            "not json here", "\n",
        ),
    );
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("2 branch point(s)") && out.stdout.contains("uuid a1"),
        "the a1 rewind + the u1 assistant-retry fork:\n{}",
        out.stdout
    );
    // The retry fork's children are ASSISTANT records (a1 L2 + a1b L8, 45s apart):
    // assistant children count, and a sub-minute gap renders bare seconds.
    assert!(
        out.stdout.contains("uuid u1") && out.stdout.contains("widest gap 45s"),
        "assistant-retry fork with a seconds-scale gap:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(1 malformed line(s) skipped)"),
        "the torn tail line is booked:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("children 2"),
        "tool_result carriers excluded (else 4):\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("widest gap 1h59m50s"),
        "gap between L5 and L7 children:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("--line 7"),
        "refetch points at the latest child:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("does not guess"),
        "never classifies live/abandoned:\n{}",
        out.stdout
    );

    let outj = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "header");
    assert_eq!(rows[0]["mode"], "branch-points");
    let bp = rows
        .iter()
        .find(|r| r["kind"] == "branch-point")
        .expect("branch-point row");
    assert_eq!(bp["uuid"], "a1");
    assert_eq!(bp["line"], 2);
    assert_eq!(bp["widest_gap_seconds"], 7190);
    assert_eq!(bp["children"].as_array().unwrap().len(), 2);
    assert_eq!(bp["children"][0]["line"], 5);
    assert_eq!(bp["children"][1]["line"], 7);
    let summary = rows.last().unwrap();
    assert_eq!(summary["branch_points"], 2);
    assert_eq!(
        summary["conversation_records"], 6,
        "u1 a1 u2 a2 u3 a1b; carriers excluded: {}",
        outj.stdout
    );

    // A linear session reports an honest zero.
    let h2 = Home::new();
    h2.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"only"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    let lin = h2.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(lin.success);
    assert!(
        lin.stdout.contains("0 branch point(s)") && lin.stdout.contains("no forks"),
        "{}",
        lin.stdout
    );
    assert!(
        !lin.stdout.contains("malformed"),
        "a clean file prints no zero note: {}",
        lin.stdout
    );
}

#[test]
fn compaction_boundary_surfaces_logical_parent_uuid() {
    // The boundary record's parentUuid is null; logicalParentUuid names the record the
    // compaction re-links to. It rides the boundary's rendered excerpt (search + show).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"before"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"logicalParentUuid":"u1","timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":900,"postTokens":100,"durationMs":40}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "harness.compaction.boundary",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("[logicalParent=u1]"),
        "the boundary excerpt names the re-link target:\n{}",
        out.stdout
    );
    let shown = h.run(&["show", at(SESS).as_str(), "--line", "2"]);
    assert!(
        shown.stdout.contains("[logicalParent=u1]"),
        "show renders it too:\n{}",
        shown.stdout
    );
}
