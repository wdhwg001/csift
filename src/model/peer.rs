//! Teammate / peer message parsing + spawn & SendMessage helpers.

use super::*;

/// A parsed inbound `<teammate-message …>` (GOLD §4/§5): the `teammate_id` attribute (the
/// comm FROM) and, when the body is a `{"type":"<sig>"}` JSON payload, the control-signal
/// type (e.g. `idle_notification`). A prose message has `signal_type == None`.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateMessage {
    /// The `teammate_id` attribute = the SENDER (the FROM of the comm direction).
    pub teammate_id: Option<String>,
    /// The control-signal type when the body is `{"type":"<sig>"}` (idle_notification /
    /// shutdown_request / teammate_terminated / shutdown_approved / …); `None` for prose.
    pub signal_type: Option<String>,
}

#[allow(dead_code)]
impl TeammateMessage {
    /// True when this is a control SIGNAL (a `{"type":…}` payload), not a prose message →
    /// classifies `agent.communication.signal` rather than `…inbox`.
    #[must_use]
    pub fn is_signal(&self) -> bool {
        self.signal_type.is_some()
    }
}

/// True when `content` carries a `open` section tag at a valid section BOUNDARY (FINDING-1) - the
/// non-allocating predicate behind [`is_teammate_message`] / [`is_agent_message`]. Returns on the
/// FIRST [`is_section_boundary`] occurrence; a tag that only ever appears MID-PROSE yields `false`.
/// The common no-tag case costs exactly one `memmem` (the `find` returns `None` immediately), so
/// the hot path (`is_genuine_user` on every user record) is not regressed.
pub(crate) fn has_boundary_section(content: &str, open: &str) -> bool {
    let mut idx = 0;
    while let Some(rel) = content[idx..].find(open) {
        let start = idx + rel;
        if is_section_boundary(&content[..start]) {
            return true;
        }
        idx = start + open.len();
    }
    false
}

/// True when `content` is an inbound teammate/peer message (GOLD §5) - it carries a
/// [`TEAMMATE_MESSAGE_OPEN`] tag at a section BOUNDARY (FINDING-1). Real data (edge-fixtures scout):
/// the real shape is ALWAYS the relayed wrapper `Another Claude session sent a message:\n
/// <teammate-message …>\n<BODY>\n</teammate-message>\n\n<security footer>` (126 of 126), so the
/// boundary is the content start, just after the relay preamble, or right after a prior section's
/// close tag. A tag merely QUOTED mid-prose (a genuine user message that mentions the literal tag -
/// common in csift's OWN dev sessions) is NOT a boundary, so the record stays `user.message` rather
/// than being mislabeled `agent.communication.inbox` (the FINDING-1 fix).
#[allow(dead_code)]
#[must_use]
pub fn is_teammate_message(content: &str) -> bool {
    has_boundary_section(content, TEAMMATE_MESSAGE_OPEN)
}

/// True when `content` is an inbound `<agent-message from="…">` peer message (P1c M1 / FINDING-2) at
/// a section BOUNDARY - the DISTINCT peer form from [`is_teammate_message`]. Like a teammate message
/// it classifies `agent.communication.inbox`, is excluded from [`Record::is_genuine_user`], yet
/// still opens a turn. Boundary-anchored (FINDING-1) for the same reason - a quoted tag is not it.
#[allow(dead_code)]
#[must_use]
pub fn is_agent_message(content: &str) -> bool {
    has_boundary_section(content, AGENT_MESSAGE_OPEN)
}

/// True when `content` is ANY inbound peer message - a `<teammate-message>` OR an `<agent-message>`
/// at a section boundary (GOLD §1 + P1c M1 + FINDING-2). Both are PEER-agent messages, never the
/// operator: excluded from [`Record::is_genuine_user`] yet still turn-opening ([`Record::opens_turn`]).
#[must_use]
pub fn is_peer_message(content: &str) -> bool {
    is_teammate_message(content) || is_agent_message(content)
}

/// Parse an inbound teammate/peer message (GOLD §5) into its `teammate_id` + optional signal
/// type. `None` when `content` is not a teammate message. CODEPOINT-SAFE: every slice is
/// taken on ASCII byte offsets returned by `str::find` (the tag/attribute delimiters), never
/// inside a (possibly CJK) message body.
#[allow(dead_code)]
#[must_use]
pub fn parse_teammate_message(content: &str) -> Option<TeammateMessage> {
    // The FIRST boundary-anchored `<teammate-message>` section (FINDING-1): reuses the
    // boundary-aware scan so a tag quoted mid-prose is never parsed as a teammate message.
    parse_all_teammate_messages(content).into_iter().next()
}

