//! What `csift msg` counts: every unreadable line of the three files it reads, and every
//! message id either of the two channel files knows about.
//!
//! Both are the never-silent law applied to the reconciliation surface. A line the schema
//! cannot read is dropped from the answer, so it has to be reported; a message whose inbox
//! line is gone but whose ledger remembers it is still a message this lane holds.

use crate::harness::*;

const ENC: &str = "-Users-dev-counts";
const SESS: &str = "00000000-0000-4000-8000-0000000000c1";
const RELAY: &str = "aRelay-0123456789abcdef";
/// Enqueued and emitted: the ordinary shape.
const QUEUED: &str = "aaaaaaaaaaaaaaa1";
/// Emitted, with its inbox line no longer on disk (a hand-pruned inbox).
const LEDGER_ONLY: &str = "bbbbbbbbbbbbbbb2";

fn channel_path(rel: &str) -> String {
    format!("{ENC}/{SESS}/csift-channel/{rel}")
}

fn emit_line(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"emit","event":"UserPromptSubmit","slot":1,"part":1,"parts":1,"vehicle":"additionalContext","ts_utc":"2026-06-07T05:00:12Z","hook_session":"{SESS}","hook_agent_id":null,"block_count":null}}"#
    )
}

/// A lane holding one ordinary message and one the inbox has forgotten, with two unreadable
/// inbox lines, one unreadable ledger line and one unreadable transcript line - four in all,
/// spread over the three files so no single count can stand in for the total.
fn counts_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n{}\n{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"start the relay work"}}}}"#
            ),
            format_args!(
                r#"{{"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:07.000Z","attachment":{{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","hookName":"csift deliver --slot 1","content":["[csift-channel v1 id={QUEUED} part=1/1 mode=steer from={RELAY} from-session=00000000 relation=sibling to={SESS}]\nthrottlebeacon the region queue\n--- end ---"]}}}}"#
            ),
            "this transcript line is not a record at all",
        ),
    );
    h.write(
        &channel_path(&format!("inbox/{SESS}.jsonl")),
        &format!(
            "{}\n{}\n{}\n",
            "not an inbox line",
            r#"{"id":"truncated by a torn"#,
            format_args!(
                r#"{{"id":"{QUEUED}","enqueued_utc":"2026-06-07T05:00:03Z","mode":"steer","expires_utc":"2099-01-01T00:00:00Z"}}"#
            ),
        ),
    );
    h.write(
        &channel_path(&format!("ledger/{SESS}.jsonl")),
        &format!(
            "{}\n{}\n{}\n",
            "not a ledger line",
            emit_line(QUEUED),
            emit_line(LEDGER_ONLY),
        ),
    );
    h
}

#[test]
fn every_unreadable_line_of_all_three_files_reaches_one_count() {
    // The lane view reads two files and the transcript scan reads a third, and the number a
    // caller sees has to be their SUM: a count that dropped either half would report a whole
    // file as clean, which is exactly the silence the law forbids.
    let h = counts_home();
    let out = h.run(&["msg", "--lane", &at(SESS), "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let summary = json_summary(&out.stdout);
    assert_eq!(
        summary["skipped_lines"], 4,
        "two inbox lines, one ledger line and one transcript line:\n{}",
        out.stdout
    );
}

#[test]
fn one_message_reports_the_same_four_unreadable_lines_as_the_listing() {
    // Addressing one id narrows the transcript prefilter, never the accounting: the same
    // three files were read, so the same four lines are disclosed.
    let h = counts_home();
    let out = h.run(&["msg", QUEUED, "--lane", &at(SESS), "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    assert_eq!(
        json_summary(&out.stdout)["skipped_lines"],
        4,
        "{}",
        out.stdout
    );
}

#[test]
fn a_message_the_inbox_has_forgotten_is_still_one_the_ledger_holds() {
    // The two files are a UNION, not an intersection: an inbox line can be pruned by hand
    // while the ledger still records what csift did about that message, and dropping it from
    // the listing would make a delivered message unfindable from the lane that received it.
    let h = counts_home();
    let out = h.run(&["msg", "--lane", &at(SESS), "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let ids: Vec<String> = json_rows(&out.stdout, "msg")
        .iter()
        .map(|r| r["id"].as_str().expect("an id").to_string())
        .collect();
    assert!(
        ids.contains(&LEDGER_ONLY.to_string()) && ids.contains(&QUEUED.to_string()),
        "both ids are the lane's: {ids:?}"
    );
    assert_eq!(json_summary(&out.stdout)["messages"], 2);
}
