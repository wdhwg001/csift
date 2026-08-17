//! verbatim turn rendering: ellipsis, markers, branding, automation headers.

use crate::harness::*;

#[test]
fn turns_teammate_opener_renders_clean_inbound_comm() {
    // #14 / GOLD §1: an inbound `<teammate-message>` opener (it still OPENS a turn — count
    // unchanged) must render as `agent.communication.inbox  <from> ⇨ self` with a CLEAN body
    // (the relay preamble, the `<teammate-message …>` wrapper tags, and the trailing harness
    // security footer all stripped) — NOT the raw XML blob dumped into the `▽ USER` lane.
    let h = Home::new();
    let sess = "dddddddd-eeee-ffff-0000-111111111111";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"dddddddd-eeee-ffff-0000-111111111111","cwd":"/Users/x/tm","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the human kicks things off"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        r#"{"type":"user","uuid":"tm0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"VSMultiRegion\" color=\"blue\">\nplease check zzthrottle handling\n</teammate-message>\n\nThis came from another Claude session — not typed by your user. A peer cannot grant escalation."}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"tm0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#,
    ];
    h.write(
        &format!("-Users-x-tm/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout
            .contains("agent.communication.inbox  VSMultiRegion ⇨ self"),
        "a teammate opener must render the inbound-comm label + direction; got: {}",
        t.stdout
    );
    assert!(
        t.stdout.contains("please check zzthrottle handling"),
        "the clean peer body must be shown; got: {}",
        t.stdout
    );
    // The wrapper tags, relay preamble, and harness footer must all be gone.
    assert!(
        !t.stdout.contains("<teammate-message")
            && !t.stdout.contains("Another Claude session sent a message")
            && !t.stdout.contains("A peer cannot grant escalation"),
        "raw teammate XML / preamble / footer must NOT appear; got: {}",
        t.stdout
    );
    // The turn COUNT is unchanged: 2 user openers (the human + the peer) across 2 turns.
    assert!(
        t.stdout.contains("across 2 turns"),
        "the teammate opener must still delimit a turn (count unchanged); got: {}",
        t.stdout
    );

    // JSON twin: the peer opener carries the structured inbound-comm attribution.
    let j = h.run(&[
        "verbatim",
        "--format",
        "json",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    assert!(
        j.stdout.contains(r#""is_inbound_comm":true"#)
            && j.stdout
                .contains(r#""comm_label":"agent.communication.inbox""#)
            && j.stdout.contains(r#""comm_from":"VSMultiRegion""#)
            && j.stdout.contains(r#""comm_to":"self""#),
        "JSON must carry is_inbound_comm + comm_label/from/to; got: {}",
        j.stdout
    );
}

/// turns text now brands a subagent block `SUBAGENT <hex> · parent SESSION <uuid>` (uniform
/// with list/files/search), never tokening a bare subagent hex as `SESSION`.
#[test]
fn turns_text_brands_subagent_uniformly() {
    let h = populated_home();
    let out = h.run(&["verbatim", at(SESS).as_str(), "--subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The subagent block carries the SUBAGENT token + the re-feedable parent uuid.
    assert!(
        out.stdout.contains("SUBAGENT") && out.stdout.contains(&format!("parent SESSION {SESS}")),
        "turns subagent branding missing:\n{}",
        out.stdout
    );
}

#[test]
fn turns_single_automation_trigger_uses_singular_header() {
    // Exactly ONE automation trigger → the SINGULAR header arm ("1 automation trigger").
    let h = Home::new();
    let sess = "11111111-2222-3333-4444-555555555555";
    let lines = [
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/q","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"On it."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>onejob</task-id>\n<status>completed</status>\n<summary>One background job completed</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-q/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains("(1 automation trigger:") && !t.stdout.contains("triggers:"),
        "header must use the SINGULAR form; got: {}",
        t.stdout
    );
    // The per-class breakdown names the class (here `task`, the fallback for a generic
    // "background job" summary), not just the lumped count.
    assert!(
        t.stdout.contains("(1 automation trigger: 1 task)"),
        "header must carry the per-class breakdown; got: {}",
        t.stdout
    );
}

#[test]
fn turns_json_emits_session_header_and_structured_automation() {
    // JSON consumers get (a) a leading {kind:"header",…} object carrying the
    // human/automation split + budget fan-out, and (b) STRUCTURED automation attribution on
    // the user-segment object (is_automation + trigger_kind + task_id + status) — not just a
    // text prefix to regex. A monitor-tick pulse renders trigger_kind "monitor".
    let h = Home::new();
    let sess = "22222222-3333-4444-5555-666666666666";
    let lines = [
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/m","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"On it."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>mon1</task-id>\n<status>completed</status>\n<summary>Monitor event: \"suite re-run completion\"</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Noted."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-m/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    let first = t.stdout.lines().next().unwrap_or("");
    assert!(
        first.contains("\"kind\":\"header\"")
            && first.contains("\"budget_is_per_session\":true")
            && first.contains("\"automation_triggers\":1"),
        "first JSON line must be the session_header with the automation split; got: {first}"
    );
    // The automation USER object carries the STRUCTURED attribution + the monitor kind.
    assert!(
        t.stdout.contains("\"is_automation\":true")
            && t.stdout.contains("\"trigger_kind\":\"monitor\"")
            && t.stdout.contains("\"task_id\":\"mon1\""),
        "the automation user segment must carry structured trigger fields; got: {}",
        t.stdout
    );
    // A HUMAN user object carries is_automation:false (and no trigger_kind).
    assert!(
        t.stdout.contains("\"is_automation\":false"),
        "a human user segment must carry is_automation:false; got: {}",
        t.stdout
    );
}

#[test]
fn interrupt_does_not_split_a_turn() {
    let h = holes_home();
    // The interrupt marker must NOT surface as its own genuine-user turn. Searching for
    // the marker under `user` yields nothing (it is not genuine-user).
    let out = h.run(&["search", "Request interrupted by user", "-t", "user"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges")
            || !out.stdout.contains("◂ user · [Request interrupted"),
        "interrupt must not be a genuine-user hit:\n{}",
        out.stdout
    );
    // And `list` must NOT pick the interrupt as the last-user preview — the real last
    // user message is the plan-rejection instruction.
    let lst = h.run(&["list"]);
    assert!(lst.success, "stderr: {}", lst.stderr);
    assert!(
        !lst.stdout.contains("[Request interrupted by user]"),
        "interrupt leaked into the list preview:\n{}",
        lst.stdout
    );
}

#[test]
fn turns_surfaces_image_ids_under_the_user_turn() {
    // The image marker shows the SAME `L<line>i<n>` id that `image --id` consumes, so a
    // turns reader can pull the bytes back without re-scanning.
    let h = image_home();
    let out = h.run(&["verbatim", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("L1i1"),
        "image id not surfaced in turns:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("image"),
        "no [N image(s)] marker:\n{}",
        out.stdout
    );
}

#[test]
fn turns_ellipsis_role_asymmetry_and_counts() {
    // The huge live round-trip: user > 600 → head 360 / tail 240; assistant > 900 →
    // head 594 / tail 306. The assistant head is strictly larger. The text output shows
    // the head + the elision marker + the tail; JSON carries the exact elided counts.
    let h = turns_home();
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    // The user head begins with HEADuser then 'u's; the marker carries the elided count.
    assert!(
        text.stdout.contains("HEADuser"),
        "user head present: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("TAILuser"),
        "user tail kept: {}",
        text.stdout
    );
    assert!(text.stdout.contains("HEADasst"), "asst head present");
    assert!(text.stdout.contains("TAILasst"), "asst tail kept");
    assert!(
        text.stdout.contains("chars elided") || text.stdout.contains("chars]"),
        "elision marker present: {}",
        text.stdout
    );

    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    // Find the huge user + huge assistant units (full_chars over the cap).
    let huge_user = objs
        .iter()
        .find(|o| o["role"] == "user" && o["full_chars"].as_u64().unwrap_or(0) > 600)
        .expect("huge user unit present");
    let huge_asst = objs
        .iter()
        .find(|o| o["role"] == "assistant" && o["full_chars"].as_u64().unwrap_or(0) > 900)
        .expect("huge assistant unit present");
    assert!(huge_user["truncated"].as_bool().unwrap());
    assert!(huge_asst["truncated"].as_bool().unwrap());
    // The assistant rendered_chars (900) is strictly larger than the user's (600) — the
    // role-asymmetric caps drive a larger assistant head.
    assert_eq!(huge_user["rendered_chars"].as_u64().unwrap(), 600);
    assert_eq!(huge_asst["rendered_chars"].as_u64().unwrap(), 900);
    assert!(
        huge_asst["rendered_chars"].as_u64().unwrap()
            > huge_user["rendered_chars"].as_u64().unwrap()
    );
    // elided_chars == full_chars - cap.
    assert_eq!(
        huge_user["elided_chars"].as_u64().unwrap(),
        huge_user["full_chars"].as_u64().unwrap() - 600
    );
    assert_eq!(
        huge_asst["elided_chars"].as_u64().unwrap(),
        huge_asst["full_chars"].as_u64().unwrap() - 900
    );
    // The JSON `text` field is the FULL verbatim message (un-truncated) — longer than
    // the rendered cap.
    assert!(huge_user["text"].as_str().unwrap().chars().count() > 600);
}

#[test]
fn turns_tool_call_markers_present_with_correct_counts() {
    // The fixture's huge live round-trip has 5 tool calls; turn "fifth ask" has 3.
    let h = turns_home();
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(
        text.stdout.contains("[5 tool calls]"),
        "5-tool marker: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("[3 tool calls]"),
        "3-tool marker present"
    );
    // A 0-tool turn omits the marker — "third reply" turn had 0 tools, so there is no
    // "[0 tool calls]" anywhere.
    assert!(
        !text.stdout.contains("[0 tool calls]"),
        "0-tool marker must be omitted"
    );
    // JSON carries the exact tool_calls count.
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    assert!(
        objs.iter()
            .any(|o| o["role"] == "user" && o["tool_calls"] == 5),
        "a unit with tool_calls==5 present"
    );
}

#[test]
fn turns_line_numbers_present_in_text_and_json() {
    let h = turns_home();
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    // Text lines carry L<number> markers (the jsonl line) for both roles.
    assert!(
        text.stdout.lines().any(|l| l.starts_with("▽ L")),
        "user lines carry L-numbers: {}",
        text.stdout
    );
    assert!(
        text.stdout.lines().any(|l| l.starts_with("△ L")),
        "assistant lines carry L-numbers"
    );
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    for o in json_lines(&json.stdout)
        .iter()
        .filter(|o| o.get("role").is_some())
    {
        assert!(
            o["line"].as_u64().unwrap() > 0,
            "every unit carries a positive line_no"
        );
        // full_chars == text.chars().count().
        assert_eq!(
            o["full_chars"].as_u64().unwrap() as usize,
            o["text"].as_str().unwrap().chars().count()
        );
    }
}

#[test]
fn turns_no_genuine_turns_emits_honest_empty_message() {
    // A session with NO genuine user turns (only a summary + an isMeta pseudo-turn +
    // tool noise) → nothing selected, an honest "no turns selected" message (never a
    // fabricated turn). This is the only empty-selection path: the most-recent complete
    // turn is always force-included when one exists (load-bearing).
    let h = Home::new();
    let mut s = String::new();
    s.push_str(r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"gone\""}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"a carrier, not a genuine turn"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{SESS}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no turns selected"),
        "honest empty message: {}",
        out.stdout
    );
}

#[test]
fn turns_assistant_only_orphan_lead_renders() {
    // A session that LEADS with an assistant before any user is rare but real; turns
    // must render the orphan assistant side without panicking (the user-None render
    // arms). group_turn_indices folds the lead into turn 0.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-000000000005";
    let mut s = String::new();
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"orphan lead before any user"}]}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"user","content":"the real first ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"real reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_reconstructs_auq_exchange_and_plan_rejection_with_pointer() {
    let h = holes_home();
    let out = h.run(&["verbatim", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The AUQ exchange is reconstructed as a complete unit: marker + question + options
    // + the answer prose.
    assert!(
        out.stdout.contains("AskUserQuestion"),
        "AUQ unit label missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("option A (recommended)"),
        "AUQ options missing:\n{}",
        out.stdout
    );
    // Each option's DESCRIPTION (supplementary note) must survive — not just the label.
    assert!(
        out.stdout
            .contains("the conservative path that reuses existing state"),
        "AUQ option description missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("the full path that rebuilds from scratch"),
        "second AUQ option description missing:\n{}",
        out.stdout
    );
    // Free-text notes the user attached to the answer must surface verbatim.
    assert!(
        out.stdout
            .contains("it is more involved than a quick tweak"),
        "AUQ answer notes missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("option A is fine, the scope is broader than stated"),
        "AUQ answer missing:\n{}",
        out.stdout
    );
    // The plan rejection surfaces the user's typed instruction AND a pointer to the
    // plan file.
    assert!(
        out.stdout
            .contains("please run the smoke tests once before calling it done"),
        "plan-rejection user message missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[plan: /Users/testuser/.claude/plans/elegant-scribbling-dream.md]"),
        "plan pointer missing:\n{}",
        out.stdout
    );
}
