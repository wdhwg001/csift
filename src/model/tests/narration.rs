//! The narration signature decoder: tag extraction, degradation, the LAST-field rule.
//!
//! Every fixture is HAND-EMITTED protobuf with neutral text (the binding fixture rule);
//! real transcripts are cited in comments only, never inlined.

use super::*;

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
    out
}

/// One length-delimited (wire 2) field.
fn field(num: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = varint((num << 3) | 2);
    out.extend(varint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn b64(bytes: &[u8]) -> String {
    const AL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(AL[(n >> 18) as usize & 63] as char);
        out.push(AL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            AL[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            AL[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The client's tag shape: an outer version varint (field 1, wire 0) then the nested
/// 2 -> 1 -> 8 tag path.
fn tagged_signature(tag: &str) -> String {
    let mut outer = vec![0x08, 0x02];
    outer.extend(field(2, &field(1, &field(8, tag.as_bytes()))));
    b64(&outer)
}

#[test]
fn narration_tag_decodes_and_classifies() {
    let n = tagged_signature("narration");
    assert_eq!(
        thinking_signature_tag(Some(&n)).as_deref(),
        Some("narration")
    );
    assert_eq!(
        thinking_block_class(Some(&n)),
        Class::AgentThinkingNarration
    );
    let t = tagged_signature("thinking");
    assert_eq!(
        thinking_signature_tag(Some(&t)).as_deref(),
        Some("thinking")
    );
    assert_eq!(thinking_block_class(Some(&t)), Class::AgentThinking);
    // The tag set is OPEN: an unknown value stays plain thinking, never an error.
    let f = tagged_signature("future-tag");
    assert_eq!(
        thinking_signature_tag(Some(&f)).as_deref(),
        Some("future-tag")
    );
    assert_eq!(thinking_block_class(Some(&f)), Class::AgentThinking);
}

#[test]
fn every_degradation_path_falls_to_plain_thinking() {
    // Design probe 6, cases (i)-(v) plus structural tears.
    let cases: Vec<Option<String>> = vec![
        None,
        Some(String::new()),
        Some("!!!not-base64!!!".to_string()),
        Some(b64(&[0xff; 16])),                // wire type 7 at the top level
        Some(b64(&field(2, &field(3, b"x")))), // field 2 present, no inner field 1
        Some(b64(&field(2, &field(1, &field(9, b"narration"))))), // no inner field 8
        Some(b64(&[0x80])),                    // truncated varint
        Some(b64(&[0x12, 0x64, 0x01, 0x02])),  // length says 100, 2 bytes present
        Some(b64(&field(2, &field(1, &field(8, &[0xff, 0xfe]))))), // non-UTF-8 tag
    ];
    for sig in &cases {
        assert_eq!(thinking_signature_tag(sig.as_deref()), None, "{sig:?}");
        assert_eq!(
            thinking_block_class(sig.as_deref()),
            Class::AgentThinking,
            "{sig:?}"
        );
    }
}

#[test]
fn last_field_wins_at_every_level() {
    // Real signatures never repeat the field (FIRST == LAST on 13,957 measured), so this
    // hand fixture is the ONLY pin of the client's LAST-field rule.
    let outer_repeat = {
        let mut b = field(2, &field(1, &field(8, b"thinking")));
        b.extend(field(2, &field(1, &field(8, b"narration"))));
        b64(&b)
    };
    assert_eq!(
        thinking_block_class(Some(&outer_repeat)),
        Class::AgentThinkingNarration
    );
    let inner_repeat = {
        let mut inner = field(8, b"narration");
        inner.extend(field(8, b"thinking"));
        b64(&field(2, &field(1, &inner)))
    };
    assert_eq!(
        thinking_block_class(Some(&inner_repeat)),
        Class::AgentThinking
    );
}

#[test]
fn foreign_fields_skip_and_unknown_wire_aborts() {
    // Varint, fixed64 and fixed32 fields interleave freely around the tag path.
    let mut b = vec![0x08, 0x04]; // field 1 varint (the v4 version byte)
    b.extend([0x19, 1, 2, 3, 4, 5, 6, 7, 8]); // field 3, wire 1 (8 bytes)
    b.extend([0x25, 9, 9, 9, 9]); // field 4, wire 5 (4 bytes)
    b.extend(field(2, &field(1, &field(8, b"narration"))));
    assert_eq!(
        thinking_block_class(Some(&b64(&b))),
        Class::AgentThinkingNarration
    );
    // An unknown wire type aborts the walk to untagged, even after a valid tag field.
    let mut torn = field(2, &field(1, &field(8, b"narration")));
    torn.push(0x0b); // field 1, wire 3 (group start - never emitted by the API)
    assert_eq!(
        thinking_block_class(Some(&b64(&torn))),
        Class::AgentThinking
    );
}

#[test]
fn whole_string_decode_survives_a_giant_leading_field() {
    // Real signatures reach 200K+ base64 chars with the tag at the END; a prefix decode
    // reports every long signature untagged (the inverted-census bug this pins against).
    let mut b = field(15, &vec![0x2a; 60_000]);
    b.extend(field(2, &field(1, &field(8, b"narration"))));
    let sig = b64(&b);
    assert!(sig.len() > 80_000);
    assert_eq!(
        thinking_block_class(Some(&sig)),
        Class::AgentThinkingNarration
    );
}

#[test]
fn classify_splits_thinking_records_by_signature_only() {
    // A narration record (empty text included) classifies by SIGNATURE, never adjacency.
    let sig = tagged_signature("narration");
    for text in ["placeholder narration summary", ""] {
        let rec: Record = serde_json::from_str(&format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"{text}","signature":"{sig}"}}]}}}}"#
        ))
        .unwrap();
        assert_eq!(
            rec.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentThinkingNarration]
        );
    }
    // A thinking-tagged and an untagged block both stay plain reasoning.
    let tsig = tagged_signature("thinking");
    for sig_json in [format!(r#","signature":"{tsig}""#), String::new()] {
        let rec: Record = serde_json::from_str(&format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"placeholder reasoning text"{sig_json}}}]}}}}"#
        ))
        .unwrap();
        assert_eq!(
            rec.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentThinking]
        );
    }
}

#[test]
fn the_byte_gate_is_sound_across_all_three_alignments() {
    // The hot-path gate skips the decode when no base64 alignment of the tag is
    // present; shifting the tag's stream offset by 0/1/2 bytes exercises every
    // alignment - a lost needle silently misclassifies one of these.
    for pad in 0..3usize {
        let mut outer = vec![0x08, 0x02];
        outer.extend(field(14, &vec![0x2a; 4 + pad]));
        outer.extend(field(2, &field(1, &field(8, b"narration"))));
        let sig = b64(&outer);
        assert_eq!(
            thinking_block_class(Some(&sig)),
            Class::AgentThinkingNarration,
            "pad {pad}"
        );
    }
    // Whitespace inside the base64 breaks needle adjacency; the gate must re-open and
    // the decoder (whitespace-tolerant) still finds the tag.
    let mut outer = vec![0x08, 0x02];
    outer.extend(field(2, &field(1, &field(8, b"narration"))));
    let sig = b64(&outer);
    let mid = sig.len() / 2;
    let spaced = format!("{} {}", &sig[..mid], &sig[mid..]);
    assert_eq!(
        thinking_block_class(Some(&spaced)),
        Class::AgentThinkingNarration
    );
}
