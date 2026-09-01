//! search label taxonomy acceptance: role families render under their leaves.

use crate::harness::*;

#[test]
fn acceptance_communication_sent_spawn_and_subagent_opener() {
    // §C8 a Task/Agent spawn tool_use → `agent.communication.sent` (self ⇨ child); §C6 SendMessage
    // message → sent; §C9 the subagent transcript opener → `agent.communication.inbox` (parent ⇨ self).
    let h = acceptance_home();

    let spawn = acc(&h, "zzspawn", "agent.communication.sent");
    assert!(spawn.success, "C8: stderr {}", spawn.stderr);
    assert!(
        spawn.stdout.contains("agent.communication.sent")
            && spawn.stdout.contains("self ⇨ audit-x"),
        "C8 spawn → comm.sent (self ⇨ audit-x):\n{}",
        spawn.stdout
    );

    let sent = acc(&h, "zzsent", "agent.communication.sent");
    assert!(
        sent.stdout.contains("agent.communication.sent")
            && sent.stdout.contains("self ⇨ GraftBoard"),
        "C6 SendMessage message → comm.sent (self ⇨ GraftBoard):\n{}",
        sent.stdout
    );

    // C9 needs subagent span (the opener lives in the subagent transcript).
    let opener = h.run(&[
        "search",
        "zzopener",
        "-t",
        "agent.communication.inbox",
        &at(ACC_SESS),
    ]);
    assert!(opener.success, "C9: stderr {}", opener.stderr);
    assert!(
        opener.stdout.contains("agent.communication.inbox") && opener.stdout.contains("⇨ self"),
        "C9 subagent opener → comm.inbox (parent ⇨ self):\n{}",
        opener.stdout
    );
}

#[test]
fn slash_command_wrapper_extracted_in_both_tag_orders() {
    // The slash-command wrapper appears in TWO tag orders in real corpora: OLD
    // (`<command-name>` first) and NEW (`<command-message>` first - current CC).
    // Detection must catch both; the rendered body is `/name args` (never wrapper XML),
    // and a pattern INSIDE the args matches through the literal prefilter + whole-file
    // gate (args are verbatim raw substrings).
    let h = Home::new();
    let sess = "5c5d5e5f-2222-4333-8444-955566677788";
    let body = concat!(
        r#"{"type":"user","uuid":"c1","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args>old order zqxjkvold</command-args>"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a1","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"first reply"}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c2","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"<command-message>csift</command-message>\n<command-name>/csift</command-name>\n<command-args>new order zqxjkvnew</command-args>"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a2","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"second reply"}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c3","sessionId":"5c5d5e5f-2222-4333-8444-955566677788","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"<command-message>compact</command-message>\n<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
        "\n",
    );
    h.write(
        &format!("-Users-testuser-Projects-slash/{sess}.jsonl"),
        body,
    );
    let at = format!("@{sess}");

    // A literal inside the NEW-order args matches through prefilter/gate; the excerpt is
    // the extracted `/name args` form, never wrapper XML.
    let out = h.run(&["search", "zqxjkvnew", &at]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("/csift new order zqxjkvnew"),
        "extracted render: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("<command-args>"),
        "wrapper XML must not leak: {}",
        out.stdout
    );
    assert!(out.stdout.contains("user.message"), "{}", out.stdout);

    // Same for the OLD order.
    let out = h.run(&["search", "zqxjkvold", &at]);
    assert!(
        out.stdout.contains("/compact old order zqxjkvold"),
        "{}",
        out.stdout
    );

    // `show` renders the same extraction (shared engine).
    let out = h.run(&["show", &at, "--line", "3"]);
    assert!(
        out.stdout.contains("/csift new order zqxjkvnew"),
        "{}",
        out.stdout
    );

    // A NO-ARGS wrapper (either order) is machinery: never user.message, and it must NOT
    // count as a genuine turn opener (all three wrappers fold; no turn boundary shifts).
    let out = h.run(&["search", "", &at, "--count-by", "label", "--format", "json"]);
    let rows = json_rows(&out.stdout, "census");
    let user_msgs = rows
        .iter()
        .find(|r| r["key"] == "user.message")
        .and_then(|r| r["records"].as_u64())
        .unwrap_or(0);
    assert_eq!(user_msgs, 2, "only the two with-args wrappers: {rows:?}");
    let invocations = rows
        .iter()
        .find(|r| r["key"] == "harness.command.invocation")
        .and_then(|r| r["records"].as_u64())
        .unwrap_or(0);
    assert_eq!(invocations, 3, "all three wrappers: {rows:?}");

    // The explicit harness lens still reaches the wrapper form. (`-c` counts EXCHANGES,
    // and no wrapper opens a turn, so all three fold into the single turn-0 lead - the
    // per-RECORD count is the census assertion above.)
    let out = h.run(&["search", "", &at, "-t", "harness.command.invocation", "-c"]);
    assert_eq!(out.stdout.trim(), "1", "{}", out.stdout);
    let out = h.run(&["search", "", &at, "-t", "harness.command.invocation"]);
    assert!(
        out.stdout.contains("harness.command.invocation"),
        "{}",
        out.stdout
    );
}

