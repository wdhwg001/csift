//! Session/agent id shape predicates: uuids, prefixes, bare hex, teammate ids.

use super::*;

// ── Bare-uuid positional routing (so `csift files <uuid>` works as documented) ──

#[test]
fn is_uuid_recognizes_canonical_form() {
    assert!(is_uuid("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
    // Wrong group lengths, non-hex, missing dashes, real paths → not a uuid.
    assert!(!is_uuid("0a1b2c3d-4e5f-4a6b-8c7d"));
    assert!(!is_uuid("zzzzzzzz-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(!is_uuid("-Users-testuser-Projects-foo"));
    assert!(!is_uuid("/Users/testuser/Projects/foo"));
    assert!(!is_uuid("."));
}

#[test]
fn is_bare_subagent_hex_recognizes_agent_ids() {
    assert!(is_bare_subagent_hex("ae24045bd6d4bdaff"));
    assert!(is_bare_subagent_hex("a585e25a580c59e7a"));
    // Too short, dashed (uuid), or a word → not a bare subagent hex.
    assert!(!is_bare_subagent_hex("abc123")); // < 12
    assert!(!is_bare_subagent_hex(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
    assert!(!is_bare_subagent_hex("plain-token"));
}

#[test]
fn is_teammate_agent_id_recognizes_name_embedded_ids() {
    // The new `in_process_teammate` shape: a<Name>-<hex>. These are the canonical ids
    // `csift agents` prints for a teammate, and must round-trip as `@<id>` targets.
    assert!(is_teammate_agent_id("aVSRepro-68a2a1661c9390c1"));
    assert!(is_teammate_agent_id("aVSSpeedField-d5dab904cc98a239"));
    assert!(is_teammate_agent_id("aVSMultiRegion-06fb13dd400b53a5"));
    // A teammate NAME may itself carry dashes (real data: teammate "P1-engine") — the
    // head is dash-tolerant so the id `csift agents` prints still round-trips.
    assert!(is_teammate_agent_id("aP1-engine-9cf2f06d6235ca64"));
    // A bare hex (built-in/workflow) has no dash → NOT teammate-shaped (it routes via
    // is_bare_subagent_hex instead).
    assert!(!is_teammate_agent_id("ae24045bd6d4bdaff"));
    // A uuid is rejected: the explicit is_uuid guard (an `a`-led uuid would otherwise
    // pass the dash-tolerant head with its exactly-12-hex final segment).
    assert!(!is_teammate_agent_id(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
    assert!(!is_teammate_agent_id(
        "a93b39f8-1681-4535-88eb-5b8ecce0abcd"
    ));
    // An encoded project dir starts with `-` (leading-slash sanitisation) → head not `a`.
    assert!(!is_teammate_agent_id("-Users-testuser-Projects-foo"));
    // Hex tail too short, or a non-hex tail → rejected.
    assert!(!is_teammate_agent_id("aVSRepro-68a2a1")); // tail < 12
    assert!(!is_teammate_agent_id("aVSRepro-zzzzzzzzzzzz")); // non-hex tail
}

#[test]
fn is_subagent_id_accepts_both_bare_hex_and_teammate() {
    // The unified gate the @-grammar routes through.
    assert!(is_subagent_id("ae24045bd6d4bdaff")); // built-in/workflow bare hex
    assert!(is_subagent_id("aVSRepro-68a2a1661c9390c1")); // teammate
    assert!(is_subagent_id("aP1-engine-9cf2f06d6235ca64")); // teammate with dashed name
    assert!(!is_subagent_id("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")); // uuid
    assert!(!is_subagent_id("a93b39f8-1681-4535-88eb-5b8ecce0abcd")); // a-led uuid
    assert!(!is_subagent_id("-Users-testuser-Projects-foo")); // encoded dir
    assert!(!is_subagent_id("abc123")); // too short
}

#[test]
fn pins_single_session_covers_at_tokens_and_jsonl() {
    assert!(pins_single_session("@main"));
    assert!(pins_single_session("@trap:CrimsonWillowFen5180"));
    assert!(pins_single_session("@0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(pins_single_session("@ae24045bd6d4bdaff"));
    assert!(pins_single_session("@aVSRepro-68a2a1661c9390c1")); // teammate id pins one
    assert!(pins_single_session("@aP1-engine-9cf2f06d6235ca64")); // dashed-name teammate
    assert!(pins_single_session("@13d9645a")); // uuid-prefix
    assert!(pins_single_session("/a/b/0a1b2c3d.jsonl"));
    // A bare uuid (no `@`), an encoded token, a plain path, `.` → NOT a session pin.
    assert!(!pins_single_session("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(!pins_single_session("-Users-testuser-Projects-foo"));
    assert!(!pins_single_session("."));
}

#[test]
fn is_uuid_prefix_covers_first_segment_not_full_or_agent() {
    assert!(is_uuid_prefix("13d9")); // 4 hex (minimum)
    assert!(is_uuid_prefix("13d9645a")); // 8 hex (the uuid first segment)
    assert!(is_uuid_prefix("13d9645a3a5")); // 11 hex (max dash-less)
                                            // Too short (<4), dash-less ≥12 (agent-hex territory), non-hex, off-template dash → NOT a prefix.
    assert!(!is_uuid_prefix("13d")); // 3
    assert!(!is_uuid_prefix("13d9645a3a5b")); // 12 dash-less → agent hex
    assert!(!is_uuid_prefix("13d9645g")); // non-hex g
    assert!(!is_uuid_prefix("13d9-645a")); // dash off the 8-4-4-4-12 template
                                           // LITERAL layout prefixes (collision-lengthened header tokens) ARE prefixes.
    assert!(is_uuid_prefix("13d9645a-3a5")); // 12 chars, dash at template position 8
    assert!(is_uuid_prefix("13d9645a-3a5b-4a92")); // deeper into the layout
    assert!(is_uuid_prefix("13d9645a-3a5b-4a92-b83d-e0f94c5a9b9")); // 35 (max — one short of full)
    assert!(!is_uuid_prefix("13d9645a-3a5b-4a92-b83d-e0f94c5a9b90")); // 36 = a FULL uuid, not a prefix
    assert!(!is_uuid_prefix("13d9645a-3a5g")); // non-hex inside the layout
}
