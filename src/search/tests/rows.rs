//! The borrowed file-order ROW SPACE: the two merges that put a surface's searchable
//! records and the chain-structural spine rows back into ONE index space.
//!
//! Keeping the two kinds in separate vectors is what stops the narrow row from being
//! carried at a record's width, and these merges are the whole price of that. What they
//! owe the rest of the tree is ORDER - ascending by physical jsonl line, with the
//! line-less elicitation-sidecar records after every scanned row - because every chain
//! answer, every turn boundary and every `turn_lines` span is addressed by an index into
//! the result. An out-of-order merge puts a record in the wrong turn silently.

use super::*;

/// A spine row at `line`, lifted from a real non-candidate line the way the scan lifts it.
fn spine_at(line: usize) -> crate::parse::SpineRow {
    let raw = format!(
        r#"{{"type":"attachment","uuid":"x{line}","parentUuid":"p{line}","timestamp":"2026-06-07T05:00:00.000Z","attachment":{{"type":"hook_additional_context","content":["ctx"]}}}}"#
    );
    crate::parse::spine_record(line, raw.as_bytes()).expect("an attachment lifts a spine row")
}

/// A retained record at `line`. Line `0` is a merged elicitation-sidecar record: it has no
/// physical line at all, which is exactly the case the merge has to keep out of file order.
fn kept_at(line: usize) -> Kept {
    Kept {
        rec: rec(&format!(
            r#"{{"type":"user","uuid":"u{line}","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"row {line}"}}}}"#
        )),
        can_hit: true,
        line_no: line,
        from_sidecar: line == 0,
    }
}

fn row_lines(rows: &[Row<'_>]) -> Vec<usize> {
    rows.iter().map(|r| Row::line_no(*r)).collect()
}

#[test]
fn merge_spine_interleaves_the_scanned_and_demoted_streams_by_line() {
    // Two ascending streams over DISJOINT lines: the scan's own spine rows, and the ones
    // the demotion produced from records the gates left unsearchable. The chain reads the
    // result as file order, so they interleave - a concatenation would hand it a DAG whose
    // rows arrive in an order no transcript ever had.
    let scanned: Vec<_> = [2usize, 4, 9].into_iter().map(spine_at).collect();
    let demoted: Vec<_> = [1usize, 5, 7].into_iter().map(spine_at).collect();
    let out = merge_spine(scanned, demoted);
    assert_eq!(
        out.iter()
            .map(crate::parse::SpineRow::line)
            .collect::<Vec<_>>(),
        vec![1, 2, 4, 5, 7, 9]
    );
}

#[test]
fn merge_spine_handles_a_stream_that_runs_out_first() {
    // The tail drain is its own arm: whichever stream empties, the rest must follow in
    // order and nothing may be dropped.
    let scanned: Vec<_> = [1usize, 2].into_iter().map(spine_at).collect();
    let demoted: Vec<_> = [8usize, 9, 10].into_iter().map(spine_at).collect();
    let out = merge_spine(scanned, demoted);
    assert_eq!(
        out.iter()
            .map(crate::parse::SpineRow::line)
            .collect::<Vec<_>>(),
        vec![1, 2, 8, 9, 10]
    );
}

#[test]
fn merge_rows_puts_records_and_spine_rows_in_file_order() {
    // A spine line BEFORE the first record and one BETWEEN two records, so both arms of
    // the interleave run. A merge that gives up and drains instead lands every spine row
    // in one block - the same lines, a different file order, and every turn membership
    // downstream of it is wrong.
    let records = vec![kept_at(5), kept_at(7)];
    let spine = vec![spine_at(2), spine_at(6)];
    let out = merge_rows(&records, &spine);
    assert_eq!(row_lines(&out), vec![2, 5, 6, 7]);
    assert!(out[0].kept().is_none(), "L2 is a chain-only spine row");
    assert!(out[1].kept().is_some(), "L5 is a searchable record");
    assert!(out[2].kept().is_none(), "L6 is a chain-only spine row");
    assert!(out[3].kept().is_some(), "L7 is a searchable record");
}

#[test]
fn a_sidecar_record_lands_after_every_scanned_row() {
    // A merged elicitation-sidecar record carries NO physical line (0). It is not part of
    // file order, so it must never win the line compare: the spine rows that physically
    // follow the last scanned record still come first, and the sidecar record is the tail.
    let records = vec![kept_at(2), kept_at(0)];
    let spine = vec![spine_at(5)];
    let out = merge_rows(&records, &spine);
    assert_eq!(row_lines(&out), vec![2, 5, 0]);
    assert!(
        out[2].kept().is_some_and(|k| k.from_sidecar),
        "the line-less record is last, not first"
    );
}
