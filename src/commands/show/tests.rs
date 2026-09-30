//! The cap and its continuation, pinned on unit lists built by hand: the e2e tests in
//! `tests/cli/show/` drive the same rule through real transcripts.

use super::*;

fn specs(explicit: &[usize], ranges: &[(usize, usize)]) -> LineSpecs {
    LineSpecs {
        explicit: explicit.iter().copied().collect(),
        ranges: ranges
            .iter()
            .map(|&(lo, hi)| LineRange {
                lo,
                hi,
                written: format!("{lo}..{hi}"),
            })
            .collect(),
        past_end: 0,
        total_lines: 100,
    }
}

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
fn a_comma_list_continues_as_its_own_ranges_clipped_at_the_first_dropped_line() {
    let s = specs(&[], &[(1, 4), (9, 14)]);
    let units: Vec<Unit> = [1, 2, 3, 4, 9, 12, 14].into_iter().map(line).collect();
    let none: [String; 0] = [];
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(capped.keep, [true, true, false, false, false, false, false]);
    assert_eq!(capped.dropped, 5);
    assert_eq!(args(&capped), Some("--line 3..4,9..14"));
    assert_eq!(capped.continuation.as_ref().map(|c| c.overlap), Some(0));
    // A cut past the first range leaves it out whole; an explicit line past the cut stays.
    let s = specs(&[20], &[(1, 4), (9, 14)]);
    let units: Vec<Unit> = [1, 2, 3, 4, 9, 20].into_iter().map(line).collect();
    let capped = cap_units(
        &units,
        4,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(args(&capped), Some("--line 9..14,20"));
    // An explicit line below the cut is not carried.
    let s = specs(&[2], &[(5, 8)]);
    let units: Vec<Unit> = [2, 5, 6].into_iter().map(line).collect();
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(args(&capped), Some("--line 6..8"));
}

#[test]
fn a_uuid_list_continues_with_the_dropped_uuids_in_the_order_given() {
    let s = specs(&[], &[]);
    let units = [named(1, "a"), named(3, "b"), named(8, "c"), named(11, "d")];
    let given: Vec<String> = ["d", "a", "c", "b"].map(String::from).to_vec();
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &given,
        },
    );
    assert_eq!(capped.keep, [true, true, false, false]);
    assert_eq!(args(&capped), Some("--uuid d,c"));
}

#[test]
fn a_uuid_whose_records_fall_on_both_sides_of_the_cut_is_named_by_its_dropped_lines() {
    let s = specs(&[], &[]);
    // A replay copy on L2 carries the uuid of its survivor on L9: `--uuid r` would fetch the
    // kept copy again, so the survivor is named by its line.
    let units = [named(1, "a"), named(2, "r"), named(5, "b"), named(9, "r")];
    let given: Vec<String> = ["a", "r", "b"].map(String::from).to_vec();
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &given,
        },
    );
    assert_eq!(args(&capped), Some("--line 9 --uuid b"));
}

#[test]
fn a_mixed_address_joins_both_parts_and_leaves_out_a_uuid_its_lines_already_carry() {
    // `--line 1..6 --uuid x,y`: x's record sits on L5, inside the clipped range; y's on L30.
    let s = specs(&[], &[(1, 6)]);
    let units = [line(1), line(2), named(5, "x"), line(6), named(30, "y")];
    let given: Vec<String> = ["x", "y"].map(String::from).to_vec();
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &given,
        },
    );
    assert_eq!(args(&capped), Some("--line 5..6 --uuid y"));
    // With nothing dropped on the line side, that part is left out.
    let s = specs(&[1], &[]);
    let units = [line(1), named(30, "y")];
    let capped = cap_units(
        &units,
        1,
        &Addressed::Records {
            specs: &s,
            uuids: &given,
        },
    );
    assert_eq!(args(&capped), Some("--uuid y"));
}

#[test]
fn a_record_with_no_line_sorts_last_and_continues_by_its_uuid() {
    let s = specs(&[], &[(1, 3)]);
    let units = [line(1), line(2), named(0, "pending")];
    let given: Vec<String> = vec!["pending".to_string()];
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &given,
        },
    );
    assert_eq!(capped.keep, [true, true, false]);
    assert_eq!(args(&capped), Some("--uuid pending"));
}

