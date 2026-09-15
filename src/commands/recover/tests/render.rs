//! Snapshot/report rendering: provenance labels, bodies, counts, gutters.

use super::*;

// ── (1b) gutter strip - BOTH tab and arrow forms ──

#[test]
fn strip_gutter_handles_tab_and_arrow_and_skips_unguttered() {
    // Tab gutter (current CC), arrow gutter (older), and a no-gutter line (skipped, never
    // fabricated). Locale-neutral content with an emoji.
    let snippet = "1\tcafé🛠\n2\u{2192}second\nno gutter here\n3\tthird";
    let got = strip_gutter(snippet);
    assert_eq!(
        got,
        vec![
            (1, "café🛠".to_string()),
            (2, "second".to_string()),
            (3, "third".to_string())
        ],
        "tab + arrow recovered, un-guttered line skipped (never fabricated)"
    );
}

#[test]
fn fmt_counts_omits_zeroes_and_flags_heuristic_bash() {
    let c = EventCounts {
        read_full: 2,
        read_windowed: 1,
        edit: 3,
        edit_unanchorable: 1,
        bash: 2,
        ..EventCounts::default()
    };
    let s = fmt_counts(&c);
    assert!(s.contains("3 read (2 full, 1 windowed)"), "{s}");
    assert!(s.contains("3 edit (1 un-anchorable)"), "{s}");
    assert!(s.contains("2 bash (heuristic)"), "{s}");
    assert!(!s.contains("write"), "zero writes omitted: {s}");
    assert_eq!(fmt_counts(&EventCounts::default()), "0");
}

// ── helpers ──

#[test]
fn split_lines_drops_only_trailing_newline() {
    assert_eq!(
        split_lines("a\nb\n"),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(split_lines("a\nb"), vec!["a".to_string(), "b".to_string()]);
    assert_eq!(split_lines(""), Vec::<String>::new());
    assert_eq!(line_count("a\nb\nc"), 3);
}

#[test]
fn truncate_excerpt_is_char_counted_and_explicit() {
    let long: String = "é".repeat(EXCERPT_MAX + 5);
    let t = truncate_excerpt(&long);
    assert!(t.contains("… (+5 chars)"), "explicit marker: {t}");
    assert_eq!(truncate_excerpt("short"), "short");
}

#[test]
fn first_line_trims_and_takes_first() {
    assert_eq!(first_line("  hello \nworld"), "hello");
    assert_eq!(first_line(""), "");
}

#[test]
fn snap_source_and_confidence_labels() {
    assert_eq!(SnapSource::Write.label(), "write");
    assert_eq!(SnapSource::FullRead.label(), "full-read");
    assert_eq!(SnapSource::FileAttachment.label(), "file-attachment");
    assert_eq!(Confidence::Authoritative.label(), "AUTHORITATIVE");
    assert_eq!(Confidence::Heuristic.label(), "HEURISTIC");
    assert_eq!(Confidence::Authoritative.json(), "authoritative");
    assert_eq!(Confidence::Heuristic.json(), "heuristic");
}

#[test]
fn render_snapshot_body_empty_known_is_explicit() {
    // No known lines + a seen total → the whole file is one explicit gap, never content.
    let body = render_snapshot_body(&[], 5, false);
    assert!(body.contains("??? lines 1..5 unknown"), "{body}");
    // No known lines + no total → an honest "no content" note.
    let none = render_snapshot_body(&[], 0, false);
    assert!(none.contains("no content seen"), "{none}");
}

#[test]
fn apply_line_range_filters_known_lines() {
    let lines = vec![
        (1usize, "a".to_string()),
        (5, "b".to_string()),
        (10, "c".to_string()),
    ];
    let got = apply_line_range(
        lines.clone(),
        Some(crate::text::parse_range_spec("5..10", "--file-lines", true).unwrap()),
    );
    assert_eq!(got, vec![(5, "b".to_string()), (10, "c".to_string())]);
    // None → unchanged.
    assert_eq!(apply_line_range(lines.clone(), None), lines);
}

#[test]
fn fmt_counts_write_only_and_external_and_integrity_arms() {
    // The first fmt_counts test left the write / external_edit / history_snapshot /
    // integrity_error display arms uncovered (it only had reads/edits/bash). Drive them.
    let c = EventCounts {
        write: 1,
        external_edit: 2,
        history_snapshot: 1,
        integrity_error: 3,
        ..EventCounts::default()
    };
    let s = fmt_counts(&c);
    assert!(s.contains("1 write"), "write arm: {s}");
    assert!(s.contains("2 external-edit"), "external-edit arm: {s}");
    assert!(
        s.contains("1 history-snapshot"),
        "history-snapshot arm: {s}"
    );
    assert!(s.contains("3 integrity-error"), "integrity-error arm: {s}");
    // An edit with NO un-anchorable companion → the empty-suffix branch of the edit arm.
    let only_edit = EventCounts {
        edit: 2,
        ..EventCounts::default()
    };
    let se = fmt_counts(&only_edit);
    assert_eq!(
        se, "2 edit",
        "no un-anchorable suffix when all edits anchored"
    );
}

#[test]
fn render_snapshot_body_inline_truncates_long_known_lines() {
    // `inline_trunc = true` truncates an over-long known line with the explicit marker; the
    // first body test only exercised the verbatim (`false`) path.
    let long = "z".repeat(EXCERPT_MAX + 10);
    let known = vec![(1usize, long.clone())];
    let trunc = render_snapshot_body(&known, 1, true);
    assert!(
        trunc.contains("… (+10 chars)"),
        "inline-truncated with explicit marker: {}",
        &trunc[..trunc.len().min(80)]
    );
    // The verbatim form keeps the full line.
    let verbatim = render_snapshot_body(&known, 1, false);
    assert!(verbatim.contains(&long), "verbatim form is not truncated");
}

#[test]
fn strip_gutter_skips_digits_without_a_separator_and_unparseable_widths() {
    // A line with leading digits but NEITHER a tab NOR an arrow separator → the final else
    // `continue` (no gutter recognized). And a digit run too large for usize → the
    // `digits.parse::<usize>()` Err side: skipped, never fabricated.
    let huge = "9".repeat(40); // overflows usize
    let snippet = format!("12 no-separator-here\n{huge}\tlost\n7\tkept");
    let got = strip_gutter(&snippet);
    assert_eq!(
        got,
        vec![(7, "kept".to_string())],
        "only the well-formed tab-guttered line survives: {got:?}"
    );
}
