//! The context-flood guard both fetch modes share: which units a cap keeps, and the
//! continuation that fetches the rest, written in the ADDRESS DOMAIN the reader used.
//!
//! A continuation is only as good as the domain it is written in. One `--line A..B` range from
//! the first dropped line to the end of what was addressed is exact for a single line range
//! and for nothing else: under a comma list it spans the gaps between the ranges, under a uuid
//! list every record between the first and the last uuid, and under a turn window the records
//! the window leaves out (an abandoned draft, an attachment line). So a line address continues
//! as its own ranges clipped at the first dropped line, a uuid address as the dropped uuids,
//! and a turn window as a turn window.

use super::*;

/// One fetched unit as the cap sees it: a hit of the rendered fetch, a line of the raw one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unit<'a> {
    /// The physical jsonl line, 1-based. 0 is a record with no line: the elicitation-sidecar
    /// merge builds its rows with line 0, and a transcript row is numbered from 1, so the one
    /// test covers the sidecar row.
    pub(crate) line: usize,
    /// The record's own uuid.
    pub(crate) uuid: Option<&'a str>,
    /// The numbered turn the unit belongs to; `None` outside the numbering.
    pub(crate) turn: Option<usize>,
}

/// What the reader addressed, which is the domain the continuation is written in. Clap refuses
/// `--turn` beside `--line` or `--uuid`, so a fetch is one of the two.
#[derive(Debug)]
pub(crate) enum Addressed<'a> {
    /// `--line` specs and/or `--uuid`s, the uuids in the order given.
    Records {
        specs: &'a LineSpecs,
        uuids: &'a [String],
    },
    /// A `--turn` window, by the last turn it resolved to.
    Turns { last: usize },
}

/// The continuation of a capped fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Continuation {
    /// The address arguments: `--line 3..4,9..14`, `--uuid u1,u2`, both, or `--turn 2..5`.
    pub(crate) args: String,
    /// Units the continuation fetches that were already shown: a turn window cannot start
    /// mid-turn, so when the cap cut inside a turn the continuation repeats that turn's kept
    /// units. 0 for a line or uuid address, and for a turn window cut on a turn boundary.
    pub(crate) overlap: usize,
    /// The turn the repeated units belong to, set only when `overlap` is non-zero.
    pub(crate) overlap_turn: Option<usize>,
}

/// The cap's verdict over a unit list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Capped {
    /// One flag per unit, in the order given: true keeps it.
    pub(crate) keep: Vec<bool>,
    /// Units the cap dropped.
    pub(crate) dropped: usize,
    /// How to fetch them, when something was dropped and the domain can say it.
    pub(crate) continuation: Option<Continuation>,
}

/// A unit with no physical line sorts after every unit that has one.
fn line_key(u: &Unit) -> usize {
    if u.line == 0 {
        usize::MAX
    } else {
        u.line
    }
}

/// Keep the units that sort lowest, whole records - a record's units share its key, and a cut
/// through one would leave the continuation re-fetching its kept half - as many as fit in `cap`
/// and always at least one. Records sort by line; a turn window sorts by turn, then line, so
/// what it keeps is a prefix of the turns its continuation re-enters.
pub(crate) fn cap_units(units: &[Unit], cap: usize, addressed: &Addressed) -> Capped {
    let key = |u: &Unit| match addressed {
        Addressed::Records { .. } => (0, line_key(u)),
        Addressed::Turns { .. } => (u.turn.unwrap_or(usize::MAX), line_key(u)),
    };
    let everything = Capped {
        keep: vec![true; units.len()],
        dropped: 0,
        continuation: None,
    };
    if units.len() <= cap {
        return everything;
    }
    let mut keys: Vec<(usize, usize)> = units.iter().map(key).collect();
    keys.sort_unstable();
    // The first key past the cap; a first record holding more than `cap` units is kept whole.
    let cutoff = if keys[cap] == keys[0] {
        match keys.iter().copied().find(|&k| k > keys[0]) {
            Some(k) => k,
            None => return everything,
        }
    } else {
        keys[cap]
    };
    let keep: Vec<bool> = units.iter().map(|u| key(u) < cutoff).collect();
    let dropped = keep.iter().filter(|&&k| !k).count();
    let continuation = match addressed {
        Addressed::Records { specs, uuids } => {
            records_continuation(units, &keep, cutoff.1, specs, uuids)
        }
        Addressed::Turns { last } => turns_continuation(units, &keep, cutoff.0, *last),
    };
    Capped {
        keep,
        dropped,
        continuation,
    }
}

