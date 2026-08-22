//! Claude Code freshness-signal adoption: the staleReadFileStateHint parser and
//! boundary, the staleRecovered annotation, the widened integrity-error classifier,
//! the degraded external-edit form, and the re-armed originalFile check.

use super::*;

#[test]
fn stale_read_hint_parser_covers_the_wire_shapes() {
    let (paths, more) = parse_stale_read_hint(
        "[This command modified 1 file you've previously read: deps/recipes.json. \
         Call Read before editing.]",
    )
    .expect("single file");
    assert_eq!(paths, ["deps/recipes.json"]);
    assert_eq!(more, 0);

    let (paths, more) = parse_stale_read_hint(
        "[This command modified 6 files you've previously read: src/a.rs, src/b.rs, \
         src/c.rs, src/d.rs, src/e.rs and 1 more. Call Read before editing.]",
    )
    .expect("truncated list");
    assert_eq!(paths.len(), 5);
    assert_eq!(paths[0], "src/a.rs");
    assert_eq!(paths[4], "src/e.rs");
    assert_eq!(more, 1);

    assert!(parse_stale_read_hint("Shell cwd was reset to /x").is_none());
    assert!(parse_stale_read_hint("[This command modified nothing]").is_none());
}

#[test]
fn hint_names_the_file_and_becomes_a_hard_boundary() {
    // The hint rides the Bash RESULT record; its relative path resolves against that
    // record's own cwd before matching the absolute --file.
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"./gen.sh"}}]}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/work/proj","toolUseResult":{"stdout":"done","stderr":"","staleReadFileStateHint":"[This command modified 1 file you've previously read: src/api.rs. Call Read before editing.]"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"done"}]}}"#,
    ]);
    let events = extract_events(&records, "/work/proj/src/api.rs");
    assert_eq!(events.len(), 1, "hint event: {events:?}");
    match &events[0].kind {
        EventKind::StaleReadHint { path } => assert_eq!(path, "/work/proj/src/api.rs"),
        other => panic!("expected StaleReadHint, got {other:?}"),
    }
    let rep = replay(&events, None);
    assert_eq!(rep.counts.stale_hint, 1);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "hint_modified");
    assert_eq!(rep.boundaries[0].confidence, Confidence::Authoritative);
    assert_eq!(rep.boundaries_hard_count(), 1, "the hint invalidates");
    // A different target in the same session sees nothing from this hint.
    assert!(extract_events(&records, "/work/proj/other.rs").is_empty());
}

#[test]
fn stale_recovered_flag_is_an_authoritative_annotation() {
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit"}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"filePath":"/p/a.rs","oldString":"x","newString":"y","staleRecovered":true},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#,
    ]);
    let events = extract_events(&records, "/p/a.rs");
    assert_eq!(events.len(), 2, "the Edit and its annotation: {events:?}");
    assert!(matches!(events[1].kind, EventKind::StaleRecovered));
    let rep = replay(&events, None);
    assert_eq!(rep.counts.stale_recovered, 1);
    let b = rep
        .boundaries
        .iter()
        .find(|b| b.kind == "stale_recovered")
        .expect("annotation boundary");
    assert_eq!(b.confidence, Confidence::Authoritative);
    assert_eq!(
        rep.boundaries_hard_count(),
        0,
        "stale_recovered never invalidates"
    );
}

#[test]
fn widened_integrity_classifier_counts_failed_ops() {
    use serde_json::json;
    assert_eq!(
        classify_integrity_error(&json!("String to replace not found in file. String: x")),
        Some(IntegrityKind::StringNotFound)
    );
    assert_eq!(
        classify_integrity_error(&json!(
            "File does not exist. Note: your current working directory is /p."
        )),
        Some(IntegrityKind::FileDoesNotExist)
    );
    // Counted, never a boundary.
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::IntegrityError {
            kind: IntegrityKind::StringNotFound,
            raw: "String to replace not found in file.".into(),
        },
    }];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.integrity_error, 1);
    assert!(rep.boundaries.is_empty());
}

#[test]
fn degraded_external_edit_names_the_missing_snippet() {
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::ExternalEdit {
            snippet: Vec::new(),
        },
    }];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries[0]
            .detail
            .contains("no snippet: the change exceeded the attachment budget"),
        "degraded form named: {}",
        rep.boundaries[0].detail
    );
}

#[test]
fn soft_bash_boundary_no_longer_disarms_the_originalfile_check() {
    // Full anchor -> bash touch (soft) -> an Edit whose originalFile disagrees with
    // the replayed buffer. The disagreement must STILL fire: originalFile is disk
    // ground truth, and catching post-bash drift is exactly this check's job.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "alpha\nbeta\ngamma".into(),
                total_lines: 3,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::BashTouch {
                verb: "sed-i".into(),
                path: "/p/a.rs".into(),
                resolution: "absolute",
            },
        },
        FileEvent {
            line_no: 3,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "beta".into(),
                    new_string: "BETA".into(),
                    replace_all: false,
                }],
                original_file: Some("ALPHA2\nbeta\nGAMMA2".into()),
                structured_patch: None,
            },
        },
    ];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries
            .iter()
            .any(|b| b.kind == "original_file_disagreement"),
        "the check stays armed across a soft boundary: {:?}",
        rep.boundaries
    );
}

#[test]
fn external_edit_detail_gains_the_formatter_clue_when_one_ran() {
    let mk_scan = |opaque: Vec<OpaqueCommand>| ScanResult {
        session_id: "sess1".into(),
        is_subagent: false,
        parent_session_id: "sess1".into(),
        events: Vec::new(),
        opaque,
        merged_line_origin: std::collections::BTreeMap::new(),
        skipped_lines: 0,
    };
    let b = Boundary {
        line_no: 9,
        turn_index: 2,
        timestamp_utc: None,
        kind: "external_edit",
        confidence: Confidence::Authoritative,
        detail: "edited_text_file attachment (file changed outside the tool stream)".into(),
    };
    let with = mk_scan(vec![OpaqueCommand {
        session_id: "sess1".into(),
        line_no: 7,
        turn_index: 1,
        timestamp_utc: None,
        marker: "fmt:cargo".into(),
    }]);
    let clued = boundary_detail_with_clue(&with, &b);
    assert!(
        clued.contains("a formatter-class command (fmt:cargo) ran at L7 in this window"),
        "clue: {clued}"
    );
    // No formatter-class command in scope: the detail never speculates.
    let without = mk_scan(vec![OpaqueCommand {
        session_id: "sess1".into(),
        line_no: 7,
        turn_index: 1,
        timestamp_utc: None,
        marker: "pkg:npm".into(),
    }]);
    assert_eq!(boundary_detail_with_clue(&without, &b), b.detail);
    // A non-external-edit boundary is left alone even with a formatter in scope.
    let msr = Boundary {
        kind: "modified_since_read",
        ..b.clone()
    };
    assert_eq!(boundary_detail_with_clue(&with, &msr), msr.detail);
}
