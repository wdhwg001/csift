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

/// A single parsed jsonl line. Unknown top-level fields are ignored by serde.
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
    /// Rule (Phase 2 will exercise this against fixtures):
    /// - must be `type:"user"` with `message.role == "user"`;
    /// - `isCompactSummary` must be falsey;
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
}
