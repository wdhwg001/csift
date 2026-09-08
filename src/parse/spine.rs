//! The conversation-chain SPINE: the structural fields of a line the record
//! prefilters drop.
//!
//! Claude Code reconstructs a resumed conversation by walking `parentUuid` from one
//! leaf, and that walk threads through `attachment` and `system` records - a prompt
//! submitted after a SessionStart hook is parented to the hook's attachment record,
//! not to the assistant message before it. csift's §7d candidate prefilter keeps only
//! role-bearing lines, so a chain built from those alone breaks at the first
//! attachment and resolves nothing (measured: 1 of 30,613 conversation records
//! reachable on a real session).
//!
//! So the non-candidate arm of a full scan lifts the five fields the walk reads -
//! `type`, `uuid`, `parentUuid`, `timestamp`, `isSidechain` - plus `subtype`,
//! `logicalParentUuid` and `compactMetadata` for the compaction boundary, into a
//! [`Record`] carrying NOTHING else. The walk is depth-1 over the top-level object and
//! its cost is the line's LENGTH: a record's `type` is written after its payload on 87%
//! of lines and its `uuid` and `timestamp` on 98% (claim REC-104), so there is no early
//! exit to take. What the walk does not pay for is the payload itself - it is skipped,
//! never parsed and never allocated, and a snapshot attachment routinely embeds a whole
//! file.
//!
//! The walk is factored apart from the [`Record`] build for that reason: `spine_fields`
//! borrows the raw value spans out of the line and decodes nothing, so a caller that
//! already has its own reason to look at the line (a line-type census, a candidate parse)
//! could lift the chain fields out of the same pass instead of walking it twice. It is
//! private to this module today; widening it is the shared seam those callers need.

use super::*;

/// The four record types Claude Code's loader admits into its uuid map, plus the
/// `last-prompt` metadata line whose `leafUuid` names the leaf that walk starts from.
/// A spine row for any other line is pointless, so the walk stops the moment it reads a
/// `type` outside this set. The argument is the RAW value span, quotes included, so no
/// value is decoded before it is known to be wanted.
fn spine_type_wanted(raw: &[u8]) -> bool {
    matches!(
        raw,
        br#""user""#
            | br#""assistant""#
            | br#""attachment""#
            | br#""system""#
            | br#""last-prompt""#
    )
}

/// Lift one raw jsonl line's chain-structural fields into an otherwise EMPTY
/// [`Record`]. `None` when the line is blank, is not a `{…}` object, is malformed, or
/// carries a `type` outside [`spine_type_wanted`].
///
/// The returned record has no `message`, so it classifies to nothing, opens no turn
/// and emits no hit - callers mark it with `Kept::spine` and skip it in every
/// record-consuming pass. It exists only so the chain walk can see the DAG.
pub(crate) fn spine_record(line: &[u8]) -> Option<Record> {
    let f = spine_fields(line)?;
    // The [`Record`] is built ONCE, at the end. It is a wide struct and the walk touches
    // three lines in every four of a real transcript, so filling it field by field inside
    // the loop paid for a default-zeroed struct even on the lines the walk then rejects.
    Some(Record {
        r#type: str_value(f.r#type),
        subtype: f.subtype.and_then(str_value),
        uuid: f.uuid.and_then(str_value),
        parent_uuid: f.parent_uuid.and_then(str_value),
        logical_parent_uuid: f.logical_parent_uuid.and_then(str_value),
        leaf_uuid: f.leaf_uuid.and_then(str_value),
        timestamp: f.timestamp.and_then(str_value),
        is_sidechain: f.is_sidechain.and_then(bool_value),
        explicit: f.explicit.and_then(bool_value),
        rewound: f.rewound.and_then(bool_value),
        compact_metadata: f
            .compact_metadata
            .and_then(|raw| serde_json::from_slice(raw).ok()),
        ..Record::default()
    })
}

/// The RAW value spans of the keys the chain reads, borrowed straight out of the line.
/// Nothing is decoded or allocated here, so a line the walk ends up rejecting costs its
/// bytes and nothing else.
#[derive(Default)]
struct SpineFields<'a> {
    r#type: &'a [u8],
    subtype: Option<&'a [u8]>,
    uuid: Option<&'a [u8]>,
    parent_uuid: Option<&'a [u8]>,
    logical_parent_uuid: Option<&'a [u8]>,
    leaf_uuid: Option<&'a [u8]>,
    timestamp: Option<&'a [u8]>,
    is_sidechain: Option<&'a [u8]>,
    explicit: Option<&'a [u8]>,
    rewound: Option<&'a [u8]>,
    compact_metadata: Option<&'a [u8]>,
}

