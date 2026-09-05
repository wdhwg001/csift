//! `csift msg` + `csift ack`: the ledger's intent joined to the receiver transcript's fact,
//! every verdict, the lane view and its filters, and who is allowed to ack.

use crate::harness::*;

const ENC: &str = "-Users-dev-relay-harbor";
const SESS: &str = "00000000-0000-4000-8000-000000000021";
const RELAY: &str = "aRelay-0123456789abcdef";

/// One id per verdict, so a single fixture answers the whole ladder.
const DELIVERED: &str = "aaaaaaaaaaaaaaa1";
const INTENT: &str = "bbbbbbbbbbbbbbb2";
const QUEUED: &str = "ccccccccccccccc3";
const HELD: &str = "ddddddddddddddd4";
const EXPIRED: &str = "eeeeeeeeeeeeeee5";
const ACKED: &str = "fffffffffffffff6";
const REFUSED: &str = "0000000000000007";

fn channel_path(rel: &str) -> String {
    format!("{ENC}/{SESS}/csift-channel/{rel}")
}

/// Join fixture lines the way csift's own append writes them: every line NEWLINE-terminated,
/// so a later append starts its own line instead of running onto the last one.
fn jsonl(lines: &[String]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// A first-chunk delivery record for `id`, as a hook would have injected it.
fn delivery_record(uuid: &str, ts: &str, id: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","attachment":{{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","hookName":"csift deliver --slot 1","content":["[csift-channel v1 id={id} part=1/1 mode=steer from={RELAY} from-session=00000000 relation=sibling to={SESS}]\nthrottlebeacon the region queue\n--- end ---"]}}}}"#
    )
}

fn inbox_line(id: &str, ts: &str, mode: &str, expires: &str) -> String {
    format!(r#"{{"id":"{id}","enqueued_utc":"{ts}","mode":"{mode}","expires_utc":"{expires}"}}"#)
}

fn emit_line(id: &str, ts: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"emit","event":"UserPromptSubmit","slot":1,"part":1,"parts":1,"vehicle":"additionalContext","ts_utc":"{ts}","hook_session":"{SESS}","hook_agent_id":null,"block_count":null}}"#
    )
}

