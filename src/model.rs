//! JSON-line record + content-block data model.
//!
//! One JSON object per line. The shape below was verified + extended against real
//! `~/.claude/projects/**/*.jsonl` data (2026-06-07). Parsing discipline:
//!
//! - **Tolerate unknown fields.** Real records carry far more than the brief
//!   listed (`attachment`, `file-history-snapshot`, `queue-operation`, `isMeta`,
//!   `isSidechain`, `userType`, `toolUseResult`, `slug`, `entrypoint`, …). We
//!   deserialize only what we use and ignore the rest — never crash on a new field.
//! - **Tolerate missing `timestamp`.** Metadata-only records (`last-prompt`,
//!   `ai-title`, `permission-mode`, `file-history-snapshot`) have no timestamp;
//!   they are skipped in time logic, never panic.
//! - **`message.content` is string OR array.** Older / genuine-user text is a bare
//!   string; everything else is an array of typed blocks.
//!
//! ## Genuine-user vs tool-result-carrier (load-bearing)
//!
//! A `type:"user"` record is NOT always a human turn. In one real session: 332
//! genuine string-content users + 61 text-block users vs **1619** tool_result
//! carriers. A genuine user turn is: string content (and NOT `isCompactSummary`),
//! or content whose blocks are text (no `tool_result`). See [`Record::is_genuine_user`].
//!
//! ## Compaction
//!
//! A compaction summary is a `type:"user"` record with `isCompactSummary: true`
//! and `isVisibleInTranscriptOnly: true`, carrying string content — it must be
//! excluded from "genuine user". A separate `type:"system"`
//! `subtype:"compact_boundary"` record carries the metrics
//! (`trigger`, `preTokens`, `postTokens`, `durationMs`).

use serde::Deserialize;

/// The synthesized prefixes Claude Code writes into the `tool_result` answering an
/// `AskUserQuestion` (§4.4). CC has shipped (at least) TWO phrasings for the same
/// synthesized answer — verified across real `~/.claude/projects` data:
///
/// - `"User has answered your questions: \"<q>\"=\"<a>\". …"`
/// - `"Your questions have been answered: \"<q>\"=\"<a>\". …"`  (the dominant form
///   in current data; a single hardcoded marker missed it entirely)
///
/// Some sessions span a version transition and contain BOTH forms, so an AUQ
/// answer must be recognised if it carries EITHER prefix. Used to surface AUQ
/// answers under the `user` category without re-parsing `toolUseResult`.
pub const AUQ_ANSWER_MARKERS: &[&str] = &[
    "User has answered your questions",
    "Your questions have been answered",
];

/// True when `text` (a `tool_result`'s rendered content) is a synthesized
/// AskUserQuestion answer — i.e. it contains any known AUQ-answer marker (§4.4).
#[must_use]
pub fn is_auq_answer_text(text: &str) -> bool {
    AUQ_ANSWER_MARKERS.iter().any(|m| text.contains(m))
}

/// A single parsed jsonl line. Unknown top-level fields are ignored by serde.
///
/// Several fields below are deserialized for completeness of the documented record
/// model (SPEC §3.2) and to keep parsing tolerant, but are not (yet) read by any
/// handler — e.g. `parent_uuid` (the §6.4 round-trip reconstruction keys on file
/// order + genuine-user delimiting, not the uuid tree), `is_sidechain`,
/// `is_visible_in_transcript_only`, `subtype`, `content`. They are part of the
/// data contract, intentionally retained, hence the targeted allow rather than
/// deleting SPEC-mandated shape.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Record {
    /// Record discriminator: "user", "assistant", "system", "summary",
    /// "last-prompt", "attachment", … (open set — keep as String, never enum-panic).
    #[serde(default)]
    pub r#type: Option<String>,

    #[serde(default)]
    pub uuid: Option<String>,

    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<String>,

    /// ISO8601 UTC, e.g. `2026-06-07T05:43:00.000Z`. Absent on metadata-only records.
    #[serde(default)]
    pub timestamp: Option<String>,

    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,

    #[serde(default)]
    pub cwd: Option<String>,

    /// Claude Code version string.
    #[serde(default)]
    pub version: Option<String>,

    #[serde(default, rename = "gitBranch")]
    pub git_branch: Option<String>,

    #[serde(default, rename = "isSidechain")]
    pub is_sidechain: Option<bool>,

    /// Compaction summary marker — when true, this user record is NOT a human turn.
    #[serde(default, rename = "isCompactSummary")]
    pub is_compact_summary: Option<bool>,

    /// System-injected pseudo-turn marker (§4.2). `true` ⇒ a `type:"user"` record
    /// whose string/text content LOOKS human ("Continue from where you left off.",
    /// loop ticks, stop-hook feedback, `<local-command-caveat>…`) but is machine-
    /// generated — must be excluded from genuine-user and from turn-delimiting.
    #[serde(default, rename = "isMeta")]
    pub is_meta: Option<bool>,

    /// Co-set with `isCompactSummary` on compaction-summary records (§4.7).
    #[serde(default, rename = "isVisibleInTranscriptOnly")]
    pub is_visible_in_transcript_only: Option<bool>,

    /// `system` record subtype: stop_hook_summary | turn_duration | away_summary
    /// | compact_boundary | …
    #[serde(default)]
    pub subtype: Option<String>,

    /// `system` record inline content (e.g. away_summary text).
    #[serde(default)]
    pub content: Option<serde_json::Value>,

    /// The role-bearing message payload (present on user/assistant records).
    #[serde(default)]
    pub message: Option<Message>,

    /// Structured echo on tool-result carriers (§4.6). Kept raw; we read
    /// `persistedOutputPath`/`persistedOutputSize` from it for `--resolve-persisted`.
    #[serde(default, rename = "toolUseResult")]
    pub tool_use_result: Option<serde_json::Value>,
}

