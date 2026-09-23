//! The is-genuine-user gate and its exclusions: carriers, meta, interrupts, wrappers, synthetic markers.

use super::*;

#[test]
fn genuine_user_string_content() {
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"hello"}}"#);
    assert!(r.is_genuine_user());
}

#[test]
fn tool_result_carrier_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn compaction_summary_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"summary..."}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn is_meta_user_is_not_genuine_user() {
    // §4.2 TRAP: looks human, is system-injected - must be excluded.
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(r.genuine_user_text().is_none());
}

#[test]
fn is_meta_local_command_caveat_excluded() {
    // The real-data shape: <local-command-caveat> on an isMeta user record.
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: ...</local-command-caveat>"}}"#,
    );
    assert!(!r.is_genuine_user());
}

// ── Branch-completeness: the negative / fallback arms ──

#[test]
fn is_genuine_user_false_for_non_user_type() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn is_genuine_user_false_when_message_absent() {
    // A `type:"user"` record with NO `message` object at all (the `let Some(msg)
    // else` arm).
    let r = parse(r#"{"type":"user","uuid":"x"}"#);
    assert!(!r.is_genuine_user());
}

#[test]
fn is_genuine_user_false_when_role_not_user() {
    // role mismatch inside an otherwise user-typed record.
    let r = parse(r#"{"type":"user","message":{"role":"assistant","content":"hi"}}"#);
    assert!(!r.is_genuine_user());
}

#[test]
fn is_genuine_user_false_when_content_absent() {
    // message present, role user, but NO content (the `None => false` arm).
    let r = parse(r#"{"type":"user","message":{"role":"user"}}"#);
    assert!(!r.is_genuine_user());
    assert!(r.genuine_user_text().is_none());
}

#[test]
fn is_genuine_user_false_for_blocks_without_text() {
    // Block content that has NO text block (only an image) → not genuine.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
    );
    assert!(!r.is_genuine_user());
}

// ── §4.2.1 interrupts: synthesized markers are NOT genuine-user (spurious-boundary
//    removal). Real shape: a text-block user record whose text is EXACTLY the marker
//    (116 + 21 occurrences in the corpus, none isMeta, none carrying extra prose). ──

#[test]
fn interrupt_marker_plain_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
    );
    assert!(
        !r.is_genuine_user(),
        "interrupt marker must not open a turn"
    );
    assert!(!r.opens_turn());
}

#[test]
fn interrupt_marker_for_tool_use_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(!r.opens_turn());
}

#[test]
fn interrupt_marker_as_string_content_is_not_genuine_user() {
    // The same marker can arrive as bare-string content too - still excluded.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn text_that_merely_contains_interrupt_phrase_is_still_genuine() {
    // Exact-match only: a real message that QUOTES the phrase must stay genuine
    // (codepoint-safe `==`, never a substring/slice).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"why did i see [Request interrupted by user] earlier?"}}"#,
    );
    assert!(
        r.is_genuine_user(),
        "a message merely containing the phrase is still a human turn"
    );
}

// ── §4.2.2 / §4.2.3: <local-command-stdout> output + <command-name> slash wrapper
//    are machine-templated, NOT genuine-user (string content, non-isMeta). ──

#[test]
fn local_command_stdout_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>✓ Updated oh-my-claudecode.</local-command-stdout>"}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(!r.opens_turn());
}

#[test]
fn command_name_wrapper_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"}}"#,
    );
    assert!(
        !r.is_genuine_user(),
        "slash-command wrapper must not open a turn"
    );
    assert!(!r.opens_turn());
    // Empty args → no recoverable prose.
    assert!(r.slash_command_args().is_none());
}

#[test]
fn command_name_wrapper_with_args_recovers_prose() {
    // Real shape: `/compact Just shipped spec-batch-14 …` - the typed prose lives in
    // <command-args>; it is recovered (and is the reconstructed user text), but the
    // wrapper itself is still not a standalone genuine-user record.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args> Just shipped spec-batch-14, summarize</command-args>"}}"#,
    );
    assert!(!r.is_genuine_user());
    assert_eq!(
        r.slash_command_args().as_deref(),
        Some("Just shipped spec-batch-14, summarize")
    );
    // Rendered as `/name args` - the prose keeps its command context, and the
    // wrapper XML never masquerades as the body.
    assert_eq!(
        r.reconstructed_user_text(None).as_deref(),
        Some("/compact Just shipped spec-batch-14, summarize")
    );
}