/// The receiver session: a transcript carrying two real deliveries, and a channel root
/// holding one message per verdict.
fn relay_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &jsonl(&[
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay work"}}"#.to_string(),
            delivery_record("att1", "2026-06-07T05:00:07.000Z", DELIVERED),
            delivery_record("att2", "2026-06-07T05:00:08.000Z", ACKED),
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"relay work done"}]}}"#.to_string(),
        ]),
    );
    h.write(
        &channel_path(&format!("inbox/{SESS}.jsonl")),
        &jsonl(&[
            inbox_line(
                REFUSED,
                "2026-06-07T05:00:00Z",
                "steer",
                "2099-01-01T00:00:00Z",
            ),
            inbox_line(
                EXPIRED,
                "2026-06-07T05:00:01Z",
                "queue",
                "2020-01-01T00:00:00Z",
            ),
            inbox_line(
                HELD,
                "2026-06-07T05:00:02Z",
                "queue",
                "2099-01-01T00:00:00Z",
            ),
            inbox_line(
                QUEUED,
                "2026-06-07T05:00:03Z",
                "steer",
                "2099-01-01T00:00:00Z",
            ),
            inbox_line(
                INTENT,
                "2026-06-07T05:00:04Z",
                "steer",
                "2099-01-01T00:00:00Z",
            ),
            inbox_line(
                ACKED,
                "2026-06-07T05:00:05Z",
                "steer",
                "2099-01-01T00:00:00Z",
            ),
            inbox_line(
                DELIVERED,
                "2026-06-07T05:00:06Z",
                "steer",
                "2099-01-01T00:00:00Z",
            ),
        ]),
    );
    h.write(
        &channel_path(&format!("ledger/{SESS}.jsonl")),
        &jsonl(&[
            format!(r#"{{"id":"{REFUSED}","kind":"refused","reason":"served-call","ts_utc":"2026-06-07T05:00:10Z"}}"#),
            format!(r#"{{"id":"{HELD}","kind":"held","reason":"block-cap","ts_utc":"2026-06-07T05:00:11Z"}}"#),
            emit_line(INTENT, "2026-06-07T05:00:12Z"),
            emit_line(ACKED, "2026-06-07T05:00:13Z"),
            format!(r#"{{"id":"{ACKED}","kind":"ack","ts_utc":"2026-06-07T05:00:14Z"}}"#),
            emit_line(DELIVERED, "2026-06-07T05:00:15Z"),
        ]),
    );
    // Only the delivered message keeps its source file: the reconciliation must work from
    // the ledger and the inbox alone for the rest.
    h.write(
        &channel_path(&format!("messages/{DELIVERED}.json")),
        &format!(
            r#"{{"id":"{DELIVERED}","ts_utc":"2026-06-07T05:00:06Z","from":{{"kind":"lane","session":"00000000-0000-4000-8000-000000000022","lane":"{RELAY}","label":null,"cwd":"/Users/dev/relay"}},"to":{{"session":"{SESS}","lane":"{SESS}","form":"transcript","routing_id":null}},"mode":"steer","ttl_secs":43200,"relation":"sibling","cross_project":false,"body":"throttlebeacon the region queue"}}"#
        ),
    );
    h
}

#[test]
fn every_verdict_comes_from_the_two_halves() {
    let h = relay_home();
    // The whole ladder, one fixture: what separates DELIVERED from INTENT-ONLY is the
    // receiver's own transcript, nothing csift wrote.
    for (id, want) in [
        (DELIVERED, "DELIVERED"),
        (INTENT, "INTENT-ONLY"),
        (QUEUED, "QUEUED"),
        (HELD, "HELD"),
        (EXPIRED, "EXPIRED"),
        (ACKED, "ACKED"),
        (REFUSED, "REFUSED"),
    ] {
        let out = h.run(&["msg", id, "--lane", &at(SESS)]);
        assert!(out.success, "{id} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains(&format!("{id}  {want}")),
            "{id} should read {want}:\n{}",
            out.stdout
        );
    }
    // The two emitted-but-unproven halves are named apart, and the reasons are printed.
    let intent = h.run(&["msg", INTENT, "--lane", &at(SESS)]);
    assert!(
        intent
            .stdout
            .contains("fact        none in this lane's transcript"),
        "intent-only names the missing half:\n{}",
        intent.stdout
    );
    let held = h.run(&["msg", HELD, "--lane", &at(SESS)]);
    assert!(
        held.stdout.contains("held        block-cap"),
        "a hold prints its reason:\n{}",
        held.stdout
    );
    let refused = h.run(&["msg", REFUSED, "--lane", &at(SESS)]);
    assert!(
        refused.stdout.contains("refused     served-call"),
        "a refusal prints its reason:\n{}",
        refused.stdout
    );
    // An acked message still reports the fact: the ack is a claim about reading, not about
    // arrival, so both are shown.
    let acked = h.run(&["msg", ACKED, "--lane", &at(SESS)]);
    assert!(
        acked.stdout.contains("fact        L3 uuid att2")
            && acked.stdout.contains("acked       yes"),
        "an acked delivery keeps its fact:\n{}",
        acked.stdout
    );
}

#[test]
fn a_single_message_report_names_its_sender_and_the_equivalent_search() {
    let h = relay_home();
    let out = h.run(&["msg", DELIVERED, "--lane", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("from        {RELAY}"))
            && out.stdout.contains("mode steer")
            && out.stdout.contains("relation sibling"),
        "the message source is read for the sender:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("fact        L2 uuid att1"),
        "the fact cites the record that proves it:\n{}",
        out.stdout
    );
    // The equivalence the help states: the fact half is a plain search anyone can run.
    assert!(
        out.stdout
            .contains(&format!("csift search '<id>' @{SESS} --additional-context")),
        "the fact half is stated as a runnable search:\n{}",
        out.stdout
    );
}

#[test]
fn the_lane_view_lists_newest_first_and_each_filter_narrows_it() {
    let h = relay_home();
    let all = h.run(&["msg", "--lane", &at(SESS)]);
    assert!(all.success, "stderr: {}", all.stderr);
    assert!(all.stdout.contains("7 message(s)"), "{}", all.stdout);
    let first = all
        .stdout
        .lines()
        .find(|l| l.starts_with(DELIVERED) || l.starts_with(REFUSED))
        .unwrap_or_default();
    assert!(
        first.starts_with(DELIVERED),
        "newest first, so the last enqueued message leads:\n{}",
        all.stdout
    );

    let pending = h.run(&["msg", "--lane", &at(SESS), "--pending"]);
    assert!(
        pending.stdout.contains("1 message(s)") && pending.stdout.contains(QUEUED),
        "--pending is what is still waiting:\n{}",
        pending.stdout
    );

    let held = h.run(&["msg", "--lane", &at(SESS), "--held"]);
    assert!(
        held.stdout.contains("1 message(s)") && held.stdout.contains(HELD),
        "--held is a hold with no later emit:\n{}",
        held.stdout
    );

    let sent = h.run(&["msg", "--lane", &at(SESS), "--sent"]);
    assert!(
        sent.stdout.contains("3 message(s)")
            && sent.stdout.contains(DELIVERED)
            && sent.stdout.contains(INTENT)
            && sent.stdout.contains(ACKED)
            && !sent.stdout.contains(QUEUED),
        "--sent is everything csift emitted a chunk of:\n{}",
        sent.stdout
    );
}

#[test]
fn the_json_row_carries_the_verdict_the_emits_and_the_fact() {
    let h = relay_home();
    let out = h.run(&["msg", DELIVERED, "--lane", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 3, "header, one row, summary:\n{}", out.stdout);
    let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(header["kind"], "header");
    assert_eq!(header["command"], "msg");
    assert_eq!(header["lane"], SESS);
    let row: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(row["kind"], "msg");
    assert_eq!(row["id"], DELIVERED);
    assert_eq!(row["verdict"], "DELIVERED");
    assert_eq!(row["mode"], "steer");
    assert_eq!(row["session"], SESS);
    assert_eq!(row["emits"][0]["event"], "UserPromptSubmit");
    assert_eq!(row["emits"][0]["vehicle"], "additionalContext");
    assert!(
        row["emits"][0]["ts_local"].is_string(),
        "every machine instant is a utc/local pair: {row}"
    );
    assert_eq!(row["fact"]["line"], 2);
    assert_eq!(row["fact"]["uuid"], "att1");
    assert_eq!(row["held"], false);
    assert_eq!(row["acked"], false);
    let summary: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(summary["kind"], "summary");
    assert_eq!(summary["messages"], 1);

    // The intent-only row's fact is an explicit null, never a fabricated locator.
    let intent = h.run(&["msg", INTENT, "--lane", &at(SESS), "--format", "json"]);
    let row: serde_json::Value =
        serde_json::from_str(intent.stdout.lines().nth(1).unwrap()).unwrap();
    assert_eq!(row["verdict"], "INTENT-ONLY");
    assert!(row["fact"].is_null(), "no record, no locator: {row}");
}

#[test]
fn ack_appends_a_ledger_line_and_a_later_msg_reads_acked() {
    let h = relay_home();
    let before = h.run(&["msg", QUEUED, "--lane", &at(SESS)]);
    assert!(before.stdout.contains(&format!("{QUEUED}  QUEUED")));

    let ack = h.run_with_env(
        &["ack", QUEUED, "--lane", &at(SESS)],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(ack.success, "stderr: {}", ack.stderr);
    assert!(
        ack.stdout
            .contains(&format!("acked {QUEUED} in lane {SESS}")),
        "the receipt names what was acked:\n{}",
        ack.stdout
    );

    let after = h.run(&["msg", QUEUED, "--lane", &at(SESS)]);
    assert!(
        after.stdout.contains(&format!("{QUEUED}  ACKED")),
        "the ack is read back from the lane's own ledger:\n{}",
        after.stdout
    );
    // Acking twice keeps both lines (append-only) and says so rather than pretending the
    // second call did nothing.
    let again = h.run_with_env(
        &["ack", QUEUED, "--lane", &at(SESS), "--format", "json"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    let row: serde_json::Value =
        serde_json::from_str(again.stdout.lines().nth(1).unwrap()).unwrap();
    assert_eq!(row["kind"], "ack");
    assert_eq!(row["already_acked"], true);
    assert!(row["ts_local"].is_string());
}

#[test]
fn an_external_caller_cannot_ack() {
    let h = relay_home();
    // No CLAUDE_CODE_SESSION_ID: an ack asserts that a LANE read the message, which a
    // process outside Claude Code cannot know - and naming a lane does not make it one.
    let out = h.run(&["ack", QUEUED, "--lane", &at(SESS)]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("external caller cannot ack"),
        "the refusal states the reason:\n{}",
        out.stderr
    );
    // ...and nothing was written: the message is still queued.
    let after = h.run(&["msg", QUEUED, "--lane", &at(SESS)]);
    assert!(after.stdout.contains(&format!("{QUEUED}  QUEUED")));
}

#[test]
fn acking_an_id_the_lane_never_held_is_a_hard_error() {
    let h = relay_home();
    let out = h.run_with_env(
        &["ack", "1111111111111111", "--lane", &at(SESS)],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no record of message") && out.stderr.contains("joins to nothing"),
        "the error says why the write was refused:\n{}",
        out.stderr
    );
}

#[test]
fn an_id_the_lane_never_held_is_a_hard_error() {
    let h = relay_home();
    let out = h.run(&["msg", "1111111111111111", "--lane", &at(SESS)]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no channel record of message"),
        "an address that resolves to nothing is a hard error:\n{}",
        out.stderr
    );
    // A malformed id is caught by the grammar, before any file is opened.
    let bad = h.run(&["msg", "not-an-id", "--lane", &at(SESS)]);
    assert!(!bad.success);
    assert!(
        bad.stderr.contains("16 lowercase hex characters"),
        "the id grammar is stated:\n{}",
        bad.stderr
    );
}

#[test]
fn without_a_lane_the_calling_session_is_used_and_the_path_is_disclosed() {
    let h = relay_home();
    let out = h.run_with_env(&["msg", DELIVERED], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(&format!("{DELIVERED}  DELIVERED")));
    // The environment names the TOP-LEVEL session in every lane, so the resolution path is
    // always printed rather than left to be assumed.
    assert!(
        out.stderr.contains("CLAUDE_CODE_SESSION_ID")
            && out.stderr.contains("--lane @<your agent id>"),
        "the lane note is unconditional:\n{}",
        out.stderr
    );
}

#[test]
fn an_external_caller_is_told_to_name_a_lane() {
    let h = relay_home();
    let out = h.run(&["msg"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("not a Claude Code lane") && out.stderr.contains("--lane @<lane>"),
        "the remediation is the flag:\n{}",
        out.stderr
    );
}

#[test]
fn a_lane_with_no_channel_directory_reads_as_empty_not_as_an_error() {
    // A session that was never sent anything has no sidecar at all. Absence is the honest
    // answer, not a failure.
    let h = Home::new();
    let other = "00000000-0000-4000-8000-000000000031";
    h.write(
        &format!("{ENC}/{other}.jsonl"),
        "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"nothing here\"}}\n",
    );
    let out = h.run(&["msg", "--lane", &at(other)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no channel messages") && out.stdout.contains("0 message(s)"),
        "an empty channel is an honest empty:\n{}",
        out.stdout
    );
}

#[test]
fn a_message_addressed_at_a_child_lane_is_found_from_the_session() {
    // `messages/` is keyed by SESSION and the state files by LANE, so a caller in the main
    // session asking about a message sent to one of its subagents must be answered, not
    // told the id is unknown.
    let h = relay_home();
    let agent = "a00112233445566a";
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{agent}.jsonl"),
        &jsonl(&[
            r#"{"type":"user","uuid":"s1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"child lane opener"}}"#.to_string(),
            delivery_record("satt1", "2026-06-07T05:00:20.000Z", "1234567890abcdef"),
        ]),
    );
    h.write(
        &channel_path(&format!("inbox/{agent}.jsonl")),
        &jsonl(&[inbox_line(
            "1234567890abcdef",
            "2026-06-07T05:00:19Z",
            "steer",
            "2099-01-01T00:00:00Z",
        )]),
    );
    h.write(
        &channel_path(&format!("ledger/{agent}.jsonl")),
        &jsonl(&[emit_line("1234567890abcdef", "2026-06-07T05:00:20Z")]),
    );
    h.write(
        &channel_path("messages/1234567890abcdef.json"),
        &format!(
            r#"{{"id":"1234567890abcdef","ts_utc":"2026-06-07T05:00:19Z","from":{{"kind":"external","session":null,"lane":null,"label":"harbor","cwd":null}},"to":{{"session":"{SESS}","lane":"{agent}","form":"transcript","routing_id":null}},"mode":"steer","ttl_secs":43200,"relation":"external","cross_project":false,"body":"regionsweep"}}"#
        ),
    );
    let out = h.run(&["msg", "1234567890abcdef", "--lane", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1234567890abcdef  DELIVERED") && out.stdout.contains("fact        L2"),
        "the child lane's own transcript is the fact:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains(&format!("addressed at lane {agent}")),
        "the redirect is disclosed:\n{}",
        out.stderr
    );
}
