//! list clone lineage: compaction-forked transcripts detected, origin joined.

use crate::harness::*;

#[test]
fn list_clone_lineage_detected_and_joined() {
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let parent = "aaaa1111-2222-4333-8444-555566667777";
    let clone = "bbbb1111-2222-4333-8444-555566667777";
    let coclone = "dddd1111-2222-4333-8444-555566667777";
    let bystander = "cccc1111-2222-4333-8444-555566667777";
    let bx = "0f1e2d3c-4b5a-4697-8807-16f5e4d3c2b1";
    let boundary = format!(
        r#"{{"type":"system","subtype":"compact_boundary","uuid":"{bx}","timestamp":"2026-06-07T05:10:00.000Z","content":"Compacted","compactMetadata":{{"trigger":"auto","preTokens":1000,"postTokens":100,"durationMs":5}}}}"#
    );
    // Parent: the boundary lives MID-FILE (the compaction actually happened here).
    h.write(
        &format!("{enc}/{parent}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the reef"}}}}"#, "\n",
                "{b}\n",
                r#"{{"type":"user","uuid":"u2","timestamp":"2026-06-07T05:11:00.000Z","message":{{"role":"user","content":"carry on"}}}}"#, "\n",
            ),
            b = boundary
        ),
    );
    // Clone + co-clone: an untimestamped bookkeeping line, then the SAME boundary as
    // the first timestamped record, then own turns.
    for id in [clone, coclone] {
        h.write(
            &format!("{enc}/{id}.jsonl"),
            &format!(
                concat!(
                    r#"{{"type":"ai-title","title":"copied lane"}}"#, "\n",
                    "{b}\n",
                    r#"{{"type":"user","uuid":"u3","timestamp":"2026-06-07T05:12:00.000Z","message":{{"role":"user","content":"forked work"}}}}"#, "\n",
                ),
                b = boundary
            ),
        );
    }
    // Bystander: quotes the boundary uuid in PROSE only - never an origin.
    h.write(
        &format!("{enc}/{bystander}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"u4","timestamp":"2026-06-07T05:13:00.000Z","message":{{"role":"user","content":"looking at boundary {bx} from another session"}}}}"#
            )
        ),
    );
    let out = h.run(&["list", &format!("@{clone}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!(
            "forked from SESSION {parent} at compaction boundary {bx}"
        )),
        "clone line joins to the origin:\n{}",
        out.stdout
    );
    let j = h.run(&["list", &format!("@{clone}"), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(row["is_clone"], true, "{}", j.stdout);
    assert_eq!(
        row["clone_of"], parent,
        "co-clone and prose quote never join: {}",
        j.stdout
    );
    assert_eq!(row["clone_boundary_uuid"], bx, "{}", j.stdout);
    // The parent itself is NOT a clone.
    let pj = h.run(&["list", &format!("@{parent}"), "--format", "json"]);
    let prow: serde_json::Value = pj
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(prow["is_clone"], false, "{}", pj.stdout);
    assert!(prow["clone_of"].is_null(), "{}", pj.stdout);
    assert!(
        !pj.stdout.contains("forked from"),
        "no clone line on the origin:\n{}",
        pj.stdout
    );
    // Clean fixtures: the malformed-line note never prints for a zero count.
    assert!(
        !out.stdout.contains("malformed"),
        "no zero-count malformed note:\n{}",
        out.stdout
    );
}

#[test]
fn list_clone_without_origin_discloses_honestly() {
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let clone = "eeee1111-2222-4333-8444-555566667777";
    h.write(
        &format!("{enc}/{clone}.jsonl"),
        concat!(
            r#"{"type":"system","subtype":"compact_boundary","uuid":"9a8b7c6d-5e4f-4321-8765-43210fedcba9","timestamp":"2026-06-07T05:10:00.000Z","content":"Compacted"}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:12:00.000Z","message":{"role":"user","content":"orphan fork"}}"#, "\n",
        ),
    );
    let out = h.run(&["list", &format!("@{clone}")]);
    assert!(
        out.stdout.contains("origin not in this"),
        "unjoined clone discloses:\n{}",
        out.stdout
    );
    let j = h.run(&["list", &format!("@{clone}"), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(row["is_clone"], true, "{}", j.stdout);
    assert!(row["clone_of"].is_null(), "{}", j.stdout);
}
