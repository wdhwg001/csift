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
//! [`spine_fields`] is the shared seam, and it is `pub(crate)` for exactly that: it
//! borrows the raw value spans out of the line and decodes nothing, so a caller that
//! already has its own reason to look at the line (a line-type census, a candidate parse)
//! lifts the chain fields out of the same pass instead of walking it twice.
//! [`spine_record`] is just the [`Record`] build on top of it.
//!
//! [`line_type_and_spine`] is that seam's one additive entry: `stats` already
//! deserializes every non-candidate line in full for its exact line-type census, so the
//! chain fields ride out of THAT parse into the same [`SpineFields`] and through the same
//! [`spine_record_from`] build, rather than costing the line a second time.

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
    Some(spine_record_from(&spine_fields(line)?))
}

/// Decode one [`SpineFields`] into the [`Record`] the chain walks. Shared, so the walk
/// entry and the census entry cannot drift into decoding a field differently.
///
/// The [`Record`] is built ONCE, at the end. It is a wide struct and the walk touches
/// three lines in every four of a real transcript, so filling it field by field inside
/// the loop paid for a default-zeroed struct even on the lines the walk then rejects.
pub(crate) fn spine_record_from(f: &SpineFields<'_>) -> Record {
    Record {
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
    }
}

/// Reduce an ALREADY-PARSED [`Record`] to the same chain-structural row
/// [`spine_record`] lifts off the raw line: the chain fields cloned, everything else
/// dropped (no `message`, no `toolUseResult`, no attachment payload), so the result
/// classifies to nothing, opens no turn and emits no hit.
///
/// The caller is a scan that already paid for the full parse and then found the record
/// unsearchable under its gates. DELETING it there is what breaks the chain - the walk
/// threads `parentUuid` THROUGH `attachment` and `system` records, so a missing node
/// fails every record above it open - and keeping it whole would leak a gated leaf into
/// the output. Demoting it is the only answer that is right on both axes.
///
/// A unit test pins this field for field against [`spine_record`] on the same line, so
/// the two ways into a spine row cannot drift.
pub(crate) fn spine_from_record(rec: &Record) -> Record {
    Record {
        r#type: rec.r#type.clone(),
        subtype: rec.subtype.clone(),
        uuid: rec.uuid.clone(),
        parent_uuid: rec.parent_uuid.clone(),
        logical_parent_uuid: rec.logical_parent_uuid.clone(),
        leaf_uuid: rec.leaf_uuid.clone(),
        timestamp: rec.timestamp.clone(),
        is_sidechain: rec.is_sidechain,
        explicit: rec.explicit,
        rewound: rec.rewound,
        compact_metadata: rec.compact_metadata.clone(),
        ..Record::default()
    }
}

/// The RAW value spans of the keys the chain reads, borrowed straight out of the line.
/// Nothing is decoded or allocated here, so a line the walk ends up rejecting costs its
/// bytes and nothing else.
#[derive(Debug, Default)]
pub(crate) struct SpineFields<'a> {
    pub(crate) r#type: &'a [u8],
    pub(crate) subtype: Option<&'a [u8]>,
    pub(crate) uuid: Option<&'a [u8]>,
    pub(crate) parent_uuid: Option<&'a [u8]>,
    pub(crate) logical_parent_uuid: Option<&'a [u8]>,
    pub(crate) leaf_uuid: Option<&'a [u8]>,
    pub(crate) timestamp: Option<&'a [u8]>,
    pub(crate) is_sidechain: Option<&'a [u8]>,
    pub(crate) explicit: Option<&'a [u8]>,
    pub(crate) rewound: Option<&'a [u8]>,
    pub(crate) compact_metadata: Option<&'a [u8]>,
}

/// The depth-1 key walk itself. `type` is NOT an early exit for the OTHER keys: measured
/// over a real corpus, `parentUuid` is the first key of every one of 673,710 lines of an
/// admitted type while `uuid` and `timestamp` sit AFTER the payload key on 662,470 of them
/// (claim REC-104) - so the walk has to reach the end of the object either way, and a
/// "stop once the wanted keys are in hand" shortcut would stop on almost no line.
pub(crate) fn spine_fields(line: &[u8]) -> Option<SpineFields<'_>> {
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
        // A scalar's span runs to the delimiter, so `true ,` would carry its trailing
        // space into a byte-exact `bool_value` / `str_value` compare and read as ABSENT.
        // Trimmed here, once, because the serde entry gets whitespace-free spans and the
        // two must agree on `isSidechain` / `explicit` / `rewound` - the fields the leaf
        // choice steers on.
        let raw = payload[start..i].trim_ascii_end();
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

