//! The per-lane ledger: line projections, the fold into per-message state, and the
//! consecutive-block count a queue delivery leans on.

use super::*;

fn ts(s: &str) -> String {
    s.to_string()
}

fn all_kinds() -> Vec<LedgerLine> {
    vec![
        emit(1, 2, Vehicle::AdditionalContext),
        LedgerLine::Held {
            id: MSG_ID.to_string(),
            reason: "block-cap".to_string(),
            ts_utc: ts("2026-06-07T05:00:06Z"),
        },
        LedgerLine::Expired {
            id: MSG_ID.to_string(),
            ts_utc: ts("2026-06-07T05:00:07Z"),
        },
        LedgerLine::Ack {
            id: MSG_ID.to_string(),
            ts_utc: ts("2026-06-07T05:00:08Z"),
        },
        LedgerLine::Redelivered {
            id: MSG_ID.to_string(),
            source: RedeliverSource::Compact,
            ts_utc: ts("2026-06-07T05:00:09Z"),
        },
        LedgerLine::Refused {
            id: None,
            reason: "served call: empty transcript_path".to_string(),
            ts_utc: ts("2026-06-07T05:00:10Z"),
        },
    ]
}

#[test]
fn every_ledger_kind_round_trips_through_its_projection() {
    for line in all_kinds() {
        let json = line.to_json();
        let back = LedgerLine::from_json(&json)
            .unwrap_or_else(|| panic!("{} did not parse back", json["kind"]));
        assert_eq!(back, line);
    }
}

#[test]
fn the_kind_and_the_id_are_readable_without_matching_the_variant() {
    let kinds: Vec<String> = all_kinds()
        .iter()
        .map(|l| l.to_json()["kind"].as_str().unwrap_or("?").to_string())
        .collect();
    assert_eq!(
        kinds,
        vec!["emit", "held", "expired", "ack", "redelivered", "refused"]
    );
    assert_eq!(all_kinds()[0].id(), Some(MSG_ID));
    // A refusal can predate knowing which message it was about.
    assert_eq!(all_kinds()[5].id(), None);
    assert_eq!(all_kinds()[5].ts_utc(), "2026-06-07T05:00:10Z");
}

#[test]
fn an_unknown_kind_or_a_missing_stamp_does_not_parse() {
    let mut v = emit(1, 1, Vehicle::Exit2).to_json();
    v["kind"] = serde_json::json!("teleported");
    assert!(LedgerLine::from_json(&v).is_none());

    let mut v = emit(1, 1, Vehicle::Exit2).to_json();
    v.as_object_mut().unwrap().remove("ts_utc");
    assert!(LedgerLine::from_json(&v).is_none());

    let mut v = emit(1, 1, Vehicle::Exit2).to_json();
    v["vehicle"] = serde_json::json!("smoke-signal");
    assert!(LedgerLine::from_json(&v).is_none());
}

#[test]
fn a_ledger_file_round_trips_and_counts_what_it_cannot_read() {
    let fx = Fixture::new();
    for line in all_kinds() {
        append_ledger(&fx.root, AGENT, &line).unwrap();
    }
    append_line(
        &ledger_path(&fx.root, AGENT).unwrap(),
        "{\"kind\":\"emit\"}",
    )
    .unwrap();
    let (lines, skipped) = read_ledger(&fx.root, AGENT).unwrap();
    assert_eq!(lines, all_kinds());
    assert_eq!(skipped, 1);
}

#[test]
fn the_fold_dedupes_by_id_and_a_repeated_part_counts_once() {
    let lines = vec![
        emit(1, 2, Vehicle::AdditionalContext),
        emit(1, 2, Vehicle::AdditionalContext),
        emit(2, 2, Vehicle::AdditionalContext),
    ];
    let states = states(&lines);
    assert_eq!(states.len(), 1);
    let st = &states[MSG_ID];
    assert_eq!(st.emitted_parts.len(), 2);
    assert_eq!(st.parts_expected, Some(2));
    assert!(st.first_part_emitted());
    assert_eq!(st.first_ts_utc.as_deref(), Some("2026-06-07T05:00:05Z"));
}

