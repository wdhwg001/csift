//! The file-order ROW SPACE the survival axis is addressed by.
//!
//! A scan of one transcript produces two kinds of row, and they are deliberately kept in
//! separate vectors: the searchable records the surface's prefilter admitted, and the
//! chain-structural SPINE rows of the lines it dropped. A spine row is a fraction of a
//! record's width and most lines of a real transcript are spine, so one shared vector
//! would size every element to the record and carry the file's whole non-record majority
//! at record width - measured at ~120 MB per hand-off on a 397 MB transcript.
//!
//! [`Row`] is what puts the two back into ONE file-order index space, borrowed rather
//! than copied. Every chain answer ([`crate::model::Chain`]) is addressed by an index into
//! that merged list, and [`Row::kept`] is the single gate every emission pass takes: a
//! spine row carries no `message`, classifies to nothing and renders nothing.

use super::*;

/// A retained record. `can_hit` is the §7d keyword-prefilter verdict on the raw
/// line: when `false`, the line provably lacks the required literal, so it can
/// never be a regex hit and we skip the (more expensive) per-block regex matching
/// on it - but it is STILL retained so it can appear as a sibling record in a
/// matched turn's complete round-trip (SPEC §6.4). When the matcher has no
/// anchorable literal (case-insensitive or regex-with-metachars) every record is
/// `can_hit`.
#[derive(Debug)]
pub(crate) struct Kept {
    pub(crate) rec: Record,
    pub(crate) can_hit: bool,
    /// 1-based PHYSICAL line number of this record in its source jsonl (from the scanner) -
    /// a stable address (jsonl is append-only), surfaced per hit so `csift show --line N` (and
    /// raw `sed -n 'Np'`) can re-fetch the exact record. `0` for a merged elicitation-sidecar
    /// record (it has no physical transcript line - see `from_sidecar`).
    pub(crate) line_no: usize,
    /// True when this record was merged from the elicitation SIDECAR (§3.10), not scanned from
    /// the native jsonl. Such a record has no physical `line_no` (0); its hits render
    /// `(elicitation sidecar)` instead of `Lnnnn`.
    pub(crate) from_sidecar: bool,
}

/// One row of the file-order list the SURVIVAL AXIS walks: a searchable record, or a
/// chain-only SPINE row.
///
/// A spine row is an `attachment`/`system`/`last-prompt` line the §7d candidate prefilter
/// drops, lifted to its chain-structural fields ONLY ([`crate::parse::spine_record`]) so
/// the chain can see the DAG it walks. It carries no `message`, classifies to nothing and
/// is skipped by every record-consuming pass - it exists for [`crate::model::Chain`] alone.
///
/// The two kinds ride SEPARATE vectors (a spine row is a fraction of a record's width and
/// most lines of a real transcript are spine), and this borrowed row is what puts them back
/// in ONE file-order index space - the space every chain answer is addressed by.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Row<'a> {
    /// A searchable record: the only kind that classifies, matches, or emits.
    Rec(&'a Kept),
    /// A chain-only structural row (line number + the lifted record).
    Spine(&'a (usize, Record)),
}

impl<'a> Row<'a> {
    /// The searchable record, or `None` for a spine row - the ONE gate every emission pass
    /// takes.
    pub(crate) fn kept(self) -> Option<&'a Kept> {
        match self {
            Row::Rec(k) => Some(k),
            Row::Spine(_) => None,
        }
    }

    /// The underlying record. A spine row's carries the chain-structural fields and
    /// nothing else.
    pub(crate) fn rec(self) -> &'a Record {
        match self {
            Row::Rec(k) => &k.rec,
            Row::Spine((_, r)) => r,
        }
    }

    /// The 1-based physical jsonl line (`0` for a merged elicitation-sidecar record).
    pub(crate) fn line_no(self) -> usize {
        match self {
            Row::Rec(k) => k.line_no,
            Row::Spine((line, _)) => *line,
        }
    }
}

/// Merge two ascending-by-line spine streams into one: the scan's own, and the rows the
/// C-39 demotion produced from records the gates left unsearchable.
pub(crate) fn merge_spine(
    scanned: Vec<(usize, Record)>,
    demoted: Vec<(usize, Record)>,
) -> Vec<(usize, Record)> {
    let mut out: Vec<(usize, Record)> = Vec::with_capacity(scanned.len() + demoted.len());
    let mut a = scanned.into_iter().peekable();
    let mut b = demoted.into_iter().peekable();
    loop {
        let take_a = match (a.peek(), b.peek()) {
            (Some(x), Some(y)) => x.0 <= y.0,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_a {
            out.extend(a.next());
        } else {
            out.extend(b.next());
        }
    }
    out
}

/// Merge the searchable records and the chain-only spine rows of one transcript back into
/// ONE file-order list. Both inputs are ascending by line; the elicitation-sidecar records
/// carry no line (0) and are appended by the caller AFTER the scanned ones, so the merge
/// runs over the scanned prefix and the sidecar tail follows it unchanged.
pub(crate) fn merge_rows<'a>(records: &'a [Kept], spine: &'a [(usize, Record)]) -> Vec<Row<'a>> {
    let mut out: Vec<Row<'a>> = Vec::with_capacity(records.len() + spine.len());
    let (mut a, mut b) = (0usize, 0usize);
    while a < records.len() && b < spine.len() {
        // A sidecar record (line 0) never wins the compare: it is not part of file order,
        // so it must land after every scanned row, which the drain below does.
        if records[a].line_no > 0 && records[a].line_no <= spine[b].0 {
            out.push(Row::Rec(&records[a]));
            a += 1;
        } else if records[a].line_no > 0 {
            out.push(Row::Spine(&spine[b]));
            b += 1;
        } else {
            break;
        }
    }
    while b < spine.len() {
        out.push(Row::Spine(&spine[b]));
        b += 1;
    }
    for k in &records[a..] {
        out.push(Row::Rec(k));
    }
    out
}
