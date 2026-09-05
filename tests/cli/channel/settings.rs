//! The settings disclosure: every channel verdict names the cascade it rested on.
//!
//! A gate verdict, the slot census and every hook risk come off ONE fold of the receiver's
//! settings scopes, so `send` and `whoami --to` both print which files contributed, which the
//! reader tried and did not get, and which inputs leave nothing on disk at all.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-relay";
const SESS: &str = "00000000-0000-4000-8000-000000000001";
const LANE: &str = "a1000000000000001";
const CWD: &str = "/Users/dev/Projects/relay";

/// A user-scope settings file with one delivery slot on PostToolUse.
const USER_SETTINGS: &str = concat!(
    r#"{"hooks":{"PostToolUse":[{"hooks":["#,
    r#"{"type":"command","command":"csift deliver --slot 1"}]}]}}"#,
);

fn record(uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{SESS}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// A session with one running subagent lane. Only the user scope exists on disk: the project
/// and local scopes are read from the RECEIVER's own recorded cwd, which no test may create
/// (it is a real filesystem path outside the harness), so those scopes are exercised here as
/// the absent rows they are.
fn home() -> Home {
    let h = Home::new();
    h.write_claude("settings.json", USER_SETTINGS);
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{}\n", record("m1", "2026-06-07T04:00:00.000Z", "hello")),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{LANE}.jsonl"),
        &format!("{}\n", record("l1", "2026-06-07T05:00:00.000Z", "go")),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{LANE}.meta.json"),
        r#"{"agentType":"general-purpose","description":"relay probe"}"#,
    );
    h
}

fn lane_env() -> Vec<(&'static str, &'static str)> {
    vec![("CLAUDE_CODE_SESSION_ID", SESS)]
}

#[test]
fn a_send_receipt_names_the_scopes_it_read_and_the_ones_it_did_not() {
    let h = home();
    let out = h.run_with_env(&["send", &at(LANE), "check the beacon"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("  settings    read: user  ·  absent:"),
        "the receipt names the scopes the verdict rested on:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("absent: project")
            && out.stdout.contains("local")
            && out.stdout.contains("policy-remote"),
        "a scope the reader tried and did not get is named as absent, not omitted:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("unobservable: --settings")
            && out.stdout.contains("--setting-sources / --restricted")
            && out.stdout.contains("MDM policy"),
        "the unobservable list rides verbatim, so `unknown` arrives with its reason:\n{}",
        out.stdout
    );
}

#[test]
fn the_send_row_carries_the_sources_and_the_unobservable_list() {
    let h = home();
    let out = h.run_with_env(
        &["send", &at(LANE), "check the beacon", "--format", "json"],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    let row = &json_rows(&out.stdout, "send")[0];
    let sources = row["settings"]["sources"].as_array().expect("sources");
    let user = sources
        .iter()
        .find(|s| s["scope"] == "user")
        .expect("the user scope is always tried");
    assert_eq!(user["read"], true);
    assert!(user["path"].as_str().unwrap().ends_with("settings.json"));
    assert!(user["note"].is_null(), "a file that parsed carries no note");
    assert!(
        sources
            .iter()
            .any(|s| s["scope"] == "project" && s["read"] == false),
        "an absent scope is a row with `read: false`, never a missing row: {row}"
    );
    assert_eq!(
        row["settings"]["unobservable"].as_array().unwrap().len(),
        5,
        "every unobservable input is listed: {row}"
    );
}

#[test]
fn a_reach_prediction_discloses_the_same_cascade_as_a_send() {
    let h = home();
    let text = h.run(&["whoami", "--to", &at(LANE)]);
    assert!(text.success, "{}", text.stderr);
    assert!(
        text.stdout.contains("  settings    read: user  ·  absent:"),
        "a prediction names its sources too:\n{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("unobservable: --settings"),
        "{}",
        text.stdout
    );

    let json = h.run(&["whoami", "--to", &at(LANE), "--format", "json"]);
    let row = &json_rows(&json.stdout, "reach")[0];
    assert!(
        row["settings"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["scope"] == "user" && s["read"] == true),
        "the reach row carries the same inventory: {row}"
    );
    assert_eq!(row["settings"]["unobservable"].as_array().unwrap().len(), 5);
}

#[test]
fn a_broken_settings_file_reaches_the_receipt_as_a_note() {
    // A malformed file contributes nothing, so the note is the ONLY place its existence is
    // visible - without it a reader cannot tell a broken scope from a missing one.
    let h = home();
    h.write_claude("settings.json", "{ this is not json");
    let out = h.run_with_env(&["send", &at(LANE), "check the beacon"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("note: user - malformed JSON"),
        "a broken file is named with its reason:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("read: none"),
        "and it did not contribute:\n{}",
        out.stdout
    );
}
