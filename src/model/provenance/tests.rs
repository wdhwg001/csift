//! The provenance vocabulary and index, pinned directly.

use super::*;

/// The vocabulary, typed out: EVERY variant, in the enum's own order, and its word.
///
/// Hand-kept rather than derived, because a list derived from the enum would assert
/// nothing about what the vocabulary CLAIMS - which is the whole reason the set is closed.
/// It lives here rather than as a production `EdgeKind::ALL` because nothing in the binary
/// enumerates the vocabulary (the census keys on the words a scan actually saw), and a
/// const with none but test readers is dead code this crate declines to carry.
///
/// What makes it a COMPLETE list rather than a sample is [`ordinal`]: the enum's own
/// exhaustiveness check turns a variant missing from here into a compile error. Without
/// that the list was seventeen of nineteen, `parent` and `result_of` were absent, and a
/// twentieth relation whose word collided with `leaf` passed the whole suite - because the
/// list it was missing from was the list the uniqueness test walked.
const EVERY_KIND: [EdgeKind; 19] = [
    EdgeKind::AttachedTo,
    EdgeKind::ArmedBy,
    EdgeKind::Banner,
    EdgeKind::Blocked,
    EdgeKind::ContinuedIn,
    EdgeKind::Fires,
    EdgeKind::ForToolUse,
    EdgeKind::ForkedFrom,
    EdgeKind::HookRun,
    EdgeKind::Interrupted,
    EdgeKind::Leaf,
    EdgeKind::Parent,
    EdgeKind::Redoes,
    EdgeKind::Refused,
    EdgeKind::RepliesTo,
    EdgeKind::Reports,
    EdgeKind::ResultOf,
    EdgeKind::Versions,
    EdgeKind::Wraps,
];

/// Where each variant sits in [`EVERY_KIND`].
///
/// The match is EXHAUSTIVE, so a variant added to [`EdgeKind`] and not to the list does not
/// compile: the only way to make it compile is to give it a position, and the test below
/// then demands that position actually hold it.
fn ordinal(k: EdgeKind) -> usize {
    match k {
        EdgeKind::AttachedTo => 0,
        EdgeKind::ArmedBy => 1,
        EdgeKind::Banner => 2,
        EdgeKind::Blocked => 3,
        EdgeKind::ContinuedIn => 4,
        EdgeKind::Fires => 5,
        EdgeKind::ForToolUse => 6,
        EdgeKind::ForkedFrom => 7,
        EdgeKind::HookRun => 8,
        EdgeKind::Interrupted => 9,
        EdgeKind::Leaf => 10,
        EdgeKind::Parent => 11,
        EdgeKind::Redoes => 12,
        EdgeKind::Refused => 13,
        EdgeKind::RepliesTo => 14,
        EdgeKind::Reports => 15,
        EdgeKind::ResultOf => 16,
        EdgeKind::Versions => 17,
        EdgeKind::Wraps => 18,
    }
}

/// The list IS the enum: nineteen words, each variant once, in the enum's own order.
///
/// The count is the vocabulary's own claim - every document that teaches it says nineteen -
/// so it is asserted rather than derived from the list's length, which would agree with
/// itself whatever the list held.
#[test]
fn the_vocabulary_list_holds_every_variant_in_enum_order() {
    assert_eq!(
        EVERY_KIND.len(),
        19,
        "the closed vocabulary is nineteen relations"
    );
    for (i, k) in EVERY_KIND.into_iter().enumerate() {
        assert_eq!(ordinal(k), i, "{k:?} is listed out of order, or twice");
    }
}

