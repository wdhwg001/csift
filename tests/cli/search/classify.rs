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
    // Its tool_use (L3) is in scope, so the label renders the `▹` pair; the `Read` tool name still
    // trails it (so the `agent.tool.result Read` substring holds).
    assert!(
        out.stdout.contains("agent.tool.result Read"),
        "the response names the tool it answers: {}",
        out.stdout
    );
}

#[test]
fn turns_and_search_label_automation_triggers() {
    // A `<task-notification>` automation trigger opens a turn but must render as the
    // parsed `[<kind> <id> …]` ATTRIBUTION label - with the TRUE kind parsed from the
    // summary (a `Background command "…"` summary renders `background-command`, NOT the old
    // blanket `workflow`) - never the raw XML blob - and `turns` reports the automation
    // count in its header.
    let h = Home::new();
    let sess = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/Users/x/p","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the build please"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Starting the build now."}]}}"#,
        r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>wf12abc</task-id>\n<tool-use-id>toolu_z</tool-use-id>\n<output-file>/tmp/wf12abc.output</output-file>\n<status>completed</status>\n<summary>Background command \"Run the build\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"n0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The build finished cleanly."}]}}"#,
        // A SECOND automation trigger → exercises the PLURAL header arm (N == 2).
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
    // `<task-notification>` now classifies as `harness.notification.background-command` (NOT
    // `user` - the §1 reparent), so it surfaces under that selector (or `-t harness.notification`).
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
    // GOLD §7: a tool_use joined to its tool_result by tool_use_id renders `▹`; an unreturned
    // tool_use renders `(no result - pending)`.
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
fn search_sendmessage_dedups_to_comm_sent_with_direction() {
    // GOLD §3 Q4 dedup + §4 direction: a `SendMessage` tool_use carries BOTH `agent.tool.use` and
    // `agent.communication.sent`; with no `-t` the richest (comm) view wins (ONE hit), rendering
    // `self ⇨ to`, and JSON `labels` still lists both.
    let h = Home::new();
    let sess = "33333333-4444-5555-6666-777777777777";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"33333333-4444-5555-6666-777777777777","cwd":"/Users/x/send","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"coordinate"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sm1","name":"SendMessage","input":{"to":"GraftBoard","type":"message","message":"ship the zzmsg fix"}}]}}"#,
    ];
    h.write(
        &format!("-Users-x-send/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    let j = h.run(&[
        "search",
        "zzmsg",
        at(sess).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    let hits = env["hits"].as_array().expect("hits");
    assert_eq!(
        hits.len(),
        1,
        "the dual-label SendMessage emits ONCE: {}",
        j.stdout
    );
    assert_eq!(hits[0]["label"], "agent.communication.sent");
    assert_eq!(hits[0]["to"], "GraftBoard");
    let labels = hits[0]["labels"].as_array().expect("labels");
    assert!(
        labels.iter().any(|l| l == "agent.tool.use")
            && labels.iter().any(|l| l == "agent.communication.sent"),
        "labels lists the full set: {}",
        j.stdout
    );
}

#[test]
fn search_auq_answer_dedups_to_user_answer() {
    // GOLD §3 Q4: an AUQ answer carries BOTH `user.answer` and `agent.tool.result`; with no `-t`
    // the richest (user.answer) view wins - ONE hit, not two.
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
fn search_notification_with_result_renders_inbox_child_to_self_per_section() {
    // P3a G1 + G4/G5 + self-alias: a `<task-notification>` carrying a `<result>` is the background
    // agent's report → it classifies BOTH `harness.notification.*` (the pulse) AND
    // `agent.communication.inbox` (child ⇨ self, via the embedded `<tool-use-id>`). The render now
    // emits ONE hit PER label (per-section), the inbox hit carrying the RESOLVED child ⇨ self
    // direction - and the owner's own uuid renders as `self`.
    let enc = "-Users-x-bg";
    let sess = "12121212-3434-5656-7878-9a9a9a9a9a9a";
    let child = "c0ffee1234567890";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"12121212-3434-5656-7878-9a9a9a9a9a9a","cwd":"/Users/x/bg","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"launch the bg agent"}}"#, "\n",
            r#"{"type":"user","uuid":"n0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>bgjob</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<status>completed</status>\n<summary>Background command \"zzboth probe\" completed (exit code 0)</summary>\n<result>zzboth the agent reported done</result>\n</task-notification>"}}"#, "\n",
        ),
    );
    // The discovered subagent whose spawn `toolUseId` == the notification's `<tool-use-id>`, so the
    // inbox FROM resolves to this child id (the id-join).
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{child}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"c0ffee1234567890","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"do the bg work"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{child}.meta.json"),
        r#"{"agentType":"executor","toolUseId":"toolu_bg"}"#,
    );

    // Default -t (all): the ONE notification record → TWO per-section hits.
    let j = h.run(&[
        "search",
        "zzboth",
        at(sess).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    let hits = env["hits"].as_array().expect("hits");
    assert_eq!(
        hits.len(),
        2,
        "a notification-with-<result> renders per-section (notification + inbox): {}",
        j.stdout
    );
    let notif = hits
        .iter()
        .find(|x| x["label"] == "harness.notification.background-command")
        .expect("the notification hit");
    assert!(
        notif["from"].is_null(),
        "the notification view has no direction"
    );
    let inbox = hits
        .iter()
        .find(|x| x["label"] == "agent.communication.inbox")
        .expect("the inbox hit");
    assert_eq!(inbox["from"], child, "inbox FROM = the resolved child id");
    assert_eq!(inbox["to"], "self", "inbox TO = self (the owner alias)");

    // Under `-t agent.communication.inbox` the text render shows the `child ⇨ self` direction.
    let t = h.run(&[
        "search",
        "zzboth",
        "-t",
        "agent.communication.inbox",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains(&format!("{child} ⇨ self")),
        "the inbox hit renders the resolved child ⇨ self direction; got: {}",
        t.stdout
    );
}

#[test]
fn search_batched_cross_family_sections_render_per_section() {
    // P3a G4/G5: ONE `type:user` record batching a `<task-notification>` AND an inbound
    // `<teammate-message>` of MIXED family → the render emits ONE hit PER section, each with its
    // OWN label + direction (not one collapsed richest-label hit). A shared token in both sections
    // surfaces both; `labels` lists the full union on each hit.
    let enc = "-Users-x-mix";
    let sess = "13131313-2424-3535-4646-575757575757";
    let h = Home::new();
    let batched = "<task-notification>\\n<task-id>mixjob</task-id>\\n<status>completed</status>\\n<summary>Background command \\\"zzmix build\\\" completed (exit code 0)</summary>\\n</task-notification>\\nAnother Claude session sent a message:\\n<teammate-message teammate_id=\\\"PeerOne\\\">zzmix please rebase</teammate-message>";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"u0","sessionId":"13131313-2424-3535-4646-575757575757","cwd":"/Users/x/mix","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#, "\n",
                r#"{{"type":"user","uuid":"b0","timestamp":"2026-06-07T05:10:00.000Z","message":{{"role":"user","content":"{batched}"}}}}"#, "\n",
            ),
            batched = batched,
        ),
    );

    let j = h.run(&[
        "search",
        "zzmix",
        at(sess).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    let hits = env["hits"].as_array().expect("hits");
    assert_eq!(
        hits.len(),
        2,
        "the cross-family batched record renders per-section (notification + inbox): {}",
        j.stdout
    );
    assert!(
        hits.iter()
            .any(|x| x["label"] == "harness.notification.background-command"),
        "the notification section surfaces: {}",
        j.stdout
    );
    let inbox = hits
        .iter()
        .find(|x| x["label"] == "agent.communication.inbox")
        .expect("the teammate inbox section");
    assert_eq!(inbox["from"], "PeerOne", "inbox FROM = the peer sender");
    assert_eq!(inbox["to"], "self");
    // Each per-section hit carries the FULL label union (GOLD §3).
    let labels = inbox["labels"].as_array().expect("labels");
    assert!(
        labels
            .iter()
            .any(|l| l == "harness.notification.background-command")
            && labels.iter().any(|l| l == "agent.communication.inbox"),
        "labels lists the full cross-family union: {}",
        j.stdout
    );
}

#[test]
fn search_comm_direction_aliases_owner_to_self_both_sides() {
    // P3a self-alias: the transcript owner's own uuid renders as `self` on EITHER side - a
    // `SendMessage` (sent) shows `self ⇨ <peer>` (FROM aliased), an inbound `<teammate-message>`
    // (inbox) shows `<peer> ⇨ self` (TO aliased); the peer id is kept verbatim.
    let enc = "-Users-x-self";
    let sess = "14141414-2525-3636-4747-585858585858";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"14141414-2525-3636-4747-585858585858","cwd":"/Users/x/self","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"coordinate"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sm1","name":"SendMessage","input":{"to":"GraftBoard","type":"message","message":"ship the zzsend fix"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"tm0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"GraftBoard\">zzrecv looks good</teammate-message>"}}"#, "\n",
        ),
    );

    let sent = h.run(&[
        "search",
        "zzsend",
        "-t",
        "agent.communication.sent",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(sent.success, "stderr: {}", sent.stderr);
    assert!(
        sent.stdout.contains("self ⇨ GraftBoard"),
        "sent renders self ⇨ peer (FROM aliased); got: {}",
        sent.stdout
    );

    let inbox = h.run(&[
        "search",
        "zzrecv",
        "-t",
        "agent.communication.inbox",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("GraftBoard ⇨ self"),
        "inbox renders peer ⇨ self (TO aliased); got: {}",
        inbox.stdout
    );
}

#[test]
fn search_subagent_scope_spawn_lookup_resolves_in_subagent_spawn_and_return() {
    // P3a subagent-scope spawn lookup: when scanning a SUBAGENT transcript, the spawn lookup is
    // built from its PARENT session (whose sidecar holds the flat set of ALL subagents), so an
    // IN-SUBAGENT spawn resolves `self ⇨ <grandchild>` and an in-subagent Task-return resolves to
    // `agent.communication.inbox <grandchild> ⇨ self`. (Before the fix the lookup was top-level
    // only → both degraded.)
    let enc = "-Users-x-nest";
    let sess = "15151515-2626-3737-4848-595959595959";
    let agent_a = "aaaa1111bbbb2222";
    let agent_b = "bbbb3333cccc4444";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"15151515-2626-3737-4848-595959595959","cwd":"/Users/x/nest","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"delegate"}}"#, "\n",
        ),
    );
    // Subagent A: spawns a Task (id=toolu_inner) and later receives its return (same id).
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_a}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaaa1111bbbb2222","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"do the parent-of-nest work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_inner","name":"Task","input":{"description":"zzspawn grandchild","subagent_type":"executor"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sr0","timestamp":"2026-06-07T05:30:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_inner","content":"zzreturn grandchild done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_a}.meta.json"),
        r#"{"agentType":"executor","toolUseId":"toolu_outerA"}"#,
    );
    // Grandchild B: discovered under the SAME parent dir; its spawn `toolUseId` is the IN-SUBAGENT
    // Task tool_use id, so the lookup (built from the parent) joins toolu_inner → B.
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_b}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"bbbb3333cccc4444","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":"grandchild seed"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{agent_b}.meta.json"),
        r#"{"agentType":"executor","toolUseId":"toolu_inner"}"#,
    );

    // The in-subagent SPAWN resolves `self ⇨ <grandchild>` (scan only A via --no-subagents).
    let sent = h.run(&[
        "search",
        "zzspawn",
        "-t",
        "agent.communication.sent",
        at(agent_a).as_str(),
        "--no-subagents",
    ]);
    assert!(sent.success, "stderr: {}", sent.stderr);
    assert!(
        sent.stdout.contains(&format!("self ⇨ {agent_b}")),
        "in-subagent spawn must resolve self ⇨ grandchild; got: {}",
        sent.stdout
    );

    // The in-subagent Task-RETURN resolves to `agent.communication.inbox <grandchild> ⇨ self`.
    let inbox = h.run(&[
        "search",
        "zzreturn",
        "-t",
        "agent.communication.inbox",
        at(agent_a).as_str(),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("agent.communication.inbox"),
        "in-subagent Task-return must surface as inbox; got: {}",
        inbox.stdout
    );
    assert!(
        inbox.stdout.contains(&format!("{agent_b} ⇨ self")),
        "the return resolves grandchild ⇨ self; got: {}",
        inbox.stdout
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
    //     under `user` - it IS the user's typed message. (Regression: previously dropped,
    //     so this returned "no matching exchanges".)
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