/// A turn window continues from the turn of the first dropped unit to the window's own end.
fn turns_continuation(
    units: &[Unit],
    keep: &[bool],
    first: usize,
    last: usize,
) -> Option<Continuation> {
    if first == usize::MAX {
        return None;
    }
    let overlap = units
        .iter()
        .zip(keep)
        .filter(|(u, &k)| k && u.turn == Some(first))
        .count();
    let args = if first == last {
        format!("--turn {first}")
    } else {
        format!("--turn {first}..{last}")
    };
    Some(Continuation {
        args,
        overlap,
        overlap_turn: (overlap > 0).then_some(first),
    })
}

/// A line and uuid address continues in its own terms: the addressed line ranges clipped at the
/// first dropped line, and the uuids every record of which was dropped. A uuid whose records
/// fall on both sides of the cut (a replay copy on an earlier line carries its survivor's uuid)
/// names its dropped records by line instead, because `--uuid` would fetch the kept one again.
fn records_continuation(
    units: &[Unit],
    keep: &[bool],
    cutoff: usize,
    specs: &LineSpecs,
    uuids: &[String],
) -> Option<Continuation> {
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    if cutoff != usize::MAX {
        pieces.extend(
            specs
                .explicit
                .iter()
                .filter(|&&n| n >= cutoff)
                .map(|&n| (n, n)),
        );
        pieces.extend(
            specs
                .ranges
                .iter()
                .filter(|r| r.hi >= cutoff)
                .map(|r| (r.lo.max(cutoff), r.hi)),
        );
    }
    // A piece starts at or after the cutoff, so a unit with no line is never covered.
    let covered = |line: usize| pieces.iter().any(|&(a, b)| (a..=b).contains(&line));
    let mut by_line: Vec<(usize, usize)> = Vec::new();
    let mut named: Vec<&str> = Vec::new();
    for u in uuids {
        let carrying: Vec<(&Unit, bool)> = units
            .iter()
            .zip(keep.iter().copied())
            .filter(|(x, _)| x.uuid == Some(u.as_str()))
            .collect();
        let lost: Vec<&Unit> = carrying
            .iter()
            .filter(|(_, k)| !k)
            .map(|(x, _)| *x)
            .collect();
        if lost.is_empty() {
            continue;
        }
        if carrying.iter().any(|(_, k)| *k) {
            by_line.extend(lost.iter().map(|x| (x.line, x.line)));
        } else if !lost.iter().all(|x| covered(x.line)) {
            named.push(u.as_str());
        }
    }
    pieces.extend(by_line);
    let mut parts: Vec<String> = Vec::new();
    if !pieces.is_empty() {
        parts.push(format!("--line {}", join_line_pieces(pieces)));
    }
    if !named.is_empty() {
        parts.push(format!("--uuid {}", named.join(",")));
    }
    (!parts.is_empty()).then(|| Continuation {
        args: parts.join(" "),
        overlap: 0,
        overlap_turn: None,
    })
}

/// Sort line pieces, merge the ones that overlap or touch, and spell them in the `--line`
/// grammar: `N` for one line, `A..B` for more.
fn join_line_pieces(mut pieces: Vec<(usize, usize)>) -> String {
    pieces.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in pieces {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
        .iter()
        .map(|&(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}..{b}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