#[test]
fn a_hold_an_expiry_and_an_ack_land_on_the_state_the_verdict_reads() {
    let mut lines: Vec<LedgerLine> = Vec::new();
    // Nothing at all: the message is not in the ledger, so it has no state.
    assert!(states(&lines).is_empty());

    lines.push(LedgerLine::Held {
        id: MSG_ID.to_string(),
        reason: "block-cap".to_string(),
        ts_utc: ts("2026-06-07T05:00:05Z"),
    });
    let held = &states(&lines)[MSG_ID];
    assert_eq!(held.held_reasons, vec!["block-cap".to_string()]);
    assert!(!held.expired && !held.acked);

    // An emit after a hold does not erase it: the reason stays on the state and the
    // verdict ladder is what decides that the hold is over.
    lines.push(emit(1, 1, Vehicle::AdditionalContext));
    assert!(!states(&lines)[MSG_ID].held_reasons.is_empty());

    lines.push(LedgerLine::Expired {
        id: MSG_ID.to_string(),
        ts_utc: ts("2026-06-07T05:00:07Z"),
    });
    assert!(states(&lines)[MSG_ID].expired);

    lines.push(LedgerLine::Ack {
        id: MSG_ID.to_string(),
        ts_utc: ts("2026-06-07T05:00:08Z"),
    });
    let acked = &states(&lines)[MSG_ID];
    assert!(acked.acked);
    assert_eq!(acked.last_ts_utc.as_deref(), Some("2026-06-07T05:00:08Z"));
}

#[test]
fn a_redelivery_is_recorded_with_its_source_and_does_not_reopen_the_state() {
    let lines = vec![
        emit(1, 1, Vehicle::AdditionalContext),
        LedgerLine::Redelivered {
            id: MSG_ID.to_string(),
            source: RedeliverSource::Compact,
            ts_utc: ts("2026-06-07T05:00:09Z"),
        },
    ];
    let st = &states(&lines)[MSG_ID];
    assert_eq!(st.redelivered, vec![RedeliverSource::Compact]);
    assert!(st.first_part_emitted());
    assert_eq!(
        RedeliverSource::parse("resume"),
        Some(RedeliverSource::Resume)
    );
    assert_eq!(RedeliverSource::parse("startup"), None);
}

#[test]
fn a_line_with_no_id_is_not_folded_into_any_message_state() {
    let lines = vec![
        emit(1, 1, Vehicle::AdditionalContext),
        LedgerLine::Refused {
            id: None,
            reason: "served call".to_string(),
            ts_utc: ts("2026-06-07T05:00:10Z"),
        },
    ];
    let states = states(&lines);
    assert_eq!(states.len(), 1);
    assert!(states[MSG_ID].refused_reasons.is_empty());
}

#[test]
fn the_consecutive_block_count_stops_at_another_vehicle_or_at_an_ack() {
    assert_eq!(consecutive_exit2_blocks(&[]), 0);

    let mut lines = vec![
        emit(1, 1, Vehicle::Exit2),
        emit(1, 1, Vehicle::Exit2),
        emit(1, 1, Vehicle::Exit2),
    ];
    assert_eq!(consecutive_exit2_blocks(&lines), 3);
    assert!(states(&lines)[MSG_ID].has_exit2());
    assert_eq!(states(&lines)[MSG_ID].exit2_emits, 3);

    // A hold in between says nothing about blocking, so the run continues through it.
    lines.push(LedgerLine::Held {
        id: MSG_ID.to_string(),
        reason: "block-cap".to_string(),
        ts_utc: ts("2026-06-07T05:00:06Z"),
    });
    assert_eq!(consecutive_exit2_blocks(&lines), 3);

    // An additionalContext emit ends the run.
    lines.push(emit(1, 1, Vehicle::AdditionalContext));
    assert_eq!(consecutive_exit2_blocks(&lines), 0);

    // And so does an ack, even with exit2 emits after the reset.
    lines.push(emit(1, 1, Vehicle::Exit2));
    assert_eq!(consecutive_exit2_blocks(&lines), 1);
    lines.push(LedgerLine::Ack {
        id: MSG_ID.to_string(),
        ts_utc: ts("2026-06-07T05:00:08Z"),
    });
    assert_eq!(consecutive_exit2_blocks(&lines), 0);
}

#[test]
fn the_block_count_rides_the_exit2_emit_line_itself() {
    let line = emit(1, 1, Vehicle::Exit2);
    let LedgerLine::Emit { block_count, .. } = &line else {
        panic!("emit");
    };
    assert_eq!(*block_count, Some(1));
    let back = LedgerLine::from_json(&line.to_json()).unwrap();
    assert_eq!(back, line);
    // The additionalContext vehicle never blocks, so it carries no count.
    let plain = emit(1, 1, Vehicle::AdditionalContext);
    let LedgerLine::Emit { block_count, .. } = &plain else {
        panic!("emit");
    };
    assert_eq!(*block_count, None);
}
