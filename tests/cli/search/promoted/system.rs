//! The `harness.system` family at the surface: one leaf per `type:"system"`
//! subtype, each gated behind a selector that reaches it, each rendering the
//! fabricated per-event-kind head the whole-file gate has to re-render.

use super::*;

/// The `harness.system` family, one leaf per subtype. L1 user . L2 an `informational`
/// warning (the Remote Control disconnect notice shape), L3 an `agents_killed`
/// record with a non-string content, L4 a `compact_boundary` (its own leaf, never
/// the catch-all), L5 assistant.
fn system_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"system","subtype":"informational","level":"warning","uuid":"i1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","content":"Remote Control disconnected - signed-in account changed on this machine - run /remote-control to start a session"}"#, "\n",
            r#"{"type":"system","subtype":"agents_killed","uuid":"k1","parentUuid":"i1","timestamp":"2026-06-07T05:00:02.000Z","content":{"count":2,"reason":"user"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:00:03.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":100}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"b1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn the_system_leaves_are_gated_rendered_and_gate_sound() {
    let h = system_home();
    // A bare scan never parses the line; the diagnosis names the gate.
    let bare = h.run(&["search", "Remote Control disconnected", &at(SESS)]);
    assert!(
        bare.success && bare.stdout.contains("no matching exchanges"),
        "{}",
        bare.stdout
    );
    assert!(
        bare.stderr.contains("harness.system.notice"),
        "{}",
        bare.stderr
    );
    // The CATEGORY reaches both records, each under its own event-kind leaf, rendered as
    // `[notice <level>] <content>` / `[kill] <json>`.
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.system.*"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.system.notice")
            && out
                .stdout
                .contains("[notice warning] Remote Control disconnected"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("harness.system.kill")
            && out.stdout.contains(r#"[kill] {"count":2,"reason":"user"}"#),
        "a kill notice is its own event kind, not the notice leaf:\n{}",
        out.stdout
    );
    // The compaction boundary keeps its own leaf: it is never a harness.system one.
    assert!(
        !out.stdout.contains("Conversation compacted"),
        "{}",
        out.stdout
    );
    // Both records sit in the same turn: one exchange, two hit lines.
    assert!(out.stdout.contains("matched 1 exchange"), "{}", out.stdout);
    // The category GLOB and the harness glob reach it too; neither the bare role nor the
    // bare category does - their visible face holds only harness.system.command.
    let prefix = h.run(&[
        "search",
        "Remote Control",
        &at(SESS),
        "-t",
        "harness.system.*",
    ]);
    assert!(
        prefix.stdout.contains("[notice warning]"),
        "{}",
        prefix.stdout
    );
    let role = h.run(&["search", "Remote Control", &at(SESS), "-t", "harness"]);
    assert!(
        role.stdout.contains("no matching exchanges"),
        "{}",
        role.stdout
    );
    // The whole-file gate stays sound: a pattern matching ONLY the fabricated head.
    let head = h.run(&[
        "search",
        r"\[notice warning\]",
        &at(SESS),
        "-t",
        "harness.system.notice",
    ]);
    assert!(
        head.stdout.contains("matched 1 exchange"),
        "{}",
        head.stdout
    );
    // JSON carries the leaf; `show --line` renders it flag-free (the refetch law).
    let js = h.run(&[
        "search",
        "Remote Control",
        &at(SESS),
        "-t",
        "harness.system.notice",
        "--format",
        "json",
    ]);
    assert!(
        js.stdout.contains(r#""label":"harness.system.notice""#),
        "{}",
        js.stdout
    );
    let shown = h.run(&["show", &at(SESS), "--line", "2"]);
    assert!(
        shown.success
            && shown
                .stdout
                .contains("[notice warning] Remote Control disconnected"),
        "{}\n{}",
        shown.stdout,
        shown.stderr
    );
    // The census counts it under its leaf.
    let census = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.*",
        "--count-by",
        "label",
    ]);
    assert!(
        census.stdout.contains("1  harness.system.notice")
            && census.stdout.contains("1  harness.system.kill"),
        "{}",
        census.stdout
    );
}
