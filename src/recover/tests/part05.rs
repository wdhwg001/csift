use super::*;

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
fn gap_ranges_whole_file_unknown_when_no_known_lines() {
    // Empty known + total>0 → the `else if known.is_empty() && total > 0` arm yields one
    // whole-file gap. (The interior/trailing arms were already covered.)
    assert_eq!(gap_ranges(&[], 4), vec![(1, 4)]);
    // Empty known + total == 0 → no gaps at all (nothing seen, nothing to mark).
    assert_eq!(gap_ranges(&[], 0), Vec::<(usize, usize)>::new());
}

#[test]
fn structured_patch_multi_hunk_running_offset_anchors_later_hunk() {
    // A two-hunk patch where the FIRST hunk inserts extra lines (positive running offset),
    // shifting the SECOND hunk's anchored position. This exercises the cross-hunk offset
    // accounting (each later hunk's oldStart maps onto the already-shifted dense vector).
    // Buffer is a\nb\nc; hunk 1 expands line 1 into three lines, hunk 2 then changes the
    // (offset-shifted) last line.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb\nc".into(),
                total_lines: 3,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "a".into(),
                    new_string: "a1\na2\na3".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![
                    // hunk 1: line 1 (a) → three lines → running offset +2.
                    PatchHunk {
                        old_start: 1,
                        old_lines: 1,
                        new_lines: 3,
                        lines: vec!["-a".into(), "+a1".into(), "+a2".into(), "+a3".into()],
                    },
                    // hunk 2: original line 3 (c) → C. With offset +2 its start is 5, end 6,
                    // past the span+1 (=5) dense length → forces the resize.
                    PatchHunk {
                        old_start: 3,
                        old_lines: 1,
                        new_lines: 1,
                        lines: vec!["-c".into(), "+C".into()],
                    },
                ]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![
            (1, "a1".into()),
            (2, "a2".into()),
            (3, "a3".into()),
            (4, "b".into()),
            (5, "C".into()),
        ],
        "both hunks applied with the running offset; the trailing line C was reached via the resize"
    );
}

#[test]
fn replay_edit_as_first_event_opens_a_segment() {
    // An Edit arriving with NO prior read/anchor (seg_open == None) takes the TRUE side of
    // `if seg_open.is_none()` and OPENS a fresh segment. With no known content it cannot
    // anchor (un-anchorable hole), but a segment is opened so the op is timeline-visible.
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::Edit {
            hunks: vec![EditHunk {
                old_string: "x".into(),
                new_string: "y".into(),
                replace_all: false,
            }],
            original_file: None,
            structured_patch: None,
        },
    }];
    let rep = replay(&events, None);
    assert_eq!(
        rep.segments.len(),
        1,
        "the lone edit opened a segment (seg_open was None → the if-body ran)"
    );
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "with no known content the edit is an honest hole"
    );
}

#[test]
fn collect_tool_use_paths_skips_missing_and_empty_file_path() {
    // A Read tool_use with NO file_path input, and an Edit tool_use with an EMPTY one →
    // neither is recorded (the `if let Some(p)` None side + the `!p.is_empty()` false side).
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"r1","name":"Read","input":{"limit":5}},{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"","old_string":"x","new_string":"y"}}]}}"#,
    ]);
    let mut out = std::collections::BTreeMap::new();
    for (_, r) in &recs {
        collect_tool_use_paths(r.blocks(), &mut out);
    }
    assert!(
        out.is_empty(),
        "a Read with no file_path and an Edit with an empty one record no id→path entry: {out:?}"
    );
}

#[test]
fn bash_tool_use_without_command_is_ignored() {
    // A Bash tool_use with no `command` input → the `if let Some(cmd)` None side; no
    // BashTouch is produced (and nothing panics on the missing field).
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"timeout":1000}}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert!(
        ev.is_empty(),
        "a Bash tool_use with no command touches nothing: {}",
        ev.len()
    );
}

