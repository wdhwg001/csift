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
