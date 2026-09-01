//! File-history snapshot instrument: divergence rebase, content-less silence
//! boundary, generation resets, and the mtime-verified content gate.

use super::*;

fn write_event(line_no: usize, content: &str) -> FileEvent {
    FileEvent {
        line_no,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::FullSnapshot {
            content: content.to_string(),
            total_lines: line_count(content),
            source: SnapSource::Write,
        },
    }
}

fn marker(line_no: usize, version: u64, content: Option<&str>) -> FileEvent {
    FileEvent {
        line_no,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::HistorySnapshotMarker {
            version: Some(version),
            backup_file: None,
            backup_time: None,
            content: content.map(str::to_string),
        },
    }
}

#[test]
fn divergent_snapshot_rebases_and_discloses() {
    // The incident shape: a tool write, then a snapshot whose verified content
    // DISAGREES (a harness-side write deleted a line) - the replay rebases on the
    // snapshot and a later edit applies on the rebased base.
    let events = vec![
        write_event(1, "alpha\nkeep = yes\n"),
        marker(5, 2, Some("alpha\n")),
        FileEvent {
            line_no: 8,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "alpha".into(),
                    new_string: "alpha-2".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: None,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.snapshot_rebase, 1);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "external_write");
    assert!(
        rep.boundaries[0].detail.contains("REBASED"),
        "{:?}",
        rep.boundaries
    );
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "alpha-2".to_string())],
        "the edit lands on the REBASED base; the deleted line never resurrects"
    );

    // Agreement: no boundary, no rebase.
    let events = vec![write_event(1, "alpha\n"), marker(5, 2, Some("alpha\n"))];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.snapshot_rebase, 0);
    assert!(rep.boundaries.is_empty(), "{:?}", rep.boundaries);
}

#[test]
fn content_less_silent_jump_is_a_hard_boundary() {
    // v1 seen, a version JUMP with no mutation event in between and no verified
    // content: the silence signal alone - an authoritative content-less boundary.
    let events = vec![
        write_event(1, "alpha\n"),
        marker(3, 1, None),
        marker(6, 2, None),
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "external_write");
    assert!(
        rep.boundaries[0].detail.contains("unavailable"),
        "{:?}",
        rep.boundaries
    );
    assert_eq!(rep.boundaries_hard_count(), 1, "external_write is HARD");
    // The buffer stays (StaleReadHint shape - content-less, nothing to splice).
    assert_eq!(rep.final_buffer.known_lines().len(), 1);

    // The same jump WITH a tool write in the interval explains itself: no boundary.
    let events = vec![
        write_event(1, "alpha\n"),
        marker(3, 1, None),
        write_event(5, "alpha\nbeta\n"),
        marker(6, 2, None),
    ];
    let rep = replay(&events, None);
    assert!(rep.boundaries.is_empty(), "{:?}", rep.boundaries);

    // An unchanged version never signals.
    let events = vec![
        write_event(1, "a\n"),
        marker(3, 1, None),
        marker(6, 1, None),
    ];
    let rep = replay(&events, None);
    assert!(rep.boundaries.is_empty(), "{:?}", rep.boundaries);
}

#[test]
fn generation_reset_bounds_the_number_signal_but_not_the_content_check() {
    // A version DECREASE = the counter restarted (a new process). The number-based
    // silence signal must NOT fire across the reset...
    let events = vec![
        write_event(1, "alpha\n"),
        marker(3, 7, None),
        marker(6, 2, None),
    ];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries.is_empty(),
        "a reset is not a write: {:?}",
        rep.boundaries
    );

    // ...but a verified CONTENT disagreement still rebases across it.
    let events = vec![
        write_event(1, "alpha\n"),
        marker(3, 7, None),
        marker(6, 2, Some("gamma\n")),
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.snapshot_rebase, 1);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "gamma".to_string())]
    );
}

