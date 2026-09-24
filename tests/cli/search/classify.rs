//! search classification semantics: dedup to richest view, pairing, directions.

use crate::harness::*;

#[test]
fn search_tool_response_names_the_tool_it_answers() {
    let h = populated_home();
    // Fixture L4 = a tool_result for tool_use_id `call0`, whose tool_use (L3) is `Read`.
    let out = h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "agent.tool.result",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Its tool_use (L3) is in scope, so the label renders the `▹` pair; the provenance root
    // (the opener of its turn, three parent hops up) follows, and the `Read` tool name still
    // trails the whole zone.
    assert!(
        out.stdout.contains("agent.tool.result [root L1 +3] Read"),
        "the response names the tool it answers: {}",
        out.stdout
    );
}

#[test]
fn turns_and_search_label_automation_triggers() {
    // A `<task-notification>` automation trigger opens a turn but must render as the
    // parsed `[<kind> <id> ...]` ATTRIBUTION label - with the TRUE kind parsed from the
    // summary (a `Background command "..."` summary renders `background-command`, never a
    // blanket `workflow`) - never the raw XML blob - and `turns` reports the automation
    // count in its header.
    let h = Home::new();
    let sess = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/Users/x/p","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the build please"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Starting the build now."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf12abc</task-id>\n<tool-use-id>toolu_z</tool-use-id>\n<output-file>/tmp/wf12abc.output</output-file>\n<status>completed</status>\n<summary>Background command \"Run the build\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The build finished cleanly."}]}}"#,
        // A SECOND automation trigger -> exercises the PLURAL header arm (N == 2).
        r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:20:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf34def</task-id>\n<status>completed</status>\n<summary>Background command \"Run the tests\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"n1","timestamp":"2026-06-07T05:20:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"All tests passed."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-p/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    // turns: the header reports the automation count; the body shows the attribution label
    // and never the raw XML.
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
            .contains("(2 automation triggers: 2 background-command)"),
        "header must report the plural automation count + per-class breakdown; got: {}",
        t.stdout
    );
    assert!(
        t.stdout.contains("[background-command wf12abc completed]"),
        "automation opener must render with its TRUE kind (background-command); got: {}",
        t.stdout
    );
    assert!(
        !t.stdout.contains("<task-notification>") && !t.stdout.contains("<output-file>"),
        "raw task-notification XML must NOT appear; got: {}",
        t.stdout
    );

    // search: the same attribution label is matchable; the raw blob is not surfaced. The
    // `<task-notification>` classifies as `harness.notification.background-command` rather
    // than `user` (a machine trigger is not the operator), so it surfaces under that
    // selector (or `-t harness.notification`).
    let s = h.run(&[
        "search",
        "background-command",
        "-t",
        "harness.notification.background-command",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains("[background-command wf12abc completed]"),
        "search must surface the attribution label under harness.notification; got: {}",
        s.stdout
    );
    // And it must NOT surface under `-t user` anymore (the reparent - regression guard).
    let not_user = h.run(&[
        "search",
        "background-command",
        "-t",
        "user",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(not_user.success, "stderr: {}", not_user.stderr);
    assert!(
        not_user.stdout.contains("no matching exchanges"),
        "a <task-notification> must NOT surface under -t user; got: {}",
        not_user.stdout
    );
    assert!(
        !s.stdout.contains("<output-file>"),
        "search must not surface the raw XML wrapper; got: {}",
        s.stdout
    );
}

#[test]
fn search_renders_tool_use_result_pairing() {
    // The PAIRING marker: a tool_use joined to its tool_result by tool_use_id renders `▹`;
    // an unreturned tool_use renders `(no result - pending)`.
    let h = Home::new();
    let sess = "22222222-3333-4444-5555-666666666666";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"22222222-3333-4444-5555-666666666666","cwd":"/Users/x/pair","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"run it"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call1","name":"Bash","input":{"command":"echo zzpaired"}}]}}"#,
        r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call1","content":"zzpaired done"}]}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"c0","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call2","name":"Bash","input":{"command":"echo zzpending"}}]}}"#,
    ];
    h.write(
        &format!("-Users-x-pair/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    let paired = h.run(&[
        "search",
        "zzpaired",
        "-t",
        "agent.tool",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(paired.success, "stderr: {}", paired.stderr);
    assert!(
        paired.stdout.contains("agent.tool.use ▹ agent.tool.result"),
        "a use joined to its result renders ▹; got: {}",
        paired.stdout
    );

    let pending = h.run(&[
        "search",
        "zzpending",
        "-t",
        "agent.tool.use",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(pending.success, "stderr: {}", pending.stderr);
    assert!(
        pending
            .stdout
            .contains("agent.tool.use (no result — pending)"),
        "an unreturned use renders the pending note; got: {}",
        pending.stdout
    );
}

#[test]
fn search_auq_answer_dedups_to_user_answer() {
    // The richest-view dedup: an AUQ answer carries BOTH `user.answer` and
    // `agent.tool.result`; with no `-t` the richest (user.answer) view wins - ONE hit, not two.
    let h = Home::new();
    let sess = "44444444-5555-6666-7777-888888888888";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"44444444-5555-6666-7777-888888888888","cwd":"/Users/x/auq","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the zzopt option\". You can now continue."}]}}"#,
    ];
    h.write(
        &format!("-Users-x-auq/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    let j = h.run(&[
        "search",
        "zzopt",
        at(sess).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    let hits = env["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "the AUQ answer emits ONCE: {}", j.stdout);
    assert_eq!(hits[0]["label"], "user.answer");
}

#[test]
fn orphan_reconciliation_pulse_renders_every_task_id_and_its_kind() {
    // ONE pulse closing SEVERAL tasks (the next session start reconciling what the previous
    // one left open) plus the `__orphan_summary__:<kind>` sentinel, which names no task. The
    // label must carry every real id and the reconciliation kind - never the sentinel - and
    // the hit row must carry the same ids as data.
    let h = Home::new();
    let sess = "cccccccc-dddd-eeee-ffff-000000000000";
    let pulse = "<task-notification>\\n<task-id>bzz111111</task-id>\\n<task-id>bzz222222</task-id>\\n<task-id>bzz333333</task-id>\\n<task-id>__orphan_summary__:shell</task-id>\\n<status>stopped</status>\\n<summary>3 background shell task(s) from the previous session have no completion record.</summary>\\n</task-notification>";
    let line = format!(
        r#"{{"type":"user","uuid":"n0","sessionId":"{sess}","cwd":"/Users/x/p","timestamp":"2026-06-07T06:00:00.000Z","message":{{"role":"user","content":"{pulse}"}}}}"#
    );
    h.write(&format!("-Users-x-p/{sess}.jsonl"), &(line + "\n"));
    let args = ["search", "completion record", "-t", "harness.notification"];
    let out = h.run(&[&args[..], &[at(sess).as_str()]].concat());
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "[task bzz111111, bzz222222, bzz333333 stopped] (orphan reconciliation: shell)"
        ),
        "every task id + the reconciliation kind belong in the label:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("[task __orphan_summary__"),
        "the sentinel names no task and must never hold the id slot:\n{}",
        out.stdout
    );
    let js = h.run(&[&args[..], &[at(sess).as_str(), "--format", "json"]].concat());
    assert!(js.success, "stderr: {}", js.stderr);
    assert!(
        js.stdout
            .contains(r#""task_ids":["bzz111111","bzz222222","bzz333333"]"#)
            && js.stdout.contains(r#""orphan_kind":"shell""#),
        "the hit row carries every real id + the kind as data:\n{}",
        js.stdout
    );
}

#[test]
fn search_finds_auq_option_descriptions_and_answer_notes_under_user() {
    let h = holes_home();
    // (1) A phrase that lives ONLY in an option's `description` must be searchable in the
    //     reconstructed USER turn (not merely in the raw assistant tool-call JSON).
    let desc = h.run(&[
        "search",
        "the conservative path that reuses existing state",
        "-t",
        "user",
        at(SESS).as_str(),
    ]);
    assert!(desc.success, "stderr: {}", desc.stderr);
    assert!(
        desc.stdout
            .contains("the conservative path that reuses existing state"),
        "option description not searchable under user:\n{}",
        desc.stdout
    );
    // (2) A phrase that lives ONLY in the answer's `annotations.notes` must be searchable
    //     under `user` - it IS the user's typed message, and an AUQ render that reads the
    //     `answers` map alone drops it, leaving this search "no matching exchanges".
    let notes = h.run(&[
        "search",
        "more involved than a quick tweak",
        "-t",
        "user",
        at(SESS).as_str(),
    ]);
    assert!(notes.success, "stderr: {}", notes.stderr);
    assert!(
        notes.stdout.contains("more involved than a quick tweak"),
        "answer notes not searchable under user:\n{}",
        notes.stdout
    );
}

/// An assistant turn carrying TEXT and an IMAGE is ONE hit under one leaf. Give the image
/// block a `harness.context.image` of its own and the turn emits two hits, the harness view
/// printed first while `labels[]`, ordered richest-first, puts the plain message first - the
/// emission order and the label order disagreeing about a single record.
/// `harness.context.image` is a SHAPE the harness writes (the isMeta image-only user
/// record, and the `[Image: ` reference in its text form), and over two projects roots an
/// image block rides an assistant record zero times.
#[test]
fn an_assistant_turn_with_text_and_an_image_is_one_hit() {
    let h = Home::new();
    let sess = "3b2a1908-7e6f-4d5c-9b8a-1c2d3e4f5a6b";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-11T01:00:00.000Z","message":{"role":"user","content":"zzask for the chart"}}"#, "\n",
            r#"{"type":"assistant","uuid":"s1","parentUuid":"u1","timestamp":"2026-06-11T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzchart here is the reef plot"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBOR"}}]}}"#, "\n",
        ),
    );

    let out = h.run(&["search", "zzchart", &at(sess), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        // The exchange row is the one carrying `hits`; the envelope's header and summary
        // rows carry none, and keying on their `kind` would depend on key ORDER.
        .flat_map(|v| {
            v.get("hits")
                .and_then(|h| h.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "one record, one hit - got {:?}",
        hits.iter()
            .map(|h| h.get("label").cloned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        hits[0].get("labels"),
        Some(&serde_json::json!(["agent.message.plain"])),
        "and one leaf: {}",
        hits[0]
    );
}

mod comm;
mod continuation;
