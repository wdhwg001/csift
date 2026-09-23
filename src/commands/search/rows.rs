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

/// A retained record. `can_hit` is the section 7d keyword-prefilter verdict on the raw
/// line: when `false`, the line provably lacks the required literal, so it can
/// never be a regex hit and we skip the (more expensive) per-block regex matching
/// on it - but it is STILL retained so it can appear as a sibling record in a
/// matched turn's complete round-trip (SPEC section 6.4). When the matcher has no
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
    /// True when this record was merged from the elicitation SIDECAR (section 3.10), not scanned from
    /// the native jsonl. Such a record has no physical `line_no` (0); its hits render
    /// `(elicitation sidecar)` instead of `Lnnnn`.
    pub(crate) from_sidecar: bool,
    /// The record's own source line, kept ONLY for a NON-ROLE line - one a gate or an
    /// explicit address admitted, never one the section 7d role keep already had. Those are the
    /// lines whose content the `Record` struct cannot answer for: it deserializes what
    /// csift uses and ignores the rest, and a bookkeeping line consists of exactly the
    /// rest. The render and the unmodeled unit read it; `None` on every conversation
    /// record, so the scan allocates nothing for a line it would have kept anyway.
    pub(crate) gated_raw: Option<String>,
}

/// One row of the file-order list the SURVIVAL AXIS walks: a searchable record, or a
/// chain-only SPINE row.
///
/// A spine row is an `attachment`/`system`/`last-prompt` line the section 7d candidate prefilter
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
    /// A chain-only structural row.
    Spine(&'a crate::parse::SpineRow),
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

    /// This row as the chain reads it.
    pub(crate) fn node(self) -> crate::model::ChainNode<'a> {
        match self {
            Row::Rec(k) => crate::model::ChainNode::Full(&k.rec),
            Row::Spine(s) => crate::model::ChainNode::Spine(s),
        }
    }

    /// The 1-based physical jsonl line (`0` for a merged elicitation-sidecar record).
    pub(crate) fn line_no(self) -> usize {
        match self {
            Row::Rec(k) => k.line_no,
            Row::Spine(s) => s.line(),
        }
    }

    /// This row's `uuid` - a spine row carries one too (it is what the chain keys on).
    pub(crate) fn uuid(self) -> Option<&'a str> {
        self.node().uuid()
    }

    /// This row's RAW timestamp: the turn's chronological key comes from its FIRST row,
    /// which is a spine row whenever a turn's own opener was dropped by the prefilter.
    pub(crate) fn timestamp(self) -> Option<&'a str> {
        self.node().timestamp()
    }
}

/// The `uuid -> jsonl LINE` answer for one transcript, in the search surface's own row
/// space: the chain's uuid map ([`crate::model::UuidIndex`]) joined to the merged row
/// list that map was built over.
///
/// It exists because the two halves of the question live in different places. The model
/// layer knows which RECORD a uuid names (its map is what the walk runs on); only the
/// surface knows what LINE that record sat on, because a row is either a record or a
/// spine row and each carries its own line. Neither half builds anything new: the map is
/// moved out of the build, and the rows are the ones already in hand.
///
/// A line of `0` reads as absent. That is the merged elicitation-sidecar record, which
/// has no physical line at all - pointing a reader at `L0` would be worse than saying
/// nothing.
pub(crate) struct RowLines<'a> {
    index: crate::model::UuidIndex<'a>,
    rows: &'a [Row<'a>],
}

impl<'a> RowLines<'a> {
    /// Join the build's map to the rows it was built over.
    pub(crate) fn new(index: crate::model::UuidIndex<'a>, rows: &'a [Row<'a>]) -> Self {
        RowLines { index, rows }
    }
}

impl crate::model::LineOfUuid for RowLines<'_> {
    fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
        let line = self.rows.get(self.index.index_of(uuid)?)?.line_no();
        (line > 0).then_some(line)
    }
}

// Hand-written because the crate denies a type without one (`missing_debug_implementations`)
// and neither field can derive it: a `UuidIndex` borrows the build's map and `rows` is a
// borrowed slice, so printing them would dump the transcript. The two SIZES are what a
// reader debugging a `for` edge actually wants - did the map see this uuid, and over how
// many rows.
impl std::fmt::Debug for RowLines<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowLines")
            .field("uuids", &self.index.len())
            .field("rows", &self.rows.len())
            .finish()
    }
}

/// Merge two ascending-by-line spine streams into one: the scan's own, and the rows
/// demoted from records the gates left unsearchable.
pub(crate) fn merge_spine(
    scanned: Vec<crate::parse::SpineRow>,
    demoted: Vec<crate::parse::SpineRow>,
) -> Vec<crate::parse::SpineRow> {
    let mut out: Vec<crate::parse::SpineRow> = Vec::with_capacity(scanned.len() + demoted.len());
    let mut a = scanned.into_iter().peekable();
    let mut b = demoted.into_iter().peekable();
    loop {
        let take_a = match (a.peek(), b.peek()) {
            (Some(x), Some(y)) => x.line() <= y.line(),
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
pub(crate) fn merge_rows<'a>(
    records: &'a [Kept],
    spine: &'a [crate::parse::SpineRow],
) -> Vec<Row<'a>> {
    let mut out: Vec<Row<'a>> = Vec::with_capacity(records.len() + spine.len());
    let (mut a, mut b) = (0usize, 0usize);
    while a < records.len() && b < spine.len() {
        // A sidecar record (line 0) never wins the compare: it is not part of file order,
        // so it must land after every scanned row, which the drain below does.
        if records[a].line_no > 0 && records[a].line_no <= spine[b].line() {
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
