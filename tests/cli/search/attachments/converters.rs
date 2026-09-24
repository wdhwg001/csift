//! Two payload types the converter treats in OPPOSITE ways, and what each reaches.
//!
//! `mcp_instructions_delta` is turned into a user message the model reads, so its record
//! carries the conversion as a top-level `rendered` and the unit renders that. The
//! thinking-drop accounting has an explicit empty arm, so its record carries no
//! conversion at all and the unit renders the payload verbatim. Both are gated: an
//! attachment line reaches a scan only through `-t` or `--attachments`.

use crate::harness::*;

const SESS: &str = "11111111-2222-4333-8444-555555555503";
const PROJ: &str = "-Users-dev-converters";

fn home() -> Home {
    let lines = [
        r#"{"type":"user","uuid":"m0","sessionId":"11111111-2222-4333-8444-555555555503","cwd":"/Users/dev/converters","version":"2.1.278","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzconv list the servers"}}"#,
        // L2 the MCP instructions delta, with the conversion the harness persisted
        // The conversion is an ARRAY of content parts, which is the shape the harness
        // persists; `r##` because a `"#` inside an `r#` string closes it and the
        // conversion carries a Markdown heading.
        r##"{"type":"attachment","uuid":"m1","parentUuid":"m0","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"mcp_instructions_delta","addedNames":["zzconv-server"],"addedBlocks":[{"serverName":"zzconv-server","block":"zzconv use the probe tool first"}],"removedNames":[]},"rendered":[{"content":"<system-reminder>\n# MCP Server Instructions\n\nzzconv use the probe tool first\n</system-reminder>"}]}"##,
        // L3 the thinking-drop accounting, which converts to nothing and so carries none
        r#"{"type":"attachment","uuid":"m2","parentUuid":"m1","timestamp":"2026-06-07T05:00:02.000Z","attachment":{"type":"thinking_drop","requestId":"req_zzconv","model":"a-model","querySource":"repl_main_thread","thinkingBlocksSent":82,"thinkingTurnsSent":82,"newlyDropped":{"blockCount":82,"turnCount":82,"reason":"prefix_mismatch"}}}"#,
        r#"{"type":"assistant","uuid":"m3","parentUuid":"m2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzconv one server joined"}]}}"#,
    ];
    let h = Home::new();
    h.write(&format!("{PROJ}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

#[test]
fn both_payloads_carry_their_leaf_and_neither_reaches_a_flagless_scan() {
    let h = home();
    // GATED: an attachment line is not parsed by a scan that did not ask for one, so the
    // only hits here are the human turn and the assistant reply.
    let bare = h.run(&["search", "zzconv", at(SESS).as_str(), "--no-subagents"]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        !bare.stdout.contains("harness.context.listing")
            && !bare.stdout.contains("harness.context.task"),
        "a flagless scan reaches neither: {}",
        bare.stdout
    );
    // The listing leaf, which is where the SERVER INSTRUCTIONS sit: an inventory the
    // harness injects, and the one member of that group the converter turns into content.
    let listing = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "harness.context.listing",
    ]);
    assert!(listing.success, "stderr: {}", listing.stderr);
    assert!(
        listing.stdout.contains("harness.context.listing"),
        "the delta carries the listing leaf: {}",
        listing.stdout
    );
    // The task leaf, which is where the thinking-drop accounting sits, beside the record
    // of the blocks the harness stripped.
    let task = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "harness.context.task",
    ]);
    assert!(task.success, "stderr: {}", task.stderr);
    assert!(
        task.stdout.contains("harness.context.task"),
        "the drop accounting carries the task leaf: {}",
        task.stdout
    );
}

#[test]
fn the_body_each_unit_renders_follows_the_conversion_the_record_carries() {
    let h = home();
    // The delta's record carries a persisted conversion, so the body is THAT and the
    // marker says so - which is what makes the injected instructions searchable.
    let rendered = h.run(&["show", at(SESS).as_str(), "--line", "2"]);
    assert!(rendered.success, "stderr: {}", rendered.stderr);
    assert!(
        rendered.stdout.contains("[rendered]")
            && rendered.stdout.contains("# MCP Server Instructions"),
        "the conversion is the body: {}",
        rendered.stdout
    );
    // The drop accounting converts to nothing, so its record carries no conversion and
    // the unit falls to the verbatim payload - which is the only place its numbers are.
    let payload = h.run(&["show", at(SESS).as_str(), "--line", "3"]);
    assert!(payload.success, "stderr: {}", payload.stderr);
    assert!(
        payload.stdout.contains("[payload]") && payload.stdout.contains("prefix_mismatch"),
        "the payload is the body: {}",
        payload.stdout
    );
    // The model received one and not the other, and the delivery axis says so per record.
    assert!(
        !rendered.stdout.contains("[not delivered]"),
        "the converted one is delivered: {}",
        rendered.stdout
    );
    assert!(
        payload.stdout.contains("[not delivered]"),
        "the dropped one is not: {}",
        payload.stdout
    );
}

#[test]
fn the_attachment_census_now_counts_both_payload_types() {
    let h = home();
    // The census reads EMITTED units, so a payload csift modelled no leaf for could not
    // appear on this axis at all - which is what both rows fix.
    let c = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--count-by",
        "attachment",
    ]);
    assert!(c.success, "stderr: {}", c.stderr);
    assert!(
        c.stdout.contains("mcp_instructions_delta") && c.stdout.contains("thinking_drop"),
        "both payload types are census keys: {}",
        c.stdout
    );
}
