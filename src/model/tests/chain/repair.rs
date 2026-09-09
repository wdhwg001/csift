//! The two rescues the walk runs when plain ancestry is not enough: the 5 s parent repair
//! and the same-`message.id` group.

use super::*;

/// A record with `isSidechain` written explicitly - the repair keys on it, so a fixture
/// that leaves it off cannot separate the lanes.
fn lane(ty: &str, uuid: &str, parent: &str, side: bool, ts: &str) -> String {
    let p = if parent.is_empty() {
        "null".to_string()
    } else {
        format!("\"{parent}\"")
    };
    let msg = if ty == "user" {
        r#"{"role":"user","content":"prompt"}"#.to_string()
    } else {
        format!(
            r#"{{"role":"assistant","id":"m-{uuid}","content":[{{"type":"text","text":"reply"}}]}}"#
        )
    };
    format!(
        r#"{{"type":"{ty}","uuid":"{uuid}","parentUuid":{p},"isSidechain":{side},"timestamp":"{ts}","message":{msg}}}"#
    )
}

#[test]
fn the_parent_repair_takes_the_nearest_unvisited_record_of_the_same_lane() {
    // `a9` names a parent this file does not carry, so plain ancestry stops there and the
    // repair decides whether anything above it is still the conversation. It scans BACKWARDS
    // from `a9`'s own instant: the first record it meets is `a9` itself (already visited),
    // then a record 2 s earlier on the SIDECHAIN lane (wrong lane), then the prompt 3 s
    // earlier on this one. Only the last of those may be taken - stopping early, scanning
    // the wrong way, or dropping either test hands the walk a different parent or none.
    let r = recs(&[
        &lane("user", "u0", "", false, "2026-06-07T05:00:00.000Z"),
        &lane("assistant", "s1", "", true, "2026-06-07T05:00:01.000Z"),
        &lane(
            "assistant",
            "a9",
            "not-in-this-file",
            false,
            "2026-06-07T05:00:03.000Z",
        ),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(2), "the file tail is the leaf");
    let s = survivals(&c, r.len());
    assert_eq!(s[2], "live");
    assert_eq!(
        s[0], "live",
        "the repair carried the walk across a parent that names nothing, and it carried \
         it to the prompt rather than to the nearer record on the sidechain lane - taking \
         that one would end the walk one record short and leave the prompt pre-cut"
    );
}

#[test]
fn the_repair_window_includes_its_own_edge_and_stops_past_it() {
    // The window is 5 s. A candidate exactly 5.000 s earlier is inside it; one 6 s earlier
    // is not, and then nothing above the broken parent is claimed at all.
    let at_edge = recs(&[
        &lane("user", "u0", "", false, "2026-06-07T05:00:00.000Z"),
        &lane(
            "assistant",
            "a9",
            "not-in-this-file",
            false,
            "2026-06-07T05:00:05.000Z",
        ),
    ]);
    let c = Chain::build(&at_edge, None);
    assert_eq!(
        survivals(&c, at_edge.len()),
        vec!["live", "live"],
        "5.000 s is inside the window"
    );

    let past_edge = recs(&[
        &lane("user", "u0", "", false, "2026-06-07T05:00:00.000Z"),
        &lane(
            "assistant",
            "a9",
            "not-in-this-file",
            false,
            "2026-06-07T05:00:06.000Z",
        ),
    ]);
    let c = Chain::build(&past_edge, None);
    let s = survivals(&c, past_edge.len());
    assert_eq!(s[1], "live", "the tail is still the leaf");
    assert_eq!(
        s[0], "pre-cut",
        "6 s is outside it: the walk ends and csift claims nothing above the break"
    );
}

#[test]
fn a_tool_result_carrier_of_an_on_chain_assistant_is_rescued_by_its_message_id_group() {
    // One assistant message spans several lines sharing a `message.id`, and the tool_result
    // carriers parented to ANY of them belong to the same turn. `c0b` hangs off `a0`, which
    // the walk itself reached - so the group has to include the assistants the walk already
    // visited, not only the siblings the rescue just added.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#,
        r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        r#"{"type":"user","uuid":"c0b","parentUuid":"a0","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t9","content":"also ok"}]}}"#,
        &a("a1", "c0", "done", "2026-06-07T05:00:08.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        survivals(&c, r.len()),
        vec!["live"; 5],
        "the second carrier hangs off an assistant the walk reached"
    );
}
