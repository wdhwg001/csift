//! The delivery decisions: which events carry which mode, which chunk one slot takes, and
//! how much room the turn-blocking vehicle has left.

use super::*;

/// A hook payload for one event. The transcript path is synthetic: nothing in these tests
/// reads it, because the plan is built from the channel directory alone.
fn hook(event: &str) -> HookInput {
    HookInput {
        session_id: SESSION.to_string(),
        transcript_path: "/Users/dev/relay/session.jsonl".to_string(),
        cwd: Some("/Users/dev/relay".to_string()),
        agent_id: Some(AGENT.to_string()),
        agent_type: Some("probe".to_string()),
        hook_event_name: event.to_string(),
        source: None,
        stop_hook_active: false,
        agent_transcript_path: None,
    }
}

fn session_start(source: &str) -> HookInput {
    HookInput {
        source: Some(source.to_string()),
        ..hook("SessionStart")
    }
}

const OTHER_MSG_ID: &str = "fedcba9876543210";
const NOW: &str = "2026-06-07T06:00:00Z";

/// Write a message source and enqueue it for `AGENT`.
fn enqueue(fx: &Fixture, id: &str, mode: Mode, body: &str, expires: Option<&str>) {
    let mut msg = message(body);
    msg.id = id.to_string();
    msg.mode = mode;
    write_message(&fx.root, &msg).unwrap();
    append_inbox(
        &fx.root,
        AGENT,
        &InboxLine {
            id: id.to_string(),
            enqueued_utc: "2026-06-07T05:00:05Z".to_string(),
            mode,
            expires_utc: expires.map(std::string::ToString::to_string),
        },
    )
    .unwrap();
}

fn plan_at(fx: &Fixture, hook: &HookInput, ledger: &[LedgerLine]) -> Plan {
    plan(&fx.root, AGENT, hook, ledger, NOW).unwrap()
}

// ---------------------------------------------------------------- the event filter

#[test]
fn the_eight_delivery_events_carry_a_steer_message_and_nothing_else_does() {
    for event in STEER_EVENTS {
        assert!(is_steer_event(event), "{event} is a delivery event");
    }
    for event in [
        "PreCompact",
        "PostCompact",
        "Notification",
        "TeammateIdle",
        "TaskCompleted",
        "SessionEnd",
        "",
    ] {
        assert!(!is_steer_event(event), "{event} is not a delivery event");
    }
}

#[test]
fn a_queue_message_rides_a_turn_boundary_or_a_session_reentry() {
    for event in ["UserPromptSubmit", "Stop", "SubagentStop"] {
        assert!(is_queue_event(event, None), "{event} ends a turn");
    }
    assert!(is_queue_event("SessionStart", Some("resume")));
    assert!(is_queue_event("SessionStart", Some("compact")));
    assert!(!is_queue_event("SessionStart", Some("startup")));
    assert!(!is_queue_event("SessionStart", Some("clear")));
    assert!(!is_queue_event("SessionStart", Some("fork")));
    assert!(!is_queue_event("SessionStart", None));
    for event in [
        "PreToolUse",
        "PostToolUse",
        "PostToolBatch",
        "SubagentStart",
    ] {
        assert!(!is_queue_event(event, None), "{event} is mid-turn");
    }
}

#[test]
fn a_queue_message_waits_for_the_turn_boundary_a_steer_message_does_not() {
    let fx = Fixture::new();
    enqueue(&fx, MSG_ID, Mode::Queue, "hold for the boundary", None);
    assert!(plan_at(&fx, &hook("PostToolUse"), &[]).chunks.is_empty());
    let at_stop = plan_at(&fx, &hook("Stop"), &[]);
    assert_eq!(at_stop.chunks.len(), 1);
    assert_eq!(at_stop.chunks[0].mode, Mode::Queue);

    let fx2 = Fixture::new();
    enqueue(&fx2, MSG_ID, Mode::Steer, "ride anything", None);
    assert_eq!(plan_at(&fx2, &hook("PostToolUse"), &[]).chunks.len(), 1);
}

// ------------------------------------------------------------- the chunk assignment

#[test]
fn several_small_messages_take_one_slot_each_in_inbox_order() {
    let fx = Fixture::new();
    enqueue(&fx, MSG_ID, Mode::Steer, "first", None);
    enqueue(&fx, OTHER_MSG_ID, Mode::Steer, "second", None);
    let firing = plan_at(&fx, &hook("PostToolUse"), &[]);
    assert_eq!(firing.chunks.len(), 2);
    assert_eq!(firing.chunks[0].id, MSG_ID);
    assert_eq!(firing.chunks[1].id, OTHER_MSG_ID);
    // Each keeps its own full header, so a slot never emits a headerless fragment.
    for chunk in &firing.chunks {
        assert_eq!(chunk.part, 1);
        assert_eq!(chunk.parts, 1);
        assert!(chunk.text.starts_with(CHANNEL_MARKER));
    }
}

