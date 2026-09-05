//! The armed marker: it accumulates slots, keeps what the caller did not know, and is
//! absent rather than wrong when no hook has ever run.

use super::*;

#[test]
fn no_marker_means_no_delivery_hook_has_run_in_this_lane() {
    let fx = Fixture::new();
    assert!(read_armed(&fx.root, AGENT).unwrap().is_none());
}

#[test]
fn a_refresh_accumulates_slots_across_events() {
    let fx = Fixture::new();
    let first = refresh_armed(
        &fx.root,
        AGENT,
        1,
        "SessionStart",
        "2026-06-07T05:00:05Z",
        Some(SESSION),
        Some("2.1.258"),
    )
    .unwrap();
    assert_eq!(
        first.slots_seen.iter().copied().collect::<Vec<_>>(),
        vec![1]
    );

    let second = refresh_armed(
        &fx.root,
        AGENT,
        3,
        "Stop",
        "2026-06-07T05:01:05Z",
        Some(SESSION),
        None,
    )
    .unwrap();
    // Slot 1 is still known: slots fire one process each, and slot 3 refreshing the
    // marker must not erase the evidence that slot 1 exists.
    assert_eq!(
        second.slots_seen.iter().copied().collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(second.last_event.as_deref(), Some("Stop"));
    assert_eq!(second.last_ts_utc.as_deref(), Some("2026-06-07T05:01:05Z"));
    // The version the second caller did not know keeps its previous value rather than
    // being overwritten with nothing.
    assert_eq!(second.claude_code_version.as_deref(), Some("2.1.258"));

    let read_back = read_armed(&fx.root, AGENT).unwrap().unwrap();
    assert_eq!(read_back, second);
}

#[test]
fn a_repeated_slot_does_not_multiply_and_lanes_do_not_share_a_marker() {
    let fx = Fixture::new();
    for _ in 0..3 {
        refresh_armed(
            &fx.root,
            TEAMMATE,
            2,
            "PostToolUse",
            "2026-06-07T05:00:05Z",
            None,
            None,
        )
        .unwrap();
    }
    let marker = read_armed(&fx.root, TEAMMATE).unwrap().unwrap();
    assert_eq!(marker.slots_seen.len(), 1);
    assert_eq!(marker.hook_session, None);
    // A second lane has its own file.
    assert!(read_armed(&fx.root, AGENT).unwrap().is_none());
}

#[test]
fn the_marker_round_trips_through_its_projection() {
    let marker = ArmedMarker {
        slots_seen: [1u32, 2, 4].into_iter().collect(),
        last_event: Some("SubagentStop".to_string()),
        last_ts_utc: Some("2026-06-07T05:00:05Z".to_string()),
        hook_session: Some(SESSION.to_string()),
        claude_code_version: Some("2.1.258".to_string()),
    };
    assert_eq!(ArmedMarker::from_json(&marker.to_json()), marker);
    assert_eq!(marker.to_json()["slots_seen"], serde_json::json!([1, 2, 4]));
}

#[test]
fn a_damaged_marker_reads_as_absent_and_the_next_refresh_rebuilds_it() {
    let fx = Fixture::new();
    let path = armed_path(&fx.root, AGENT).unwrap();
    append_line(&path, "half a marker").unwrap();
    assert!(read_armed(&fx.root, AGENT).unwrap().is_none());
    let rebuilt = refresh_armed(
        &fx.root,
        AGENT,
        1,
        "SessionStart",
        "2026-06-07T05:00:05Z",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        rebuilt.slots_seen.iter().copied().collect::<Vec<_>>(),
        vec![1]
    );
    assert!(read_armed(&fx.root, AGENT).unwrap().is_some());
}

#[test]
fn a_marker_with_unreadable_slot_entries_keeps_the_readable_ones() {
    let v = serde_json::json!({
        "slots_seen": [1, "two", 3, -4, 5.5],
        "last_event": "Stop",
    });
    let marker = ArmedMarker::from_json(&v);
    assert_eq!(
        marker.slots_seen.iter().copied().collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(marker.last_event.as_deref(), Some("Stop"));
    assert_eq!(marker.last_ts_utc, None);
}
