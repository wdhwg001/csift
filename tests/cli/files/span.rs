//! files subagent spanning and id-domain discrimination.

use crate::harness::*;

#[test]
fn files_spans_subagent_mutations() {
    // A subagent that Writes a file → its mutation is attributed under the session by
    // default (OMC fan-out edits happen in subagents). --no-subagents drops it.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub111.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub111","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"sub: write a file"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw1","name":"Write","input":{"file_path":"/tmp/subagent-out.md","content":"z"}}]}}"#, "\n",
        ),
    );
    let with = h.run(&["files", at(SESS).as_str()]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(
        with.stdout.contains("/tmp"),
        "subagent write spanned: {}",
        with.stdout
    );
    let without = h.run(&["files", at(SESS).as_str(), "--no-subagents"]);
    assert!(without.success, "stderr: {}", without.stderr);
    assert!(
        without.stdout.contains("no file mutations found"),
        "--no-subagents drops the subagent write: {}",
        without.stdout
    );
}

#[test]
fn files_default_spans_subagents_and_no_subagents_restricts() {
    let h = Home::new();
    subagents_only_scenario(&h);

    // Default (spans subagents): BOTH the parent and subagent files surface.
    let with = h.run(&["files", at(SESS).as_str(), "--by", "file"]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(with.stdout.contains("/parent/p.md"), "got: {}", with.stdout);
    assert!(with.stdout.contains("/sub/s.md"), "got: {}", with.stdout);

    // --no-subagents: ONLY the parent file (the subagent mutation drops out).
    let top = h.run(&["files", at(SESS).as_str(), "--by", "file", "--no-subagents"]);
    assert!(top.success, "stderr: {}", top.stderr);
    assert!(top.stdout.contains("/parent/p.md"), "got: {}", top.stdout);
    assert!(
        !top.stdout.contains("/sub/s.md"),
        "--no-subagents must exclude the subagent file: {}",
        top.stdout
    );
}

#[test]
fn files_timeline_json_marks_subagent_rows_with_refeedable_parent() {
    // The timeline JSON discriminates the id-domain: a subagent row carries is_subagent=true
    // + the re-feedable parent uuid; a top-level row carries is_subagent=false and
    // parent_session_id == session_id (so a consumer can always `csift turns <parent>`).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "timeline",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let parent_row = objs
        .iter()
        .find(|o| o["path"] == "/parent/p.md")
        .expect("parent row present");
    assert_eq!(parent_row["is_subagent"], serde_json::json!(false));
    assert_eq!(parent_row["session_id"], serde_json::json!(SESS));
    assert_eq!(parent_row["parent_session_id"], serde_json::json!(SESS));

    let sub_row = objs
        .iter()
        .find(|o| o["path"] == "/sub/s.md")
        .expect("subagent row present");
    assert_eq!(sub_row["is_subagent"], serde_json::json!(true));
    assert_eq!(sub_row["session_id"], serde_json::json!("sub111"));
    // The hex session_id is NOT re-feedable; the parent uuid IS.
    assert_eq!(sub_row["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn files_grouped_json_and_text_discriminate_subagent_id_domain() {
    // The r6 id-domain fix extends is_subagent + parent_session_id to the GROUPED views
    // (not just --timeline): a --by-file subagent row carries the discriminator in JSON and
    // is branded `SUBAGENT <hex> · parent SESSION <uuid>` in text (never a bare-hex SESSION).
    let h = Home::new();
    subagents_only_scenario(&h);

    let j = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "file",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs = json_lines(&j.stdout);
    let sub = objs
        .iter()
        .find(|o| o["kind"] == "file" && o["path"] == "/sub/s.md")
        .expect("subagent grouped row present");
    assert_eq!(sub["is_subagent"], serde_json::json!(true));
    assert_eq!(sub["session_id"], serde_json::json!("sub111"));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));
    let parent = objs
        .iter()
        .find(|o| o["kind"] == "file" && o["path"] == "/parent/p.md")
        .expect("parent grouped row present");
    assert_eq!(parent["is_subagent"], serde_json::json!(false));
    assert_eq!(parent["parent_session_id"], serde_json::json!(SESS));

    let t = h.run(&["files", at(SESS).as_str(), "--by", "file"]);
    assert!(t.success, "stderr: {}", t.stderr);
    // The subagent group's header is branded SUBAGENT + the re-feedable parent uuid.
    assert!(
        t.stdout
            .contains(&format!("SUBAGENT sub111  ·  parent SESSION {SESS}")),
        "subagent group not branded: {}",
        t.stdout
    );
    // The parent group's header keeps the plain SESSION <uuid> form.
    assert!(
        t.stdout.contains(&format!("SESSION {SESS}")),
        "top-level group lost its SESSION header: {}",
        t.stdout
    );
}