#[test]
fn the_cap_keeps_whole_records_and_at_least_one() {
    let s = specs(&[], &[(2, 4)]);
    let none: [String; 0] = [];
    // L2 holds two units under a one-unit cap: both kept, the cut at L3.
    let units = [line(2), line(2), line(3), line(4)];
    let capped = cap_units(
        &units,
        1,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(capped.keep, [true, true, false, false]);
    assert_eq!(args(&capped), Some("--line 3..4"));
    // A cut that lands inside a later record keeps it out whole.
    let units = [line(2), line(3), line(3), line(4)];
    let capped = cap_units(
        &units,
        2,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(capped.keep, [true, false, false, false]);
    // One record past the cap in all its units: nothing to cut, nothing dropped.
    let units = [line(2), line(2), line(2)];
    let capped = cap_units(
        &units,
        1,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(capped.keep, [true, true, true]);
    assert_eq!(capped.dropped, 0);
    assert_eq!(capped.continuation, None);
    // Within the cap: everything kept.
    let capped = cap_units(
        &units,
        3,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!((capped.dropped, capped.continuation), (0, None));
}

#[test]
fn a_turn_window_continues_as_a_window_and_counts_what_it_repeats() {
    let window = Addressed::Turns { last: 5 };
    // The cut on a turn boundary: t2 onward, nothing repeated.
    let units = [in_turn(1, 1), in_turn(2, 1), in_turn(4, 2), in_turn(6, 3)];
    let capped = cap_units(&units, 2, &window);
    let c = capped.continuation.expect("a continuation");
    assert_eq!(
        (c.args.as_str(), c.overlap, c.overlap_turn),
        ("--turn 2..5", 0, None)
    );
    // The cut inside t1: the window re-enters t1 and repeats its one kept unit.
    let capped = cap_units(&units, 1, &window);
    let c = capped.continuation.expect("a continuation");
    assert_eq!(
        (c.args.as_str(), c.overlap, c.overlap_turn),
        ("--turn 1..5", 1, Some(1))
    );
    // The window's last turn alone is written as one turn.
    let capped = cap_units(&units, 3, &Addressed::Turns { last: 3 });
    assert_eq!(args(&capped), Some("--turn 3"));
}

#[test]
fn a_turn_window_keeps_its_earliest_turns_whatever_their_lines() {
    // t1's record sits on a line above t0's last one: the window keeps t0 whole first.
    let units = [in_turn(5, 0), in_turn(9, 0), in_turn(7, 1)];
    let capped = cap_units(&units, 2, &Addressed::Turns { last: 1 });
    assert_eq!(capped.keep, [true, true, false]);
    assert_eq!(args(&capped), Some("--turn 1"));
}

#[test]
fn units_outside_the_numbering_give_a_window_no_continuation() {
    let units = [in_turn(1, 0), line(2), line(3)];
    let capped = cap_units(&units, 1, &Addressed::Turns { last: 0 });
    assert_eq!(capped.dropped, 2);
    assert_eq!(capped.continuation, None);
}

#[test]
fn touching_and_overlapping_line_pieces_merge() {
    let s = specs(&[7], &[(3, 5), (6, 6), (4, 9), (20, 22)]);
    let none: [String; 0] = [];
    let units: Vec<Unit> = [1, 3, 20].into_iter().map(line).collect();
    let capped = cap_units(
        &units,
        1,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(args(&capped), Some("--line 3..9,20..22"));
    // Pieces that only touch - one ends on the line before the next begins - merge too, and
    // a gap of one line keeps them apart.
    let s = specs(&[], &[(3, 5), (6, 8), (10, 11)]);
    let capped = cap_units(
        &units,
        1,
        &Addressed::Records {
            specs: &s,
            uuids: &none,
        },
    );
    assert_eq!(args(&capped), Some("--line 3..8,10..11"));
}

#[test]
fn the_overlap_clause_names_the_turn_and_the_unit() {
    assert_eq!(overlap_note(0, None, "unit"), "");
    assert_eq!(overlap_note(0, Some(3), "unit"), "");
    assert_eq!(
        overlap_note(2, Some(3), "line"),
        " (repeats the 2 line(s) of t3 shown above: a turn window starts at a turn's first record)"
    );
}