/// The line-type census and the chain spine out of ONE pass: full JSON syntax validation
/// plus the top-level `type` value, and - for a line type the loader admits - the same
/// [`SpineFields`] [`spine_fields`] borrows, decoded by the same [`spine_record_from`].
/// Blank -> `Ok(None)`; a typeless object -> `"(untyped)"`; a malformed line -> `Err`.
///
/// This is the exact-census entry, and `stats` is the named corruption authority: a
/// `{...}`-framed line with an invalid INTERIOR fails HERE, where the O(1) shape check and
/// the tolerant [`spine_fields`] walk both pass it. That validation is a whole pass over
/// the line, so the chain fields ride out of it rather than costing a second one: the
/// UNFUSED shape - a narrow type probe plus a separate [`spine_record`] walk - measured
/// about 40 ms of user CPU more on the largest transcript of a real corpus, 720 MB whose
/// 91,975 lines include 49,902 attachments (medians of 16 and of 20 `csift stats` runs per
/// arm, arms interleaved forwards and backwards each round, user CPU from `getrusage`
/// deltas, same-binary control drifting 0.3-0.4%).
///
/// The two entries therefore differ in ONE thing only, the walk that finds the spans -
/// `spine_fields` for a caller with no parse to ride on, serde for the caller that already
/// validates - and a unit test pins them field for field.
pub(crate) fn line_type_and_spine(
    line: &[u8],
) -> std::result::Result<Option<(String, Option<Record>)>, ()> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    // The same key set [`SpineFields`] holds, captured as borrowed `RawValue` spans so
    // serde validates the whole line without decoding or allocating any of them.
    #[derive(serde::Deserialize)]
    struct Probe<'a> {
        #[serde(rename = "type", borrow, default)]
        r#type: Option<&'a serde_json::value::RawValue>,
        #[serde(borrow, default)]
        subtype: Option<&'a serde_json::value::RawValue>,
        #[serde(borrow, default)]
        uuid: Option<&'a serde_json::value::RawValue>,
        #[serde(rename = "parentUuid", borrow, default)]
        parent_uuid: Option<&'a serde_json::value::RawValue>,
        #[serde(rename = "logicalParentUuid", borrow, default)]
        logical_parent_uuid: Option<&'a serde_json::value::RawValue>,
        #[serde(rename = "leafUuid", borrow, default)]
        leaf_uuid: Option<&'a serde_json::value::RawValue>,
        #[serde(borrow, default)]
        timestamp: Option<&'a serde_json::value::RawValue>,
        #[serde(rename = "isSidechain", borrow, default)]
        is_sidechain: Option<&'a serde_json::value::RawValue>,
        #[serde(borrow, default)]
        explicit: Option<&'a serde_json::value::RawValue>,
        #[serde(borrow, default)]
        rewound: Option<&'a serde_json::value::RawValue>,
        #[serde(rename = "compactMetadata", borrow, default)]
        compact_metadata: Option<&'a serde_json::value::RawValue>,
    }
    let p: Probe = serde_json::from_slice(line).map_err(|_| ())?;
    // `RawValue::get` is the value's RAW text, quotes included - exactly the span shape
    // `SpineFields` carries, so the type gate and the decode below are the walk's own.
    fn raw(v: Option<&serde_json::value::RawValue>) -> Option<&[u8]> {
        v.map(|r| r.get().as_bytes())
    }
    let census = raw(p.r#type)
        .and_then(str_value)
        .unwrap_or_else(|| "(untyped)".to_string());
    let Some(ty) = raw(p.r#type).filter(|t| spine_type_wanted(t)) else {
        return Ok(Some((census, None)));
    };
    let fields = SpineFields {
        r#type: ty,
        subtype: raw(p.subtype),
        uuid: raw(p.uuid),
        parent_uuid: raw(p.parent_uuid),
        logical_parent_uuid: raw(p.logical_parent_uuid),
        leaf_uuid: raw(p.leaf_uuid),
        timestamp: raw(p.timestamp),
        is_sidechain: raw(p.is_sidechain),
        explicit: raw(p.explicit),
        rewound: raw(p.rewound),
        compact_metadata: raw(p.compact_metadata),
    };
    Ok(Some((census, Some(spine_record_from(&fields)))))
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
