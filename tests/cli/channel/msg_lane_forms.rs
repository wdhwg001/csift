//! `--lane` takes every id form csift prints.
//!
//! A lane is always an id, so `--lane` routes through the shared `@`-resolver rather than a
//! private parser: the bare `a<16 hex>` a subagent is named by, the name-embedded teammate id,
//! and a teammate's ROUTING form all name the same channel files. The routing form is the one
//! that can collide, so it resolves to the transcript form and the files stay keyed by that.

use crate::harness::*;

const ENC: &str = "-Users-dev-relay-harbor";
const SESS: &str = "00000000-0000-4000-8000-000000000051";
const AGENT: &str = "a0011223344556677";
const TEAMMATE: &str = "aRelay-0123456789abcdef";

const TO_AGENT: &str = "4444444444444444";
const TO_TEAMMATE: &str = "5555555555555555";

fn channel_path(rel: &str) -> String {
    format!("{ENC}/{SESS}/csift-channel/{rel}")
}

fn jsonl(lines: &[String]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

fn delivery_record(uuid: &str, ts: &str, id: &str, lane: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","attachment":{{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","hookName":"csift deliver --slot 1","content":["[csift-channel v1 id={id} part=1/1 mode=steer from={SESS} from-session=00000000 relation=parent to={lane}]\nthe beacon for this lane\n--- end ---"]}}}}"#
    )
}

fn lane_transcript(uuid: &str, id: &str, lane: &str) -> String {
    jsonl(&[
        format!(
            r#"{{"type":"user","uuid":"{uuid}","isSidechain":true,"timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"lane opener"}}}}"#
        ),
        delivery_record(&format!("{uuid}att"), "2026-06-07T05:00:05.000Z", id, lane),
    ])
}

fn inbox_line(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","enqueued_utc":"2026-06-07T05:00:01Z","mode":"steer","expires_utc":"2099-01-01T00:00:00Z"}}"#
    )
}

fn emit_line(id: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"emit","event":"UserPromptSubmit","slot":1,"part":1,"parts":1,"vehicle":"additionalContext","ts_utc":"2026-06-07T05:00:05Z","hook_session":"{SESS}","hook_agent_id":null,"block_count":null}}"#
    )
}

/// A session with two child lanes - one unnamed subagent, one teammate - each holding one
/// delivered message of its own.
fn relay_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &jsonl(&[
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay work"}}"#.to_string(),
        ]),
    );
    for (lane, id, uuid, meta) in [
        (
            AGENT,
            TO_AGENT,
            "s1",
            r#"{"agentType":"general-purpose","description":"relay probe"}"#,
        ),
        (
            TEAMMATE,
            TO_TEAMMATE,
            "s2",
            r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"harbor"}"#,
        ),
    ] {
        h.write(
            &format!("{ENC}/{SESS}/subagents/agent-{lane}.jsonl"),
            &lane_transcript(uuid, id, lane),
        );
        h.write(
            &format!("{ENC}/{SESS}/subagents/agent-{lane}.meta.json"),
            meta,
        );
        h.write(
            &channel_path(&format!("inbox/{lane}.jsonl")),
            &jsonl(&[inbox_line(id)]),
        );
        h.write(
            &channel_path(&format!("ledger/{lane}.jsonl")),
            &jsonl(&[emit_line(id)]),
        );
    }
    h
}

fn ledger_lines(h: &Home, lane: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(
        h.projects()
            .join(channel_path(&format!("ledger/{lane}.jsonl"))),
    )
    .unwrap_or_default()
    .lines()
    .map(|l| serde_json::from_str(l).expect("a ledger line"))
    .collect()
}

#[test]
fn a_bare_agent_id_names_the_child_lanes_own_channel_files() {
    let h = relay_home();
    let out = h.run(&["msg", "--lane", &at(AGENT)]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("lane {AGENT} (session {SESS})")),
        "the header names the lane AND the session that owns its files:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(TO_AGENT) && !out.stdout.contains(TO_TEAMMATE),
        "a lane sees its own inbox, not its sibling's:\n{}",
        out.stdout
    );
    // The fact half is read from the LANE's own transcript, which is where a delivery lands.
    let one = h.run(&["msg", TO_AGENT, "--lane", &at(AGENT)]);
    assert!(
        one.stdout.contains(&format!("{TO_AGENT}  DELIVERED"))
            && one.stdout.contains("fact        L2"),
        "{}",
        one.stdout
    );
}

#[test]
fn the_teammate_routing_form_resolves_to_the_same_lane_as_its_transcript_id() {
    let h = relay_home();
    let by_id = h.run(&["msg", "--lane", &at(TEAMMATE)]);
    let by_routing = h.run(&["msg", "--lane", "@Relay@harbor"]);
    assert!(by_id.success && by_routing.success, "{}", by_routing.stderr);
    assert_eq!(
        by_id.stdout, by_routing.stdout,
        "the routing form is an alias for the transcript form, not a second lane"
    );
    assert!(
        by_routing
            .stdout
            .contains(&format!("lane {TEAMMATE} (session {SESS})")),
        "the routing form resolves to the transcript id, which never collides:\n{}",
        by_routing.stdout
    );
    // A `@`-less spelling is accepted too: a lane is always an id, so there is no path form
    // to confuse it with.
    let bare = h.run(&["msg", "--lane", "Relay@harbor"]);
    assert!(bare.success, "{}", bare.stderr);
    assert_eq!(bare.stdout, by_routing.stdout);
}

#[test]
fn ack_writes_to_the_lane_named_in_either_form_and_never_to_the_session() {
    let h = relay_home();
    let env = [("CLAUDE_CODE_SESSION_ID", SESS)];

    let by_id = h.run_with_env(&["ack", TO_AGENT, "--lane", &at(AGENT)], &env);
    assert!(by_id.success, "{}", by_id.stderr);
    assert!(
        by_id
            .stdout
            .contains(&format!("acked {TO_AGENT} in lane {AGENT}")),
        "{}",
        by_id.stdout
    );
    let rows = ledger_lines(&h, AGENT);
    assert_eq!(rows.len(), 2, "the ack is appended, nothing rewritten");
    assert_eq!(rows[1]["kind"], "ack");
    assert_eq!(rows[1]["id"], TO_AGENT);

    let by_routing = h.run_with_env(&["ack", TO_TEAMMATE, "--lane", "@Relay@harbor"], &env);
    assert!(by_routing.success, "{}", by_routing.stderr);
    assert!(
        by_routing
            .stdout
            .contains(&format!("acked {TO_TEAMMATE} in lane {TEAMMATE}")),
        "the receipt names the TRANSCRIPT form, which is what the files are keyed by:\n{}",
        by_routing.stdout
    );
    assert_eq!(ledger_lines(&h, TEAMMATE).len(), 2);

    // The session's own ledger was never touched: an ack is a statement by the lane that read
    // the message, and `--lane` says which lane that is.
    assert!(
        !h.projects()
            .join(channel_path(&format!("ledger/{SESS}.jsonl")))
            .exists(),
        "acking a child lane must not write the session's ledger"
    );

    // And a lane cannot ack a message addressed at its sibling.
    let wrong = h.run_with_env(&["ack", TO_TEAMMATE, "--lane", &at(AGENT)], &env);
    assert!(!wrong.success, "stdout: {}", wrong.stdout);
    assert!(
        wrong.stderr.contains("no record of message"),
        "{}",
        wrong.stderr
    );
}
