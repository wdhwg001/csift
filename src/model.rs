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

    /// Top-level `attachment` payload (a sibling of `message`, not a content block).
    /// Real records carry attachments for hook output, `edited_text_file` external
    /// edits, `file` snapshots, etc. Kept RAW (like `tool_use_result`) and read only
    /// by `recover` (file-reconstruction); additive + tolerant, so no other subcommand
    /// changes behaviour.
    #[serde(default)]
    pub attachment: Option<serde_json::Value>,

    /// `file-history-snapshot` payload (a top-level sibling). Carries
    /// `{messageId, trackedFileBackups: {<path>: {backupFileName, version, backupTime}}}`.
    /// Read only by `recover` to know a disk backup EXISTED for a path at a time
    /// (a coverage annotation); the on-disk blob name is not derivable from it (the
    /// real `backupFileName` is frequently `null`), so it is never used to fabricate
    /// content. Additive + tolerant.
    #[serde(default)]
    pub snapshot: Option<serde_json::Value>,
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

/// A file-mutating operation kind, keyed off the tool that performed it. The
/// structured tools (`Write`/`Edit`/`MultiEdit`/`NotebookEdit`) are AUTHORITATIVE —
/// they name an exact `file_path`/`notebook_path`. `BashMutation` is HEURISTIC: it is
/// parsed lexically from a Bash command string (see [`crate::bash_mutations`]), which
/// cannot be a true shell parse, so it is labelled heuristic everywhere it surfaces.
///
/// Write/Edit/NotebookEdit/MultiEdit are kept DISTINCT (not collapsed to "mutation")
/// because the acid-test question — "how many files did it create vs edit" — needs
/// create-vs-edit discrimination, and the per-op counts are a stated output of
/// `csift files --by-file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    /// `Write` tool — writes a file whole (a create when the path was new).
    Write,
    /// `Edit` tool — a single in-place string replacement in an existing file.
    Edit,
    /// `NotebookEdit` tool — edits a Jupyter notebook cell (`notebook_path`).
    NotebookEdit,
    /// `MultiEdit` tool — multiple edits to one file in a single call.
    MultiEdit,
    /// A file mutation inferred HEURISTICALLY from a Bash command string.
    BashMutation,
}

impl FileOp {
    /// Stable lowercase label used in CLI output + JSON (mirrors `SubagentKind::label`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            FileOp::Write => "write",
            FileOp::Edit => "edit",
            FileOp::NotebookEdit => "notebook-edit",
            FileOp::MultiEdit => "multi-edit",
            FileOp::BashMutation => "bash",
        }
    }

    /// True only for [`FileOp::BashMutation`] — drives the explicit "heuristic"
    /// labelling in `files` output (Bash mutations are a best-effort lexical parse,
    /// never authoritative).
    #[must_use]
    pub fn is_heuristic(self) -> bool {
        matches!(self, FileOp::BashMutation)
    }
}

/// One extracted file-mutation fact, pure per-record (the turn index is assigned by
/// the `files` module during turn reconstruction, NOT stored here). The `path` is the
/// absolute path exactly as written in the record — never re-encoded or absolutized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMutation {
    /// The path as written in the record (NOT re-encoded / absolutized).
    pub path: String,
    pub op: FileOp,
    /// Raw ISO8601 UTC timestamp from the tool_use record, if present.
    pub timestamp_utc: Option<String>,
    /// `true` when the paired carrier reported `toolUseResult.type == "create"` (a new
    /// file). On a bare tool_use record the carrier field is usually absent, so this
    /// defaults `false` ("unknown / treat as edit"); the joiner in `files` enriches it.
    pub is_create: bool,
}

