//! The D7 boundary keep: what `line_is_transcript_candidate` admits under it.
//!
//! The keep is a conjunction of two needles, and the pins below hold both halves in
//! place: the rare literal alone would admit any payload that merely mentions the word,
//! and the key-only `"subtype"` needle is what separates a real boundary from one.

use super::*;

/// The gates a FLAGLESS scan derives: the D7 boundary keep and the always-on channel
/// keep are the only two on, which is exactly the query the false admits cost.
fn flagless_gates() -> CandidateGates {
    CandidateGates {
        compact_boundary: true,
        channel: true,
        // A DEFAULT-ON view needle is part of what a flagless scan IS, so the fixture
        // derives the mask instead of hard-coding a zero that would stop tracking it.
        attachment_views: attachment_view_mask(&args(""), false),
        ..CandidateGates::default()
    }
}

/// A true boundary: `type:"system"` with the modeled subtype, no role marker anywhere.
const BOUNDARY: &str = r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#;

/// The same record reserialized with whitespace around every colon - the R13 shape. The
/// key needle is quoted, so it survives intact.
const BOUNDARY_RESERIALIZED: &str = r#"{"type" : "system", "subtype" : "compact_boundary", "uuid" : "b1", "compactMetadata" : {"trigger" : "auto"}}"#;

/// An attachment whose payload MENTIONS the literal in prose. No role marker, no channel
/// literal, and - the discriminator - no `"subtype"` key.
const MENTION_ATTACHMENT: &str = r#"{"type":"attachment","uuid":"at1","parentUuid":"a1","timestamp":"2026-06-07T05:02:00.000Z","attachment":{"type":"task_reminder","content":"render compact_boundary metadata"}}"#;

/// A `queue-operation` line whose queued prompt mentions the literal. Same shape of false
/// admit from the other line type, and the gated `user.queued` leaf is off here.
const MENTION_QUEUE: &str = r#"{"type":"queue-operation","operation":"enqueue","content":"explain what compact_boundary means"}"#;

#[test]
fn the_boundary_keep_admits_a_boundary_in_either_serialization() {
    let gates = flagless_gates();
    assert!(
        line_is_transcript_candidate(BOUNDARY.as_bytes(), &gates),
        "a compact boundary is the whole point of the D7 keep"
    );
    assert!(
        line_is_transcript_candidate(BOUNDARY_RESERIALIZED.as_bytes(), &gates),
        "the key-only needle is quoted, so a reserialized boundary is still admitted"
    );
}

#[test]
fn the_boundary_keep_refuses_a_payload_that_only_mentions_the_literal() {
    let gates = flagless_gates();
    assert!(
        !line_is_transcript_candidate(MENTION_ATTACHMENT.as_bytes(), &gates),
        "an attachment carrying the literal without the key is not a boundary"
    );
    assert!(
        !line_is_transcript_candidate(MENTION_QUEUE.as_bytes(), &gates),
        "nor is a queued prompt that merely uses the word"
    );
}