#[test]
fn history_snapshot_without_tracked_backups_is_ignored() {
    // A file-history-snapshot whose snapshot object lacks `trackedFileBackups` → the
    // `if let Some(tfb)` None side; no marker event is emitted.
    let recs = numbered(&[
        r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-06-07T05:00:00.000Z","messageId":"m1"}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert!(
        ev.is_empty(),
        "a snapshot with no trackedFileBackups yields no marker: {}",
        ev.len()
    );
}

#[test]
fn apply_edit_falls_back_to_string_edit_when_structured_patch_is_empty() {
    // A structured_patch of `Some(vec![])` (empty) takes the `if !patches.is_empty()` FALSE
    // side and falls through to the string-replacement path, which still applies cleanly.
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("hello\nworld", 2, 1);
    let hunks = vec![EditHunk {
        old_string: "world".into(),
        new_string: "there".into(),
        replace_all: false,
    }];
    let outcome = apply_edit(&mut buf, &hunks, &Some(vec![]), 9);
    assert_eq!(
        outcome,
        EditOutcome::Applied,
        "empty patch → string fallback applied"
    );
    assert_eq!(
        buf.known_lines(),
        vec![(1, "hello".into()), (2, "there".into())]
    );
}

#[test]
fn structured_patch_pure_insertion_adjacent_to_known_lines_applies() {
    // A pure insertion (old_lines == 0) adjacent to known content drives the
    // `h.old_lines > 0` FALSE side of both the region-known and context-verify guards, and
    // the insertion is applied (the gap-isolated variant is covered separately as a hole).
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb".into(),
                total_lines: 2,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "a".into(),
                    new_string: "a\nINS".into(),
                    replace_all: false,
                }],
                original_file: None,
                // Insert after line 1 (old_lines 0, adjacent to the known line 1).
                structured_patch: Some(vec![PatchHunk {
                    old_start: 2,
                    old_lines: 0,
                    new_lines: 1,
                    lines: vec!["+INS".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "a".into()), (2, "INS".into()), (3, "b".into())],
        "the adjacent pure insertion is applied without fabricating islands"
    );
    assert_eq!(
        rep.counts.edit_unanchorable, 0,
        "an anchored insertion is not a hole"
    );
}

#[test]
fn edit_originalfile_present_but_no_full_anchor_does_not_flag() {
    // An Edit carrying an originalFile arrives BEFORE any full anchor (had_full_anchor is
    // false) → the `had_full_anchor && …` short-circuit FALSE side: no disagreement
    // boundary is raised (we cannot prove drift without an anchor to compare against).
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::Edit {
            hunks: vec![EditHunk {
                old_string: "a".into(),
                new_string: "A".into(),
                replace_all: false,
            }],
            original_file: Some("totally\ndifferent".into()),
            structured_patch: None,
        },
    }];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries.is_empty(),
        "no anchor yet → originalFile cannot be cross-checked → no false boundary"
    );
}

#[test]
fn structured_patch_skips_context_check_when_old_region_len_mismatches() {
    // A malformed hunk that claims old_lines=2 but lists only ONE old-region line drives the
    // `old_region.len() == h.old_lines` FALSE side: the context-equality check is skipped
    // (we cannot compare a mismatched region), but the region-known guard still applies. The
    // region IS known here (lines 1-2), so the edit is applied by position without the
    // (impossible) context comparison.
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\nc", 3, 1);
    // old_lines=2 but only one " " context line listed → old_region.len()==1 != 2.
    let patches = vec![PatchHunk {
        old_start: 1,
        old_lines: 2,
        new_lines: 1,
        lines: vec![" a".into(), "+merged".into()],
    }];
    let outcome = apply_structured_patch(&mut buf, &patches, 9);
    assert_eq!(
        outcome,
        EditOutcome::Applied,
        "a mismatched-length old-region is applied by position, skipping the context check"
    );
    // The new region is the context (` a`) + added (`+merged`) lines, spliced over the
    // declared 2-line old span (lines 1-2), so line 3 (c) is preserved after them.
    assert_eq!(
        buf.known_lines(),
        vec![(1, "a".into()), (2, "merged".into()), (3, "c".into())]
    );
}