/// Parse ALL `<teammate-message …>` sections in `content` (edge-fixtures G4/G5 batching): one
/// `type:"user"` record can carry SEVERAL sections of MIXED kind (prose + idle_notification +
/// teammate_terminated + …). Returns one [`TeammateMessage`] per section, in file order - the
/// caller unions their labels. Each section's body/signal is scoped to its own close tag (so a
/// later section, the trailing security footer, and inter-section text never bleed in). Empty
/// when `content` has no tag. CODEPOINT-SAFE: ASCII-offset slicing only.
#[allow(dead_code)]
#[must_use]
pub fn parse_all_teammate_messages(content: &str) -> Vec<TeammateMessage> {
    let mut out = Vec::new();
    scan_tag_sections(
        content,
        TEAMMATE_MESSAGE_OPEN,
        TEAMMATE_MESSAGE_CLOSE,
        |_, section| {
            out.push(TeammateMessage {
                teammate_id: extract_xml_attr(section, "teammate_id"),
                signal_type: teammate_signal_type(section),
            });
        },
    );
    out
}

/// True when an open tag whose PREFIX (the text before it) is `prefix` sits at a valid SECTION
/// BOUNDARY (FINDING-1): the content start (only whitespace precedes), immediately after the
/// relayed peer preamble (one of [`PEER_MESSAGE_PREAMBLES`]), or right after a prior section's CLOSE tag
/// (`</task-notification>` / `</teammate-message>` / `</agent-message>`, modulo trailing
/// whitespace). A tag that appears MID-PROSE - a genuine user message merely QUOTING the literal
/// tag, common in csift's own dev sessions - is NOT a boundary, so it does not start a section and
/// the record stays `user.message`. Codepoint-safe: pure suffix tests on `trim_end`, no slicing.
pub(crate) fn is_section_boundary(prefix: &str) -> bool {
    let t = prefix.trim_end();
    t.is_empty()
        || PEER_MESSAGE_PREAMBLES.iter().any(|p| t.ends_with(p))
        || t.ends_with(TASK_NOTIFICATION_CLOSE)
        || t.ends_with(TEAMMATE_MESSAGE_CLOSE)
        || t.ends_with(AGENT_MESSAGE_CLOSE)
}

/// Invoke `emit(offset, section)` for each BOUNDARY-anchored `<open …>…</close>` section in
/// `content`, in file order. `offset` is the byte offset of the section's open tag; `section` is the
/// slice from the open tag through (inclusive) its close tag - or to end-of-string if the close tag
/// is absent (malformed). Only an open tag at an [`is_section_boundary`] starts a section
/// (FINDING-1); a tag quoted mid-prose is skipped (advance past it and keep scanning for a later
/// boundary-anchored one). The scan advances past each section's close tag (or, if absent, to end so
/// it always terminates). Shared by the teammate / agent-message / task-notification section scans so
/// they never drift. CODEPOINT-SAFE: ASCII-offset slicing only (`str::find` on the tags).
pub(crate) fn scan_tag_sections<F: FnMut(usize, &str)>(
    content: &str,
    open: &str,
    close: &str,
    mut emit: F,
) {
    let mut idx = 0;
    while let Some(rel) = content[idx..].find(open) {
        let start = idx + rel;
        if !is_section_boundary(&content[..start]) {
            // A tag QUOTED mid-prose - not a section start; step past it and keep scanning.
            idx = start + open.len();
            continue;
        }
        let after = &content[start..];
        let end_rel = after.find(close).map_or(after.len(), |c| c + close.len());
        emit(start, &after[..end_rel]);
        idx = start + end_rel;
    }
}

/// One inbound peer-message section located in a `type:"user"` record's text (GOLD §5 + P1c M1):
/// a `<teammate-message …>` OR the distinct `<agent-message from="…">` peer form. Carries the
/// SENDER id (the comm FROM), whether the body is a control SIGNAL (a teammate `{"type":…}`
/// payload - an `<agent-message>` is always prose → inbox), and the byte OFFSET of its open tag
/// (so a batched scan can MASK a peer tag quoted inside a `<task-notification>` span).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerSection {
    pub(crate) from: Option<String>,
    pub(crate) is_signal: bool,
    pub(crate) offset: usize,
    /// The raw section slice (open tag through close tag) - the per-section render body (GOLD
    /// §3 G4/G5). Excludes the relay preamble and the trailing security footer (both fall
    /// OUTSIDE the tag span), so a batched record renders each peer message on its own.
    pub(crate) text: String,
}

