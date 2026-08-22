//! Window disclosure units: the suggested search, opaque collection, hard/soft
//! boundary counts, and the resolved-path bash join.

use super::*;

#[test]
fn suggested_search_escapes_and_bounds() {
    let cmd = suggested_search(
        "notes.md",
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        Some("2026-06-07T05:00:00.000Z"),
        Some("2026-06-07T06:00:00.000Z"),
    );
    assert_eq!(
        cmd,
        "csift search 'notes\\.md' @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d -t agent.tool.use \
         --since 2026-06-07T05:00:00.000Z --until 2026-06-07T06:00:00.000Z"
    );
    // No instants -> no bounds; regex metachars escaped; a quote splices shell-safely.
    let bare = suggested_search("a+b's.md", "deadbeefcafe", None, None);
    assert_eq!(
        bare,
        "csift search 'a\\+b'\\''s\\.md' @deadbeefcafe -t agent.tool.use"
    );
}

#[test]
fn collect_opaque_counts_markers_and_powershell_not_index_git() {
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo fmt && npm install"}}]}}"#,
        // Index/ref-only git commands are not worktree threats; checkout is.
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b2","name":"Bash","input":{"command":"git add . && git commit -m x && git checkout -- ."}}]}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"PowerShell","input":{"command":"Set-Content -Path out.txt -Value hi"}}]}}"#,
    ]);
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices_deduped(&recs, |r| *r);
    let opaque = collect_opaque_commands("sess1", &records, &turns);
    let markers: Vec<&str> = opaque.iter().map(|o| o.marker.as_str()).collect();
    assert_eq!(
        markers,
        ["fmt:cargo", "pkg:npm", "git:checkout", "powershell"],
        "add/commit excluded; worktree-rewriting checkout kept"
    );
    assert!(opaque.iter().all(|o| o.session_id == "sess1"));
    assert_eq!(opaque[0].line_no, 2);
}

#[test]
fn hard_and_soft_boundary_counts_split_by_invalidation() {
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
            kind: EventKind::IntegrityError {
                kind: IntegrityKind::ModifiedSinceRead,
                raw: "File has been modified since read".into(),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.boundaries.len(), 2);
    assert_eq!(rep.boundaries_hard_count(), 1, "only the MSR invalidates");
    assert_eq!(rep.boundaries_soft_count(), 1, "the bash touch is soft");
}

#[test]
fn bash_join_resolves_against_the_record_cwd_with_verbatim_belt() {
    let records = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"echo x >> notes.md"}}]}}"#,
    ]);
    // Absolute --file matches the RESOLVED spelling of the relative operand.
    let abs = extract_events(&records, "/work/proj/notes.md");
    assert_eq!(abs.len(), 1, "resolved join: {abs:?}");
    match &abs[0].kind {
        EventKind::BashTouch {
            path, resolution, ..
        } => {
            assert_eq!(path, "/work/proj/notes.md");
            assert_eq!(*resolution, "cwd-joined");
        }
        other => panic!("expected BashTouch, got {other:?}"),
    }
    // The verbatim belt: a relative --file still matches what the command typed.
    let rel = extract_events(&records, "notes.md");
    assert_eq!(rel.len(), 1, "verbatim belt: {rel:?}");
}
