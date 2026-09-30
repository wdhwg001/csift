//! The context-flood guard both fetch modes share: which units a cap keeps, and the
//! continuation that fetches the rest, written in the ADDRESS DOMAIN the reader used.
//!
//! A continuation is only as good as the domain it is written in. One `--line A..B` range from
//! the first dropped line to the end of what was addressed is exact for a single line range
//! and for nothing else: under a comma list it spans the gaps between the ranges, under a uuid
//! list every record between the first and the last uuid, and under a turn window the records
//! the window leaves out (an abandoned draft, an attachment line). So a line address continues
//! as its own ranges clipped at the first dropped line, a uuid address as the dropped uuids,
//! and a turn window as a turn window. A window cannot start mid-turn, so it continues from
//! the turn the cap cut inside and passes over that turn's units already shown with `--skip`:
//! without the skip, a turn wider than the cap continued as the command just run.

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

/// The continuation of a capped fetch: the address arguments that fetch what the cap dropped
/// and nothing that was shown - `--line 3..4,9..14`, `--uuid u1,u2`, both, or a turn window
/// `--turn 2..5 --skip 3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Continuation {
    pub(crate) args: String,
}

/// What became of one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// `--skip` passed over it.
    Skipped,
    Kept,
    /// The cap dropped it.
    Dropped,
}

/// The cap's verdict over a unit list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Capped {
    /// One flag per unit, in the order given: true keeps it.
    pub(crate) keep: Vec<bool>,
    /// Units `--skip` passed over, ahead of the cap.
    pub(crate) skipped: usize,
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

/// Pass over the first `skip` units and keep the next ones that sort lowest, whole records - a
/// record's units share its key, and a cut through one would leave the continuation
/// re-fetching its kept half - as many as fit in `cap` and always at least one. Records sort
/// by line; a turn window sorts by turn, then line, so what it keeps is a prefix of the turns
/// its continuation re-enters. `skip` counts in that same order, units of one record in the
/// order they render, so a continuation that names the units already shown moves forward.
pub(crate) fn cap_units(units: &[Unit], skip: usize, cap: usize, addressed: &Addressed) -> Capped {
    let key = |u: &Unit| match addressed {
        Addressed::Records { .. } => (0, line_key(u)),
        Addressed::Turns { .. } => (u.turn.unwrap_or(usize::MAX), line_key(u)),
    };
    let mut order: Vec<usize> = (0..units.len()).collect();
    // Stable: the units of one record keep the order they render in.
    order.sort_by_key(|&i| key(&units[i]));
    let skipped = skip.min(units.len());
    let mut fate = vec![Fate::Kept; units.len()];
    for &i in &order[..skipped] {
        fate[i] = Fate::Skipped;
    }
    let rest = &order[skipped..];
    let verdict = |fate: Vec<Fate>, continuation| Capped {
        keep: fate.iter().map(|&f| f == Fate::Kept).collect(),
        skipped,
        dropped: fate.iter().filter(|&&f| f == Fate::Dropped).count(),
        continuation,
    };
    if rest.len() <= cap {
        return verdict(fate, None);
    }
    let first = key(&units[rest[0]]);
    // The first key past the cap; a first record holding more than `cap` units is kept whole.
    let cutoff = match key(&units[rest[cap]]) {
        k if k != first => k,
        _ => match rest.iter().map(|&i| key(&units[i])).find(|&k| k > first) {
            Some(k) => k,
            None => return verdict(fate, None),
        },
    };
    for &i in rest {
        if key(&units[i]) >= cutoff {
            fate[i] = Fate::Dropped;
        }
    }
    let continuation = match addressed {
        Addressed::Records { specs, uuids } => {
            records_continuation(units, &fate, cutoff.1, specs, uuids)
        }
        Addressed::Turns { last } => turns_continuation(units, &fate, cutoff.0, *last),
    };
    verdict(fate, continuation)
}

/// A turn window continues from the turn of the first dropped unit to the window's own end,
/// passing over the units of that turn already shown - by an earlier `--skip` or on this
/// page. A window cannot start mid-turn, so without the skip a cut inside a turn wider than
/// the cap would continue as the command just run.
fn turns_continuation(
    units: &[Unit],
    fate: &[Fate],
    first: usize,
    last: usize,
) -> Option<Continuation> {
    if first == usize::MAX {
        return None;
    }
    let shown = units
        .iter()
        .zip(fate)
        .filter(|(u, &f)| f != Fate::Dropped && u.turn == Some(first))
        .count();
    let window = if first == last {
        format!("--turn {first}")
    } else {
        format!("--turn {first}..{last}")
    };
    let args = if shown == 0 {
        window
    } else {
        format!("{window} --skip {shown}")
    };
    Some(Continuation { args })
}

/// A line and uuid address continues in its own terms: the addressed line ranges clipped at the
/// first dropped line, and the uuids every record of which was dropped. A uuid whose records
/// fall on both sides of the cut (a replay copy on an earlier line carries its survivor's uuid)
/// names its dropped records by line instead, because `--uuid` would fetch the kept one - or
/// the one `--skip` passed over - again. No `--skip` rides along: nothing it names was shown.
fn records_continuation(
    units: &[Unit],
    fate: &[Fate],
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
        let carrying: Vec<(&Unit, Fate)> = units
            .iter()
            .zip(fate.iter().copied())
            .filter(|(x, _)| x.uuid == Some(u.as_str()))
            .collect();
        let lost: Vec<&Unit> = carrying
            .iter()
            .filter(|(_, f)| *f == Fate::Dropped)
            .map(|(x, _)| *x)
            .collect();
        if lost.is_empty() {
            continue;
        }
        if carrying.iter().any(|(_, f)| *f != Fate::Dropped) {
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
