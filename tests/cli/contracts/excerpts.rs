//! The excerpt marker's count agrees with its noun on every surface that clips an excerpt.
//!
//! `text::truncate_excerpt` and `search`'s match-centered window both end a clipped excerpt
//! with `… (+N chars)`, and a text one character over its cap is the case a reader meets
//! whenever a cap is set near a typical length. Each surface below is handed a text exactly
//! one character longer than its own cap - 200 for the `list` and `agents` previews, 400 for
//! the `search` and `show --turn` excerpts - and must name that one character in the
//! singular, with no plural form of the count anywhere on the page.

use crate::harness::*;

const ENC: &str = "-Users-dev-excerpt-marker";
const SESS: &str = "7e6d5c4b-3a2f-4e1d-8c0b-9a8f7e6d5c4b";
const AGENT: &str = "a0123456789abcdef";

fn one_over_home() -> Home {
    let h = Home::new();
    let prompt = "q".repeat(201);
    let reply = "r".repeat(401);
    let returned = "s".repeat(201);
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"{p}"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{{"id":"m1","role":"assistant","model":"claude-x","stop_reason":"end_turn","content":[{{"type":"text","text":"{r}"}}]}}}}"#, "\n",
            ),
            p = prompt,
            r = reply
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"su1","isSidechain":true,"agentId":"{a}","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"user","content":"do the part"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"sa1","parentUuid":"su1","isSidechain":true,"agentId":"{a}","timestamp":"2026-06-07T05:00:03.000Z","message":{{"id":"m2","role":"assistant","model":"claude-x","stop_reason":"end_turn","content":[{{"type":"text","text":"{s}"}}]}}}}"#, "\n",
            ),
            a = AGENT,
            s = returned
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.meta.json"),
        r#"{"agentType":"general-purpose","description":"part worker"}"#,
    );
    h
}

#[test]
fn one_clipped_character_reads_in_the_singular_on_every_surface() {
    let h = one_over_home();
    let s = at(SESS);
    let cases: [(&str, Vec<&str>); 5] = [
        (
            "search head",
            vec![
                "search",
                "",
                &s,
                "--no-subagents",
                "-t",
                "agent.message.plain",
            ],
        ),
        (
            "search window",
            vec!["search", "^rrr", &s, "--no-subagents"],
        ),
        ("list", vec!["list", &s]),
        ("show --turn", vec!["show", &s, "--turn", "0"]),
        ("agents", vec!["agents", &s, "--returned-message"]),
    ];
    for (surface, argv) in cases {
        let out = h.run(&argv);
        assert!(out.success, "{surface}: {}", out.stderr);
        assert!(
            out.stdout.contains("\u{2026} (+1 char)"),
            "{surface} names one clipped character in the singular:\n{}",
            out.stdout
        );
        assert!(
            !out.stdout.contains("(+1 chars)"),
            "{surface} prints the plural beside a count of one:\n{}",
            out.stdout
        );
    }
}
