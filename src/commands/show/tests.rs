//! The cap and its continuation, pinned on unit lists built by hand: the e2e tests in
//! `tests/cli/show/` drive the same rule through real transcripts.

use super::*;

fn line(line: usize) -> Unit<'static> {
    Unit {
        line,
        uuid: None,
        turn: None,
    }
}

fn named(line: usize, uuid: &'static str) -> Unit<'static> {
    Unit {
        line,
        uuid: Some(uuid),
        turn: None,
    }
}

fn in_turn(line: usize, turn: usize) -> Unit<'static> {
    Unit {
        line,
        uuid: None,
        turn: Some(turn),
    }
}

fn args(c: &Capped) -> Option<&str> {
    c.continuation.as_ref().map(|c| c.args.as_str())
}

#[test]
fn the_continuation_names_exactly_the_dropped_lines() {
    // A comma list's gaps (L5..L8, L10, L11, L13) are not named: nothing was dropped there.
    let units: Vec<Unit> = [1, 2, 3, 4, 9, 12, 14].into_iter().map(line).collect();
    let capped = cap_units(&units, 2, Addressed::Records);
    assert_eq!(capped.keep, [true, true, false, false, false, false, false]);
    assert_eq!(capped.dropped, 5);
    assert_eq!(args(&capped), Some("--line 3..4,9,12,14"));
}

#[test]
fn a_uuid_address_continues_by_the_dropped_lines() {
    // Given in any order, the uuids' records are named by line: the line names one record,
    // where a uuid shared by a replay copy and its survivor names two.
    let units = [named(11, "d"), named(1, "a"), named(8, "c"), named(3, "b")];
    let capped = cap_units(&units, 2, Addressed::Records);
    assert_eq!(capped.keep, [false, true, false, true]);
    assert_eq!(args(&capped), Some("--line 8,11"));
    let units = [named(1, "a"), named(2, "r"), named(5, "b"), named(9, "r")];
    let capped = cap_units(&units, 2, Addressed::Records);
    assert_eq!(args(&capped), Some("--line 5,9"));
}

#[test]
fn a_record_with_no_line_sorts_last_and_continues_by_its_uuid() {
    let units = [line(1), line(2), named(0, "pending")];
    let capped = cap_units(&units, 2, Addressed::Records);
    assert_eq!(capped.keep, [true, true, false]);
    assert_eq!(args(&capped), Some("--uuid pending"));
    // Beside dropped lines, the uuid follows them, each named once.
    let units = [
        line(1),
        line(2),
        named(0, "p"),
        line(3),
        named(0, "p"),
        named(0, "q"),
    ];
    let capped = cap_units(&units, 1, Addressed::Records);
    assert_eq!(args(&capped), Some("--line 2..3 --uuid p,q"));
}

#[test]
fn the_cap_keeps_whole_records_and_at_least_one() {
    // L2 holds two units under a one-unit cap: both kept, the cut at L3.
    let units = [line(2), line(2), line(3), line(4)];
    let capped = cap_units(&units, 1, Addressed::Records);
    assert_eq!(capped.keep, [true, true, false, false]);
    assert_eq!(args(&capped), Some("--line 3..4"));
    // A cut that lands inside a later record keeps it out whole, and its line once.
    let units = [line(2), line(3), line(3), line(4)];
    let capped = cap_units(&units, 2, Addressed::Records);
    assert_eq!(capped.keep, [true, false, false, false]);
    assert_eq!((capped.dropped, args(&capped)), (3, Some("--line 3..4")));
    // One record past the cap in all its units: nothing to cut, nothing dropped.
    let units = [line(2), line(2), line(2)];
    let capped = cap_units(&units, 1, Addressed::Records);
    assert_eq!(capped.keep, [true, true, true]);
    assert_eq!((capped.dropped, capped.continuation), (0, None));
    // Within the cap, and exactly at it: everything kept.
    for cap in [3, 4] {
        let capped = cap_units(&units, cap, Addressed::Records);
        assert_eq!(capped.keep, [true, true, true]);
        assert_eq!((capped.dropped, capped.continuation), (0, None));
    }
}

#[test]
fn a_turn_window_continues_by_the_lines_it_dropped() {
    // The cut on a turn boundary and the cut inside a turn both name the dropped lines: a
    // window cannot start mid-turn, and lines can.
    let units = [in_turn(1, 1), in_turn(2, 1), in_turn(4, 2), in_turn(6, 3)];
    let capped = cap_units(&units, 2, Addressed::Turns);
    assert_eq!(args(&capped), Some("--line 4,6"));
    let capped = cap_units(&units, 1, Addressed::Turns);
    assert_eq!(args(&capped), Some("--line 2,4,6"));
}

#[test]
fn a_turn_wider_than_the_cap_is_paged_forward_one_cap_at_a_time() {
    // Five units in t1, cap 2: each page names only what it left out, so the pages never
    // repeat a unit and never stall.
    let units: Vec<Unit> = (1..=5)
        .map(|l| in_turn(l, 1))
        .chain([in_turn(9, 2)])
        .collect();
    let first = cap_units(&units, 2, Addressed::Turns);
    assert_eq!(first.keep, [true, true, false, false, false, false]);
    assert_eq!(args(&first), Some("--line 3..5,9"));
    let rest: Vec<Unit> = units[2..].iter().map(|u| line(u.line)).collect();
    let second = cap_units(&rest, 2, Addressed::Records);
    assert_eq!(args(&second), Some("--line 5,9"));
}

#[test]
fn a_turn_window_keeps_its_earliest_turns_whatever_their_lines() {
    // t1's record sits on a line above t0's last one: the window keeps t0 whole first.
    let units = [in_turn(5, 0), in_turn(9, 0), in_turn(7, 1)];
    let capped = cap_units(&units, 2, Addressed::Turns);
    assert_eq!(capped.keep, [true, true, false]);
    assert_eq!(args(&capped), Some("--line 7"));
    // Addressed by line, the same units keep the lowest lines instead.
    let capped = cap_units(&units, 2, Addressed::Records);
    assert_eq!(capped.keep, [true, false, true]);
    assert_eq!(args(&capped), Some("--line 9"));
}

#[test]
fn units_outside_the_numbering_sort_after_every_turn() {
    let units = [line(1), in_turn(4, 0), line(2)];
    let capped = cap_units(&units, 1, Addressed::Turns);
    assert_eq!(capped.keep, [false, true, false]);
    assert_eq!((capped.dropped, args(&capped)), (2, Some("--line 1..2")));
}

#[test]
fn touching_and_overlapping_line_pieces_merge() {
    // Lines that touch merge into one range; a gap of one line keeps two ranges apart.
    let units: Vec<Unit> = [1, 3, 4, 5, 6, 7, 8, 10, 11, 20]
        .into_iter()
        .map(line)
        .collect();
    let capped = cap_units(&units, 1, Addressed::Records);
    assert_eq!(args(&capped), Some("--line 3..8,10..11,20"));
    // A line two units share is named once.
    let units = [line(1), line(6), line(6), line(7)];
    let capped = cap_units(&units, 1, Addressed::Records);
    assert_eq!(args(&capped), Some("--line 6..7"));
}