/// The `message` object on user / assistant records.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub role: Option<String>,

    /// Either a bare string (genuine text) or an array of typed blocks.
    #[serde(default)]
    pub content: Option<Content>,
}

/// `message.content` is polymorphic: a plain string or a list of blocks.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Bare-string content (older format / genuine user text / compaction summary).
    Text(String),
    /// Array of typed content blocks.
    Blocks(Vec<Block>),
}

/// A typed content block within `message.content`. `#[serde(other)]` Unknown
/// catches any future block type without a parse error.
///
/// Some block fields are deserialized for the documented model (SPEC §3.5) but not
/// read by current logic (`signature`, the `ToolUse.id`, the `ToolResult`
/// `tool_use_id`/`is_error`, `Image.source`). The round-trip reconstruction returns
/// the whole turn, so `tool_use`↔`tool_result` pairing-by-id is not needed (both sit
/// in the same emitted exchange); the fields stay for shape-completeness + future use.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        #[serde(default)]
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        input: Option<serde_json::Value>,
    },
    ToolResult {
        #[serde(default, rename = "tool_use_id")]
        tool_use_id: Option<String>,
        /// String OR array of {type:text,text}/{type:image} — keep raw.
        #[serde(default)]
        content: Option<serde_json::Value>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    Image {
        #[serde(default)]
        source: Option<serde_json::Value>,
    },
    /// Any block type not modeled above — never a parse failure.
    #[serde(other)]
    Unknown,
}

impl Record {
    /// True when this record is `type == "<t>"`.
    #[must_use]
    pub fn is_type(&self, t: &str) -> bool {
        self.r#type.as_deref() == Some(t)
    }

    /// True when this record is a GENUINE human turn (or a user answer to
    /// AskUserQuestion), NOT a `tool_result`-carrier and NOT a compaction summary.
    ///
    /// Rule (§4.1, ALL must hold):
    /// - must be `type:"user"` with `message.role == "user"`;
    /// - `isCompactSummary` must be falsey (excludes compaction summaries, §4.7);
    /// - `isMeta` must be falsey (excludes system-injected pseudo-turns, §4.2);
    /// - string content => genuine; block content => genuine iff it contains a
    ///   text block and NO `tool_result` block.
    #[must_use]
    pub fn is_genuine_user(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        if self.is_compact_summary.unwrap_or(false) {
            return false;
        }
        // §4.2 TRAP: isMeta:true user records look human but are system-injected.
        if self.is_meta.unwrap_or(false) {
            return false;
        }
        let Some(msg) = &self.message else {
            return false;
        };
        if msg.role.as_deref() != Some("user") {
            return false;
        }
        match &msg.content {
            Some(Content::Text(_)) => true,
            Some(Content::Blocks(blocks)) => {
                let has_tool_result = blocks.iter().any(|b| matches!(b, Block::ToolResult { .. }));
                let has_text = blocks.iter().any(|b| matches!(b, Block::Text { .. }));
                has_text && !has_tool_result
            }
            None => false,
        }
    }

