//! The provenance vocabulary and index, pinned directly.

use super::*;

/// The vocabulary IS [`EdgeKind::ALL`], generated beside the enum by the `edge_kinds!`
/// declaration, so a variant cannot exist outside it. The tests below walk that.
///
/// It was a hand-kept array here, and twice that was not enough. First it held all but two
/// of the set - `parent` and `result_of` absent - so the uniqueness and JSON-alone
/// tests were statements about a sample. Then an exhaustive variant-to-position `match`
/// was added to close it, and it did not: it forces an author to TOUCH this file, not to
/// put the variant into the list, and a twentieth variant given an arm, a colliding word
/// and no row in the array passed clippy and the whole suite.
/// The vocabulary is TWENTY-SIX relations, and that is the claim every document makes.
///
/// Asserted against the number rather than derived from the list, which would agree with
/// itself whatever the list held - and since the list is generated from the enum, this is
/// the enum's own variant count.
#[test]
fn the_vocabulary_is_twenty_six_relations() {
    assert_eq!(
        EdgeKind::ALL.len(),
        26,
        "the closed vocabulary is twenty-six relations"
    );
}

/// Exactly SIX relations ride JSON alone, and all six are the engine's own.
///
/// A family edge exists because a record carries a key naming a target, which is what the
/// label zone is for. The engine's six are read off the file's shape instead, and each
/// already reaches a reader through the unit it sits on - `parent` through the record
/// above, `result_of` through the pairing glyph, and the four SURVIVAL relations through
/// `[replay copy of L<n>]`, `[abandoned]` / `[rewound]` and the draft-distance line. They
/// are the documented exception, and an exception worth stating is worth pinning: a seventh
/// would quietly remove a relation from every text surface at once, and a sixth removed
/// would put a duplicate tag on every abandoned unit.
#[test]
fn only_the_engine_relations_ride_json_alone() {
    let hidden: Vec<&str> = EdgeKind::ALL
        .iter()
        .filter(|k| !k.rendered_in_label())
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        hidden,
        [
            "branches",
            "continues",
            "parent",
            "replays",
            "result_of",
            "supersedes"
        ]
    );
}

/// Every relation word is DISTINCT.
///
/// Two variants sharing a word would make the relation ambiguous wherever it is printed -
/// a reader following `attached_to` would be following two different relations - and the
/// enum is the only place that can be checked, because `as_str` is a match nobody reads
/// end to end.
#[test]
fn every_edge_kind_has_its_own_word() {
    let mut words: Vec<&str> = EdgeKind::ALL.iter().map(|k| k.as_str()).collect();
    let before = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), before, "two relations share a word: {words:?}");
}

/// No word is empty and none carries a character a reader would have to quote to pass it
/// back - the words are printed in a label zone and read back as census keys.
#[test]
fn every_word_is_printable_as_it_stands() {
    for k in EdgeKind::ALL {
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

/// The FOUR root answers, each with its own shape, and none of them an absent field.
///
/// The fourth is the one a real transcript meets first: a session opens with a synthetic
/// pre-user lead the turn grouper folds into turn 0, and those units have no opener above
/// them at all. It was constructed nowhere in this file while the test's own name said
/// three, so the shape that the first lines of every live transcript produce was pinned by
/// the e2e alone.
#[test]
fn the_root_has_four_answers_and_says_which_it_is() {
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

    let lead = RootLink::no_opener();
    assert_eq!(lead.line, None);
    assert_eq!(lead.uuid, None);
    assert_eq!(lead.hops, None);
    assert_eq!(
        lead.note,
        Some(ROOT_NO_OPENER),
        "there is nothing above this unit to name, which is a fact and not a gap"
    );
    assert_eq!(
        lead.label_zone(),
        None,
        "no line, so no tag - the note carries the whole answer"
    );

    // The four notes are four DISTINCT values: a consumer switches on this field, so two
    // answers sharing one would be one answer wearing two shapes.
    let mut notes = vec![opener.note, chained.note, placed.note, lead.note];
    let before = notes.len();
    notes.sort_unstable();
    notes.dedup();
    assert_eq!(notes.len(), before, "two answers share a note: {notes:?}");
}

/// A root ALWAYS has a line, and this is why: the only record csift holds with no physical
/// line is a merged elicitation-sidecar record, and neither shape the sidecar carries opens
/// a turn - an unresolved AskUserQuestion or ExitPlanMode rides an `assistant` record, and
/// an MCP elicitation rides a `system` one. So no turn's opener can be one, and the guard
/// that used to collapse a zero line stood for a caller that does not exist.
///
/// Pinned on the SHAPES rather than on the guard, because the guard was never the fact: if
/// one of these ever did open a turn, a `None` line would hide it behind a fifth wire shape
/// no manual names, and this test would name it instead.
#[test]
fn no_sidecar_shape_opens_a_turn_so_a_root_always_has_a_line() {
    for line in [
        r#"{"type":"assistant","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_01x","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01x","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
        r#"{"type":"system","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp","csiftKey":"some-server","content":"MCP elicitation pending"}"#,
    ] {
        let r: crate::model::Record = serde_json::from_str(line).expect("fixture parses");
        assert!(r.is_elicitation_marker(), "the fixture is a marker: {line}");
        assert!(!r.opens_turn(), "a sidecar record opens no turn: {line}");
    }
    let r = RootLink::under(12, Some("u-1".to_string()), Some(1));
    assert_eq!(r.line, Some(12), "the address a reader can fetch");
    assert_eq!(r.label_zone().as_deref(), Some("[root L12 +1]"));
}
