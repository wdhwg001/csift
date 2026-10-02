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

/// A built-in subagent of [`SESS`], with its meta and one Bash call carrying a trap marker, so
/// the same lane can be named by its id and by `@trap:`.
const SUBAGENT: &str = "a00000000000000aa";
const MARKER: &str = "PlumWaxenHarbor5719";

fn subagent_lane() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"s1","timestamp":"2026-06-07T05:01:00.000Z","cwd":"{CWD}","version":"2.1.258","isSidechain":true,"message":{{"role":"user","content":"look"}}}}"#
        ),
        format_args!(
            r#"{{"type":"assistant","uuid":"s2","parentUuid":"s1","timestamp":"2026-06-07T05:01:05.000Z","version":"2.1.258","isSidechain":true,"message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_trap01","name":"Bash","input":{{"command":"csift whoami @trap:{MARKER} --to @{DONE_TEAMMATE}"}}}}]}}}}"#
        )
    )
}

fn home_with_subagent() -> Home {
    let h = home();
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{SUBAGENT}.jsonl"),
        &subagent_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{SUBAGENT}.meta.json"),
        r#"{"agentType":"general-purpose","description":"look around"}"#,
    );
    h
}

#[test]
fn the_positional_names_the_sender_of_the_prediction() {
    // `whoami` answers who "I" am, and `--to` predicts a send from that lane. The environment
    // names the top-level session in every lane, so the positional is how a subagent says it is
    // the sender: by its own id, or by a trap marker its own Bash call carries.
    let h = home_with_subagent();
    let env = [("CLAUDE_CODE_SESSION_ID", SESS)];
    let assumed = h.run_with_env(&["whoami", "--to", &at(DONE_TEAMMATE)], &env);
    assert!(assumed.success, "stderr: {}", assumed.stderr);
    assert!(
        assumed
            .stderr
            .contains("name your own lane as the positional"),
        "{}",
        assumed.stderr
    );
    assert!(!assumed.stderr.contains("--from"), "{}", assumed.stderr);
    assert!(
        assumed
            .stdout
            .contains(&format!("  caller      lane {SESS}")),
        "{}",
        assumed.stdout
    );
    let trap = format!("@trap:{MARKER}");
    for me in [at(SUBAGENT), trap] {
        let out = h.run_with_env(&["whoami", &me, "--to", &at(DONE_TEAMMATE)], &env);
        assert!(out.success, "{me}: {}", out.stderr);
        assert!(!out.stderr.contains("lane unknown"), "{me}: {}", out.stderr);
        // The positional's own output comes first, then the prediction from that lane.
        let self_at = out.stdout.find(&format!("self     {SUBAGENT}"));
        let reach_at = out.stdout.find("csift channel");
        assert!(
            matches!((self_at, reach_at), (Some(a), Some(b)) if a < b),
            "{me}: the self section precedes the prediction:\n{}",
            out.stdout
        );
        assert!(
            out.stdout
                .contains(&format!("  caller      lane {SUBAGENT}")),
            "{me}: the sender is the lane itself:\n{}",
            out.stdout
        );
    }
    // `@main` names the top-level session: the same sender, named rather than assumed.
    let main = h.run_with_env(&["whoami", "@main", "--to", &at(DONE_TEAMMATE)], &env);
    assert!(main.success, "stderr: {}", main.stderr);
    assert!(!main.stderr.contains("lane unknown"), "{}", main.stderr);
    assert!(
        main.stdout.contains(&format!("  caller      lane {SESS}")),
        "{}",
        main.stdout
    );
    // The flag that named a sender is gone: a sender is the positional.
    let gone = h.run_with_env(
        &["whoami", "--to", &at(DONE_TEAMMATE), "--from", "@main"],
        &env,
    );
    assert_eq!(gone.code, Some(2), "{}", gone.stderr);
}

#[test]
fn a_positional_prediction_is_one_json_stream() {
    // The positional owns the envelope: its rows, then the `reach` row, then ONE summary that
    // carries the positional's counts and the prediction's two keys.
    let h = home_with_subagent();
    let env = [("CLAUDE_CODE_SESSION_ID", SESS)];
    let out = h.run_with_env(
        &[
            "whoami",
            &at(SUBAGENT),
            "--to",
            &at(DONE_TEAMMATE),
            "--format",
            "json",
        ],
        &env,
    );
    assert!(out.success, "{}", out.stderr);
    let rows: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("one JSON object per line"))
        .collect();
    let kinds: Vec<&str> = rows.iter().filter_map(|r| r["kind"].as_str()).collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "header").count(),
        1,
        "{kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "summary").count(),
        1,
        "{kinds:?}"
    );
    assert_eq!(kinds.first(), Some(&"header"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"summary"), "{kinds:?}");
    let self_at = kinds.iter().position(|k| *k == "self");
    let reach_at = kinds.iter().position(|k| *k == "reach");
    assert!(
        matches!((self_at, reach_at), (Some(a), Some(b)) if a < b),
        "{kinds:?}"
    );
    let reach = &rows[reach_at.unwrap_or_default()];
    assert_eq!(reach["caller"]["label"], SUBAGENT, "{reach}");
    let summary = &rows[rows.len() - 1];
    for key in [
        "identities",
        "live_child_lanes",
        "other_live_lanes",
        "queued",
        "refused",
    ] {
        assert!(summary.get(key).is_some(), "summary lacks {key}: {summary}");
    }
}