#[test]
fn acceptance_user_role_message_shapes() {
    // §A1 string · §A2 text-block array · §A3 recovered <command-args> prose - all `user.message`.
    let h = acceptance_home();
    for (oracle, token) in [
        ("A1 string", "zzgenuine"),
        ("A2 text-block", "zzblocktext"),
        ("A3 command-args", "zzcmdargs"),
    ] {
        let out = acc(&h, token, "user.message");
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains("user.message"),
            "{oracle} must classify user.message:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_communication_signals_render_direction() {
    // §C2 bare-lead inbox · §C3 idle_notification · §C4 teammate_terminated · §C5 shutdown_approved
    // · §C7 SendMessage shutdown_request - each renders `from ⇨ to` (the owner side is `self`).
    let h = acceptance_home();
    let cases = [
        (
            "C2 inbox",
            "zzbareinbox",
            "agent.communication.inbox",
            "team-lead ⇨ self",
        ),
        (
            "C3 idle",
            "zzidle",
            "agent.communication.signal",
            "SOurDnd ⇨ self",
        ),
        (
            "C4 terminated",
            "zzterminated",
            "agent.communication.signal",
            "system ⇨ self",
        ),
        (
            "C5 approved",
            "zzapproved",
            "agent.communication.signal",
            "B38 ⇨ self",
        ),
        (
            "C7 shutdown_req",
            "zzshutdownreq",
            "agent.communication.signal",
            "self ⇨ GraftBoard",
        ),
    ];
    for (oracle, token, selector, dir) in cases {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains(dir),
            "{oracle} must render direction `{dir}`:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_harness_notification_monitor() {
    // §D4 / §G6 - a Monitor `<task-notification>` pulse (UNATTESTED in the corpus → synthetic) →
    // `harness.notification.monitor`, rendered as the `[monitor <id> <status>] <summary>` label.
    let h = acceptance_home();
    let out = acc(&h, "zzmonitor", "harness.notification.monitor");
    assert!(out.success, "D4: stderr {}", out.stderr);
    assert!(
        out.stdout.contains("harness.notification.monitor"),
        "D4/G6 monitor pulse → harness.notification.monitor:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("[monitor mon1"),
        "D4/G6 must render the automation_label, not raw XML:\n{}",
        out.stdout
    );
}

#[test]
fn acceptance_harness_command_and_interrupt() {
    // §D8 <command-name> wrapper · §D9 <local-command-stdout> · §D10/§D11 the two interrupt markers.
    let h = acceptance_home();
    for (oracle, token, selector) in [
        ("D8 invocation", "zzcmdargs", "harness.command.invocation"),
        ("D9 stdout", "zzstdout", "harness.command.stdout"),
        (
            "D10 interrupt.user",
            "interrupted by user",
            "harness.interrupt.user",
        ),
        (
            "D11 interrupt.tool",
            "interrupted by user for tool",
            "harness.interrupt.tool",
        ),
    ] {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_harness_schedule_and_meta() {
    // §D12 fired wakeup tick · §D13 continuation · §G2 meta.hook (stop-hook feedback) · §G2 meta.loop
    // (autonomous-loop driver). All ride on isMeta records that classify (not user.message).
    let h = acceptance_home();
    for (oracle, token, selector) in [
        ("D12 wakeup", "zzwakeup", "harness.schedule.wakeup"),
        (
            "D13 continuation",
            "Continue from where you left off",
            "harness.schedule.continuation",
        ),
        ("G2 meta.hook", "zzhook", "harness.meta.hook"),
        ("G2 meta.loop", "zzloop", "harness.meta.loop"),
    ] {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn search_teammate_message_is_inbox_not_user_regression() {
    // GOLD §1 / oracle §H: an inbound `<teammate-message>` is `type:user/role:user/string` and
    // matches NO synthetic marker, so the OLD `is_genuine_user` counted it as the human. The
    // cutover classifies it `agent.communication.inbox` (from ⇨ self) and DROPS it from `user`.
    let h = Home::new();
    let sess = "11111111-2222-3333-4444-555555555555";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"11111111-2222-3333-4444-555555555555","cwd":"/Users/x/team","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the human asks about throughput"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        r#"{"type":"user","uuid":"tm0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"VSMultiRegion\" color=\"blue\">\nplease check the rate limit handling\n</teammate-message>"}}"#,
    ];
    h.write(
        &format!("-Users-x-team/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    // Under `-t agent.communication.inbox` it surfaces WITH the `from ⇨ to` direction.
    let inbox = h.run(&[
        "search",
        "rate limit",
        "-t",
        "agent.communication.inbox",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("agent.communication.inbox"),
        "teammate message must classify as inbox; got: {}",
        inbox.stdout
    );
    assert!(
        inbox.stdout.contains("VSMultiRegion ⇨"),
        "the comm direction `from ⇨ to` must render; got: {}",
        inbox.stdout
    );

    // Under `-t user` it must NOT appear (the §1 bug fix) - the human turn does, the peer does not.
    let user = h.run(&[
        "search",
        "rate limit",
        "-t",
        "user",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(user.success, "stderr: {}", user.stderr);
    assert!(
        user.stdout.contains("no matching exchanges"),
        "a teammate message must NOT surface under -t user; got: {}",
        user.stdout
    );
}

#[test]
fn search_redacted_thinking_is_agent_thinking() {
    // #12 / oracle B3 (G7): a `redacted_thinking` block (opaque encrypted reasoning, no readable
    // text) is UNATTESTED in the corpus, so this SYNTHETIC fixture exercises it. It must classify
    // `agent.thinking` and surface under `-t agent.thinking` as a `[redacted thinking]` placeholder
    // (never the opaque `data` blob). The search pattern `redacted` is present BOTH in the raw line
    // (the `redacted_thinking` type) - so the literal prefilter keeps the line - AND in the rendered
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
fn search_quoted_tags_mid_prose_stay_user_message() {
    // FINDING-1: a genuine user message that merely QUOTES `<task-notification>` /
    // `<teammate-message>` mid-prose stays `user.message` - it is NOT reclassified
    // `harness.notification` / `agent.communication.inbox` (this bit csift's OWN dev sessions,
    // which quote these tags constantly).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"In csift the <task-notification> pulse and the <teammate-message peer form both route through classify zzquoted."}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ack"}]}}"#, "\n",
        ),
    );
    // Found under user.message …
    let um = h.run(&[
        "search",
        "zzquoted",
        "-t",
        "user.message",
        &at(SESS),
        "--no-subagents",
    ]);
    assert!(um.success, "stderr: {}", um.stderr);
    assert!(
        um.stdout.contains("user.message"),
        "FINDING-1: quoted tags stay user.message:\n{}",
        um.stdout
    );
    // … and NOT under harness.notification …
    let notif = h.run(&[
        "search",
        "zzquoted",
        "-t",
        "harness.notification",
        &at(SESS),
        "--no-subagents",
    ]);
    assert!(notif.success, "stderr: {}", notif.stderr);
    assert!(
        notif.stdout.contains("no matching exchanges"),
        "FINDING-1: a quoted <task-notification> is not a notification:\n{}",
        notif.stdout
    );
    // … and NOT under agent.communication.inbox.
    let inbox = h.run(&[
        "search",
        "zzquoted",
        "-t",
        "agent.communication.inbox",
        &at(SESS),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("no matching exchanges"),
        "FINDING-1: a quoted <teammate-message> is not inbox:\n{}",
        inbox.stdout
    );
}

#[test]
fn acceptance_compaction_summary_and_boundary_searchable() {
    // §D6 the isCompactSummary record is a `type:"user"` record → searchable as
    // `harness.compaction.summary`. §D7 (user-reversed): the `compact_boundary` is a `type:"system"`
    // record NOW ALSO search-surfaced - the §7 prefilter keeps it (one memmem on `compact_boundary`)
    // and `record_raw_text` renders its top-level content + compactMetadata as the match/excerpt, so
    // compaction points can be enumerated + inspected.
    let h = acceptance_home();

    let summary = acc(&h, "zzsummary", "harness.compaction.summary");
    assert!(summary.success, "D6: stderr {}", summary.stderr);
    assert!(
        summary.stdout.contains("harness.compaction.summary"),
        "D6 compaction summary → searchable:\n{}",
        summary.stdout
    );

    let boundary = acc(&h, "zzboundary", "harness.compaction.boundary");
    assert!(boundary.success, "D7: stderr {}", boundary.stderr);
    assert!(
        boundary.stdout.contains("harness.compaction.boundary"),
        "D7 compact_boundary → now searchable:\n{}",
        boundary.stdout
    );
    // The compactMetadata renders as the excerpt (trigger / pre/post tokens / duration).
    assert!(
        boundary.stdout.contains("trigger") && boundary.stdout.contains("auto"),
        "D7 boundary excerpt carries its compactMetadata:\n{}",
        boundary.stdout
    );
}

#[test]
fn acceptance_excluded_and_unmarked_meta_carry_no_label() {
    // §E an `attachment` carries no label; §J an isMeta record matching no harness marker is EXCLUDED
    // (never `user.message`). Neither surfaces under ANY selector.
    let h = acceptance_home();
    for (oracle, token) in [
        ("E attachment", "zzattach"),
        ("J isMeta-unmarked", "zzunmarked"),
    ] {
        // No `-t` → every label eligible; still nothing, because classify returns empty.
        let out = h.run(&["search", token, &at(ACC_SESS), "--no-subagents"]);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains("no matching exchanges"),
            "{oracle} must carry no label (no hit):\n{}",
            out.stdout
        );
    }
}

#[test]
fn harness_role_excludes_the_boundary_and_the_drilldown_keeps_it() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"2026-06-07T05:01:00.000Z","content":"Compacted","compactMetadata":{"trigger":"auto","preTokens":900,"postTokens":90,"durationMs":4}}"#, "\n",
        ),
    );
    // The bare role: the metrics-only boundary is not the conversation.
    let role = h.run(&["search", "", &at(SESS), "-t", "harness"]);
    assert!(
        !role.stdout.contains("compaction boundary"),
        "-t harness excludes the boundary:\n{}",
        role.stdout
    );
    // The deliberate drill-down keeps its full set - compaction points stay findable.
    let drill = h.run(&["search", "", &at(SESS), "-t", "harness.compaction"]);
    assert!(
        drill.stdout.contains("trigger=auto"),
        "-t harness.compaction still reaches the boundary:\n{}",
        drill.stdout
    );
    // And the glob reaches everything too.
    let glob = h.run(&["search", "", &at(SESS), "-t", "harness.*"]);
    assert!(glob.stdout.contains("trigger=auto"), "{}", glob.stdout);
}