    /// Plain-text rendering of a GENUINE user message for the `list`/`search`
    /// excerpt: the raw string, or the concatenation of all `text` blocks. Returns
    /// `None` for non-user / non-genuine records. Whitespace-normalized to a single
    /// line (callers truncate explicitly — never silently).
    #[must_use]
    pub fn genuine_user_text(&self) -> Option<String> {
        if !self.is_genuine_user() {
            return None;
        }
        let content = self.message.as_ref()?.content.as_ref()?;
        Some(flatten_content_text(content))
    }

    /// The content blocks of this record's message, if it has an array body.
    /// Returns `None` for string content or no message.
    #[must_use]
    pub fn blocks(&self) -> Option<&[Block]> {
        match self.message.as_ref()?.content.as_ref()? {
            Content::Blocks(blocks) => Some(blocks),
            Content::Text(_) => None,
        }
    }

    /// True when this is an AUQ-answer carrier: a `type:"user"` record carrying a
    /// `tool_result` block whose textual content is a synthesized AUQ-answer string
    /// (any known marker, §4.4 — both `"User has answered your questions: …"` and
    /// `"Your questions have been answered: …"`). Such a record is surfaced under the
    /// `user` category even though it rides on a carrier.
    #[must_use]
    pub fn is_auq_answer(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        let Some(blocks) = self.blocks() else {
            return false;
        };
        blocks.iter().any(|b| match b {
            Block::ToolResult { content, .. } => content
                .as_ref()
                .map(tool_result_content_text)
                .is_some_and(|t| is_auq_answer_text(&t)),
            _ => false,
        })
    }

