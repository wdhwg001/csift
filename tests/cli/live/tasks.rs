//! status end to end: the settled-children fold and the harness task list.

use crate::harness::*;

#[test]
fn p13_settled_children_fold_and_tasks_section() {
    let h = Home::new();
    live_eot_main(&h);
    // Two settled child lanes (ancient paired tails ending in end_turn).
    for hexid in ["a1b2c3d4e5f60718", "b2c3d4e5f6071829"] {
        h.write(
            &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-{hexid}.jsonl"),
            concat!(
                r#"{"type":"assistant","uuid":"sa1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ct1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
                r#"{"type":"user","uuid":"sr1","parentUuid":"sa1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ct1","content":"done"}]}}"#, "\n",
                r#"{"type":"assistant","uuid":"sa2","parentUuid":"sr1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"child done"}]}}"#, "\n",
            ),
        );
    }
    // Tasks under BOTH directory-name forms (they merge into one list).
    h.write_claude(
        &format!("tasks/{LIVE_SESS}/1.json"),
        r#"{"id":"1","subject":"Chart the reef","status":"completed"}"#,
    );
    h.write_claude(
        &format!("tasks/{LIVE_SESS}/3.json"),
        r#"{"id":"3","subject":"Mark the buoys","status":"pending","blockedBy":["2"]}"#,
    );
    h.write_claude(
        &format!("tasks/session-{}/2.json", &LIVE_SESS[..8]),
        r#"{"id":"2","subject":"Sound the channel","status":"in_progress","blockedBy":[]}"#,
    );
    h.write_claude(
        &format!("tasks/session-{}/4.json", &LIVE_SESS[..8]),
        r#"{"id":"4","subject":"Log the tide","status":"completed"}"#,
    );
    // Decoys: a non-json file, a malformed json, and a NUMERIC-id task - all
    // tolerated (the first two silently, the number rendered as a string id).
    h.write_claude(&format!("tasks/{LIVE_SESS}/notes.txt"), "not a task");
    h.write_claude(&format!("tasks/{LIVE_SESS}/9.json"), "{broken");
    h.write_claude(
        &format!("tasks/{LIVE_SESS}/5.json"),
        r#"{"id":5,"subject":"Refit the hull","status":"completed"}"#,
    );
    // A numeric-id OPEN task renders its number and sorts NUMERICALLY (12 after 3,
    // where a lexicographic order would put "12" first).
    h.write_claude(
        &format!("tasks/{LIVE_SESS}/12.json"),
        r#"{"id":12,"subject":"Chart the drift","status":"pending"}"#,
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Settled lanes fold to a count line; no per-lane settled rows survive.
    assert!(
        out.stdout.contains("2 settled lane(s) folded"),
        "fold line:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("settled  last record"),
        "no per-lane settled rows:\n{}",
        out.stdout
    );
    // Tasks: in_progress leads, the blocked pending row names its blocker, completed
    // folds to the summary count.
    let i2 = out.stdout.find("#2 in_progress  Sound the channel");
    let i3 = out
        .stdout
        .find("#3 pending  Mark the buoys  (blocked by #2)");
    assert!(
        i2.is_some() && i3.is_some() && i2 < i3,
        "task rows ordered in_progress-first:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("tasks     3 open ; 3 completed"),
        "tasks summary (decoys skipped, numeric ids counted):\n{}",
        out.stdout
    );
    let i12 = out.stdout.find("#12 pending  Chart the drift");
    assert!(
        i12.is_some() && i3 < i12,
        "numeric id order (3 before 12):\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("Chart the reef"),
        "completed tasks never render as rows:\n{}",
        out.stdout
    );

    // JSON: children carries only live lanes, the fold count and tasks ride the row.
    let j = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "verdict")
        .expect("verdict row");
    assert_eq!(
        row["children"].as_array().map(Vec::len),
        Some(0),
        "{}",
        j.stdout
    );
    assert_eq!(row["settled_children"], 2, "{}", j.stdout);
    assert_eq!(
        row["tasks"].as_array().map(Vec::len),
        Some(3),
        "{}",
        j.stdout
    );
    assert_eq!(row["tasks"][0]["id"], "2", "{}", j.stdout);
    assert_eq!(row["tasks"][1]["blocked_by"][0], "2", "{}", j.stdout);
    assert_eq!(
        row["tasks"][2]["id"], "12",
        "numeric id renders: {}",
        j.stdout
    );
    assert_eq!(row["tasks_completed"], 3, "{}", j.stdout);
}

#[test]
fn p14_no_tasks_dir_means_null_not_empty() {
    // A tasks dir that EXISTS but holds nothing: the text section stays silent
    // (an all-zero line is noise), while JSON below keeps the found distinction.
    let h_empty = Home::new();
    live_eot_main(&h_empty);
    std::fs::create_dir_all(h_empty.root.join(".claude/tasks").join(LIVE_SESS)).unwrap();
    let out_empty = h_empty.run(&["status", &at(LIVE_SESS)]);
    assert!(
        !out_empty.stdout.contains("tasks "),
        "an empty dir prints no tasks section:\n{}",
        out_empty.stdout
    );

    let h = Home::new();
    live_eot_main(&h);
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        !out.stdout.contains("tasks "),
        "no tasks section without a tasks dir:\n{}",
        out.stdout
    );
    let j = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "verdict")
        .expect("verdict row");
    assert!(row["tasks"].is_null(), "{}", j.stdout);
    assert!(row["tasks_completed"].is_null(), "{}", j.stdout);
}

#[test]
fn p15_completed_only_tasks_still_summarize() {
    // A dir holding ONLY completed tasks: no rows, but the summary line shows.
    let h = Home::new();
    live_eot_main(&h);
    h.write_claude(
        &format!("tasks/{LIVE_SESS}/1.json"),
        r#"{"id":"1","subject":"Moor the skiff","status":"completed"}"#,
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("tasks     0 open ; 1 completed"),
        "completed-only dirs still summarize:\n{}",
        out.stdout
    );
}