#[test]
fn command_message_first_wrapper_detected_and_recovered() {
    // The NEWER CC tag order (`<command-message>` FIRST - both orders coexist in real
    // corpora). Detection anchored on `<command-name>` alone misclassifies this
    // as GENUINE user prose: the raw XML renders as `user.message` and opens a turn.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-message>csift</command-message>\n<command-name>/csift</command-name>\n<command-args>what changed in v5?</command-args>"}}"#,
    );
    assert!(!r.is_genuine_user(), "wrapper is never the human");
    assert_eq!(r.slash_command_name().as_deref(), Some("/csift"));
    assert_eq!(
        r.slash_command_args().as_deref(),
        Some("what changed in v5?")
    );
    assert_eq!(
        r.reconstructed_user_text(None).as_deref(),
        Some("/csift what changed in v5?")
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage, Class::CommandInvocation]
    );
    // A no-args NEW-order wrapper is pure machinery: never genuine, never user.message.
    let bare = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-message>compact</command-message>\n<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
    );
    assert!(!bare.is_genuine_user());
    assert_eq!(
        bare.classify(&ClassifyCtx::top_level()),
        vec![Class::CommandInvocation]
    );
}

#[test]
fn command_name_wrapper_with_multibyte_args_is_codepoint_safe() {
    // A multi-byte args body must be recovered whole (codepoint-safe slice on the ASCII
    // tags only) - the live panic class.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args>🤖 just shipped the batch, summarize 🎉</command-args>"}}"#,
    );
    assert_eq!(
        r.slash_command_args().as_deref(),
        Some("🤖 just shipped the batch, summarize 🎉")
    );
}

#[test]
fn opens_turn_matches_genuine_user() {
    // A plain genuine user opens a turn; a plain tool_result carrier does not.
    let genuine = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(genuine.opens_turn());
    let carrier = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
    );
    assert!(!carrier.opens_turn());
}

#[test]
fn is_synthetic_user_marker_matches_each_form() {
    assert!(is_synthetic_user_marker("[Request interrupted by user]"));
    assert!(is_synthetic_user_marker(
        "[Request interrupted by user for tool use]"
    ));
    assert!(is_synthetic_user_marker("<local-command-stdout>anything"));
    assert!(is_synthetic_user_marker("<command-name>/x</command-name>"));
    assert!(!is_synthetic_user_marker("a normal human message"));
    // Exact-match for interrupts: a longer string merely starting with the marker is
    // NOT excluded as an interrupt.
    assert!(!is_synthetic_user_marker(
        "[Request interrupted by user] and then I said more"
    ));
}

// ── One payload parse per attachment record ──

/// TWO calls, ONE parse. The instrument is identity: a second parse would build a second
/// tree at a second address, so two calls returning the SAME address is the parse count.
/// It matters because a payload can be a whole file snapshot and four layers ask this
/// record about it - the classify dispatch, the delivery override, the render and a
/// family's own facts.
#[test]
fn an_attachment_payload_is_parsed_once_per_record() {
    let r = parse(
        r#"{"type":"attachment","uuid":"a1","attachment":{"type":"hook_additional_context","content":["ctx"]}}"#,
    );
    // Nothing is parsed before the first ask: the memo is empty on a record nobody has
    // questioned yet.
    assert!(r.attachment_parsed.get().is_none());
    let first = r.attachment_value().expect("the payload parses");
    let second = r.attachment_value().expect("and answers again");
    assert!(
        std::ptr::eq(first, second),
        "two calls returned two trees - the payload was parsed twice"
    );
    assert!(r.attachment_parsed.get().is_some());
}

/// A record with NO attachment memoises nothing at all. The guard is the `attachment`
/// field, tested before the memo is touched, so the overwhelming majority of records -
/// every conversation record in every transcript - pay one `Option` test and allocate
/// nothing.
#[test]
fn a_record_with_no_attachment_memoises_nothing() {
    let r = parse(r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"hi"}}"#);
    assert!(r.attachment_value().is_none());
    assert!(r.attachment_parsed.get().is_none());
}

// -- the attachment converter's two texts, parsed once --