#[test]
fn attach_gate_requires_mtime_agreement_and_version_change() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("csift-snap-{}-{n}", std::process::id()));
    let sess = "aaaa1111-2222-4333-8444-555566667777";
    let store = home.join("file-history").join(sess);
    std::fs::create_dir_all(&store).unwrap();
    let projects = home.join("projects").join("-w");
    std::fs::create_dir_all(&projects).unwrap();
    let session_path = projects.join(format!("{sess}.jsonl"));
    std::fs::write(&session_path, "").unwrap();

    // A store blob whose mtime we control.
    let blob = store.join("abcd1234@v2");
    std::fs::write(&blob, "snap body\n").unwrap();
    let bt = "2026-06-07T05:00:00Z";
    let instant: jiff::Timestamp = bt.parse().unwrap();
    let t = std::time::UNIX_EPOCH
        + std::time::Duration::from_secs(u64::try_from(instant.as_second()).unwrap());
    std::fs::File::options()
        .write(true)
        .open(&blob)
        .unwrap()
        .set_modified(t)
        .unwrap();

    let mk = |version: u64, bt: &str| FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::HistorySnapshotMarker {
            version: Some(version),
            backup_file: Some("abcd1234@v2".into()),
            backup_time: Some(bt.to_string()),
            content: None,
        },
    };
    let content_of = |e: &FileEvent| match &e.kind {
        EventKind::HistorySnapshotMarker { content, .. } => content.clone(),
        _ => None,
    };

    // NOTE: attach resolves the store under the REAL claude home; point it at the
    // fixture via the env-independent pure path? attach_snapshot_content uses
    // crate::path::claude_home() - the OnceLock override may already be set by
    // another test, so only the verified_store_content gate is unit-tested here;
    // the full attach path is covered end to end by the e2e fixture.
    let _ = (mk(2, bt), content_of); // keep helpers referenced

    // mtime agrees: content returned.
    assert_eq!(
        verified_store_content(&blob, bt).as_deref(),
        Some("snap body\n")
    );
    // mtime far away (a generation-collided name): refused.
    assert_eq!(verified_store_content(&blob, "2026-06-09T05:00:00Z"), None);
    // garbage instant / missing file: refused.
    assert_eq!(verified_store_content(&blob, "not a time"), None);
    assert_eq!(verified_store_content(&store.join("missing@v9"), bt), None);
    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn first_marker_divergence_rebases_and_tolerance_is_inclusive() {
    // The FIRST marker is a baseline worth checking: a divergence there rebases too.
    let events = vec![write_event(1, "old\n"), marker(3, 1, Some("new\n"))];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.snapshot_rebase, 1);
    assert_eq!(rep.final_buffer.known_lines(), vec![(1, "new".to_string())]);

    // The mtime window is inclusive: exactly 120s off still verifies; 121s refuses.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("csift-tol-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let blob = dir.join("hash@v1");
    std::fs::write(&blob, "body\n").unwrap();
    let bt = "2026-06-07T05:00:00Z";
    let instant: jiff::Timestamp = bt.parse().unwrap();
    let t = std::time::UNIX_EPOCH
        + std::time::Duration::from_secs(u64::try_from(instant.as_second() + 120).unwrap());
    std::fs::File::options()
        .write(true)
        .open(&blob)
        .unwrap()
        .set_modified(t)
        .unwrap();
    assert_eq!(
        verified_store_content(&blob, bt).as_deref(),
        Some("body\n"),
        "exactly 120s off is inside the window"
    );
    let t2 = std::time::UNIX_EPOCH
        + std::time::Duration::from_secs(u64::try_from(instant.as_second() + 121).unwrap());
    std::fs::File::options()
        .write(true)
        .open(&blob)
        .unwrap()
        .set_modified(t2)
        .unwrap();
    assert_eq!(verified_store_content(&blob, bt), None, "121s is outside");
    std::fs::remove_dir_all(&dir).unwrap();
}
