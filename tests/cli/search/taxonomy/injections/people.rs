//! The records in this family that are a PERSON or another SESSION underneath: the
//! relayed human message and the two inbound relays.
//!
//! They share their leaves with records the harness wrote for itself, so every assertion
//! here is about telling those apart - which is exactly what a reader cannot do from the
//! leaf alone.

use super::*;

#[test]
fn the_two_inbound_relays_answer_for_their_own_origin() {
    let h = inject_home();
    // Both carry `agent.communication.inbox` and both answer `replies_to` empty, so a
    // leaf-keyed arm would give the peer the coordinator's reason - a false statement
    // about the record in front of the reader.
    let coord = h.run(&["search", "zzcoordinator", &at(SESS)]);
    assert!(coord.success, "stderr: {}", coord.stderr);
    assert!(
        coord.stdout.contains(
            "[for: replies_to unknown (a coordinator relay's origin carries only its kind)]"
        ),
        "the coordinator's honest empty names its own shape:\n{}",
        coord.stdout
    );
    // The direction arm reads the same field: every other inbound leaf renders an arrow.
    assert!(
        coord.stdout.contains("coordinator ⇨ self"),
        "and the relay renders its direction:\n{}",
        coord.stdout
    );

    let peer = h.run(&["search", "zzpeerbody", &at(SESS)]);
    assert!(peer.success, "stderr: {}", peer.stderr);
    assert!(
        peer.stdout.contains(
            "[for: replies_to unknown (a peer relay names the lane that sent it, not a message \
             it answers)]"
        ),
        "the peer's reason is its own:\n{}",
        peer.stdout
    );
    assert!(
        peer.stdout.contains("[peer lane task-zzlane]"),
        "and the sending lane rides beside it:\n{}",
        peer.stdout
    );
    let json = h.run(&["search", "zzpeerbody", &at(SESS), "--format", "json"]);
    let hit = first_hit(&json.stdout);
    assert_eq!(hit["peer_sender_task_id"], "task-zzlane", "{hit}");
    let edge = &hit["for"][0];
    assert!(edge["target"].is_null() && edge["basis"].is_null(), "{hit}");
    assert_eq!(
        edge["reason"], "a peer relay names the lane that sent it, not a message it answers",
        "{hit}"
    );
}

#[test]
fn the_relayed_person_is_marked_on_both_surfaces_and_opens_no_turn() {
    let h = inject_home();
    // origin.kind "human" is a FIELD, so the arm leads: the body is a person's own prose
    // and any template arm below could match whatever they happened to write.
    let relay = h.run(&["search", "zzrelay", &at(SESS), "-t", "user.message"]);
    assert!(relay.success, "stderr: {}", relay.stderr);
    // The harness DELIVERED it, so it carries the marker no other `user.message` does -
    // without it a relayed message is byte-identical on every surface to one the person
    // typed into this lane's own prompt box.
    assert!(
        relay.stdout.contains("user.message [relayed]"),
        "the relay is marked:\n{}",
        relay.stdout
    );
    // And the machine surface carries the same fact: a Marker is display-only, so the
    // JSON twin is what a consumer reading --format json can act on.
    let relay_json = h.run(&[
        "search",
        "zzrelay",
        &at(SESS),
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    let rows = json_rows(&relay_json.stdout, "exchange");
    assert_eq!(rows.len(), 1, "{}", relay_json.stdout);
    assert_eq!(rows[0]["hits"][0]["relayed"], true, "{}", rows[0]);
    // It is isMeta, so it does not OPEN a turn: it belongs to the turn the person's own
    // L1 prompt opened. The file's turn KEYS are the three records that DO open one - L1,
    // the quoting message at L22 and the PEER relay at L23, which is a delivered message
    // from another session and opens a turn exactly as a teammate message does.
    let turns = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    assert!(turns.success, "stderr: {}", turns.stderr);
    let keys: Vec<String> = json_rows(&turns.stdout, "census")
        .iter()
        .filter_map(|r| r["key"].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        keys,
        vec!["t0", "t1", "t2"],
        "three openers, three turns:\n{}",
        turns.stdout
    );
    // And the relay's OWN unit reports turn 0 - the assertion that actually moves if the
    // relay ever starts opening a turn of its own.
    assert_eq!(rows[0]["turn_index"], 0, "{}", rows[0]);
    // A person QUOTING a template keeps `user.message`: only an isMeta record can be an
    // injection, and the flag is the authorship mark the injecting call site stamps.
    let quote = h.run(&["search", "zzquote", &at(SESS), "-t", "user.message"]);
    assert!(
        quote.stdout.contains("user.message") && quote.stdout.contains("zzquote"),
        "the quoting person keeps their leaf:\n{}",
        quote.stdout
    );
}