#[test]
fn edit_originalfile_agreeing_with_buffer_raises_no_boundary() {
    // An Edit whose originalFile MATCHES the anchored buffer drives the
    // `buffer_disagrees_with_original(...)` FALSE side (anchor present, no drift) → no
    // disagreement boundary, and the edit applies normally.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb\nc".into(),
                total_lines: 3,
                source: SnapSource::FullRead,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "b".into(),
                    new_string: "B".into(),
                    replace_all: false,
                }],
                // originalFile AGREES with the replayed buffer → no disagreement.
                original_file: Some("a\nb\nc".into()),
                structured_patch: Some(vec![PatchHunk {
                    old_start: 2,
                    old_lines: 1,
                    new_lines: 1,
                    lines: vec!["-b".into(), "+B".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert!(
        !rep.boundaries
            .iter()
            .any(|b| b.kind == "original_file_disagreement"),
        "an agreeing originalFile raises no boundary"
    );
    assert_eq!(
        rep.final_buffer.known.get(&2).map(|c| c.text.as_str()),
        Some("B"),
        "the edit applied cleanly"
    );
}

#[test]
fn unified_diff_caps_leading_context_at_three_lines() {
    // A change far into the file (preceded by many identical lines) must show AT MOST 3
    // lines of leading context — driving the `ctx_back < CONTEXT` cap of the back-context
    // walk. Lines 1-6 are identical; line 7 changes. The hunk's first context line is line
    // 4 (7 minus 3), never line 1.
    let old: Vec<String> = (1..=8).map(|n| format!("line{n}")).collect();
    let mut new = old.clone();
    new[6] = "line7-CHANGED".to_string(); // change the 7th line (index 6)
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("-line7") && d.contains("+line7-CHANGED"),
        "the change: {d}"
    );
    // Exactly three leading context lines (line4, line5, line6); line3 and earlier excluded.
    assert!(
        d.contains(" line4\n line5\n line6\n"),
        "3 lines of leading context: {d}"
    );
    assert!(
        !d.contains(" line1\n") && !d.contains(" line3\n"),
        "context is capped at 3 lines (line1..line3 excluded): {d}"
    );
}

#[test]
fn unified_diff_full_context_shows_every_line() {
    // usize::MAX context (what --patches passes) reproduces the WHOLE file as context — a
    // far-away change still drags every read line into one spanning hunk. This is what makes
    // `--patches` of a fully-read, one-line-edited file contain all lines (CC's Read-before-Edit
    // guarantees those context lines were genuinely observed, so they are valid to include).
    let old: Vec<String> = (1..=8).map(|n| format!("line{n}")).collect();
    let mut new = old.clone();
    new[6] = "line7-CHANGED".to_string();
    let d = unified_diff(&old, &new, usize::MAX);
    // Every distant line appears as context — line1 and line3 are NOT excluded here.
    for n in [1, 2, 3, 4, 5, 6, 8] {
        assert!(
            d.contains(&format!(" line{n}\n")),
            "full context keeps line{n}: {d}"
        );
    }
    assert!(
        d.contains("-line7") && d.contains("+line7-CHANGED"),
        "the change is still marked: {d}"
    );
    // One spanning hunk over all 8 lines.
    assert_eq!(d.matches("@@ -").count(), 1, "single full-span hunk: {d}");
}

#[test]
fn buffer_disagrees_below_threshold_does_not_flag() {
    // A single mismatch in many comparable lines stays UNDER the 25% threshold → the
    // `mismatches * 4 >= compared` FALSE side: not flagged (one fluke is not a boundary).
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\nc\nd\ne\nf\ng\nh", 8, 1);
    // Original differs in exactly ONE of eight lines (12.5% < 25%).
    assert!(
        !buffer_disagrees_with_original(&buf, "a\nb\nc\nX\ne\nf\ng\nh"),
        "1/8 mismatch is below the disagreement threshold"
    );
}
