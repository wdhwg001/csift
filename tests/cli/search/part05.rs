use crate::harness::*;

#[test]
fn search_redacted_thinking_is_agent_thinking() {
    // #12 / oracle B3 (G7): a `redacted_thinking` block (opaque encrypted reasoning, no readable
    // text) is UNATTESTED in the corpus, so this SYNTHETIC fixture exercises it. It must classify
    // `agent.thinking` and surface under `-t agent.thinking` as a `[redacted thinking]` placeholder
    // (never the opaque `data` blob). The search pattern `redacted` is present BOTH in the raw line
    // (the `redacted_thinking` type) — so the literal prefilter keeps the line — AND in the rendered
    // placeholder, so the regex locates a match on the emitted text.
    let h = Home::new();
    let sess = "abababab-cdcd-efef-0101-232323232323";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"abababab-cdcd-efef-0101-232323232323","cwd":"/Users/x/redact","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"think hard"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="},{"type":"text","text":"done"}]}}"#,
    ];
    h.write(
        &format!("-Users-x-redact/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    let out = h.run(&[
        "search",
        "redacted",
        "-t",
        "agent.thinking",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("agent.thinking"),
        "a redacted_thinking block must classify as agent.thinking; got: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("[redacted thinking]"),
        "a redacted_thinking block must render the placeholder, not the opaque data; got: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("EncryptedOpaqueBlob"),
        "the opaque `data` blob must NOT be surfaced; got: {}",
        out.stdout
    );
}

#[test]
fn search_renders_tool_use_result_pairing() {
    // GOLD §7: a tool_use joined to its tool_result by tool_use_id renders `▹`; an unreturned
    // tool_use renders `(no result — pending)`.
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
    // the richest (user.answer) view wins — ONE hit, not two.
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
fn count_by_label_census_respects_label_filters() {
    // R7 §2.3: `-t`/`-T` decide which records ENTER the census, and the label-axis KEYS pass
    // the same predicate — a dual-labeled record (an AUQ answer = user.answer +
    // agent.tool.result) must not leak its filtered-out twin into the census keys.
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

    // Filtered: only the SURVIVING label keys appear.
    let out = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "--no-subagents",
        "-t",
        "user",
        "-T",
        "user.message",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let keys: Vec<String> = json_rows(&out.stdout, "census")
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        keys,
        vec!["user.answer".to_string()],
        "census keys must pass the -t/-T predicate: {}",
        out.stdout
    );

    // Unfiltered: the FULL label set is still censused (both leaves of the dual record).
    let full = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "--no-subagents",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let full_keys: Vec<String> = json_rows(&full.stdout, "census")
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert!(
        full_keys.contains(&"user.answer".to_string())
            && full_keys.contains(&"agent.tool.result".to_string()),
        "no filter ⇒ full label sets: {}",
        full.stdout
    );
}

#[test]
fn list_scope_banner_reports_pre_cap_scope() {
    // R7 §2.4: the scope banner / JSON header answer "how big is the covered range" — the
    // row flood-guard (`--max-count` / the unscoped default cap) must never shrink them.
    let h = populated_home(); // 1 top-level + 2 subagent = 3 in scope
    let lj = h.run(&["list", "--max-count", "2", "--format", "json"]);
    assert!(lj.success, "stderr: {}", lj.stderr);
    let header: serde_json::Value =
        serde_json::from_str(lj.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(
        header["sessions_in_scope"], 3,
        "header scope must be PRE-cap: {header}"
    );
    assert_eq!(
        json_rows(&lj.stdout, "session").len(),
        2,
        "rows stay capped"
    );
    assert_eq!(json_summary(&lj.stdout)["dropped_by_cap"], 1);

    let lt = h.run(&["list", "--max-count", "2"]);
    assert!(
        lt.stdout.contains("3 sessions in scope"),
        "text banner must be PRE-cap: {}",
        lt.stdout
    );
}

#[test]
fn search_notification_with_result_renders_inbox_child_to_self_per_section() {
    // P3a G1 + G4/G5 + self-alias: a `<task-notification>` carrying a `<result>` is the background
    // agent's report → it classifies BOTH `harness.notification.*` (the pulse) AND
    // `agent.communication.inbox` (child ⇨ self, via the embedded `<tool-use-id>`). The render now
    // emits ONE hit PER label (per-section), the inbox hit carrying the RESOLVED child ⇨ self
    // direction — and the owner's own uuid renders as `self`.
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
    // P3a self-alias: the transcript owner's own uuid renders as `self` on EITHER side — a
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