impl Record {
    /// The NON-HEURISTIC file mutations carried by this record's structured tool_use
    /// blocks (`Write`/`Edit`/`MultiEdit` → `input.file_path`; `NotebookEdit` →
    /// `input.notebook_path`). One [`FileMutation`] per qualifying block.
    ///
    /// MODELLING NOTE: in real data the `file_path` lives on the **tool_use** record
    /// while `toolUseResult.type` (`create`/`update`) lives on the **paired
    /// tool_result carrier**. This function extracts only what is locally present, so
    /// `is_create` here is consulted from THIS record's own `toolUseResult` first (it
    /// is usually absent on a tool_use record, defaulting `is_create` to `false` —
    /// honestly "unknown / treat as edit"); the `files` module (Section 3) joins the
    /// two sides by `tool_use_id` within a turn via [`Record::carrier_create_paths`]
    /// so `is_create` becomes accurate. Keeping this per-record-pure mirrors how
    /// `search` treats a turn as the join unit.
    ///
    /// Blocks whose path is absent/empty are skipped (a defensive arm, tested).
    #[must_use]
    pub fn structured_tool_mutations(&self) -> Vec<FileMutation> {
        let Some(blocks) = self.blocks() else {
            return Vec::new();
        };
        // This record's own carrier `type` (usually absent on a tool_use record).
        let self_is_create = self
            .tool_use_result
            .as_ref()
            .and_then(|tur| tur.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some("create");

        let mut out = Vec::new();
        for block in blocks {
            let Block::ToolUse { name, input, .. } = block else {
                continue;
            };
            let Some(name) = name.as_deref() else {
                continue;
            };
            let (op, key) = match name {
                "Write" => (FileOp::Write, "file_path"),
                "Edit" => (FileOp::Edit, "file_path"),
                "MultiEdit" => (FileOp::MultiEdit, "file_path"),
                "NotebookEdit" => (FileOp::NotebookEdit, "notebook_path"),
                _ => continue,
            };
            let path = input
                .as_ref()
                .and_then(|v| v.get(key))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if path.is_empty() {
                continue; // defensive: a structured tool_use with no/empty path.
            }
            out.push(FileMutation {
                path: path.to_string(),
                op,
                timestamp_utc: self.timestamp.clone(),
                is_create: self_is_create,
            });
        }
        out
    }

    /// The carrier side of a file mutation: when this record's `toolUseResult` is an
    /// object carrying a `filePath`, return `(tool_use_id, filePath, is_create)` for
    /// each `tool_result` block, so the `files` joiner can set `is_create` on the
    /// matching structured mutation (and fall back to this `filePath` if the
    /// tool_use's own path was somehow absent).
    ///
    /// `toolUseResult.type` ∈ {`create`, `update`, `file_unchanged`, `text`, `image`};
    /// only `create` ⇒ `is_create = true`, everything else ⇒ `false`. When there is no
    /// `toolUseResult`, it is not an object, or it has no `filePath`, the result is
    /// empty (defensive arms, each tested).
    #[must_use]
    pub fn carrier_create_paths(&self) -> Vec<(String, String, bool)> {
        let Some(tur) = &self.tool_use_result else {
            return Vec::new();
        };
        let Some(file_path) = tur.get("filePath").and_then(serde_json::Value::as_str) else {
            return Vec::new();
        };
        if file_path.is_empty() {
            return Vec::new();
        }
        let is_create = tur.get("type").and_then(serde_json::Value::as_str) == Some("create");

        // The carrier rides on a `tool_result` block whose `tool_use_id` joins it back
        // to the structured tool_use. Emit one tuple per tool_result block id found.
        let mut out = Vec::new();
        if let Some(blocks) = self.blocks() {
            for block in blocks {
                if let Block::ToolResult {
                    tool_use_id: Some(id),
                    ..
                } = block
                {
                    out.push((id.clone(), file_path.to_string(), is_create));
                }
            }
        }
        out
    }

    /// The Bash command string for a `Block::ToolUse { name: "Bash", .. }`
    /// (`input.command`). Returns `None` for any other record / a Bash tool_use with
    /// no command. Feeds the heuristic parser in [`crate::bash_mutations`].
    #[must_use]
    pub fn bash_command(&self) -> Option<&str> {
        let blocks = self.blocks()?;
        for block in blocks {
            if let Block::ToolUse { name, input, .. } = block {
                if name.as_deref() == Some("Bash") {
                    if let Some(cmd) = input
                        .as_ref()
                        .and_then(|v| v.get("command"))
                        .and_then(serde_json::Value::as_str)
                    {
                        return Some(cmd);
                    }
                }
            }
        }
        None
    }
}

/// Group records (in file order) into TURNS, returning one `Vec<usize>` of record
/// indices per turn — the outer index IS the 0-based turn index (genuine-user order).
///
/// The single source of truth for turn delimiting (§6.4), shared by `search`'s
/// exchange reconstruction and `files`'s mutation attribution so the two never drift:
///
/// - A turn opens on a genuine-user record (`is_genuine`); every record after it, up
///   to the next genuine-user, belongs to that turn (a `tool_result`-carrier, an
///   `isMeta` pseudo-turn, and a compaction summary are turn MEMBERS, never delimiters).
/// - Records before the first genuine-user (rare: leading tool noise) seed turn 0 so
///   they are never lost. When such a synthetic lead exists AND a real user turn
///   follows, the lead is folded into the first real turn so indices stay 0-based on
///   genuine users. With NO genuine user at all, the orphans are a standalone turn 0.
///
/// `is_genuine` is a closure (rather than calling [`Record::is_genuine_user`]
/// directly) only so callers can test the grouping over lightweight bool fixtures; in
/// production it is always `Record::is_genuine_user`.
#[must_use]
pub fn group_turn_indices<T>(records: &[T], is_genuine: impl Fn(&T) -> bool) -> Vec<Vec<usize>> {
    let mut turns: Vec<Vec<usize>> = Vec::new();
    for (i, rec) in records.iter().enumerate() {
        if is_genuine(rec) {
            turns.push(vec![i]);
        } else if let Some(last) = turns.last_mut() {
            last.push(i);
        } else {
            // Pre-first-user records seed turn 0 (a standalone turn 0 if no genuine
            // user ever opens).
            turns.push(vec![i]);
        }
    }
    // If the first group is a synthetic pre-user lead AND a real user turn follows,
    // fold the lead into the first real turn so indices align with genuine-user order.
    let synthetic_lead = records.first().is_some_and(|r| !is_genuine(r));
    if synthetic_lead && turns.len() > 1 {
        let lead = turns.remove(0);
        if let Some(first_real) = turns.first_mut() {
            let mut merged = lead;
            merged.extend(first_real.iter().copied());
            *first_real = merged;
        }
    }
    turns
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

    // ── File-mutation extraction (FileOp / FileMutation / Record helpers) ──

    #[test]
    fn file_op_label_all_variants() {
        assert_eq!(FileOp::Write.label(), "write");
        assert_eq!(FileOp::Edit.label(), "edit");
        assert_eq!(FileOp::NotebookEdit.label(), "notebook-edit");
        assert_eq!(FileOp::MultiEdit.label(), "multi-edit");
        assert_eq!(FileOp::BashMutation.label(), "bash");
    }

    #[test]
    fn file_op_is_heuristic_only_for_bash() {
        assert!(FileOp::BashMutation.is_heuristic());
        assert!(!FileOp::Write.is_heuristic());
        assert!(!FileOp::Edit.is_heuristic());
        assert!(!FileOp::NotebookEdit.is_heuristic());
        assert!(!FileOp::MultiEdit.is_heuristic());
    }

    #[test]
    fn structured_tool_mutations_write_edit_multiedit() {
        let r = parse(
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/a.md","content":"x"}},
                {"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/p/spec/gaps.md","old_string":"a","new_string":"b","replace_all":false}},
                {"type":"tool_use","id":"t3","name":"MultiEdit","input":{"file_path":"/p/multi.rs","edits":[]}}
            ]}}"#,
        );
        let muts = r.structured_tool_mutations();
        assert_eq!(muts.len(), 3);
        assert_eq!(muts[0].op, FileOp::Write);
        assert_eq!(muts[0].path, "/tmp/a.md");
        assert_eq!(
            muts[0].timestamp_utc.as_deref(),
            Some("2026-06-07T05:00:00.000Z")
        );
        assert!(!muts[0].is_create, "no carrier on tool_use → unknown=false");
        assert_eq!(muts[1].op, FileOp::Edit);
        assert_eq!(muts[1].path, "/p/spec/gaps.md");
        assert_eq!(muts[2].op, FileOp::MultiEdit);
        assert_eq!(muts[2].path, "/p/multi.rs");
    }

    #[test]
    fn structured_tool_mutations_notebook_uses_notebook_path() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"NotebookEdit","input":{"notebook_path":"/p/nb.ipynb","new_source":"code"}}
            ]}}"#,
        );
        let muts = r.structured_tool_mutations();
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].op, FileOp::NotebookEdit);
        assert_eq!(muts[0].path, "/p/nb.ipynb");
    }

    #[test]
    fn structured_tool_mutations_reads_create_from_own_carrier() {
        // The rare case where the tool_use record ALSO carries the toolUseResult with
        // type:"create" (the `self_is_create` true arm).
        let r = parse(
            r#"{"type":"assistant","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/new.md","content":"x"}}
            ]}}"#,
        );
        let muts = r.structured_tool_mutations();
        assert_eq!(muts.len(), 1);
        assert!(
            muts[0].is_create,
            "own carrier type:create → is_create true"
        );
    }

    #[test]
    fn structured_tool_mutations_skips_missing_and_empty_path() {
        // A Write with NO file_path and an Edit with an EMPTY file_path → both skipped
        // (the defensive `path.is_empty()` arm). A non-mutating tool is ignored too.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"content":"x"}},
                {"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"","old_string":"a"}},
                {"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/p/read.rs"}}
            ]}}"#,
        );
        assert!(r.structured_tool_mutations().is_empty());
    }

    #[test]
    fn structured_tool_mutations_skips_tool_use_with_no_name() {
        // A tool_use block missing its `name` → the `name.as_deref()` None arm.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","input":{"file_path":"/p/x.md"}}
            ]}}"#,
        );
        assert!(r.structured_tool_mutations().is_empty());
    }

    #[test]
    fn structured_tool_mutations_none_for_string_content() {
        // A record with string content → blocks() None → empty (the early-return arm).
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"plain"}}"#);
        assert!(r.structured_tool_mutations().is_empty());
    }

    #[test]
    fn carrier_create_paths_create_update_and_missing() {
        // type:"create" → is_create true, joined to the tool_result block's id.
        let create = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        );
        assert_eq!(
            create.carrier_create_paths(),
            vec![("t1".to_string(), "/tmp/new.md".to_string(), true)]
        );
        // type:"update" → is_create false.
        let update = parse(
            r#"{"type":"user","toolUseResult":{"type":"update","filePath":"/p/gaps.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]}}"#,
        );
        assert_eq!(
            update.carrier_create_paths(),
            vec![("t2".to_string(), "/p/gaps.md".to_string(), false)]
        );
        // No toolUseResult at all → empty (the `let Some(tur) else` arm).
        let none = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t3","content":"ok"}]}}"#,
        );
        assert!(none.carrier_create_paths().is_empty());
    }

    #[test]
    fn carrier_create_paths_file_unchanged_is_not_create() {
        // type:"file_unchanged" (the edit-touched-but-no-change case) → is_create false.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"type":"file_unchanged","filePath":"/p/same.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        );
        assert_eq!(
            r.carrier_create_paths(),
            vec![("t1".to_string(), "/p/same.md".to_string(), false)]
        );
    }

    #[test]
    fn carrier_create_paths_no_file_path_or_not_object() {
        // toolUseResult present but NO filePath → empty (the filePath None arm).
        let no_fp = parse(
            r#"{"type":"user","toolUseResult":{"type":"text","stdout":"x"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
        );
        assert!(no_fp.carrier_create_paths().is_empty());
        // toolUseResult is NOT an object (a bare string) → `.get("filePath")` is None.
        let not_obj = parse(
            r#"{"type":"user","toolUseResult":"just a string","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
        );
        assert!(not_obj.carrier_create_paths().is_empty());
        // filePath present but EMPTY → empty (the `file_path.is_empty()` arm).
        let empty_fp = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
        );
        assert!(empty_fp.carrier_create_paths().is_empty());
    }

    #[test]
    fn carrier_create_paths_skips_non_tool_result_blocks_and_no_id() {
        // A carrier with a filePath but whose only block is NOT a tool_result, OR a
        // tool_result with no tool_use_id → no joinable id, so empty.
        let no_id = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/x.md"},"message":{"role":"user","content":[{"type":"text","text":"not a tool result"},{"type":"tool_result","content":"ok"}]}}"#,
        );
        assert!(no_id.carrier_create_paths().is_empty());
    }

    #[test]
    fn carrier_create_paths_empty_when_no_blocks() {
        // filePath present but the record has string content (no blocks) → empty.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/x.md"},"message":{"role":"user","content":"string body"}}"#,
        );
        assert!(r.carrier_create_paths().is_empty());
    }

    // ── group_turn_indices (the shared §6.4 turn delimiter) ──

    #[test]
    fn group_turn_indices_basic_two_turns() {
        // bools = is_genuine_user; [user, member, member, user, member].
        let flags = [true, false, false, true, false];
        let turns = group_turn_indices(&flags, |b| *b);
        assert_eq!(turns, vec![vec![0, 1, 2], vec![3, 4]]);
    }

    #[test]
    fn group_turn_indices_synthetic_lead_folds_into_first_turn() {
        // Leading non-genuine records (a synthetic lead) fold into turn 0 when a real
        // user follows, so indices stay 0-based on genuine users.
        let flags = [false, false, true, false, true];
        let turns = group_turn_indices(&flags, |b| *b);
        // The two lead members (0,1) join the first real turn (opening at idx 2).
        assert_eq!(turns, vec![vec![0, 1, 2, 3], vec![4]]);
    }

    #[test]
    fn group_turn_indices_only_synthetic_lead_no_genuine_user() {
        // No genuine user ever → a single standalone turn 0 holding the orphans (the
        // `turns.len() > 1` false guard means no fold).
        let flags = [false, false, false];
        let turns = group_turn_indices(&flags, |b| *b);
        assert_eq!(turns, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn group_turn_indices_empty_is_empty() {
        let flags: [bool; 0] = [];
        assert!(group_turn_indices(&flags, |b| *b).is_empty());
    }

    #[test]
    fn group_turn_indices_first_record_genuine_no_fold() {
        // The first record IS genuine → no synthetic lead → no fold.
        let flags = [true, false, true];
        let turns = group_turn_indices(&flags, |b| *b);
        assert_eq!(turns, vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn bash_command_some_and_none() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"rm -rf /tmp/x","description":"clean"}}]}}"#,
        );
        assert_eq!(r.bash_command(), Some("rm -rf /tmp/x"));
        // A Bash tool_use with NO command → None.
        let no_cmd = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"x"}}]}}"#,
        );
        assert!(no_cmd.bash_command().is_none());
        // A non-Bash tool_use → None.
        let other = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/x"}}]}}"#,
        );
        assert!(other.bash_command().is_none());
        // A record with no blocks → None.
        let no_blocks = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(no_blocks.bash_command().is_none());
    }
}
