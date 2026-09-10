//! The parallel chunker's LINE NUMBERING. A jsonl line number is the address every
//! surface prints and `show --line` fetches, so a chunk boundary that lands one byte
//! early - on the newline instead of after it - hands the next chunk an empty leading
//! line and shifts what the file order looks like. The chunk COUNT is free to change
//! (it is a scheduling decision); the numbering is not.

use super::*;

/// Every line the chunked scan visits, with the number it was visited under.
fn numbered(bytes: &[u8], target_chunks: usize) -> Vec<(usize, String)> {
    let visit = |line: &[u8], line_no: usize| {
        LineVerdict::Keep((line_no, String::from_utf8_lossy(line).into_owned()))
    };
    let (rows, skipped) = scan_lines_parallel_chunked(bytes, &visit, target_chunks);
    assert_eq!(skipped, 0, "nothing here is a Skip verdict");
    rows
}

#[test]
fn a_multi_chunk_split_visits_the_same_lines_under_the_same_numbers() {
    // 40 short lines: with a few chunks the interior bounds land inside the file, which is
    // the only place the alignment can go wrong.
    let text: String = (1..=40).map(|i| format!("line-{i:02}\n")).collect();
    let one = numbered(text.as_bytes(), 1);
    assert_eq!(one.len(), 40, "one chunk visits every line once");
    assert_eq!(one[0], (1, "line-01".to_string()));
    assert_eq!(one[39], (40, "line-40".to_string()));
    for chunks in [2usize, 3, 5, 8, 13, 40] {
        assert_eq!(
            numbered(text.as_bytes(), chunks),
            one,
            "target_chunks={chunks} must be byte-for-byte the serial answer"
        );
    }
}

#[test]
fn a_file_with_no_trailing_newline_is_chunked_the_same_way() {
    // The last line of a live transcript is often still being written, so it has no
    // newline yet: the final bound is the slice end and no empty line may appear after it.
    let text = "alpha\nbravo\ncharlie\ndelta";
    let one = numbered(text.as_bytes(), 1);
    assert_eq!(one.len(), 4);
    assert_eq!(one[3], (4, "delta".to_string()));
    for chunks in [2usize, 3, 4, 9] {
        assert_eq!(
            numbered(text.as_bytes(), chunks),
            one,
            "target_chunks={chunks}"
        );
    }
}

/// The TWO-STREAM scan is the one-stream scan with the kinds kept apart, so the union of
/// its outputs must be the SAME lines under the SAME numbers - and the split must be a
/// partition, not a copy. This is the pin for the shape the survival spine rides: a
/// surface's own records on one stream, the chain-structural rows on the other.
#[test]
fn the_two_stream_scan_partitions_exactly_what_one_stream_kept() {
    // A mixed fixture: candidates (odd lines), spine-ish rows (even lines), one blank line
    // that is ignored, and one obviously-corrupt line that is counted.
    let mut text = String::new();
    for i in 1..=30 {
        if i == 11 {
            text.push_str("not json at all\n");
        } else if i == 12 {
            text.push('\n');
        } else if i % 2 == 1 {
            text.push_str(&format!("{{\"kind\":\"first\",\"n\":{i}}}\n"));
        } else {
            text.push_str(&format!("{{\"kind\":\"second\",\"n\":{i}}}\n"));
        }
    }
    let bytes = text.as_bytes();
    let is_first = |line: &[u8]| memchr::memmem::find(line, b"\"first\"").is_some();
    let is_second = |line: &[u8]| memchr::memmem::find(line, b"\"second\"").is_some();

    for chunks in [1usize, 2, 3, 7, 30] {
        // One stream, tagged - what the merged-vector shape produced.
        let merged = scan_lines_parallel_chunked(
            bytes,
            &|line: &[u8], line_no: usize| {
                if is_first(line) {
                    LineVerdict::Keep((line_no, true))
                } else if is_second(line) {
                    LineVerdict::Keep((line_no, false))
                } else {
                    non_candidate_verdict(line)
                }
            },
            chunks,
        );
        // Two streams, same routing.
        let (first, second, skipped) = scan_lines_parallel_split_chunked(
            bytes,
            &|line: &[u8], line_no: usize| {
                if is_first(line) {
                    SplitVerdict::First(line_no)
                } else if is_second(line) {
                    SplitVerdict::Second(line_no)
                } else {
                    non_candidate_split(line)
                }
            },
            chunks,
        );
        assert_eq!(skipped, merged.1, "same malformed count (chunks={chunks})");
        assert_eq!(skipped, 1, "the free-text line is the only Skip");
        let want_first: Vec<usize> = merged
            .0
            .iter()
            .filter(|(_, f)| *f)
            .map(|(n, _)| *n)
            .collect();
        let want_second: Vec<usize> = merged
            .0
            .iter()
            .filter(|(_, f)| !*f)
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(first, want_first, "first stream (chunks={chunks})");
        assert_eq!(second, want_second, "second stream (chunks={chunks})");
        // A partition: disjoint, and together the whole kept set in file order.
        let mut union = first.clone();
        union.extend(second.iter().copied());
        union.sort_unstable();
        let mut all: Vec<usize> = merged.0.iter().map(|(n, _)| *n).collect();
        all.sort_unstable();
        assert_eq!(union, all, "no line is dropped or duplicated");
    }
}