#[test]
fn a_body_over_the_chunk_budget_splits_into_ordered_parts() {
    let fx = Fixture::new();
    enqueue(&fx, MSG_ID, Mode::Steer, &"beacon ".repeat(2000), None);
    let firing = plan_at(&fx, &hook("PostToolUse"), &[]);
    assert!(firing.chunks.len() > 1, "a long body needs several slots");
    let parts = u32::try_from(firing.chunks.len()).unwrap();
    for (i, chunk) in firing.chunks.iter().enumerate() {
        assert_eq!(chunk.part, u32::try_from(i + 1).unwrap());
        assert_eq!(chunk.parts, parts);
        assert!(chunk.text.chars().count() <= CHUNK_BUDGET);
    }
    let header = parse_header(&firing.chunks[0].text).unwrap();
    assert_eq!((header.part, header.parts), (1, parts));
}

#[test]
fn an_emitted_message_leaves_the_plan_but_a_compaction_offers_it_once_more() {
    let fx = Fixture::new();
    enqueue(&fx, MSG_ID, Mode::Steer, "already sent", None);
    let sent = [emit(1, 1, Vehicle::AdditionalContext)];
    assert!(plan_at(&fx, &hook("PostToolUse"), &sent).chunks.is_empty());

    let compact = plan_at(&fx, &session_start("compact"), &sent);
    assert_eq!(compact.chunks.len(), 1);
    assert_eq!(
        compact.bookkeeping,
        vec![Bookkeeping::Redelivered(
            MSG_ID.to_string(),
            RedeliverSource::Compact
        )]
    );

    // Once the redelivery is booked and no newer emit followed, the same compaction does
    // not offer it a second time.
    let booked = [
        emit(1, 1, Vehicle::AdditionalContext),
        LedgerLine::Redelivered {
            id: MSG_ID.to_string(),
            source: RedeliverSource::Compact,
            ts_utc: NOW.to_string(),
        },
    ];
    assert!(plan_at(&fx, &session_start("compact"), &booked)
        .chunks
        .is_empty());
}

#[test]
fn a_held_message_lands_at_the_next_reentry_and_an_acked_one_never_does() {
    let fx = Fixture::new();
    enqueue(&fx, MSG_ID, Mode::Queue, "held for a lane", None);
    let held = [LedgerLine::Held {
        id: MSG_ID.to_string(),
        reason: "no-lane".to_string(),
        ts_utc: "2026-06-07T05:30:00Z".to_string(),
    }];
    assert_eq!(
        plan_at(&fx, &session_start("resume"), &held).chunks.len(),
        1
    );

    let acked = [LedgerLine::Ack {
        id: MSG_ID.to_string(),
        ts_utc: NOW.to_string(),
    }];
    assert!(plan_at(&fx, &session_start("resume"), &acked)
        .chunks
        .is_empty());
}

#[test]
fn an_expired_message_is_booked_once_and_never_sent() {
    let fx = Fixture::new();
    enqueue(
        &fx,
        MSG_ID,
        Mode::Steer,
        "too late",
        Some("2026-06-07T05:30:00Z"),
    );
    let first = plan_at(&fx, &hook("PostToolUse"), &[]);
    assert!(first.chunks.is_empty());
    assert_eq!(
        first.bookkeeping,
        vec![Bookkeeping::Expired(MSG_ID.to_string())]
    );

    let booked = [LedgerLine::Expired {
        id: MSG_ID.to_string(),
        ts_utc: NOW.to_string(),
    }];
    let second = plan_at(&fx, &hook("PostToolUse"), &booked);
    assert!(second.chunks.is_empty());
    assert!(second.bookkeeping.is_empty());
}

#[test]
fn a_message_whose_source_file_is_gone_is_booked_once_as_a_hole() {
    let fx = Fixture::new();
    append_inbox(
        &fx.root,
        AGENT,
        &InboxLine {
            id: MSG_ID.to_string(),
            enqueued_utc: "2026-06-07T05:00:05Z".to_string(),
            mode: Mode::Steer,
            expires_utc: None,
        },
    )
    .unwrap();
    let first = plan_at(&fx, &hook("PostToolUse"), &[]);
    assert!(first.chunks.is_empty());
    assert_eq!(
        first.bookkeeping,
        vec![Bookkeeping::SourceMissing(MSG_ID.to_string())]
    );

    let booked = [LedgerLine::Held {
        id: MSG_ID.to_string(),
        reason: HELD_SOURCE_MISSING.to_string(),
        ts_utc: NOW.to_string(),
    }];
    assert!(plan_at(&fx, &hook("PostToolUse"), &booked)
        .bookkeeping
        .is_empty());
}

