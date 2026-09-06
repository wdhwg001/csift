//! `whoami --to` over the two receivers csift must not promise anything to: a lane that has
//! already finished, and a headless run.
//!
//! Both answers are the send path's, run as a prediction: the point of `--to` is that it cannot
//! disagree with `csift send`, so the refusal text and the verdict are the same words a real
//! send would print - with nothing queued and nothing written.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-relay";
const SESS: &str = "00000000-0000-4000-8000-000000000071";
const HEADLESS: &str = "00000000-0000-4000-8000-000000000072";
const DONE_TEAMMATE: &str = "aRelay-0123456789abcdef";
const CWD: &str = "/Users/dev/Projects/relay";

/// Two delivery slots on PostToolUse, so a refusal cannot be blamed on a missing hook.
const SETTINGS: &str = concat!(
    r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"csift deliver --slot 1"},"#,
    r#"{"type":"command","command":"csift deliver --slot 2"}]}]}}"#,
);

/// A lane whose tail is a clean `end_turn` and which spawned nothing: finished.
fn finished_lane() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"l1","timestamp":"2026-06-07T05:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"go"}}}}"#
        ),
        r#"{"type":"assistant","uuid":"l2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#
    )
}

fn home() -> Home {
    let h = Home::new();
    h.write_claude("settings.json", SETTINGS);
    h.write(&format!("{ENC}/{SESS}.jsonl"), &finished_lane());
    h.write(&format!("{ENC}/{HEADLESS}.jsonl"), &finished_lane());
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{DONE_TEAMMATE}.jsonl"),
        &finished_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{DONE_TEAMMATE}.meta.json"),
        r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"harbor"}"#,
    );
    h
}

#[test]
fn to_a_completed_teammate_is_refused_and_names_the_resume_that_would_reach_it() {
    // A completed lane is not merely unreachable: the official path RESPAWNS it. csift will
    // not let that read as a delivery, so the prediction refuses and says what the caller
    // would actually be asking for.
    let h = home();
    let out = h.run(&["whoami", "--to", &at(DONE_TEAMMATE)]);
    assert!(
        out.success,
        "a refusal is a definitive answer: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("state     completed"),
        "the lane's own tail is the evidence:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("verdict     REFUSED") && out.stdout.contains("channel     none"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("respawns the lane rather than delivering to it")
            && out.stdout.contains("--resume"),
        "the refusal names the action a caller would really be taking:\n{}",
        out.stdout
    );
    // A prediction writes nothing, refusal or not.
    assert!(
        !h.projects()
            .join(format!("{ENC}/{SESS}/csift-channel"))
            .exists(),
        "a reach question queues nothing"
    );

    let json = h.run(&["whoami", "--to", &at(DONE_TEAMMATE), "--format", "json"]);
    let row = &json_rows(&json.stdout, "reach")[0];
    assert_eq!(row["verdict"], "REFUSED");
    assert_eq!(row["state"], "completed");
    assert!(
        row["official"].is_null(),
        "a refusal delegates nothing: {row}"
    );
    assert_eq!(json_summary(&json.stdout)["refused"], true);
}

#[test]
fn the_unknown_teams_gate_carries_both_counts_it_can_actually_see() {
    // The gate cannot be READ - the shell environment and the CLI flags that also enable it
    // leave nothing on disk - so the verdict hands over the two things csift can count and
    // lets the reader weigh them. Both numbers are load-bearing: a zero where there are team
    // directories, or a teammate count that counted the other lanes instead, would argue the
    // opposite of the truth.
    let h = home();
    h.write_claude("teams/alpha/team.json", "{}");
    h.write_claude("teams/beta/team.json", "{}");
    let out = h.run(&["whoami", "--to", &at(DONE_TEAMMATE)]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout
            .contains("use evidence: teams directories 2, teammate lanes 1"),
        "two team directories on disk, one teammate lane in the session:\n{}",
        out.stdout
    );
}

#[test]
fn to_a_headless_receiver_is_unpredictable_however_many_slots_are_configured() {
    // A `-p` run has no approval surface for an inbound message and may end before any hook
    // point is reached. csift never promises delivery to one, and the reason is the registry's
    // own `entrypoint` field rather than a guess about what the process is doing.
    let h = home();
    h.write_session_registry(
        std::process::id(),
        &format!(
            r#"{{"pid":{},"sessionId":"{HEADLESS}","status":"busy","entrypoint":"sdk-cli"}}"#,
            std::process::id()
        ),
    );
    let out = h.run(&["whoami", "--to", &at(HEADLESS)]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("verdict     UNPREDICTABLE"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("headless run") && out.stdout.contains("csift never promises"),
        "the prediction says why it will not commit:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("risk        the registry row's entrypoint is `sdk-cli`"),
        "the evidence is named as a risk line:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("slots       PostToolUse 1,2"),
        "the slots are configured, and it is still unpredictable:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("SendMessage"),
        "no official arm is offered for a headless receiver:\n{}",
        out.stdout
    );
}
