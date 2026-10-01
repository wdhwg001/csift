//! The elicitation sidecar merged into `verbatim` (section 3.10): a pending question becomes
//! its own unit on the turn's assistant side, an MCP elicitation joins it, and the sidecar's
//! unreadable lines are added to the transcript's own count. The other surfaces' merge tests
//! live in `elicitation`; this file is split from it by surface, the `plan_*` precedent.

use crate::harness::*;

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
    // A unit's role is the role of the record it renders: the pending line stands for the
    // ASSISTANT's AskUserQuestion `tool_use`, so it renders under the assistant's glyph and
    // role and never as words the person said.
    assert!(
        out.stdout
            .contains("\u{25b3} (elicitation sidecar)  ASSISTANT"),
        "the pending question is the assistant's:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("(elicitation sidecar)  USER"),
        "the pending question must not read as the person's:\n{}",
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
    assert_eq!(unit["kind"], "turn", "{unit}");
    assert_eq!(
        unit["role"], "assistant",
        "the pending question is the assistant's: {unit}"
    );
    assert_eq!(unit["is_automation"], false, "{unit}");
    assert_eq!(
        unit["text"], "AskUserQuestion: Pick a deployment target",
        "the body still names the tool and the question: {unit}"
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
fn verbatim_adds_the_sidecars_unreadable_lines_to_the_transcripts_own_count() {
    // The sidecar is read from outside the transcript's bytes, so its unreadable lines are
    // an ADDITION to whatever the scan found - and here the transcript is clean, which is
    // exactly the case where a subtraction would go below zero instead of reporting one.
    let h = Home::new();
    let enc = "-Users-dev-example-sidecar";
    let sess = "4c3b2a19-8877-4665-8443-221100ffeedd";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/elicitations.jsonl"),
        "{ this line is not json\n",
    );
    let at = format!("@{sess}");
    let out = h.run(&["verbatim", &at]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 malformed line(s) skipped"),
        "the sidecar's one unreadable line is booked:\n{}",
        out.stdout
    );
}