/// Scan ALL inbound peer-message sections (`<teammate-message>` AND `<agent-message>`) in
/// `content`, returned in file (offset) order (P1c M1 + GOLD §5 batching). Empty when `content`
/// has no peer tag. CODEPOINT-SAFE: ASCII-offset slicing only.
pub(crate) fn parse_all_peer_sections(content: &str) -> Vec<PeerSection> {
    let mut out: Vec<PeerSection> = Vec::new();
    scan_tag_sections(
        content,
        TEAMMATE_MESSAGE_OPEN,
        TEAMMATE_MESSAGE_CLOSE,
        |offset, section| {
            out.push(PeerSection {
                from: extract_xml_attr(section, "teammate_id"),
                is_signal: teammate_signal_type(section).is_some(),
                offset,
                text: section.to_string(),
            });
        },
    );
    scan_tag_sections(
        content,
        AGENT_MESSAGE_OPEN,
        AGENT_MESSAGE_CLOSE,
        |offset, section| {
            out.push(PeerSection {
                from: extract_xml_attr(section, "from"),
                is_signal: false,
                offset,
                text: section.to_string(),
            });
        },
    );
    out.sort_by_key(|p| p.offset);
    out
}

/// The inner BODY of a single peer-message section slice (`<teammate-message …>BODY</teammate-message>`
/// or `<agent-message …>BODY</agent-message>`): the prose between the open tag's `>` and the close
/// tag, trimmed - the wrapper tags stripped so a render shows only the peer's own words (the trailing
/// harness security footer already sits OUTSIDE the section slice). Falls back to the whole slice when
/// the tag bounds are absent (malformed). Codepoint-safe: ASCII-offset slicing only.
pub(crate) fn peer_section_body(section: &str) -> &str {
    let body = match section.find('>') {
        Some(i) => &section[i + 1..],
        None => section,
    };
    body.strip_suffix("</teammate-message>")
        .or_else(|| body.strip_suffix("</agent-message>"))
        .unwrap_or(body)
        .trim()
}

/// Extract a `name="value"` attribute's value from the start of an XML-ish tag, trimmed
/// (empty → `None`). Codepoint-safe: ASCII-offset `find` only.
pub(crate) fn extract_xml_attr(s: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = s.find(&needle)? + needle.len();
    let rel = s[start..].find('"')?;
    let val = s[start..start + rel].trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// The control-signal `type` of a `<teammate-message …>{"type":"<sig>"}</teammate-message>`
/// body (GOLD §5), or `None` for a prose body. Extracts the body between the opening tag's
/// `>` and `</teammate-message>`, and - only when it is a JSON object - reads its `type`.
pub(crate) fn teammate_signal_type(after_open: &str) -> Option<String> {
    let gt = after_open.find('>')?;
    let body_start = gt + 1;
    const CLOSE: &str = "</teammate-message>";
    let body_end = after_open[body_start..]
        .find(CLOSE)
        .map_or(after_open.len(), |rel| body_start + rel);
    let body = after_open[body_start..body_end].trim();
    if !body.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("type")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// True for a spawn tool (GOLD §5): `Task` / `Agent` / `Workflow` - a tool_use that spawns a
/// subagent (the `self ⇨ child` comm). Kept local so `model.rs` stays dependency-free.
pub(crate) fn is_spawn_tool_name(name: &str) -> bool {
    matches!(name, "Task" | "Agent" | "Workflow")
}

/// True when a `SendMessage` `input` is a control SIGNAL rather than a prose message (GOLD
/// §3): the top-level `type` (or a nested `message.type`) is present and is NOT `message`/
/// `direct` (e.g. `shutdown_request`/`shutdown_response`/…). Absent type ⇒ a plain message.
pub(crate) fn send_message_is_signal(input: Option<&serde_json::Value>) -> bool {
    let Some(input) = input else {
        return false;
    };
    let type_at = |v: &serde_json::Value| {
        v.get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    if let Some(t) = type_at(input) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    if let Some(t) = input.get("message").and_then(type_at) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    false
}

/// The recipient id of a `SendMessage` (the comm TO) - `input.to` preferred, else
/// `input.recipient`. `None` when neither is a non-empty string.
pub(crate) fn send_message_recipient(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?;
    for key in ["to", "recipient"] {
        if let Some(v) = input.get(key).and_then(serde_json::Value::as_str) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The spawn target NAME of a spawn tool_use (`input.name` for a teammate/named spawn, else
/// `input.subagent_type`) - used to resolve the spawned child id (the comm TO). `None` when
/// neither is present.
pub(crate) fn spawn_target_name(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?;
    for key in ["name", "subagent_type"] {
        if let Some(v) = input.get(key).and_then(serde_json::Value::as_str) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}