#[test]
fn the_key_needle_is_what_separates_them_not_the_serialization() {
    // The refusal keys on the KEY, so stripping the key from the reserialized boundary is
    // enough to lose the admission - and only that. Both lines below carry the literal and
    // the same whitespace; only one carries `"subtype"`.
    let gates = flagless_gates();
    let keyless = BOUNDARY_RESERIALIZED.replace(r#""subtype" : "#, r#""kind" : "#);
    assert!(
        keyless.contains("compact_boundary"),
        "the literal is still there - the key is the only difference"
    );
    assert!(
        !line_is_transcript_candidate(keyless.as_bytes(), &gates),
        "the same line without the key is refused"
    );
    assert!(
        line_is_transcript_candidate(BOUNDARY_RESERIALIZED.as_bytes(), &gates),
        "and with the key it is admitted"
    );
}

#[test]
fn a_gate_that_cannot_reach_the_leaf_still_pays_nothing() {
    // The conjunction sits behind the same `&&` gate it always did: a selector that cannot
    // reach `harness.compaction.boundary` refuses the boundary line outright.
    let gates = CandidateGates {
        channel: true,
        ..CandidateGates::default()
    };
    assert!(
        !line_is_transcript_candidate(BOUNDARY.as_bytes(), &gates),
        "no boundary selector, no boundary keep"
    );
}

/// A `cost-state` line and a `mode` line: two bookkeeping types, two different leaves,
/// and the mask is what keeps them apart.
const COST_LINE: &str =
    r#"{"type":"cost-state","sessionId":"s1","totalCostUSD":1.5,"totalDuration":900}"#;
const MODE_LINE: &str =
    r#"{"type":"mode","mode":"acceptEdits","timestamp":"2026-06-07T05:00:00.000Z"}"#;

/// The bookkeeping mask is PER LEAF, not per family: naming one leaf runs that leaf's
/// needle and no other, and a default scan runs none of the fifteen. Without the mask a
/// selector for one line type would parse all thirteen, which is the cost the gate law
/// exists to refuse.
#[test]
fn a_bookkeeping_needle_runs_only_for_the_leaf_its_bit_names() {
    let mut a = args("");
    a.labels = vec!["harness.bookkeeping.cost".to_string()];
    let gates = CandidateGates {
        bookkeeping: bookkeeping_mask(&a, false),
        ..CandidateGates::default()
    };
    assert!(gates.bookkeeping != 0, "the selector sets its leaf's bit");
    assert!(
        line_is_transcript_candidate(COST_LINE.as_bytes(), &gates),
        "the named leaf's own line type is admitted"
    );
    assert!(
        !line_is_transcript_candidate(MODE_LINE.as_bytes(), &gates),
        "another bookkeeping line type is NOT: its bit is clear"
    );

    // The whole group short-circuits on a zero mask, which is every default scan.
    let flagless = flagless_gates();
    assert_eq!(flagless.bookkeeping, 0);
    assert!(!line_is_transcript_candidate(
        COST_LINE.as_bytes(),
        &flagless
    ));
    assert!(!line_is_transcript_candidate(
        MODE_LINE.as_bytes(),
        &flagless
    ));

    // And the family glob sets every bit, so both lines come back.
    let mut all = args("");
    all.labels = vec!["harness.bookkeeping.*".to_string()];
    let wide = CandidateGates {
        bookkeeping: bookkeeping_mask(&all, false),
        ..CandidateGates::default()
    };
    assert!(line_is_transcript_candidate(COST_LINE.as_bytes(), &wide));
    assert!(line_is_transcript_candidate(MODE_LINE.as_bytes(), &wide));
}

/// The two attachment VIEW lines: each carries a leaf an ordinary query names, and each
/// has its own bit.
const QUEUED_COMMAND_LINE: &str = r#"{"type":"attachment","uuid":"v1","origin":{"kind":"human"},"attachment":{"type":"queued_command","command":"steer left"}}"#;
const STRUCTURED_LINE: &str = r#"{"type":"attachment","uuid":"v2","attachment":{"type":"structured_output","content":["the answer"]}}"#;
const FILE_ATTACHMENT_LINE: &str = r#"{"type":"attachment","uuid":"v3","attachment":{"type":"file","filePath":"/Users/dev/x.rs","content":"fn main(){}"}}"#;

/// Naming ONE view leaf runs that leaf's needle and no other, and a query naming neither
/// runs none - which is the whole point of the bits: `-t user` used to admit every
/// attachment line in the transcript, at four times the cost of a flagless scan. A census
/// cannot show this (a wrongly admitted line classifies to a leaf the filter drops), so
/// the candidate predicate is where it is pinned.
#[test]
fn an_attachment_view_needle_runs_only_for_the_leaf_its_bit_names() {
    let mask_for = |selector: &str| {
        let mut a = args("");
        a.labels = vec![selector.to_string()];
        attachment_view_mask(&a, false)
    };
    let gates_for = |selector: &str| CandidateGates {
        attachment_views: mask_for(selector),
        ..CandidateGates::default()
    };

    let human = gates_for("user.message");
    assert!(
        human.attachment_views != 0,
        "the selector sets its leaf's bit"
    );
    assert!(
        line_is_transcript_candidate(QUEUED_COMMAND_LINE.as_bytes(), &human),
        "the human's queued steer is admitted by its own needle"
    );
    assert!(
        !line_is_transcript_candidate(STRUCTURED_LINE.as_bytes(), &human),
        "the OTHER view's payload is not: its bit is clear"
    );
    assert!(
        !line_is_transcript_candidate(FILE_ATTACHMENT_LINE.as_bytes(), &human),
        "and an ordinary attachment line is not admitted at all - the coarse keep stays off"
    );

    let structured = gates_for("agent.message.structured");
    assert!(line_is_transcript_candidate(
        STRUCTURED_LINE.as_bytes(),
        &structured
    ));
    assert!(!line_is_transcript_candidate(
        QUEUED_COMMAND_LINE.as_bytes(),
        &structured
    ));
    assert!(!line_is_transcript_candidate(
        FILE_ATTACHMENT_LINE.as_bytes(),
        &structured
    ));

    // A FLAGLESS scan admits the DEFAULT-ON needle and nothing else. A flagless scan is a
    // superset of `-t user`, so the leaf `-t user` reaches cannot be one it misses - the
    // channel keep is default-on for the same reason. The other needle keeps the gated-leaf
    // rule, so the model's structured answer and every ordinary attachment line stay out.
    let flagless = flagless_gates();
    assert!(
        flagless.attachment_views & 1 != 0,
        "the human-steer needle is default-on"
    );
    assert!(
        flagless.attachment_views & 2 == 0,
        "the structured-answer needle still wants its selector"
    );
    assert!(line_is_transcript_candidate(
        QUEUED_COMMAND_LINE.as_bytes(),
        &flagless
    ));
    for line in [STRUCTURED_LINE, FILE_ATTACHMENT_LINE] {
        assert!(!line_is_transcript_candidate(line.as_bytes(), &flagless));
    }

    // A selector that EXCLUDES the leaf closes the default-on needle again, which is what
    // makes it a gate rather than an always-on cost: `-t harness` short-circuits before
    // the memmem exactly as it did before.
    let mut harness = args("");
    harness.labels = vec!["harness".to_string()];
    let harness_gates = CandidateGates {
        attachment_views: attachment_view_mask(&harness, false),
        ..CandidateGates::default()
    };
    assert_eq!(
        harness_gates.attachment_views, 0,
        "a selector that cannot reach either view runs neither needle"
    );
    assert!(!line_is_transcript_candidate(
        QUEUED_COMMAND_LINE.as_bytes(),
        &harness_gates
    ));
}