    /// The persisted-output file path for this carrier (§4.6), preferring the
    /// structured `toolUseResult.persistedOutputPath` (exact — no regex) and falling
    /// back to scraping the inline `Full output saved to: <path>` marker from a
    /// `tool_result` block. Returns `None` when there is no persisted pointer.
    #[must_use]
    pub fn persisted_output_path(&self) -> Option<String> {
        // Structured field first (SPEC §4.6 resolution rule).
        if let Some(tur) = &self.tool_use_result {
            if let Some(p) = tur
                .get("persistedOutputPath")
                .and_then(serde_json::Value::as_str)
            {
                if !p.is_empty() {
                    return Some(p.to_string());
                }
            }
        }
        // Inline fallback: scan tool_result content for the marker.
        let blocks = self.blocks()?;
        for b in blocks {
            if let Block::ToolResult {
                content: Some(c), ..
            } = b
            {
                let text = tool_result_content_text(c);
                if let Some(p) = scrape_persisted_path(&text) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Plain-text rendering of the assistant's VISIBLE end-of-turn message — the
    /// concatenation of its `text` blocks (`thinking`/`tool_use` excluded). Returns
    /// `None` unless this is an `assistant` record carrying at least one non-empty
    /// `text` block. This is the "last agent message" target for `list`.
    #[must_use]
    pub fn agent_text(&self) -> Option<String> {
        if !self.is_type("assistant") {
            return None;
        }
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Blocks(blocks) = content else {
            // Assistant content is always a block array in CC 2.1.x; a bare string
            // would be a genuine surprise — surface it rather than silently drop.
            if let Content::Text(s) = content {
                let t = normalize_line(s);
                return if t.is_empty() { None } else { Some(t) };
            }
            return None;
        };
        let mut parts: Vec<&str> = Vec::new();
        for b in blocks {
            if let Block::Text { text } = b {
                if !text.trim().is_empty() {
                    parts.push(text);
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(normalize_line(&parts.join(" ")))
    }
}

/// Flatten a `Content` to a single normalized line of its textual parts.
/// `string` → itself; `blocks` → all `text` blocks joined (other block types,
/// which never co-occur with a genuine user `text` block, are ignored).
fn flatten_content_text(content: &Content) -> String {
    match content {
        Content::Text(s) => normalize_line(s),
        Content::Blocks(blocks) => {
            let joined = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            normalize_line(&joined)
        }
    }
}

/// Extract the textual payload of a `tool_result` block's `content` (§4.5). The
/// content is raw `serde_json::Value`: a bare string, OR an array of
/// `{type:"text",text}` / `{type:"image"}` / `{type:"tool_reference",tool_name}`
/// objects. We concatenate every `text` field found and, for `tool_reference`,
/// surface the `tool_name` (so a regex like `ToolSearch` still matches). Anything
/// else (images, unknown shapes) contributes nothing. Whitespace is NOT normalized
/// here — callers that excerpt do their own normalization; matchers want the raw
/// text. Returns an owned `String` (possibly empty).
#[must_use]
pub fn tool_result_content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => {
            let mut parts: Vec<String> = Vec::new();
            for item in items {
                if let Some(t) = item.get("text").and_then(serde_json::Value::as_str) {
                    parts.push(t.to_string());
                } else if let Some(name) = item.get("tool_name").and_then(serde_json::Value::as_str)
                {
                    parts.push(name.to_string());
                }
            }
            parts.join("\n")
        }
        // Object/number/bool/null: render compactly so a regex can still match
        // structured payloads that aren't the common string/array shapes.
        other => other.to_string(),
    }
}

/// Scrape the inline persisted-output pointer (§4.6 fallback): the line
/// `Full output saved to: <ABSOLUTE_PATH>` inside a `<persisted-output>` block.
/// Returns the trimmed path, or `None` if the marker is absent.
fn scrape_persisted_path(text: &str) -> Option<String> {
    const MARKER: &str = "Full output saved to:";
    let idx = text.find(MARKER)?;
    let rest = &text[idx + MARKER.len()..];
    // The path runs to end-of-line.
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let path = rest[..line_end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Collapse all runs of ASCII whitespace (incl. newlines/tabs) to single spaces
/// and trim the ends, so an excerpt renders on one line. Does NOT truncate —
/// length capping with an explicit `… (+N chars)` marker is the caller's job.
pub(crate) fn normalize_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    // Trim a possible trailing space from the run-collapse above.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Record {
        serde_json::from_str(line).expect("valid record")
    }

    #[test]
    fn genuine_user_string_content() {
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"hello"}}"#);
        assert!(r.is_genuine_user());
    }

    #[test]
    fn tool_result_carrier_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn compaction_summary_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"summary..."}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn unknown_top_level_fields_are_ignored() {
        // Real records carry attachment/isMeta/slug/etc. — must not break parsing.
        let r = parse(
            r#"{"type":"assistant","attachment":{"x":1},"isMeta":true,"slug":"s","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        );
        assert!(r.is_type("assistant"));
    }

    #[test]
    fn unknown_block_type_does_not_fail() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"future_block","data":1}]}}"#,
        );
        assert!(r.is_type("assistant"));
    }

    #[test]
    fn metadata_record_without_timestamp_parses() {
        let r = parse(r#"{"type":"last-prompt","leafUuid":"abc","sessionId":"s"}"#);
        assert!(r.timestamp.is_none());
        assert!(r.is_type("last-prompt"));
    }

    #[test]
    fn is_meta_user_is_not_genuine_user() {
        // §4.2 TRAP: looks human, is system-injected — must be excluded.
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(r.genuine_user_text().is_none());
    }

    #[test]
    fn is_meta_local_command_caveat_excluded() {
        // The real-data shape: <local-command-caveat> on an isMeta user record.
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: ...</local-command-caveat>"}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn genuine_user_text_string_content() {
        let r =
            parse(r#"{"type":"user","message":{"role":"user","content":"  hello\n  world  "}}"#);
        assert_eq!(r.genuine_user_text().as_deref(), Some("hello world"));
    }

    #[test]
    fn genuine_user_text_joins_text_blocks() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}}"#,
        );
        assert_eq!(r.genuine_user_text().as_deref(), Some("line one line two"));
    }

    #[test]
    fn agent_text_extracts_visible_text_only() {
        // thinking + tool_use are NOT the visible message; only `text` counts.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"Done — built."}]}}"#,
        );
        assert_eq!(r.agent_text().as_deref(), Some("Done — built."));
    }

    #[test]
    fn agent_text_none_when_no_visible_text() {
        // A pure tool-call assistant turn has no end-of-turn message.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#,
        );
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_none_for_user_record() {
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn auq_answer_carrier_detected() {
        // Real shape: a user-carrier whose tool_result content is the synthesized
        // "User has answered your questions: …" string (§4.4).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Q\"=\"A\". You can now continue."}]}}"#,
        );
        assert!(r.is_auq_answer());
        // It is NOT a genuine user (it's a carrier) but IS an AUQ answer.
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn auq_answer_alternate_phrasing_detected() {
        // The DOMINANT real-data phrasing the single hardcoded marker used to miss:
        // "Your questions have been answered: …" (verified across real sessions —
        // 16 sessions use this form vs 13 the other, 4 contain BOTH). Must be
        // recognised under the `user` category exactly like the other phrasing.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Your questions have been answered: \"Q\"=\"A\". You can now continue with these answers in mind."}]}}"#,
        );
        assert!(r.is_auq_answer(), "alternate AUQ phrasing must be detected");
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_auq_answer_text_recognises_both_phrasings() {
        assert!(is_auq_answer_text(
            "User has answered your questions: \"q\"=\"a\"."
        ));
        assert!(is_auq_answer_text(
            "Your questions have been answered: \"q\"=\"a\"."
        ));
        assert!(!is_auq_answer_text("a normal tool output"));
    }

    #[test]
    fn plain_tool_result_is_not_auq_answer() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"just a normal tool output"}]}}"#,
        );
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn persisted_output_path_structured_field_preferred() {
        let r = parse(
            r#"{"type":"user","toolUseResult":{"persistedOutputPath":"/tmp/x/tool-results/abc.txt","persistedOutputSize":123},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (1 KB). Full output saved to: /wrong/inline/path.txt\n</persisted-output>"}]}}"#,
        );
        // Structured field wins over the inline marker.
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/x/tool-results/abc.txt")
        );
    }

    #[test]
    fn persisted_output_path_inline_fallback() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (200 KB). Full output saved to: /tmp/sess/tool-results/b070yh2rb.txt\n\nPreview (first 2KB):\n…\n</persisted-output>"}]}}"#,
        );
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/sess/tool-results/b070yh2rb.txt")
        );
    }

    #[test]
    fn persisted_output_path_none_when_absent() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"a normal small tool output"}]}}"#,
        );
        assert!(r.persisted_output_path().is_none());
    }

    #[test]
    fn scrape_persisted_path_handles_marker() {
        assert_eq!(
            scrape_persisted_path("Full output saved to: /a/b/c.txt\nmore"),
            Some("/a/b/c.txt".to_string())
        );
        assert!(scrape_persisted_path("no marker here").is_none());
    }

    #[test]
    fn tool_result_content_text_string_and_array() {
        let s = serde_json::json!("hello world");
        assert_eq!(tool_result_content_text(&s), "hello world");
        let arr = serde_json::json!([
            {"type":"text","text":"first"},
            {"type":"image","source":{}},
            {"type":"text","text":"second"},
            {"type":"tool_reference","tool_name":"WebSearch"}
        ]);
        assert_eq!(tool_result_content_text(&arr), "first\nsecond\nWebSearch");
    }

    // ── Branch-completeness: the negative / fallback arms ──

    #[test]
    fn is_genuine_user_false_for_non_user_type() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_genuine_user_false_when_message_absent() {
        // A `type:"user"` record with NO `message` object at all (the `let Some(msg)
        // else` arm).
        let r = parse(r#"{"type":"user","uuid":"x"}"#);
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_genuine_user_false_when_role_not_user() {
        // role mismatch inside an otherwise user-typed record.
        let r = parse(r#"{"type":"user","message":{"role":"assistant","content":"hi"}}"#);
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_genuine_user_false_when_content_absent() {
        // message present, role user, but NO content (the `None => false` arm).
        let r = parse(r#"{"type":"user","message":{"role":"user"}}"#);
        assert!(!r.is_genuine_user());
        assert!(r.genuine_user_text().is_none());
    }

    #[test]
    fn is_genuine_user_false_for_blocks_without_text() {
        // Block content that has NO text block (only an image) → not genuine.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn blocks_none_for_string_content() {
        // `blocks()` returns None when content is a bare string (Content::Text arm).
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"plain"}}"#);
        assert!(r.blocks().is_none());
    }

    #[test]
    fn blocks_none_when_no_message() {
        let r = parse(r#"{"type":"system","subtype":"compact_boundary"}"#);
        assert!(r.blocks().is_none());
    }

    #[test]
    fn is_auq_answer_false_for_non_user() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"x"}]}}"#,
        );
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn is_auq_answer_false_when_no_blocks() {
        // user record with string content → no blocks → not an AUQ answer.
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn persisted_output_path_empty_structured_falls_through_to_inline() {
        // An empty structured persistedOutputPath must NOT win — the inline marker
        // is used instead (the `!p.is_empty()` false arm).
        let r = parse(
            r#"{"type":"user","toolUseResult":{"persistedOutputPath":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nFull output saved to: /tmp/real/inline.txt\n</persisted-output>"}]}}"#,
        );
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/real/inline.txt")
        );
    }

    #[test]
    fn persisted_output_path_none_when_no_blocks() {
        // user record with string content → blocks() is None → `?` short-circuits.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"no persisted pointer here"}}"#,
        );
        assert!(r.persisted_output_path().is_none());
    }

    #[test]
    fn persisted_output_path_structured_present_but_missing_key_falls_to_inline() {
        // `toolUseResult` exists but lacks `persistedOutputPath` (the
        // `tur.get(...)` None arm) → fall through to the inline marker scan.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"somethingElse":1},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Full output saved to: /tmp/fallback.txt\n"}]}}"#,
        );
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/fallback.txt")
        );
    }

    #[test]
    fn persisted_output_path_skips_non_tool_result_blocks_in_inline_scan() {
        // The inline fallback loop must skip a non-ToolResult block (the `if let
        // Block::ToolResult` FALSE arm) and find the marker in a later tool_result.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"not a tool result"},{"type":"tool_result","tool_use_id":"x","content":"Full output saved to: /tmp/later.txt\n"}]}}"#,
        );
        assert_eq!(r.persisted_output_path().as_deref(), Some("/tmp/later.txt"));
    }

    #[test]
    fn is_auq_answer_skips_non_tool_result_blocks() {
        // is_auq_answer's `.any()` must return false for a block that is NOT a
        // tool_result (the match's `_ => false` arm) when no AUQ marker is present.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"plain user text, no AUQ marker"}]}}"#,
        );
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn agent_text_none_for_non_assistant() {
        let r = parse(r#"{"type":"system","subtype":"away_summary"}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_handles_bare_string_assistant_content() {
        // CC normally sends assistant content as a block array; a bare-string body is
        // a surprise we surface rather than drop (the Content::Text fallback arm).
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":"  surprise bare string  "}}"#,
        );
        assert_eq!(r.agent_text().as_deref(), Some("surprise bare string"));
    }

    #[test]
    fn agent_text_none_for_empty_bare_string() {
        // A bare-string assistant body that is all whitespace normalizes to empty → None.
        let r = parse(r#"{"type":"assistant","message":{"role":"assistant","content":"   "}}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_none_when_no_content() {
        let r = parse(r#"{"type":"assistant","message":{"role":"assistant"}}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_skips_blank_text_blocks() {
        // A text block that is all whitespace is skipped; only the real one counts.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real"}]}}"#,
        );
        assert_eq!(r.agent_text().as_deref(), Some("real"));
    }

    #[test]
    fn flatten_blocks_ignores_non_text_blocks() {
        // genuine_user_text over blocks where a non-text block is interleaved: the
        // tool_use/image are filtered out, only text survives. (Exercised via a
        // genuine-user record whose blocks mix text with an image.)
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"keep me"},{"type":"image","source":{}}]}}"#,
        );
        assert_eq!(r.genuine_user_text().as_deref(), Some("keep me"));
    }

    #[test]
    fn tool_result_content_text_object_fallback() {
        // A non-string/non-array content (e.g. an object) renders compactly so a
        // regex can still match structured payloads (the `other => to_string` arm).
        let obj = serde_json::json!({"k": "v", "n": 1});
        let out = tool_result_content_text(&obj);
        assert!(out.contains("\"k\""), "compact object render: {out}");
        // A bare number/bool/null also goes through the same arm.
        assert_eq!(tool_result_content_text(&serde_json::json!(42)), "42");
        assert_eq!(tool_result_content_text(&serde_json::json!(null)), "null");
    }

    #[test]
    fn scrape_persisted_path_empty_after_marker_is_none() {
        // The marker is present but the path is blank → None (the `path.is_empty()`
        // true arm).
        assert!(scrape_persisted_path("Full output saved to:   \nnext").is_none());
        // Marker with the path running to EOF (no trailing newline) → the `unwrap_or`
        // line_end == rest.len() branch.
        assert_eq!(
            scrape_persisted_path("Full output saved to: /a/b.txt"),
            Some("/a/b.txt".to_string())
        );
    }

    #[test]
    fn normalize_line_collapses_and_trims() {
        // Leading/trailing/internal whitespace runs collapse to single spaces; the
        // trailing-space pop loop runs.
        assert_eq!(normalize_line("  a\t\tb \n c   "), "a b c");
        assert_eq!(normalize_line(""), "");
        assert_eq!(normalize_line("   "), "");
    }
}