// ------------------------------------------------------------- the shared ledger view

#[test]
fn every_slot_of_one_firing_folds_the_ledger_prefix_the_head_recorded() {
    let chain_one = open_slot(std::process::id(), "UnitBaseline", AGENT, 1).unwrap();
    assert_eq!(fold_baseline(&chain_one, 128), 128);
    let chain_two = open_slot(std::process::id(), "UnitBaseline", AGENT, 2).unwrap();
    // Slot 2 reads the head's baseline even though the ledger has grown since.
    assert_eq!(fold_baseline(&chain_two, 512), 128);
    cleanup_if_last(&chain_two, 2);
}

#[test]
fn a_ledger_prefix_stops_at_the_baseline_and_counts_what_it_cannot_read() {
    let fx = Fixture::new();
    append_ledger(&fx.root, AGENT, &emit(1, 2, Vehicle::AdditionalContext)).unwrap();
    let after_first = ledger_len(&fx.root, AGENT).unwrap();
    append_ledger(&fx.root, AGENT, &emit(2, 2, Vehicle::Exit2)).unwrap();
    let full = ledger_len(&fx.root, AGENT).unwrap();
    assert!(full > after_first);

    let (prefix, skipped) = read_ledger_prefix(&fx.root, AGENT, after_first).unwrap();
    assert_eq!((prefix.len(), skipped), (1, 0));
    let (whole, skipped) = read_ledger_prefix(&fx.root, AGENT, full).unwrap();
    assert_eq!((whole.len(), skipped), (2, 0));

    // A line the schema cannot read is counted, never dropped in silence.
    append_line(
        &ledger_path(&fx.root, AGENT).unwrap(),
        "{\"kind\":\"nope\"}",
    )
    .unwrap();
    let grown = ledger_len(&fx.root, AGENT).unwrap();
    let (lines, skipped) = read_ledger_prefix(&fx.root, AGENT, grown).unwrap();
    assert_eq!((lines.len(), skipped), (2, 1));
}

// ------------------------------------------------------------ the block-cap arithmetic

#[test]
fn the_block_cap_defaults_to_the_harness_ceiling_and_reads_the_environment() {
    assert_eq!(block_cap(None), DEFAULT_BLOCK_CAP);
    assert_eq!(block_cap(Some("not a number")), DEFAULT_BLOCK_CAP);
    assert_eq!(block_cap(Some(" 3 ")), 3);
    assert_eq!(block_cap(Some("0")), 0);
    assert_eq!(block_cap(Some("-1")), -1);
}

#[test]
fn a_turn_blocking_emit_keeps_one_block_of_headroom_under_the_ceiling() {
    assert!(exit2_fits(8, 0, false, false));
    assert!(
        exit2_fits(8, 6, false, false),
        "the seventh block still fits"
    );
    assert!(!exit2_fits(8, 7, false, false), "the eighth would trip it");
    assert!(!exit2_fits(0, 0, false, false), "a zero cap disables it");
    assert!(!exit2_fits(-4, 0, false, false), "so does a negative one");
    assert!(
        !exit2_fits(8, 0, true, false),
        "never twice for one message"
    );
    assert!(
        !exit2_fits(8, 0, false, true),
        "a turn already continuing is told to stop blocking"
    );
}

#[test]
fn the_vehicle_is_additional_context_everywhere_but_a_stop_family_queue_message() {
    let chunk = Chunk {
        id: MSG_ID.to_string(),
        part: 1,
        parts: 1,
        mode: Mode::Queue,
        text: "body".to_string(),
        prior_exit2: false,
    };
    let steer = Chunk {
        mode: Mode::Steer,
        ..chunk.clone()
    };
    for (hook, chunk, want) in [
        (
            hook("PostToolUse"),
            chunk.clone(),
            Vehicle::AdditionalContext,
        ),
        (hook("Stop"), steer, Vehicle::AdditionalContext),
        (hook("Stop"), chunk.clone(), Vehicle::Exit2),
        (hook("SubagentStop"), chunk.clone(), Vehicle::Exit2),
    ] {
        assert_eq!(choose_vehicle(&hook, &chunk, &[], 8).vehicle, want);
    }

    let blocked = choose_vehicle(&hook("Stop"), &chunk, &[], 8);
    assert_eq!(blocked.block_count, Some(1));
    assert!(!blocked.held_block_cap);
}

#[test]
fn at_the_ceiling_the_chunk_falls_back_to_additional_context_and_says_so() {
    let chunk = Chunk {
        id: MSG_ID.to_string(),
        part: 1,
        parts: 1,
        mode: Mode::Queue,
        text: "body".to_string(),
        prior_exit2: false,
    };
    let choice = choose_vehicle(&hook("Stop"), &chunk, &[], 1);
    assert_eq!(choice.vehicle, Vehicle::AdditionalContext);
    assert_eq!(choice.block_count, None);
    assert!(choice.held_block_cap, "the downgrade is recorded");
}

