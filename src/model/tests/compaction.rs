//! C-33: the compaction MODE - the summary's `summarizeMetadata.direction` and the
//! boundary <-> summary pairing that lets a boundary name the gesture that minted it.

use super::*;

const BOUNDARY: &str = r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"manual","preTokens":900}}"#;
const BOUNDARY2: &str = r#"{"type":"system","subtype":"compact_boundary","uuid":"cb2","timestamp":"2026-06-07T06:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":800}}"#;

fn summary(extra: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"sum1","timestamp":"2026-06-07T05:10:01.000Z","isCompactSummary":true,{extra}"message":{{"role":"user","content":"This session is being continued."}}}}"#
    )
}

#[test]
fn slug_and_direction_are_the_two_stable_projections() {
    assert_eq!(SummarizeMode::Compact.slug(), "compact");
    assert_eq!(SummarizeMode::FromHere.slug(), "summarize-from-here");
    assert_eq!(SummarizeMode::UpToHere.slug(), "summarize-up-to-here");
    // A plain compaction writes no `summarizeMetadata` at all, so it names no direction.
    assert_eq!(SummarizeMode::Compact.direction(), None);
    assert_eq!(SummarizeMode::FromHere.direction(), Some("from"));
    assert_eq!(SummarizeMode::UpToHere.direction(), Some("up_to"));
    assert_eq!(
        SummarizeMode::from_direction("from"),
        Some(SummarizeMode::FromHere)
    );
    assert_eq!(
        SummarizeMode::from_direction("up_to"),
        Some(SummarizeMode::UpToHere)
    );
    // An unmodeled gesture is UNKNOWN, never folded into one of the two known ones.
    assert_eq!(SummarizeMode::from_direction("sideways"), None);
}

#[test]
fn a_summary_names_its_own_mode_from_summarize_metadata() {
    // The harness writes `summarizeMetadata` INSTEAD of `isVisibleInTranscriptOnly`.
    let plain = parse(&summary(r#""isVisibleInTranscriptOnly":true,"#));
    assert_eq!(plain.summarize_direction(), None);
    assert_eq!(
        plain.summary_compaction_mode(),
        Some(SummarizeMode::Compact)
    );

    let up = parse(&summary(
        r#""summarizeMetadata":{"messagesSummarized":66,"direction":"up_to"},"#,
    ));
    assert_eq!(up.summarize_direction(), Some("up_to"));
    assert_eq!(up.summary_compaction_mode(), Some(SummarizeMode::UpToHere));

    let from = parse(&summary(
        r#""summarizeMetadata":{"messagesSummarized":4,"direction":"from"},"#,
    ));
    assert_eq!(from.summarize_direction(), Some("from"));
    assert_eq!(
        from.summary_compaction_mode(),
        Some(SummarizeMode::FromHere)
    );

    // `userContext` is written from a possibly-undefined value: its absence changes nothing.
    let ctx = parse(&summary(
        r#""summarizeMetadata":{"messagesSummarized":4,"userContext":"the auth work","direction":"from"},"#,
    ));
    assert_eq!(ctx.summary_compaction_mode(), Some(SummarizeMode::FromHere));

    // A direction csift does not model reads UNKNOWN, not `compact` - the verbatim word is
    // still available for the render.
    let odd = parse(&summary(r#""summarizeMetadata":{"direction":"sideways"},"#));
    assert_eq!(odd.summarize_direction(), Some("sideways"));
    assert_eq!(odd.summary_compaction_mode(), None);
}

#[test]
fn only_a_compaction_summary_carries_a_mode() {
    let user = parse(
        r#"{"type":"user","uuid":"u1","timestamp":"t","message":{"role":"user","content":"hi"}}"#,
    );
    assert_eq!(user.summary_compaction_mode(), None);
    // A boundary's own metadata names no direction - it learns its mode by pairing.
    assert_eq!(parse(BOUNDARY).summary_compaction_mode(), None);
}

#[test]
fn the_index_pairs_a_boundary_with_the_summary_that_follows_it() {
    let s = summary(r#""summarizeMetadata":{"messagesSummarized":66,"direction":"up_to"},"#);
    let recs = [parse(BOUNDARY), parse(&s)];
    let ix = SummarizeIndex::from_records(&recs);
    assert_eq!(
        ix.mode_for_boundary(Some("cb1")),
        Some(SummarizeMode::UpToHere)
    );
    // A uuid that is not a boundary in this transcript pairs with nothing.
    assert_eq!(ix.mode_for_boundary(Some("cb9")), None);
    assert_eq!(ix.mode_for_boundary(None), None);
}

#[test]
fn an_ordinary_compaction_pairs_as_compact() {
    let recs = [
        parse(BOUNDARY),
        parse(&summary(r#""isVisibleInTranscriptOnly":true,"#)),
    ];
    let ix = SummarizeIndex::from_records(&recs);
    assert_eq!(
        ix.mode_for_boundary(Some("cb1")),
        Some(SummarizeMode::Compact)
    );
}

#[test]
fn an_unpaired_boundary_stays_unknown_and_never_guesses() {
    // A boundary with no summary at all after it (a windowed or truncated read).
    let lone = [parse(BOUNDARY)];
    assert_eq!(
        SummarizeIndex::from_records(&lone).mode_for_boundary(Some("cb1")),
        None
    );
    // Two boundaries in a row: the FIRST is unpaired, the second takes the summary.
    let s = summary(r#""summarizeMetadata":{"direction":"from"},"#);
    let recs = [parse(BOUNDARY), parse(BOUNDARY2), parse(&s)];
    let ix = SummarizeIndex::from_records(&recs);
    assert_eq!(ix.mode_for_boundary(Some("cb1")), None);
    assert_eq!(
        ix.mode_for_boundary(Some("cb2")),
        Some(SummarizeMode::FromHere)
    );
}

#[test]
fn a_summary_with_no_boundary_before_it_pairs_with_nothing() {
    // The compaction summary of a CLONE head (its boundary lives in the origin session).
    let recs = [parse(&summary(r#""isVisibleInTranscriptOnly":true,"#))];
    let ix = SummarizeIndex::from_records(&recs);
    assert_eq!(ix.mode_for_boundary(Some("cb1")), None);
}
