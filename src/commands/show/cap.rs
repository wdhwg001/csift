//! The context-flood guard both fetch modes share: which units a cap keeps, and the
//! continuation that fetches the rest.
//!
//! The continuation NAMES what was dropped, in every address form: the lines of the dropped
//! units, sorted, with ranges that overlap or touch merged, and a dropped unit that has no
//! line (a record merged from the elicitation sidecar) by its uuid. A reader that follows it
//! sees which records it did not get on this page, and a page never moves on past records
//! nobody named. Naming the address instead fails both ways: one `--line A..B` from the first
//! dropped line spans what the address left out (the gaps of a comma list, the records between
//! two uuids, the attachment line and the abandoned draft a turn window leaves out), and a turn
//! window cannot start mid-turn, so it cannot say where inside a turn wider than the cap the
//! page stopped.

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

/// What the reader addressed, which decides the order the cap keeps in. Clap refuses `--turn`
/// beside `--line` or `--uuid`, so a fetch is one of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Addressed {
    /// `--line` specs and/or `--uuid`s: kept by line.
    Records,
    /// A `--turn` window: kept by turn, then line, so a page is a prefix of the turns.
    Turns,
}

/// The continuation of a capped fetch: the address arguments that fetch what the cap dropped
/// and nothing that was shown - `--line 3..4,9..14`, `--uuid u1`, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Continuation {
    pub(crate) args: String,
}

/// The cap's verdict over a unit list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Capped {
    /// One flag per unit, in the order given: true keeps it.
    pub(crate) keep: Vec<bool>,
    /// Units the cap dropped.
    pub(crate) dropped: usize,
    /// How to fetch them, whenever something was dropped.
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
/// through one would leave the continuation re-fetching its kept half - as many as fit in
/// `cap` and always at least one. Records sort by line; a turn window sorts by turn, then
/// line.
pub(crate) fn cap_units(units: &[Unit], cap: usize, addressed: Addressed) -> Capped {
    let key = |u: &Unit| match addressed {
        Addressed::Records => (0, line_key(u)),
        Addressed::Turns => (u.turn.unwrap_or(usize::MAX), line_key(u)),
    };
    let all_kept = || Capped {
        keep: vec![true; units.len()],
        dropped: 0,
        continuation: None,
    };
    if units.len() <= cap {
        return all_kept();
    }
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_by_key(|&i| key(&units[i]));
    let first = key(&units[order[0]]);
    // The first key past the cap; a first record holding more than `cap` units is kept whole.
    let cutoff = match key(&units[order[cap]]) {
        k if k != first => k,
        _ => match order.iter().map(|&i| key(&units[i])).find(|&k| k > first) {
            Some(k) => k,
            None => return all_kept(),
        },
    };
    let keep: Vec<bool> = units.iter().map(|u| key(u) < cutoff).collect();
    let dropped: Vec<&Unit> = units
        .iter()
        .zip(&keep)
        .filter(|(_, &k)| !k)
        .map(|(u, _)| u)
        .collect();
    Capped {
        dropped: dropped.len(),
        continuation: Some(dropped_continuation(&dropped)),
        keep,
    }
}

/// The address arguments naming the dropped units: their lines, merged, then the uuids of the
/// ones with no line, each once, in the order they came.
fn dropped_continuation(dropped: &[&Unit]) -> Continuation {
    let lines: Vec<(usize, usize)> = dropped
        .iter()
        .filter(|u| u.line != 0)
        .map(|u| (u.line, u.line))
        .collect();
    let mut uuids: Vec<&str> = Vec::new();
    for u in dropped.iter().filter(|u| u.line == 0) {
        if let Some(id) = u.uuid {
            if !uuids.contains(&id) {
                uuids.push(id);
            }
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if !lines.is_empty() {
        parts.push(format!("--line {}", join_line_pieces(lines)));
    }
    if !uuids.is_empty() {
        parts.push(format!("--uuid {}", uuids.join(",")));
    }
    Continuation {
        args: parts.join(" "),
    }
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