/// The depth-1 key walk itself. `type` is NOT an early exit for the OTHER keys: measured
/// over a real corpus, `parentUuid` is the first key of every one of 673,710 lines of an
/// admitted type while `uuid` and `timestamp` sit AFTER the payload key on 662,470 of them
/// (claim REC-104) - so the walk has to reach the end of the object either way, and a
/// "stop once the wanted keys are in hand" shortcut would stop on almost no line.
fn spine_fields(line: &[u8]) -> Option<SpineFields<'_>> {
    let payload = line_payload(line)?;
    let mut i = skip_ws(payload, 0);
    if payload.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    let mut out = SpineFields::default();
    loop {
        i = skip_ws(payload, i);
        match payload.get(i) {
            Some(b'}') => break,
            Some(b',') => {
                i += 1;
                continue;
            }
            Some(b'"') => {}
            _ => return None,
        }
        let (key, after_key) = read_string_span(payload, i)?;
        i = skip_ws(payload, after_key);
        if payload.get(i) != Some(&b':') {
            return None;
        }
        i = skip_ws(payload, i + 1);
        let start = i;
        i = skip_value(payload, i)?;
        let raw = &payload[start..i];
        match key {
            b"type" => {
                if !spine_type_wanted(raw) {
                    return None;
                }
                out.r#type = raw;
            }
            b"subtype" => out.subtype = Some(raw),
            b"uuid" => out.uuid = Some(raw),
            b"parentUuid" => out.parent_uuid = Some(raw),
            b"logicalParentUuid" => out.logical_parent_uuid = Some(raw),
            b"leafUuid" => out.leaf_uuid = Some(raw),
            b"timestamp" => out.timestamp = Some(raw),
            b"isSidechain" => out.is_sidechain = Some(raw),
            b"explicit" => out.explicit = Some(raw),
            b"rewound" => out.rewound = Some(raw),
            b"compactMetadata" => out.compact_metadata = Some(raw),
            _ => {}
        }
    }
    (!out.r#type.is_empty()).then_some(out)
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while matches!(b.get(i), Some(c) if c.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

/// The span of a JSON string starting at `i` (which must be `"`), returning its RAW
/// inner bytes (escapes untouched - object keys in this format never carry one) and
/// the offset just past the closing quote.
fn read_string_span(b: &[u8], i: usize) -> Option<(&[u8], usize)> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Some((&b[i + 1..j], j + 1)),
            _ => j += 1,
        }
    }
    None
}

/// The offset just past the JSON value starting at `i`. Objects and arrays are skipped
/// by depth with string awareness; a scalar runs to the next `,`/`}`/`]` at depth 0.
///
/// Almost every byte of a record's payload sits INSIDE a string (a message text, a
/// base64 image, a whole file in a snapshot attachment), and inside a string only two
/// bytes matter - so the walk jumps to the next `\` or `"` with `memchr2` instead of
/// stepping. That is the difference between reading a 200 MB transcript's attachment
/// lines at scan speed and reading them a byte at a time.
fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    let mut depth = 0usize;
    let mut in_str = false;
    while j < b.len() {
        if in_str {
            // An unterminated string is a torn line: the caller counts it as malformed.
            // (A bounded inline run before the SIMD hop was measured and is NOT here: it
            // cost 4% more CPU on a real 687 MB transcript than handing every string to
            // `memchr2`, short ones included.)
            j += memchr::memchr2(b'\\', b'"', &b[j..])?;
            if b[j] == b'\\' {
                j += 2;
                continue;
            }
            in_str = false;
            j += 1;
            if depth == 0 {
                return Some(j);
            }
            continue;
        }
        let c = b[j];
        match c {
            b'"' => {
                in_str = true;
                j += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                j += 1;
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Some(j);
                }
                depth -= 1;
                j += 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            b',' if depth == 0 => return Some(j),
            _ => j += 1,
        }
    }
    (depth == 0 && !in_str).then_some(j)
}

/// A JSON string value's decoded content (`None` for `null` or any other shape). A uuid,
/// a type name and an ISO timestamp carry no escape, so the common case copies the bytes
/// straight out and only an escaped value pays for a decoder.
fn str_value(raw: &[u8]) -> Option<String> {
    if raw.first() != Some(&b'"') || raw.len() < 2 || raw.last() != Some(&b'"') {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    if !inner.contains(&b'\\') {
        return std::str::from_utf8(inner).ok().map(str::to_string);
    }
    serde_json::from_slice(raw).ok()
}

/// A JSON boolean value (`None` for any other shape).
fn bool_value(raw: &[u8]) -> Option<bool> {
    match raw {
        b"true" => Some(true),
        b"false" => Some(false),
        _ => None,
    }
}