#[test]
fn consecutive_blocks_in_the_lane_count_against_the_ceiling() {
    let chunk = Chunk {
        id: MSG_ID.to_string(),
        part: 1,
        parts: 1,
        mode: Mode::Queue,
        text: "body".to_string(),
        prior_exit2: false,
    };
    let two_blocks = [emit(1, 1, Vehicle::Exit2), emit(1, 1, Vehicle::Exit2)];
    assert_eq!(consecutive_exit2_blocks(&two_blocks), 2);
    assert_eq!(
        choose_vehicle(&hook("Stop"), &chunk, &two_blocks, 8)
            .block_count
            .unwrap(),
        3
    );
    assert!(
        choose_vehicle(&hook("Stop"), &chunk, &two_blocks, 3).held_block_cap,
        "two blocks already used leaves no room under a cap of three"
    );
}

// ------------------------------------------------------------------ the order warning

#[test]
fn a_slot_that_gave_up_waiting_emits_anyway_and_says_the_order_may_be_off() {
    let body = "[csift-channel v1 id=x part=1/1]\nbody";
    // The two outcomes that mean the chain held: nothing is added.
    for outcome in [WaitOutcome::First, WaitOutcome::Ready] {
        assert_eq!(with_order_warning(body, outcome, 2), body);
    }
    let warned = with_order_warning(body, WaitOutcome::TimedOut, 3);
    let mut lines = warned.lines();
    assert_eq!(
        lines.next().unwrap(),
        "[csift-channel warning: slot 3 emitted before slot 2; order may be disturbed]"
    );
    assert_eq!(
        lines.collect::<Vec<_>>().join("\n"),
        body,
        "the chunk itself is untouched - a timeout never costs the message"
    );
}

// -------------------------------------------------------------------- the hook output

#[test]
fn the_hook_output_is_one_object_naming_the_event_and_the_chunk() {
    let raw = hook_output("PostToolUse", "[csift-channel v1 id=x part=1/1]\nbody").unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert_eq!(
        v["hookSpecificOutput"]["additionalContext"],
        "[csift-channel v1 id=x part=1/1]\nbody"
    );
    assert_eq!(v.as_object().unwrap().len(), 1, "one key, nothing else");
}

// ------------------------------------------------------------------------- the recipe

#[test]
fn the_recipe_carries_one_chain_per_delivery_event() {
    let frag = fragment(3, RecipeShell::Bash);
    let hooks = frag["hooks"].as_object().unwrap();
    assert_eq!(hooks.len(), STEER_EVENTS.len());
    for event in STEER_EVENTS {
        let entries = hooks[event][0]["hooks"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry["type"], "command");
            assert_eq!(entry["command"], format!("csift deliver --slot {}", i + 1));
            assert!(entry.get("shell").is_none());
        }
    }
}

#[test]
fn the_powershell_recipe_names_its_runner() {
    let frag = fragment(1, RecipeShell::Powershell);
    let entry = &frag["hooks"]["Stop"][0]["hooks"][0];
    assert_eq!(entry["shell"], "powershell");
    assert_eq!(entry["command"], "csift deliver --slot 1");
}

// ------------------------------------------------------------------ the payload model

#[test]
fn a_payload_that_is_not_an_object_is_no_payload_at_all() {
    assert!(HookInput::parse("[]").is_none());
    assert!(HookInput::parse("\"Stop\"").is_none());
    assert!(HookInput::parse("not json").is_none());
    assert!(HookInput::parse("{}").is_some(), "an empty object parses");
}

#[test]
fn the_lane_is_the_agent_id_when_the_payload_names_one() {
    let with_agent = hook("Stop");
    assert_eq!(with_agent.lane(), AGENT);
    let top_level = HookInput {
        agent_id: None,
        ..hook("Stop")
    };
    assert_eq!(top_level.lane(), SESSION);
    // A blank `agent_id` is folded away at parse time, so the lane falls back to the
    // session rather than naming a file after whitespace.
    let blank = HookInput::parse(&format!(
        r#"{{"session_id":"{SESSION}","transcript_path":"/Users/dev/relay/s.jsonl","agent_id":"   ","hook_event_name":"Stop"}}"#
    ))
    .unwrap();
    assert_eq!(blank.agent_id, None);
    assert_eq!(blank.lane(), SESSION);
}

#[test]
fn a_teammate_lane_id_is_a_lane_like_any_other() {
    assert!(is_lane_id(TEAMMATE));
    assert!(is_lane_id(AGENT));
    assert!(is_lane_id(SESSION));
    assert!(!is_lane_id("served:cli"));
}
