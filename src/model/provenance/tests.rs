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
