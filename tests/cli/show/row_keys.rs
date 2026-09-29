//! A `show` row answers the keys its `search` hit answers, pinned where the value is NOT
//! null: `scheduled_at` on a fired prompt and `resume_paired` on a paired resume placeholder.
//! The shared-projector test compares every key on a pulse, where both of these read null,
//! so a row that dropped either would still pass it.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";

/// The one hit a `search --format json` run returns for `line`, and the one `show` row.
fn hit_and_row(
    h: &Home,
    sess: &str,
    leaf: &str,
    line: u64,
) -> (serde_json::Value, serde_json::Value) {
    let js = h.run(&["search", "", &at(sess), "-t", leaf, "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    let hit = json_rows(&js.stdout, "exchange")
        .into_iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .find(|h| h["line"] == line)
        .expect("the hit");
    let shown = h.run(&[
        "show",
        &at(sess),
        "--line",
        &line.to_string(),
        "--format",
        "json",
    ]);
    assert!(shown.success, "stderr: {}", shown.stderr);
    let row = json_rows(&shown.stdout, "record")
        .into_iter()
        .next()
        .expect("the row");
    (hit, row)
}

#[test]
fn a_fired_prompt_s_row_carries_its_instant() {
    let h = Home::new();
    let sess = "7a6b5c4d-3e2f-4a1b-9c8d-000000000140";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000141","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"arm it"}}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000142","parentUuid":"00000000-0000-4000-8000-000000000141","timestamp":"2026-06-07T05:28:00.100Z","content":"Running scheduled task (Jun 7 5:28am)"}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000143","parentUuid":"00000000-0000-4000-8000-000000000142","timestamp":"2026-06-07T05:28:00.200Z","isMeta":true,"promptSource":"system","message":{"role":"user","content":"zzfired prompt"}}"#, "\n",
        ),
    );
    let (hit, row) = hit_and_row(&h, sess, "harness.schedule.fire", 3);
    assert_eq!(
        hit["scheduled_at"],
        serde_json::json!("Jun 7 5:28am"),
        "{hit}"
    );
    assert_eq!(row["scheduled_at"], hit["scheduled_at"], "{row}");
}

#[test]
fn a_paired_placeholder_s_row_says_it_closes_the_pair() {
    let h = Home::new();
    let sess = "5c4b3a29-1e0d-4c3b-9a87-000000000140";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"user","uuid":"p1","parentUuid":"u1","isMeta":true,"timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"h1","parentUuid":"p1","isApiErrorMessage":false,"timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"text","text":"No response requested."}]}}"#, "\n",
        ),
    );
    let (hit, row) = hit_and_row(&h, sess, "harness.resume.placeholder", 3);
    assert_eq!(hit["resume_paired"], serde_json::json!(true), "{hit}");
    assert_eq!(row["resume_paired"], hit["resume_paired"], "{row}");
}
