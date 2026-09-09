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
