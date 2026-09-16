//! The uuid map the build hands back ([`UuidIndex`]), and the `uuid -> line` lookup on
//! [`ClassifyCtx`] that rides it.
//!
//! What has to hold: the index IS the build's own map (not a second one built beside it),
//! it answers the way the loader's map does, and a caller that never supplies it gets no
//! guessed line.

use super::*;

/// The indexed build answers exactly what the plain one does. Two entry points, one
/// walk: if these ever parted, every survival verdict in the codebase would depend on
/// which entry point a surface happened to call.
#[test]
fn the_indexed_build_is_the_same_chain() {
    let r = recs(&[
        &u("u1", "", "one", "2026-06-07T05:00:00.000Z"),
        &a("a1", "u1", "reply", "2026-06-07T05:00:01.000Z"),
        &u("u2", "a1", "two", "2026-06-07T05:00:02.000Z"),
    ]);
    let plain = Chain::build(&r, None);
    let (indexed, _) = Chain::build_by_indexed(&r, |x| ChainNode::Full(x), None);
    let states: Vec<_> = (0..r.len()).map(|i| plain.survival(i)).collect();
    let same: Vec<_> = (0..r.len()).map(|i| indexed.survival(i)).collect();
    assert_eq!(states, same);
    assert_eq!(plain.leaf_index, indexed.leaf_index);
    assert_eq!(plain.abandoned_records, indexed.abandoned_records);
}

/// The index holds ONE entry per chain-admitted uuid and nothing else - the counting rule
/// that says it is the build's own map rather than a fresh pass over the records. The
/// fixture carries three admitted records plus a `last-prompt` line, which the loader's
/// map does not admit (it is not one of the four conversation types).
#[test]
fn the_index_holds_exactly_the_chain_admitted_uuids() {
    let r = recs(&[
        &u("u1", "", "one", "2026-06-07T05:00:00.000Z"),
        &a("a1", "u1", "reply", "2026-06-07T05:00:01.000Z"),
        &att("t1", "a1", "2026-06-07T05:00:02.000Z"),
        r#"{"type":"last-prompt","leafUuid":"a1","lastPrompt":"one"}"#,
    ]);
    let (_, index) = Chain::build_by_indexed(&r, |x| ChainNode::Full(x), None);
    assert_eq!(index.len(), 3);
    assert_eq!(index.index_of("u1"), Some(0));
    assert_eq!(index.index_of("a1"), Some(1));
    assert_eq!(index.index_of("t1"), Some(2));
    // A uuid no record here carries is absent, not guessed at.
    assert_eq!(index.index_of("nowhere"), None);
}

/// A uuid two lines carry resolves to the LATER one. That is the loader's own rule (its
/// map keeps the last occurrence), and it is the whole reason an edge can be rendered as
/// an address at all: pointing a reader at the dead copy of a re-anchored record would be
/// worse than printing the bare id.
#[test]
fn a_replayed_uuid_resolves_to_the_surviving_line() {
    let r = recs(&[
        &u("u1", "", "one", "2026-06-07T05:00:00.000Z"),
        &a("a1", "u1", "first copy", "2026-06-07T05:00:01.000Z"),
        &a("a1", "u1", "second copy", "2026-06-07T05:00:02.000Z"),
    ]);
    let (_, index) = Chain::build_by_indexed(&r, |x| ChainNode::Full(x), None);
    assert_eq!(index.index_of("a1"), Some(2));
}

/// A context with NO lookup answers `None` for every uuid. This is the "costs nothing"
/// half at the point a family fact asks the question: a surface that supplies no row
/// space (a file-oriented command, a unit fixture) gets an honest absence, never a
/// fabricated line, and pays one `Option` test to find out.
#[test]
fn a_context_without_a_lookup_never_guesses_a_line() {
    let ctx = ClassifyCtx::top_level();
    assert!(ctx.lines.is_none());
    assert_eq!(ctx.line_of_uuid("u1"), None);
}

/// And a context WITH one returns what the lookup says, verbatim.
#[test]
fn a_context_with_a_lookup_returns_its_answer() {
    struct OneLine;
    impl LineOfUuid for OneLine {
        fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
            (uuid == "u1").then_some(42)
        }
    }
    let mut ctx = ClassifyCtx::top_level();
    ctx.lines = Some(&OneLine);
    assert_eq!(ctx.line_of_uuid("u1"), Some(42));
    assert_eq!(ctx.line_of_uuid("u2"), None);
}