/// BOTH conversions a record can carry come back, and nothing chooses between them.
///
/// The harness writes the converter's output onto the record as a top-level key, and a
/// `queued_command` payload carries two: the ordinary replay and the replay into a turn
/// that also holds a genuine human message. Measured over one projects root, every one of
/// the 1,507 records carrying the second carries the first as well, and the two texts
/// DIFFER on all 1,507 - so a reader shown one of them is shown the wrong one half the
/// time, and csift returns the pair.
#[test]
fn both_conversions_come_back_and_neither_is_chosen() {
    let r = parse(
        r#"{"type":"attachment","uuid":"a1","attachment":{"type":"queued_command"},
            "rendered":[{"content":"the machine form"}],
            "renderedInHumanTurn":[{"content":"the human-turn form"}]}"#,
    );
    let c = r
        .converted_text()
        .expect("the record carries both conversions");
    assert_eq!(c.rendered.as_deref(), Some("the machine form"));
    assert_eq!(c.human_turn.as_deref(), Some("the human-turn form"));
}

/// The two halves are independent: a record with only the ordinary conversion answers
/// `None` for the second rather than repeating the first, which is what would make the
/// two texts look identical on every record that has one.
#[test]
fn a_record_with_one_conversion_answers_none_for_the_other() {
    let r = parse(
        r#"{"type":"attachment","uuid":"a2","attachment":{"type":"file"},
            "rendered":[{"content":"only one"}]}"#,
    );
    let c = r
        .converted_text()
        .expect("the record carries one conversion");
    assert_eq!(c.rendered.as_deref(), Some("only one"));
    assert_eq!(c.human_turn, None);
    // And a record with NEITHER key answers nothing at all - the memo holds `None` and
    // allocates no pair of nothings, which is every record of every non-converting type.
    let bare = parse(r#"{"type":"attachment","uuid":"a3","attachment":{"type":"file"}}"#);
    assert!(bare.converted_text().is_none());
}

/// A conversion is an ARRAY of messages: several entries JOIN with a newline, and a
/// message whose `content` is a block array contributes its text blocks.
///
/// Both shapes are the writer's, not csift's: the string form is what every measured
/// record carries and the block form is what the writer also admits, so the two arms are
/// the whole domain and anything else contributes nothing.
#[test]
fn a_conversion_joins_its_entries_and_reads_both_content_shapes() {
    let r = parse(
        r#"{"type":"attachment","uuid":"a4","attachment":{"type":"file"},
            "rendered":[{"content":"first"},
                        {"content":[{"type":"text","text":"second"},{"type":"image"},{"type":"text","text":"third"}]},
                        {"content":42}]}"#,
    );
    assert_eq!(
        r.converted_text().and_then(|c| c.rendered.as_deref()),
        Some("first\nsecond\nthird"),
        "entries join, blocks join, and a value that is neither carries no text"
    );
}

/// An EMPTY conversion reads `None`, not an empty string: a conversion that produced no
/// text is the same answer as no conversion, and a caller made to tell them apart would
/// be distinguishing two ways of saying nothing.
#[test]
fn a_conversion_that_produced_no_text_reads_as_absent() {
    for line in [
        r#"{"type":"attachment","uuid":"a5","rendered":[]}"#,
        r#"{"type":"attachment","uuid":"a6","rendered":[{"content":[]}]}"#,
        r#"{"type":"attachment","uuid":"a7","rendered":"not an array"}"#,
    ] {
        assert_eq!(
            parse(line)
                .converted_text()
                .and_then(|c| c.rendered.clone()),
            None,
            "{line}"
        );
    }
}

/// ONE parse for both conversions, and it is remembered: the memo holds the PAIR, so a
/// second question about the same record costs nothing.
///
/// Asserted through the borrow the accessor hands out rather than by counting parses: two
/// calls return the SAME allocation, which is only true if the second did not parse
/// again. A conversion can embed a whole file, which is what makes the memo worth having.
#[test]
fn both_conversions_are_parsed_once_and_remembered() {
    let r = parse(
        r#"{"type":"attachment","uuid":"a8","attachment":{"type":"queued_command"},
            "rendered":[{"content":"machine"}],
            "renderedInHumanTurn":[{"content":"human"}]}"#,
    );
    let first = r.converted_text().expect("both conversions are there");
    let second = r.converted_text().expect("and still there");
    assert!(
        std::ptr::eq(first, second),
        "the second call re-parsed instead of reading the memo"
    );
    assert_eq!(first.rendered.as_deref(), Some("machine"));
}
