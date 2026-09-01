//! Bash content anchors: extraction gates, suppression, and replay placement.

use super::*;

#[test]
fn heredoc_write_becomes_a_content_event_and_supersedes_the_touch() {
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cat > notes.md <<'EOF'\nalpha\nbeta\nEOF"}}]}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#,
    ]);
    let events = extract_events(&records, "/work/proj/notes.md");
    // ONE event: the content anchor; the heuristic BashTouch for the same command
    // is suppressed (never a self-inflicted boundary beside known content).
    assert_eq!(events.len(), 1, "{events:?}");
    match &events[0].kind {
        EventKind::FullSnapshot {
            content, source, ..
        } => {
            assert_eq!(content, "alpha\nbeta\n");
            assert_eq!(*source, SnapSource::BashHeredoc);
        }
        other => panic!("expected FullSnapshot, got {other:?}"),
    }
    let rep = replay(&events, None);
    assert_eq!(rep.counts.bash_write_anchor, 1);
    assert_eq!(rep.counts.bash, 0, "no boundary from the same command");
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "alpha".to_string()), (2, "beta".to_string())]
    );
}

#[test]
fn failed_bash_write_never_anchors() {
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cat > notes.md <<'EOF'\nalpha\nEOF"}}]}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","is_error":true,"content":"Permission denied"}]}}"#,
    ]);
    let events = extract_events(&records, "/work/proj/notes.md");
    // The write failed: no content anchor; the heuristic BashTouch row remains the
    // only trace (the mutation MAY still have partially landed - stay heuristic).
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::FullSnapshot { .. })),
        "{events:?}"
    );
}

#[test]
fn cat_read_anchor_needs_the_clean_result_echo() {
    let mk = |result_line: &str| {
        numbered(&[
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cat notes.md"}}]}}"#,
            result_line,
        ])
    };
    // Clean echo: the stdout IS the file.
    let clean = mk(
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"alpha\nbeta\n","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"alpha\nbeta\n"}]}}"#,
    );
    let events = extract_events(&clean, "/work/proj/notes.md");
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(
        matches!(&events[0].kind, EventKind::FullSnapshot { content, source, .. }
            if content == "alpha\nbeta\n" && *source == SnapSource::BashCat),
        "{events:?}"
    );
    // Non-empty stderr breaks the completeness gate: no anchor.
    let noisy = mk(
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"alpha\n","stderr":"cat: warning","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"alpha\n"}]}}"#,
    );
    assert!(extract_events(&noisy, "/work/proj/notes.md").is_empty());
    // An externalized stdout is not the inline text: no anchor.
    let persisted = mk(
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"alpha\n","stderr":"","interrupted":false,"persistedOutputPath":"/tmp/x.txt"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"alpha\n"}]}}"#,
    );
    assert!(extract_events(&persisted, "/work/proj/notes.md").is_empty());
    // A carrier-less (subagent-shaped) result proves nothing: no anchor.
    let bare = mk(
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"alpha\nbeta\n"}]}}"#,
    );
    assert!(extract_events(&bare, "/work/proj/notes.md").is_empty());
}

#[test]
fn sed_window_splices_and_head_to_eof_promotes_to_full() {
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"sed -n '3,4p' f.txt"}}]}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"gamma\ndelta\n","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"gamma\ndelta\n"}]}}"#,
    ]);
    let events = extract_events(&records, "/w/f.txt");
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(
        matches!(&events[0].kind, EventKind::BashWindowRead { start_line: 3, lines }
            if lines == &vec!["gamma".to_string(), "delta".to_string()]),
        "{events:?}"
    );
    let rep = replay(&events, None);
    assert_eq!(rep.counts.bash_read_anchor, 1);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(3, "gamma".to_string()), (4, "delta".to_string())]
    );

    // head -n 10 returning 2 lines reached EOF from line 1: the WHOLE file.
    let head = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"head -n 10 f.txt"}}]}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"one\ntwo\n","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"one\ntwo\n"}]}}"#,
    ]);
    let events = extract_events(&head, "/w/f.txt");
    assert!(
        matches!(&events[0].kind, EventKind::FullSnapshot { source, .. } if *source == SnapSource::BashCat),
        "EOF-from-line-1 promotes to a full anchor: {events:?}"
    );
}

#[test]
fn append_places_on_complete_buffer_else_discloses() {
    // Complete newline-terminated buffer: the append lands at the tail.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "alpha\n".into(),
                total_lines: 1,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::BashAppend {
                content: "beta\n".into(),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.bash_write_anchor, 1);
    assert!(rep.boundaries.is_empty(), "{:?}", rep.boundaries);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "alpha".to_string()), (2, "beta".to_string())]
    );

    // An incomplete buffer (windowed knowledge only): the append point is
    // unknowable - a disclosed heuristic boundary, never a guessed placement.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::BashWindowRead {
                start_line: 3,
                lines: vec!["gamma".into()],
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::BashAppend {
                content: "beta\n".into(),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.bash_write_anchor, 0);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "bash_append_unplaced");
}

