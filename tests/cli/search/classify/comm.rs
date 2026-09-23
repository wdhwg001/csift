//! The `agent.communication.*` family end to end: what a SendMessage, a spawn, a
//! subagent return and a batched notification each dedup to, and the `from > to`
//! direction every one of them renders.

use crate::harness::*;

#[test]
fn search_sendmessage_dedups_to_comm_sent_with_direction() {
    // Richest-view dedup + the comm DIRECTION: a `SendMessage` tool_use carries BOTH
    // `agent.tool.use` and `agent.communication.sent`; with no `-t` the richest (comm) view
    // wins (ONE hit), rendering `self ⇨ to`, and JSON `labels` still lists both.
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
    // Each per-section hit carries the FULL label union.
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

/// K2: a COORDINATOR relay is a delivered message, so an ordinary scan finds it. It is an
/// isMeta record - the harness wrote it into the conversation - so it opens no turn, and
/// the opener render answers nothing for it; it is no attachment either. The record
/// therefore classified `agent.communication.inbox` and emitted NOTHING: `show --line`
/// rendered it, a census read zero, and `-t agent.communication.inbox` found none of the
/// 141 such records across 89 files of one projects root. Its own text IS the message,
/// exactly as a peer relay's is.
#[test]
fn a_coordinator_relay_is_found_by_an_ordinary_scan() {
    let h = Home::new();
    let sess = "6e5d4c3b-2a19-4087-9654-3210fedcba98";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-13T01:00:00.000Z","message":{"role":"user","content":"zzopen the lane"}}"#, "\n",
            r#"{"type":"user","uuid":"c1","parentUuid":"u1","isMeta":true,"origin":{"kind":"coordinator"},"timestamp":"2026-06-13T01:00:01.000Z","message":{"role":"user","content":"The coordinator sent a message while you were working:\nzzrelay hold the release until the audit lands"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"c1","timestamp":"2026-06-13T01:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreply understood"}]}}"#, "\n",
        ),
    );

    // A FLAGLESS scan matches the relay's own text.
    let out = h.run(&["search", "zzrelay hold the release", &at(sess)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("zzrelay hold the release")
            && out.stdout.contains("agent.communication.inbox"),
        "a flagless scan finds the relay:\n{}\nstderr: {}",
        out.stdout,
        out.stderr
    );

    // ...and the census carries the key, which is what a reader asking "did anything
    // arrive in this lane" actually runs.
    let census = h.run(&["search", "", &at(sess), "--count-by", "label"]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        census.stdout.contains("1  agent.communication.inbox"),
        "the census counts it once:\n{}",
        census.stdout
    );

    // The leaf's own selector reaches it too.
    let named = h.run(&["search", "", &at(sess), "-t", "agent.communication.inbox"]);
    assert!(named.success, "stderr: {}", named.stderr);
    assert!(
        named.stdout.contains("zzrelay hold the release"),
        "naming the leaf reaches it:\n{}",
        named.stdout
    );

    // And the hit says where the message came FROM. Every other comm leaf renders an
    // arrow; without this one the relay was the only inbound message whose hit said
    // nothing about its sender. The fixture carries the `origin` object all 147 corpus
    // relays carry, which is what the arrow is read off.
    assert!(
        named.stdout.contains("coordinator ⇨ self"),
        "the relay renders its direction:\n{}",
        named.stdout
    );
    let json = h.run(&[
        "search",
        "",
        &at(sess),
        "-t",
        "agent.communication.inbox",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let hit = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|v| v["hits"].as_array().and_then(|a| a.first().cloned()))
        .expect("the relay's hit");
    assert_eq!(hit["from"], "coordinator", "{hit}");
    assert_eq!(hit["to"], "self", "{hit}");
}
