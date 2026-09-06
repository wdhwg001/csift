//! Two things a send has to get exactly right at the boundary: the duration grammar it
//! refuses, and the arithmetic behind a verdict when the message fits the slots exactly.
//!
//! Both are places where "nearly right" is the failure mode - a ttl with no unit, and a
//! message needing precisely as many slots as the receiver has - so each is pinned to the
//! exact words and the exact verdict rather than to a substring that any answer would match.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-fit";
const SESS: &str = "00000000-0000-4000-8000-0000000000d1";
const LANE: &str = "a2000000000000001";
const CWD: &str = "/Users/dev/Projects/fit";

/// One delivery slot on the FIRST steer event and one on a later event: the same count on
/// both, so which of them the census names is a choice and not an accident.
const SETTINGS: &str = concat!(
    r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"csift deliver --slot 1"}]}],"#,
    r#""Stop":[{"hooks":[{"type":"command","command":"csift deliver --slot 1"}]}]}}"#,
);

/// A lane still mid-turn: running whenever the suite runs.
fn lane_body() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"l1","timestamp":"2026-06-07T05:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"go"}}}}"#
        ),
        r#"{"type":"assistant","uuid":"l2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"working"}]}}"#
    )
}

fn home() -> Home {
    let h = Home::new();
    h.write_claude("settings.json", SETTINGS);
    h.write(&format!("{ENC}/{SESS}.jsonl"), &lane_body());
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{LANE}.jsonl"),
        &lane_body(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{LANE}.meta.json"),
        r#"{"agentType":"general-purpose","description":"fit probe"}"#,
    );
    // One slot has actually run in the lane, so the prediction is not qualified by an
    // unarmed receiver and the verdict rests on the fit alone.
    h.write(
        &format!("{ENC}/{SESS}/csift-channel/armed/{LANE}.json"),
        &format!(
            r#"{{"slots_seen":[1],"last_event":"SessionStart","last_ts_utc":"2026-06-07T05:00:00Z","hook_session":"{SESS}","claude_code_version":"2.1.258"}}"#
        ),
    );
    h
}

fn lane_env() -> Vec<(&'static str, &'static str)> {
    vec![("CLAUDE_CODE_SESSION_ID", SESS)]
}

#[test]
fn a_ttl_with_no_unit_is_refused_by_the_grammar_not_by_its_missing_unit() {
    // `--ttl 30` is the mistake a human actually makes, and the answer has to be the grammar:
    // a quantity with no unit is not a duration with an unknown unit, it is a value that never
    // named one, and pointing at the empty unit teaches nothing.
    let h = home();
    let out = h.run_with_env(
        &["send", &at(LANE), "hold the line", "--ttl", "30"],
        &lane_env(),
    );
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("a duration is a number and a unit")
            && out.stderr.contains("`12h`, the default"),
        "the error names the grammar and its default:\n{}",
        out.stderr
    );
}

#[test]
fn a_message_that_fits_the_slots_exactly_is_ok_and_not_full() {
    // FULL means the message needs MORE slots than the receiver has. One chunk against one
    // configured slot needs no more than it has, so it is a plain OK - reading the boundary
    // as full would tell every ordinary sender to install a slot it does not need.
    let h = home();
    let out = h.run_with_env(&["send", &at(LANE), "one short line"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("verdict     OK"),
        "one chunk against one slot fits:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("message     14 char(s), 1 chunk(s)"),
        "{}",
        out.stdout
    );
}

#[test]
fn the_prediction_names_the_first_event_carrying_the_most_slots() {
    // Two events carry one slot each. The prediction has to name the one a delivery reaches
    // FIRST in the event order, because that is the hook point part 1 actually rides; naming
    // the last of the tied events would promise a turn boundary for a message that goes out
    // at the next session start.
    let h = home();
    let out = h.run_with_env(&["send", &at(LANE), "one short line"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("1 of 1 chunk(s) fit at SessionStart"),
        "{}",
        out.stdout
    );
}
