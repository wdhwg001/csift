//! The turn ROOT table, pinned directly on the function that computes it.
//!
//! Its e2e cases read the answer through a rendered hit, which is the right place to check
//! that a reader sees it and the wrong place to check the TABLE: the shapes that decide the
//! answer are positional (what sits above the opener, whether a parent link moves up the
//! turn at all) and a transcript that carried each of them would be four transcripts.

use super::*;

/// A `Kept` at the given line from one fixture line.
fn kept_at(line: &str, line_no: usize) -> Kept {
    Kept {
        rec: serde_json::from_str(line).expect("valid fixture record"),
        can_hit: true,
        line_no,
        from_sidecar: false,
        gated_raw: None,
    }
}

/// A turn over the given records, numbered from 1 as their own lines.
fn turn_of<'a>(rows: &'a [Row<'a>]) -> Turn<'a> {
    Turn {
        index: 0,
        records: rows.to_vec(),
        indices: (0..rows.len()).collect(),
    }
}

const LEAD: &str = r#"{"type":"attachment","uuid":"n0","attachment":{"type":"hook_success","hookName":"s","hookEvent":"SessionStart","command":"echo","exitCode":0,"stdout":"hi"}}"#;
const OPENER: &str = r#"{"type":"user","uuid":"n1","parentUuid":"n0","message":{"role":"user","content":"zzroot the question"}}"#;
const REPLY: &str = r#"{"type":"assistant","uuid":"n2","parentUuid":"n1","message":{"role":"assistant","content":[{"type":"text","text":"zzroot the reply"}]}}"#;
const DEEPER: &str = r#"{"type":"user","uuid":"n3","parentUuid":"n2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01z","content":"zzroot deeper"}]}}"#;

/// The OPENER is the record that opens the turn, and the three positions around it each get
/// their own answer.
///
/// Position 0 here is a hook attachment - the shape the shared grouper folds into turn 0 and
/// the shape a real transcript almost always starts with. Reading it as the opener named it
/// as the message every unit of the turn corresponds to; it now names nothing at all,
/// because the opener BELOW it would point a reader the wrong way down the file.
#[test]
fn the_opener_is_the_record_that_opens_the_turn_not_the_first_one() {
    let kept = [
        kept_at(LEAD, 1),
        kept_at(OPENER, 2),
        kept_at(REPLY, 3),
        kept_at(DEEPER, 4),
    ];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let turn = turn_of(&rows);
    let roots = TurnRoots::build(&turn);

    let lead = roots.link(&turn, 0);
    assert_eq!(lead.note, Some(crate::model::ROOT_NO_OPENER));
    assert_eq!(lead.line, None, "it sits ABOVE the opener");

    let opener = roots.link(&turn, 1);
    assert_eq!(opener.note, Some(crate::model::ROOT_IS_OPENER));

    let reply = roots.link(&turn, 2);
    assert_eq!(reply.line, Some(2), "the opener's own line");
    assert_eq!(reply.hops, Some(1));
    assert_eq!(reply.note, None);

    let deeper = roots.link(&turn, 3);
    assert_eq!(deeper.line, Some(2));
    assert_eq!(deeper.hops, Some(2), "two links up, counted once each");
}

/// A record whose parent is OUTSIDE the turn keeps the opener's address and no distance.
///
/// csift can place it - the grouping says which turn it is in - and cannot say how it got
/// there. The two facts are different and a fabricated count would hide the difference.
#[test]
fn a_record_the_chain_does_not_carry_keeps_the_address_and_no_distance() {
    let orphan = r#"{"type":"assistant","uuid":"n9","parentUuid":"somewhere-else","message":{"role":"assistant","content":[{"type":"text","text":"zzroot orphan"}]}}"#;
    let kept = [kept_at(OPENER, 1), kept_at(orphan, 2)];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let turn = turn_of(&rows);
    let roots = TurnRoots::build(&turn);

    let link = roots.link(&turn, 1);
    assert_eq!(link.line, Some(1), "the grouping still states the opener");
    assert_eq!(link.hops, None);
    assert_eq!(link.note, Some(crate::model::ROOT_UNCHAINED));
}

/// A SELF-PARENTING record gets no distance, and a link that points DOWN the turn gets none
/// either.
///
/// Both are the same rule from two sides: a parent precedes its child in file order, so a
/// link that does not move UP is a cycle or the inversion claim REC-075 measured, and the
/// honest answer is no distance rather than a number read off a loop.
#[test]
fn a_link_that_does_not_move_up_the_turn_yields_no_distance() {
    let looped = r#"{"type":"assistant","uuid":"n5","parentUuid":"n5","message":{"role":"assistant","content":[{"type":"text","text":"zzroot its own parent"}]}}"#;
    let downward = r#"{"type":"assistant","uuid":"n6","parentUuid":"n7","message":{"role":"assistant","content":[{"type":"text","text":"zzroot points below"}]}}"#;
    let below = r#"{"type":"assistant","uuid":"n7","parentUuid":"n1","message":{"role":"assistant","content":[{"type":"text","text":"zzroot the one it points at"}]}}"#;
    let kept = [
        kept_at(OPENER, 1),
        kept_at(looped, 2),
        kept_at(downward, 3),
        kept_at(below, 4),
    ];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let turn = turn_of(&rows);
    let roots = TurnRoots::build(&turn);

    assert_eq!(roots.link(&turn, 1).hops, None, "a self-parent is a cycle");
    assert_eq!(
        roots.link(&turn, 2).hops,
        None,
        "a link pointing below its own record carries no distance"
    );
    assert_eq!(
        roots.link(&turn, 3).hops,
        Some(1),
        "the record it pointed at is chained normally"
    );
}

/// A turn with NO opener at all - an abandoned branch whose head opens none - names none for
/// every position, rather than electing its first record.
#[test]
fn a_group_with_no_opener_names_none_for_every_position() {
    let kept = [kept_at(LEAD, 1), kept_at(REPLY, 2)];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let turn = turn_of(&rows);
    let roots = TurnRoots::build(&turn);
    for pos in 0..2 {
        let link = roots.link(&turn, pos);
        assert_eq!(link.note, Some(crate::model::ROOT_NO_OPENER), "pos {pos}");
        assert_eq!(link.line, None, "pos {pos}");
    }
}

/// ONE table per TURN, not one per PASS - asserted THROUGH the real path.
///
/// A turn with both matched hits and siblings runs two passes over the same records, and
/// each used to hold an `Option<TurnRoots>` of its own, so the table was built twice for a
/// turn whose cost is stated as "built on its FIRST hit and not before".
///
/// The count comes from `TurnRoots::build` itself and the driver is `reconstruct_and_match`
/// with `--siblings`, because that is the only shape where both passes run: a pin that
/// drove the cell directly and counted on the cell's own side passed BOTH mutations - an
/// unconditional rebuild inside the cell, and reverting the fix so the sibling pass gets a
/// fresh cell.
#[test]
fn a_turn_with_hits_and_siblings_builds_one_root_table() {
    // `carry` matches three records of turn 0 and none of turn 1, and turn 0's remaining
    // records become siblings - so both passes run, over one turn, and the other turn is
    // never touched.
    let mut a = args("carry");
    a.siblings = true;
    TURN_ROOT_BUILDS.with(|n| n.set(0));
    let ex = search(&fixture(), &a);
    assert_eq!(ex.len(), 1, "one matching turn");
    assert!(
        ex[0].hits.len() >= 2,
        "several records matched: {:?}",
        ex[0].hits.len()
    );
    assert!(!ex[0].siblings.is_empty(), "and others did not");
    assert_eq!(
        TURN_ROOT_BUILDS.with(std::cell::Cell::get),
        1,
        "one table for the turn, not one per pass over it"
    );
}

/// A turn that matched NOTHING pays nothing: the table is built on a turn's FIRST hit and
/// not before, which is the other half of the cost sentence.
#[test]
fn a_turn_with_no_hits_builds_no_root_table() {
    let mut a = args("zznothingmatchesthis");
    a.siblings = true;
    TURN_ROOT_BUILDS.with(|n| n.set(0));
    assert!(search(&fixture(), &a).is_empty());
    assert_eq!(TURN_ROOT_BUILDS.with(std::cell::Cell::get), 0);
}

/// Reuse cannot move an answer: the shared table answers every position exactly as a table
/// built from scratch does.
#[test]
fn the_shared_table_answers_what_a_fresh_one_does() {
    let kept = [
        kept_at(LEAD, 1),
        kept_at(OPENER, 2),
        kept_at(REPLY, 3),
        kept_at(DEEPER, 4),
    ];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let turn = turn_of(&rows);

    let mut cell = TurnRootsCell::default();
    let shared: Vec<crate::model::RootLink> = (0..rows.len())
        .map(|i| cell.get(&turn).link(&turn, i))
        .collect();
    let fresh = TurnRoots::build(&turn);
    let independent: Vec<crate::model::RootLink> =
        (0..rows.len()).map(|i| fresh.link(&turn, i)).collect();
    assert_eq!(shared, independent);
}
