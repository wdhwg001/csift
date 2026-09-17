//! The provenance vocabulary and index, pinned directly.

use super::*;

/// The vocabulary, typed out: every variant, and its word.
///
/// Hand-kept rather than derived, because a list derived from the enum would assert
/// nothing about what the vocabulary CLAIMS - which is the whole reason the set is closed.
const EVERY_KIND: [EdgeKind; 17] = [
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
    EdgeKind::Redoes,
    EdgeKind::Refused,
    EdgeKind::RepliesTo,
    EdgeKind::Reports,
    EdgeKind::Versions,
    EdgeKind::Wraps,
];

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
