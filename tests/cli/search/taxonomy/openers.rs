//! What the PERSON said, and the shapes their turn arrives in: the three message
//! forms, the slash wrapper in both tag orders, a freeform AskUserQuestion answer, and
//! the quoted-tag case that must stay theirs.

use crate::harness::*;

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
fn search_quoted_tags_mid_prose_stay_user_message() {
    // The section-boundary rule: a genuine user message that merely QUOTES
    // `<task-notification>` / `<teammate-message>` mid-prose stays `user.message` - it is
    // NOT reclassified `harness.notification` / `agent.communication.inbox`. Without the
    // boundary test this bites csift's OWN dev sessions, which quote these tags constantly.
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
fn a_freeform_askuserquestion_response_is_a_user_answer() {
    // A carrier whose toolUseResult carries `response` and an empty `answers` map (the
    // harness's `The user responded:` branch) is a user.answer hit rendering the
    // response, and the marker text is prefilter-safe under the whole-file gate.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "45454545-5656-4767-8878-989898989898";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick a route"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ask1","name":"AskUserQuestion","input":{"questions":[{"question":"Which harbor?","header":"Harbor","options":[{"label":"north","description":"a"},{"label":"south","description":"b"}],"multiSelect":false}]}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ask1","content":"The user responded: skip the harbor, go by land"}]},"toolUseResult":{"questions":[{"question":"Which harbor?","header":"Harbor","options":[{"label":"north","description":"a"},{"label":"south","description":"b"}],"multiSelect":false}],"answers":{},"response":"skip the harbor, go by land"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"r1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"by land it is"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "go by land", &at(sess), "-t", "user.answer"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.answer")
            && out.stdout.contains("response: skip the harbor, go by land"),
        "{}",
        out.stdout
    );
    let count = h.run(&["search", "", &at(sess), "-t", "user.answer", "-c"]);
    assert_eq!(count.stdout.trim(), "1", "{}", count.stdout);
    // The synthesized head alone (never a raw substring of the line beyond the marker)
    // still matches: the marker is a verifiable synth needle.
    let head = h.run(&["search", "AskUserQuestion · 1 question", &at(sess), "-c"]);
    assert_eq!(head.stdout.trim(), "1", "{}", head.stdout);
}