#[test]
fn compound_write_needs_the_clean_echo_and_no_same_path_second_touch() {
    let cmd = "cat > notes.md <<'EOF'\nalpha\nEOF\npython3 notes.md";
    let mk = |result: &str| {
        let use_line = format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":{cmd}}}}}]}}}}"#,
            cmd = serde_json::to_string(cmd).unwrap()
        );
        vec![
            (
                1usize,
                rec(
                    r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
                ),
            ),
            (2usize, rec(&use_line)),
            (3usize, rec(result)),
        ]
    };
    // Clean echo: the compound-command write anchors.
    let clean = mk(
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"ran\n","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ran\n"}]}}"#,
    );
    let events = extract_events(&clean, "/w/notes.md");
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, EventKind::FullSnapshot { source, .. } if *source == SnapSource::BashHeredoc)),
        "{events:?}"
    );
    // Noisy stderr: the chain's exit code cannot vouch for the write - no anchor,
    // the heuristic touch row stays.
    let noisy = mk(
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"","stderr":"cat: cannot write","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#,
    );
    let events = extract_events(&noisy, "/w/notes.md");
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::FullSnapshot { .. })),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, EventKind::BashTouch { .. })),
        "the heuristic row survives when the anchor is refused: {events:?}"
    );

    // A SECOND touch of the same file later in the command kills the anchor
    // (segment order is not replayed within one line).
    let twice = "cat > notes.md <<'EOF'\nalpha\nEOF\nsed -i 's/a/b/' notes.md";
    let use_line = format!(
        r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":{c}}}}}]}}}}"#,
        c = serde_json::to_string(twice).unwrap()
    );
    let records = vec![
        (
            1usize,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
            ),
        ),
        (2usize, rec(&use_line)),
        (
            3usize,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#,
            ),
        ),
    ];
    let events = extract_events(&records, "/w/notes.md");
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::FullSnapshot { .. })),
        "same-path second touch refuses the anchor: {events:?}"
    );
}

#[test]
fn window_gates_and_mid_file_eof() {
    let mk = |cmd: &str, stdout_json: &str| {
        let use_line = format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":{c}}}}}]}}}}"#,
            c = serde_json::to_string(cmd).unwrap()
        );
        let result = format!(
            r#"{{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{{"stdout":{s},"stderr":"","interrupted":false}},"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"b1","content":{s}}}]}}}}"#,
            s = stdout_json
        );
        numbered(&[
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
            use_line.clone().leak(),
            result.clone().leak(),
        ])
    };
    // More stdout lines than the window can print: not this command's output.
    let over = mk("sed -n '3,4p' f.txt", r#""a\nb\nc\n""#);
    assert!(extract_events(&over, "/w/f.txt").is_empty());
    // A to-EOF window from MID-file: a window splice, exact total at EOF.
    let mid = mk("sed -n '5,$p' f.txt", r#""five\nsix\n""#);
    let events = extract_events(&mid, "/w/f.txt");
    assert!(
        matches!(&events[0].kind, EventKind::BashWindowRead { start_line: 5, lines }
            if lines.len() == 2),
        "{events:?}"
    );
    // A to-EOF window from line 1 IS the whole file.
    let full = mk("sed -n '1,$p' f.txt", r#""one\n""#);
    let events = extract_events(&full, "/w/f.txt");
    assert!(
        matches!(&events[0].kind, EventKind::FullSnapshot { source, .. } if *source == SnapSource::BashCat),
        "{events:?}"
    );
    // An empty window print (past EOF) places nothing.
    let empty = mk("sed -n '9,9p' f.txt", r#""""#);
    assert!(extract_events(&empty, "/w/f.txt").is_empty());
    // A truncate write anchors an empty file.
    let trunc = mk("truncate -s 0 f.txt", r#""""#);
    let events = extract_events(&trunc, "/w/f.txt");
    assert!(
        matches!(&events[0].kind, EventKind::FullSnapshot { content, source, .. }
            if content.is_empty() && *source == SnapSource::BashWrite),
        "{events:?}"
    );
}

#[test]
fn compound_write_without_result_echo_stays_a_boundary() {
    // A compound-command write in a carrier-less (subagent-shaped) lane: the clean
    // echo cannot be proven, so no anchor - the heuristic touch survives.
    let cmd = "cat > notes.md <<'EOF'\nalpha\nEOF\npython3 notes.md";
    let use_line = format!(
        r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":{c}}}}}]}}}}"#,
        c = serde_json::to_string(cmd).unwrap()
    );
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        use_line.leak(),
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ran"}]}}"#,
    ]);
    let events = extract_events(&records, "/w/notes.md");
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::FullSnapshot { .. })),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, EventKind::BashTouch { .. })),
        "{events:?}"
    );
}

#[test]
fn no_target_and_interrupted_gates() {
    // No --file target: the anchor pass is a no-op (nothing to join against).
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
    ]);
    let turns =
        group_turn_indices_deduped(&records.iter().map(|(_, r)| r).collect::<Vec<_>>(), |r| *r);
    let out = collect_turn_bash_anchors(&records, &turns[0], None, &Default::default());
    assert!(out.events.is_empty() && out.suppress.is_empty());

    // An INTERRUPTED compound command cannot vouch for its write.
    let cmd = "cat > notes.md <<'EOF'\nalpha\nEOF\nsleep 60";
    let use_line = format!(
        r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/w","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":{c}}}}}]}}}}"#,
        c = serde_json::to_string(cmd).unwrap()
    );
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        use_line.leak(),
        r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"","stderr":"","interrupted":true},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#,
    ]);
    let events = extract_events(&records, "/w/notes.md");
    assert!(
        events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::FullSnapshot { .. })),
        "{events:?}"
    );
}