/// Exactly TWO relations ride JSON alone, and they are the engine's own.
///
/// A family edge exists because a record carries a key naming a target, which is what the
/// label zone is for. The engine's two are on nearly every unit and repeat what a rendered
/// transcript already shows, so they are the documented exception - and an exception worth
/// stating is worth pinning, because a third one would quietly remove a relation from every
/// text surface at once.
#[test]
fn only_the_engine_relations_ride_json_alone() {
    let hidden: Vec<&str> = EVERY_KIND
        .into_iter()
        .filter(|k| !k.rendered_in_label())
        .map(EdgeKind::as_str)
        .collect();
    assert_eq!(hidden, ["parent", "result_of"]);
}

/// Every relation word is DISTINCT.
///
/// Two variants sharing a word would make the relation ambiguous wherever it is printed -
/// a reader following `attached_to` would be following two different relations - and the
/// enum is the only place that can be checked, because `as_str` is a match nobody reads
/// end to end.
#[test]
fn every_edge_kind_has_its_own_word() {
    let mut words: Vec<&str> = EVERY_KIND.iter().map(|k| k.as_str()).collect();
    let before = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), before, "two relations share a word: {words:?}");
}

/// No word is empty and none carries a character a reader would have to quote to pass it
/// back - the words are printed in a label zone and read back as census keys.
#[test]
fn every_word_is_printable_as_it_stands() {
    for k in EVERY_KIND {
        let w = k.as_str();
        assert!(!w.is_empty(), "{k:?} has no word");
        assert!(
            w.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c == ' '),
            "{k:?} renders {w:?}, which is not a bare relation word"
        );
    }
}

/// FIRST WRITE WINS on a duplicate id, and an id the index never saw answers `None`.
///
/// The duplicate case is not hypothetical: a compaction re-anchor appends an earlier
/// record's block a second time, so the two copies carry the same `tool_use` id. The FIRST
/// is the call as it was MADE, which is where a result should be sent; a last-write index
/// would silently re-point every result in the file at the replay copy.
#[test]
fn a_tool_use_id_resolves_to_the_first_line_that_carried_it() {
    let mut ix = ProvIndex::default();
    ix.insert_tool_use("toolu_01aa", 4);
    ix.insert_tool_use("toolu_01aa", 9);
    ix.insert_tool_use("toolu_01bb", 7);
    assert_eq!(ix.tool_use_at("toolu_01aa"), Some(4));
    assert_eq!(ix.tool_use_at("toolu_01bb"), Some(7));
    assert_eq!(ix.tool_use_at("toolu_01zz"), None);
}

/// The THREE root answers, each with its own shape, and none of them an absent field.
#[test]
fn the_root_has_three_answers_and_says_which_it_is() {
    let opener = RootLink::opener();
    assert_eq!(opener.line, None);
    assert_eq!(opener.hops, None);
    assert_eq!(opener.note, Some(ROOT_IS_OPENER));
    assert_eq!(
        opener.label_zone(),
        None,
        "a tag on the opener would restate the line the reader is on"
    );

    let chained = RootLink::under(12, Some("u-1".to_string()), Some(3));
    assert_eq!(chained.line, Some(12));
    assert_eq!(chained.hops, Some(3));
    assert_eq!(chained.note, None, "a reached root has nothing to explain");
    assert_eq!(chained.label_zone().as_deref(), Some("[root L12 +3]"));

    let placed = RootLink::under(12, Some("u-1".to_string()), None);
    assert_eq!(placed.line, Some(12));
    assert_eq!(placed.hops, None);
    assert_eq!(
        placed.note,
        Some(ROOT_UNCHAINED),
        "the grouping placed it; the chain did not carry it"
    );
    assert_eq!(chained.label_zone().as_deref(), Some("[root L12 +3]"));
    assert_eq!(placed.label_zone().as_deref(), Some("[root L12]"));
}

/// A root whose opener has NO physical line (a merged elicitation-sidecar record) carries
/// no line at all rather than `L0`, which is an address that fetches nothing.
#[test]
fn a_root_with_no_physical_line_names_none() {
    let r = RootLink::under(0, Some("u-1".to_string()), Some(1));
    assert_eq!(r.line, None);
    assert_eq!(r.label_zone(), None);
}
