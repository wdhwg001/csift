//! The SESSION-LINEAGE fields of one line, read by a depth-1 key walk.
//!
//! The two lineage facts `list --lineage` answers over a WHOLE file are top-level string
//! fields: `sessionKind` (the background-lane stamp, present on every record a background
//! process writes) and `continuedInSessionId` (the handoff line's child id). Reading them
//! with a record parse would decode every payload on the way past - a megabyte attachment,
//! a base64 image, a whole file in a snapshot - to answer a question about one key.
//!
//! So this is the same shape [`super::spine_record`] takes for the chain: walk the object's
//! keys at depth 1, skip every value with the SIMD string-jumping [`super::skip_value`],
//! and decode only the two spans that matter. Two consequences are load-bearing.
//!
//! - A key NESTED inside a payload is not a field. A transcript that quotes the shape in
//!   prose (this repo's own dev sessions do) reads as carrying nothing, which is the same
//!   answer serde gives and the reason the walk is depth-1 rather than a byte search.
//! - There is no parse to FAIL, so the walk reports no malformed lines. A torn line simply
//!   carries no key, and `list`'s `skipped_lines` keeps its documented meaning - a census
//!   of the head/tail lines it read - instead of gaining a third, overlapping source that
//!   could book one line twice (the R12 rule).

use super::*;

/// The lineage fields of one line. `None` from [`lineage_fields`] means the line is not a
/// `{...}`-framed object at all; a parsed line with neither key yields both fields empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LineageFields {
    /// The top-level `sessionKind` value: `"bg"` on every record a background lane writes,
    /// absent from a foreground lane's records (the stamp is built from one env var whose
    /// value is undefined there, and the serializer drops an undefined property).
    pub(crate) session_kind: Option<String>,
    /// The top-level `continuedInSessionId` value: the child session id of a background
    /// handoff, on the parent-side `continued-in` line only.
    pub(crate) continued_in_session_id: Option<String>,
}

impl LineageFields {
    /// True when the line carries neither field - the common case, and the one a caller
    /// skips without allocating anything.
    pub(crate) fn is_empty(&self) -> bool {
        self.session_kind.is_none() && self.continued_in_session_id.is_none()
    }
}

/// Byte prefilter for a lineage sweep: a line carrying NEITHER key can be skipped before
/// the walk. Both needles are quoted KEYS, the R13 serialization-safe form - a reserialized
/// line keeps them. PROSE cannot trip either, and the reason is structural: a `"` inside a
/// JSON string is escaped, so a quoted key mentioned in a message body is the bytes
/// `\"sessionKind\"` and not the needle. What the needle does still admit is a NESTED
/// object's own key, which is unescaped in the raw bytes - and the depth-1 walk refuses it,
/// so that line costs a walk that finds nothing rather than a wrong answer.
pub(crate) fn line_has_lineage_key(line: &[u8]) -> bool {
    static KIND: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"sessionKind\""));
    static CHILD: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"continuedInSessionId\""));
    KIND.find(line).is_some() || CHILD.find(line).is_some()
}

/// The two lineage fields of one raw jsonl line, by a depth-1 key walk that never parses a
/// payload. `None` when the line is blank or is not a `{...}`-framed object.
pub(crate) fn lineage_fields(line: &[u8]) -> Option<LineageFields> {
    let payload = line_payload(line)?;
    let mut i = skip_ws(payload, 0);
    if payload.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    let mut out = LineageFields::default();
    loop {
        i = skip_ws(payload, i);
        match payload.get(i) {
            Some(b'}') => break,
            Some(b',') => {
                i += 1;
                continue;
            }
            Some(b'"') => {}
            // Anything else at a key position is a torn or foreign line: keep what the walk
            // already read rather than discarding it, the same tolerance the spine walk has
            // to a shape it cannot finish.
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
        // A scalar's span runs to the delimiter, so the trailing whitespace of `"bg" ,`
        // would defeat the byte-exact quote test in `str_value`.
        let raw = payload[start..i].trim_ascii_end();
        match key {
            b"sessionKind" => out.session_kind = str_value(raw),
            b"continuedInSessionId" => out.continued_in_session_id = str_value(raw),
            _ => {}
        }
    }
    Some(out)
}
