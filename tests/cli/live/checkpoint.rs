//! status/wait: the cost-ledger checkpoint as evidence, and the task store a clear
//! left behind.

use crate::harness::*;

/// A cost-ledger checkpoint line. `close_ms` is the instant the ledger closed, which a
/// `/clear` join reads as `startTime + totalDuration`.
fn checkpoint(session: &str, close_ms: i64) -> String {
    format!(
        r#"{{"type":"cost-state","sessionId":"{session}","totalCostUSD":0.25,"totalAPIDuration":90,"totalAPIDurationWithoutRetries":80,"totalToolDuration":9,"totalLinesAdded":2,"totalLinesRemoved":0,"totalDuration":60000,"startTime":{},"modelUsage":{{}},"hasUnknownModelCost":false}}"#,
        close_ms - 60_000
    )
}

const EOT_MAIN: &str = concat!(
    r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the shoals"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"charted; stopping"}]}}"#,
    "\n",
);

fn verdict_row(out: &Output) -> serde_json::Value {
    out.stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "verdict")
        .expect("verdict row")
}

#[test]
fn a_checkpoint_at_the_tail_is_evidence_and_never_a_verdict() {
    let h = Home::new();
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        &format!("{EOT_MAIN}{}\n", checkpoint(LIVE_SESS, 1_780_809_000_000)),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "checkpoint cost-state at L3 (written by the harness at a clear, a background \
             handover, an in-app resume or an exit; nothing appended since)"
        ),
        "the checkpoint evidence row names the line and what writes one:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("verdict  idle-eot"),
        "the verdict set is closed - a checkpoint never adds one:\n{}",
        out.stdout
    );
    // No registry row AND a checkpoint: the note gains its clause.
    assert!(
        out.stdout.contains(
            "no registry row for this session (not currently registered, or an older Claude \
             Code) - verdict from tail + children evidence, and the harness wrote its \
             checkpoint, so the session closed or was handed over"
        ),
        "the no-registry note gains the checkpoint clause:\n{}",
        out.stdout
    );
    let row = verdict_row(&h.run(&["status", &at(LIVE_SESS), "--format", "json"]));
    assert_eq!(row["last_checkpoint"]["line"], 3, "{row}");
    assert_eq!(row["last_checkpoint"]["kind"], "cost-state", "{row}");
    assert_eq!(row["verdict"], "idle-eot", "{row}");
}

#[test]
fn a_checkpoint_that_is_not_the_last_line_is_not_evidence() {
    let h = Home::new();
    // The checkpoint sits mid-file (a background handover, an in-app resume) and the
    // session kept going: there is nothing to report.
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        &format!(
            "{EOT_MAIN}{}\n{}\n",
            checkpoint(LIVE_SESS, 1_780_809_000_000),
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"carried on"}]}}"#
        ),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        !out.stdout.contains("checkpoint"),
        "a mid-file checkpoint is not a tail checkpoint:\n{}",
        out.stdout
    );
    let row = verdict_row(&h.run(&["status", &at(LIVE_SESS), "--format", "json"]));
    assert!(row["last_checkpoint"].is_null(), "{row}");
    assert!(
        !row["notes"].to_string().contains("wrote its checkpoint"),
        "no clause without a tail checkpoint: {row}"
    );
}

#[test]
fn the_tasks_store_is_found_through_the_session_own_id() {
    let h = Home::new();
    live_eot_main(&h);
    h.write_claude(
        &format!("tasks/session-{}/1.json", &LIVE_SESS[..8]),
        r#"{"id":"1","subject":"Sound the channel","status":"in_progress"}"#,
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains(&format!(
            "tasks     store: session-{} (via own id)",
            &LIVE_SESS[..8]
        )),
        "the store names the candidate that found it:\n{}",
        out.stdout
    );
    let row = verdict_row(&h.run(&["status", &at(LIVE_SESS), "--format", "json"]));
    assert_eq!(row["tasks_stores"][0]["via"], "own id", "{row}");
}

#[test]
fn the_tasks_store_is_found_through_the_cleared_from_root() {
    let h = Home::new();
    // The root started the process and named the store; a clear minted LIVE_SESS and
    // left the store behind under the root's name.
    let root = "aaaaaaaa-cccc-4ddd-8eee-ffffffffff01";
    let wrapper_ms: i64 = 1_780_809_000_000;
    h.write(
        &format!("{LIVE_ENC}/{root}.jsonl"),
        &format!(
            "{}\n{}\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work before the clear"}}"#,
            checkpoint(root, wrapper_ms - 3)
        ),
    );
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"c1","isMeta":true,"timestamp":"2026-06-07T05:09:59.900Z","message":{"role":"user","content":"<local-command-caveat>Caveat</local-command-caveat>"}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<command-name>/clear</command-name>\n<command-args></command-args>"}}"#, "\n",
            r#"{"type":"user","uuid":"c3","parentUuid":"c2","timestamp":"2026-06-07T05:10:30.000Z","message":{"role":"user","content":"after the clear"}}"#, "\n",
            r#"{"type":"assistant","uuid":"c4","parentUuid":"c3","timestamp":"2026-06-07T05:10:35.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    h.write_claude(
        &format!("tasks/session-{}/7.json", &root[..8]),
        r#"{"id":"7","subject":"Mark the buoys","status":"pending"}"#,
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains(&format!(
            "tasks     store: session-{} (via cleared_from root)",
            &root[..8]
        )),
        "the cleared chain's root names the store:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("Mark the buoys"),
        "the recovered store's tasks are listed:\n{}",
        out.stdout
    );
}

#[test]
fn the_tasks_store_is_found_through_the_team_file() {
    let h = Home::new();
    live_eot_main(&h);
    // A resumed launch: the store is named after a startup id no transcript carries,
    // and only the team file written at that startup connects the two.
    h.write_session_registry(
        4242,
        &format!(
            r#"{{"pid":4242,"sessionId":"{LIVE_SESS}","status":"idle","statusUpdatedAt":1767000000000,"startedAt":1780000000000,"kind":"interactive"}}"#
        ),
    );
    h.write_claude(
        "teams/session-deadbeef/config.json",
        r#"{"name":"session-deadbeef","createdAt":1780000001500,"leadSessionId":"deadbeef-1111-4222-8333-444444444444","members":[]}"#,
    );
    // A team file outside the window is never read.
    h.write_claude(
        "teams/session-facefeed/config.json",
        r#"{"name":"session-facefeed","createdAt":1780000009000,"leadSessionId":"facefeed-1111-4222-8333-444444444444","members":[]}"#,
    );
    h.write_claude(
        "tasks/session-deadbeef/2.json",
        r#"{"id":"2","subject":"Log the tide","status":"in_progress"}"#,
    );
    h.write_claude(
        "tasks/session-facefeed/3.json",
        r#"{"id":"3","subject":"Never read","status":"pending"}"#,
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout
            .contains("tasks     store: session-deadbeef (via team file)"),
        "the team file written at startup names the store:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("Never read"),
        "a team file outside the window is never read:\n{}",
        out.stdout
    );
}
