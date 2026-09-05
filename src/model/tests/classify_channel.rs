//! The `agent.communication.channel` leaf: detection, the dual label, the sender direction.

use super::*;

/// The first chunk of a delivery: a full envelope header (with `from=`) plus a body.
const FIRST_CHUNK: &str = r#"{"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","content":["[csift-channel v1 id=0123456789abcdef part=1/2 mode=steer from=aRelay-0123456789abcdef from-session=00000000 relation=sibling to=00000000-0000-4000-8000-000000000011]\nthrottlebeacon the region queue"]}}"#;

/// A CONTINUATION chunk: the header carries only `id=` and `part=`, so it names no sender.
const NEXT_CHUNK: &str = r#"{"type":"attachment","uuid":"att2","timestamp":"2026-06-07T05:00:02.000Z","attachment":{"type":"hook_additional_context","hookEvent":"PostToolUse","content":["[csift-channel v1 id=0123456789abcdef part=2/2]\nthe rest of the region queue\n--- end ---"]}}"#;

/// An ordinary hook context - the control for every assertion below.
const PLAIN_HOOK: &str = r#"{"type":"attachment","uuid":"att3","timestamp":"2026-06-07T05:00:03.000Z","attachment":{"type":"hook_additional_context","hookEvent":"SessionStart","content":["mistgate token applies"]}}"#;

fn ctx_owned() -> ClassifyCtx<'static> {
    ClassifyCtx {
        owner_id: Some("00000000-0000-4000-8000-000000000011"),
        ..ClassifyCtx::top_level()
    }
}

#[test]
fn channel_delivery_leads_the_hook_leaf_and_names_its_sender() {
    let r = parse(FIRST_CHUNK);
    // Both labels, the message view FIRST (richest-view law): the delivery is a message to
    // this lane, and it is still hook machinery on disk.
    assert_eq!(
        r.classify(&ctx_owned()),
        vec![Class::CommChannel, Class::MetaHook]
    );
    assert_eq!(
        r.direction(&ctx_owned()),
        Some((
            "aRelay-0123456789abcdef".to_string(),
            "00000000-0000-4000-8000-000000000011".to_string()
        ))
    );
    // Without an owner id the self side degrades to the literal, as every comm arm does.
    assert_eq!(
        r.direction(&ClassifyCtx::top_level()),
        Some(("aRelay-0123456789abcdef".to_string(), "self".to_string()))
    );
    // The delivery text is the content string VERBATIM - header line and body, nothing
    // fabricated and nothing stripped.
    let text = r.csift_channel_text().expect("a channel delivery");
    assert!(
        text.starts_with("[csift-channel v1 id=0123456789abcdef part=1/2")
            && text.ends_with("throttlebeacon the region queue"),
        "verbatim envelope: {text:?}"
    );
}

#[test]
fn a_continuation_chunk_is_the_leaf_with_no_direction() {
    let r = parse(NEXT_CHUNK);
    assert_eq!(
        r.classify(&ctx_owned()),
        vec![Class::CommChannel, Class::MetaHook]
    );
    // No `from=` in the header: the sender is unknown, so NO direction is reported rather
    // than a guessed one.
    assert_eq!(r.direction(&ctx_owned()), None);
}

#[test]
fn an_ordinary_hook_context_stays_hook_only() {
    let r = parse(PLAIN_HOOK);
    assert_eq!(r.classify(&ctx_owned()), vec![Class::MetaHook]);
    assert_eq!(r.csift_channel_text(), None);
    assert_eq!(r.direction(&ctx_owned()), None);
    // A hook context that merely QUOTES the marker mid-text is not a delivery: the header
    // must OPEN the injected string.
    let quoted = parse(
        r#"{"type":"attachment","uuid":"att4","attachment":{"type":"hook_additional_context","content":["the envelope opens [csift-channel v1 id=x] and so on"]}}"#,
    );
    assert_eq!(quoted.classify(&ctx_owned()), vec![Class::MetaHook]);
    assert_eq!(quoted.csift_channel_text(), None);
}

#[test]
fn prose_quoting_the_envelope_is_not_the_channel_leaf() {
    // The detector keys on the ATTACHMENT payload, so a human (or an agent) writing about
    // the envelope keeps its own label - csift's own dev sessions quote it constantly.
    let user = parse(
        r#"{"type":"user","uuid":"u9","message":{"role":"user","content":"the header reads [csift-channel v1 id=abc from=aRelay-0123456789abcdef]"}}"#,
    );
    assert_eq!(user.classify(&ctx_owned()), vec![Class::UserMessage]);
    assert_eq!(user.csift_channel_text(), None);
    let asst = parse(
        r#"{"type":"assistant","uuid":"a9","message":{"role":"assistant","content":[{"type":"text","text":"[csift-channel v1 id=abc from=aRelay-0123456789abcdef] is the header"}]}}"#,
    );
    assert_eq!(asst.classify(&ctx_owned()), vec![Class::AgentMessage]);
    assert_eq!(asst.csift_channel_text(), None);
}

#[test]
fn the_first_content_string_is_the_delivery() {
    // Several injected blocks: only the one the header OPENS is the delivery; the joined
    // form stays the harness.meta.hook view.
    let multi = parse(
        r#"{"type":"attachment","uuid":"att5","attachment":{"type":"hook_additional_context","content":["[csift-channel v1 id=abc part=1/1 from=aRelay-0123456789abcdef]\nfirst block","a second, unrelated block"]}}"#,
    );
    assert_eq!(
        multi.csift_channel_text().as_deref(),
        Some("[csift-channel v1 id=abc part=1/1 from=aRelay-0123456789abcdef]\nfirst block")
    );
    assert_eq!(
        multi.hook_additional_context_text().as_deref(),
        Some("[csift-channel v1 id=abc part=1/1 from=aRelay-0123456789abcdef]\nfirst block\na second, unrelated block")
    );
    // A block-shaped payload whose FIRST string is an ordinary context is not a delivery,
    // even when a later block carries an envelope (the receiver reads the first block).
    let later = parse(
        r#"{"type":"attachment","uuid":"att6","attachment":{"type":"hook_additional_context","content":["ordinary context","[csift-channel v1 id=abc from=aRelay-0123456789abcdef]\nbody"]}}"#,
    );
    assert_eq!(later.csift_channel_text(), None);
    // The bare-string payload shape is tolerated exactly as the hook extractor tolerates it.
    let bare = parse(
        r#"{"type":"attachment","uuid":"att7","attachment":{"type":"hook_additional_context","content":"[csift-channel v1 id=abc part=1/1 from=external:harbor-cron]\nrun the sweep"}}"#,
    );
    assert_eq!(
        bare.direction(&ClassifyCtx::top_level()),
        Some(("external:harbor-cron".to_string(), "self".to_string()))
    );
}

#[test]
fn the_prefilter_needle_is_a_run_of_the_header_prefix() {
    // The byte scan (search/scan.rs) and the classifier share ONE constant pair; a needle
    // that stopped being a substring of the header would silently stop admitting the line.
    assert!(
        Record::CSIFT_CHANNEL_HEADER_PREFIX.contains(Record::CSIFT_CHANNEL_NEEDLE),
        "the needle must be a run of the header prefix"
    );
    // Prefilter safety (SPEC 7d): ASCII, no whitespace-free requirement here but no
    // JSON-escaped character, so the needle survives verbatim in raw bytes.
    assert!(
        Record::CSIFT_CHANNEL_NEEDLE
            .bytes()
            .all(|b| b.is_ascii() && b != b'"' && b >= 0x20 && b != 0x7f),
        "the needle must carry no JSON-escaped character"
    );
}
