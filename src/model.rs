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

/// True when `content` (a user record's string or joined-text body) is a
/// machine-synthesized marker the user never typed (§4.2.1–.3) and so must NOT count
/// as a genuine human turn: an exact interrupt marker, a `<local-command-stdout>…`
/// output, or a `<command-name>…` slash-command wrapper. CODEPOINT-SAFE — exact `==`
/// (interrupts, whole-token) or `starts_with` (the two ASCII-tag prefixes); never a
/// byte-offset slice.
#[must_use]
pub fn is_synthetic_user_marker(content: &str) -> bool {
    INTERRUPT_MARKERS.contains(&content)
        || content.starts_with(LOCAL_COMMAND_STDOUT_PREFIX)
        || content.starts_with(COMMAND_NAME_PREFIX)
}

/// The two exact-content synthesized strings Claude Code writes when the user
/// interrupts (§4.2.1). They are a `type:"user"` `text`-block record whose content is
/// EXACTLY one of these — a machine-synthesized interrupt marker, NOT a human turn.
/// Verified across real `~/.claude/projects` data: 116 + 21 occurrences, all
/// non-`isMeta`, none carrying any extra prose (dropping them as turn boundaries loses
/// zero user content).
pub const INTERRUPT_MARKERS: &[&str] = &[
    "[Request interrupted by user]",
    "[Request interrupted by user for tool use]",
];

/// Prefix of a `<local-command-stdout>…` user record (§4.2.2) — local-command OUTPUT
/// (machine), not the user's prose. Non-`isMeta` string content (its sibling
/// `<local-command-caveat>` carries `isMeta` and is already excluded). Must NOT open a
/// turn.
pub const LOCAL_COMMAND_STDOUT_PREFIX: &str = "<local-command-stdout>";

/// Prefix of a `<command-name>/x…</command-name>` slash-command invocation record
/// (§4.2.3) — the machine-templated EXPANSION of a slash command, non-`isMeta`. The
/// templated wrapper must NOT open a turn; any genuine prose the user typed after the
/// command lives in the `<command-args>…</command-args>` body and is recovered
/// separately (see [`Record::slash_command_args`]).
pub const COMMAND_NAME_PREFIX: &str = "<command-name>";

/// Prefix of a `<task-notification>…</task-notification>` user record — a MACHINE-INJECTED
/// automation trigger (a background-command / workflow / spawned-task completion notice CC
/// inserts as a `type:"user"`, non-`isMeta`, STRING-content record). It LOOKS like a human
/// turn to [`Record::is_genuine_user`] (it passes every gate), so it DOES open a turn — but
/// it is an automation pulse, not the operator's prose. [`Record::automation_trigger`]
/// classifies it so surfaces can LABEL the segment (`[workflow <id> completed] <summary>`)
/// instead of dumping the raw `<task-id>`/`<output-file>`/`<status>` XML wrapper.
pub const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// The synthesized marker Claude Code writes into the `tool_result` when the user
/// REJECTS a tool use (§4.2.4) — fires for ANY rejected tool_use (ExitPlanMode plan
/// kick-backs AND rejected AskUserQuestion / Edit / etc.). On its own it is NOT a user
/// turn; it becomes one only when followed by the [`PLAN_REJECTION_USER_PREFIX`] tail
/// (a real typed user instruction).
pub const PLAN_REJECTION_MARKER: &str = "The user doesn't want to proceed with this tool use";

/// The fixed ASCII delimiter that precedes the user's typed instruction in a
/// rejection-with-message (§4.2.4): everything AFTER it is the genuine user message.
/// A rejection WITHOUT this delimiter (the `STOP what you are doing and wait…` form)
/// carries no typed message and must NOT open a turn.
pub const PLAN_REJECTION_USER_PREFIX: &str = "To tell you how to proceed, the user said:\n";

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
    /// - the content must NOT be a machine-synthesized marker the user never typed
    ///   (§4.2.1–.3): an interrupt marker (`[Request interrupted by user]` etc.), a
    ///   `<local-command-stdout>…` output, or a `<command-name>…` slash-command wrapper;
    /// - string content => genuine; block content => genuine iff it contains a
    ///   text block and NO `tool_result` block.
    ///
    /// CODEPOINT-SAFE: every synthetic-marker test is exact `==` or `starts_with`
    /// (whole-token / prefix), never a byte-offset slice, so a CJK body can never be
    /// split mid-codepoint.
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
        // NOTE on `isSidechain`: a subagent transcript's FIRST record is an
        // `isSidechain:true` user seed. It is NOT gated out here on purpose — `list`'s
        // per-subagent preview legitimately treats that seed as the subagent's "first
        // user message", and in TOP-LEVEL transcripts a sidechain seed does not occur in
        // any real corpus. Gating it would silently blank the subagent preview for zero
        // real benefit; the per-surface scan owns subagent-vs-parent context instead.
        let Some(msg) = &self.message else {
            return false;
        };
        if msg.role.as_deref() != Some("user") {
            return false;
        }
        match &msg.content {
            Some(Content::Text(s)) => !is_synthetic_user_marker(s),
            Some(Content::Blocks(blocks)) => {
                let has_tool_result = blocks.iter().any(|b| matches!(b, Block::ToolResult { .. }));
                let has_text = blocks.iter().any(|b| matches!(b, Block::Text { .. }));
                if !has_text || has_tool_result {
                    return false;
                }
                // §4.2.1: an interrupt marker arrives as a single `text` block whose text
                // is EXACTLY the marker — exclude it (exact match, codepoint-safe).
                let joined = flatten_content_text(msg.content.as_ref().unwrap());
                !is_synthetic_user_marker(&joined)
            }
            None => false,
        }
    }

    /// The genuine prose a user typed after a slash command (§4.2.3), recovered from the
    /// `<command-args>…</command-args>` body of a `<command-name>…` record. Returns the
    /// trimmed args text, or `None` when the record is not a slash-command wrapper or the
    /// args are empty. Codepoint-safe: uses `str::find` on the ASCII tag bounds and slices
    /// only on those ASCII byte offsets (never inside the args text), so a CJK args body
    /// is never split mid-codepoint.
    #[must_use]
    pub fn slash_command_args(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !s.starts_with(COMMAND_NAME_PREFIX) {
            return None;
        }
        const OPEN: &str = "<command-args>";
        const CLOSE: &str = "</command-args>";
        let start = s.find(OPEN)? + OPEN.len();
        let end = s[start..].find(CLOSE).map_or(s.len(), |rel| start + rel);
        let args = s[start..end].trim();
        if args.is_empty() {
            None
        } else {
            Some(normalize_line(args))
        }
    }

    /// Classify this record as a MACHINE-INJECTED automation trigger, if it is one.
    ///
    /// A `<task-notification>` record is a `type:"user"`, non-`isMeta`, STRING-content
    /// record CC inserts when a background command / spawned task / workflow completes. It
    /// passes every [`Record::is_genuine_user`] gate (so it opens a turn like a human
    /// message), but it is an automation pulse — surfacing its raw `<task-id>` /
    /// `<output-file>` / `<status>` XML as "user prose" is noise. This parser extracts the
    /// stable inner tags so a surface can render `[workflow <task-id> completed] <summary>`
    /// instead. Returns `None` for any non-`<task-notification>` record.
    ///
    /// CODEPOINT-SAFE: uses `str::find` on the ASCII tag bounds and slices only on those
    /// ASCII offsets, so a CJK summary body is never split mid-codepoint.
    #[must_use]
    pub fn automation_trigger(&self) -> Option<AutomationTrigger> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !s.starts_with(TASK_NOTIFICATION_PREFIX) {
            return None;
        }
        let task_id = extract_xml_tag(s, "task-id");
        let status = extract_xml_tag(s, "status");
        let summary = extract_xml_tag(s, "summary");
        // Monitor-class pulses carry their real outcome in `<event>` (e.g.
        // `STAGE2_OUTPUT_READY`, `[Monitor timed out — re-arm if needed.]`) and frequently have
        // NO `<status>` tag, so the label must read the event rather than defaulting status to
        // a fabricated `completed`.
        let event = extract_xml_tag(s, "event");
        let kind = AutomationKind::from_summary(summary.as_deref());
        Some(AutomationTrigger {
            kind,
            task_id,
            status,
            summary,
            event,
        })
    }

    /// The one-line ATTRIBUTION label for an automation-trigger opener, or `None` when this
    /// record is not one: `[<kind> <task-id> <status>] <summary>` where `<kind>` is the TRUE
    /// trigger class parsed from the summary's leading classifier (`background-command` /
    /// `workflow` / `agent` / fallback `task`) — NOT the hardcoded literal `workflow` that
    /// mislabeled 81% of triggers on the oracle (85 background-command + 2 agent). A missing
    /// field is elided gracefully. This is what `turns` / `search` render as the segment
    /// opener in place of the raw `<task-notification>` XML blob.
    #[must_use]
    pub fn automation_label(&self) -> Option<String> {
        let t = self.automation_trigger()?;
        let id = t.task_id.as_deref().unwrap_or("?");
        // The status slot prefers the explicit `<status>`; when it is absent (the common
        // Monitor/ScheduleWakeup case) the real outcome lives in `<event>` — so render THAT
        // (e.g. `STAGE2_OUTPUT_READY`, a timeout notice) rather than fabricating `completed`,
        // which would invert a timed-out monitor's attribution. Only when BOTH are missing do
        // we fall back to `completed`. An event payload is whitespace-normalized for the label.
        let event_norm = t
            .event
            .as_deref()
            .filter(|e| !e.is_empty())
            .map(normalize_line);
        let status = t
            .status
            .as_deref()
            .map(str::to_string)
            .or(event_norm)
            .unwrap_or_else(|| "completed".to_string());
        let head = format!("[{} {id} {status}]", t.kind.slug());
        Some(match t.summary.as_deref() {
            Some(sum) if !sum.is_empty() => format!("{head} {}", normalize_line(sum)),
            _ => head,
        })
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

    /// True when this record is an ANSWERED AskUserQuestion carrier that should open a
    /// turn (§4.4 / §6.4): a `type:"user"` record carrying a `tool_result` block whose
    /// `is_error` is not true, AND it is a real answer — signalled by a non-empty
    /// `toolUseResult.answers` object (the clean, structured source) OR, as a fallback
    /// for an older record without `toolUseResult`, the synthesized AUQ-answer marker in
    /// the tool_result content. The answer is a genuine USER message (the user's
    /// selection + prose reasoning), so it is a turn boundary.
    ///
    /// A CANCELLED / rejected / validation-errored AUQ (no `answers`, `is_error:true`,
    /// or a `Cancelled…` / `<tool_use_error>…` body) is NOT a boundary — those carry no
    /// typed user message. Verified on real data: all 81 answered carriers have
    /// non-empty `toolUseResult.answers`, the marker string, and `is_error` false; the
    /// rejection/cancel carriers have none of the three.
    #[must_use]
    pub fn is_auq_answer_boundary(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        // The carrier must ride on a non-errored tool_result block.
        let Some(blocks) = self.blocks() else {
            return false;
        };
        let mut has_non_errored_tool_result = false;
        for b in blocks {
            if let Block::ToolResult { is_error, .. } = b {
                if is_error.unwrap_or(false) {
                    return false; // an errored AUQ result (cancel/reject) is never a boundary
                }
                has_non_errored_tool_result = true;
            }
        }
        if !has_non_errored_tool_result {
            return false;
        }
        // Primary signal: structured, non-empty `toolUseResult.answers`.
        if self.auq_answers_obj().is_some() {
            return true;
        }
        // Fallback (older records without `toolUseResult`): the synthesized marker.
        self.is_auq_answer()
    }

    /// The structured `toolUseResult.answers` object (§4.4) when present AND non-empty.
    /// `None` for a cancelled/rejected AUQ (no answers) or a non-AUQ carrier.
    fn auq_answers_obj(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        let answers = self.tool_use_result.as_ref()?.get("answers")?.as_object()?;
        if answers.is_empty() {
            None
        } else {
            Some(answers)
        }
    }

    /// Reconstruct the COMPLETE AskUserQuestion exchange (§4.4) as one genuine-user unit:
    /// `[AskUserQuestion · N questions]` followed by, per question, the header, the
    /// question, its options, and the user's answer. Built from the structured
    /// `toolUseResult.questions[]` zipped with `toolUseResult.answers{}`; falls back to
    /// the synthesized `tool_result` string (parsed for `"<q>"="<a>"`) when
    /// `toolUseResult` is absent. Returns `None` when this is not an answered AUQ carrier.
    ///
    /// CODEPOINT-SAFE: works entirely on owned `String`/`&str` values pulled structurally
    /// from JSON; the only excerpting is whitespace normalization, never a byte-offset
    /// slice into a (possibly CJK) question/answer body.
    #[must_use]
    pub fn auq_exchange(&self) -> Option<String> {
        if !self.is_auq_answer_boundary() {
            return None;
        }
        // Structured path: questions[] (ordered) zipped with answers{question -> answer}.
        if let Some(answers) = self.auq_answers_obj() {
            let questions = self
                .tool_use_result
                .as_ref()
                .and_then(|t| t.get("questions"))
                .and_then(serde_json::Value::as_array);
            let mut out = String::new();
            let n = questions.map_or(answers.len(), Vec::len);
            out.push_str(&format!("[AskUserQuestion · {n} question{}]", plural(n)));
            if let Some(qs) = questions {
                for (i, q) in qs.iter().enumerate() {
                    let header = q.get("header").and_then(serde_json::Value::as_str);
                    let question = q
                        .get("question")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let opts: Vec<String> = q
                        .get("options")
                        .and_then(serde_json::Value::as_array)
                        .map(|os| {
                            os.iter()
                                .filter_map(|o| o.get("label").and_then(serde_json::Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    // The answer is keyed by the (verbatim) question string in `answers`.
                    let answer = answers
                        .get(question)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    out.push_str(&format!("\nQ{} ", i + 1));
                    if let Some(h) = header {
                        out.push_str(&format!("({}): ", normalize_line(h)));
                    }
                    out.push_str(&normalize_line(question));
                    if !opts.is_empty() {
                        out.push_str("  options: ");
                        out.push_str(&opts.join(" | "));
                    }
                    out.push_str(&format!("\nA{}: {}", i + 1, normalize_line(answer)));
                }
            } else {
                // No questions[] array (rare): list the answers map directly.
                for (i, (q, a)) in answers.iter().enumerate() {
                    out.push_str(&format!(
                        "\nQ{}: {}\nA{}: {}",
                        i + 1,
                        normalize_line(q),
                        i + 1,
                        normalize_line(a.as_str().unwrap_or_default())
                    ));
                }
            }
            return Some(out);
        }
        // Fallback path: the synthesized marker string is the whole exchange.
        self.auq_answer_marker_text()
            .map(|t| format!("[AskUserQuestion] {}", normalize_line(&t)))
    }

    /// The synthesized AUQ-answer string from this carrier's `tool_result` (§4.4) — the
    /// fallback content when `toolUseResult.answers` is absent. `None` if no AUQ marker.
    fn auq_answer_marker_text(&self) -> Option<String> {
        let blocks = self.blocks()?;
        for b in blocks {
            if let Block::ToolResult {
                content: Some(c), ..
            } = b
            {
                let t = tool_result_content_text(c);
                if is_auq_answer_text(&t) {
                    return Some(t);
                }
            }
        }
        None
    }

    /// When this record is a tool-use REJECTION carrying a typed user instruction
    /// (§4.2.4), return `(rejected_tool_use_id, user_message)`. The genuine user message
    /// is everything AFTER the fixed [`PLAN_REJECTION_USER_PREFIX`] delimiter.
    ///
    /// `None` when this is not a rejection, or it is a rejection WITHOUT a typed message
    /// (the `STOP what you are doing and wait…` form — the user clicked reject but typed
    /// nothing, so there is no user turn).
    ///
    /// CODEPOINT-SAFE: the tail is taken with `str::split_once` on the ASCII delimiter
    /// (UTF-8-safe, never a byte-offset slice); the tail (often CJK) is returned whole.
    #[must_use]
    pub fn plan_rejection_message(&self) -> Option<(Option<String>, String)> {
        if !self.is_type("user") {
            return None;
        }
        let blocks = self.blocks()?;
        for b in blocks {
            if let Block::ToolResult {
                tool_use_id,
                content: Some(c),
                is_error,
            } = b
            {
                if !is_error.unwrap_or(false) {
                    continue;
                }
                let text = tool_result_content_text(c);
                if !text.contains(PLAN_REJECTION_MARKER) {
                    continue;
                }
                // The typed instruction is everything after the fixed ASCII delimiter.
                if let Some((_, tail)) = text.split_once(PLAN_REJECTION_USER_PREFIX) {
                    let msg = tail.trim();
                    if !msg.is_empty() {
                        return Some((tool_use_id.clone(), msg.to_string()));
                    }
                }
            }
        }
        None
    }

    /// True when this record is a tool-use rejection carrying a typed user instruction
    /// (§4.2.4) and so should open a turn. A rejection without a typed message is NOT a
    /// boundary (see [`Record::plan_rejection_message`]).
    #[must_use]
    pub fn is_plan_rejection_boundary(&self) -> bool {
        self.plan_rejection_message().is_some()
    }

    /// The single boundary predicate (§6.4): this record opens a new turn iff it is a
    /// genuine human message, an ANSWERED AskUserQuestion (the answer is the user's
    /// message), or a tool-use rejection carrying a typed user instruction. Every surface
    /// (turns / search / recover / files) keys turn delimiting on THIS predicate so they
    /// never drift.
    #[must_use]
    pub fn opens_turn(&self) -> bool {
        self.is_genuine_user() || self.is_auq_answer_boundary() || self.is_plan_rejection_boundary()
    }

    /// The rendered genuine-user text for any boundary-opening record, normalized to a
    /// single line — the unified opener body used by `turns` / `search` / `list` /
    /// `recover`:
    /// - a plain genuine user → its text (same as [`Record::genuine_user_text`]);
    /// - an answered AskUserQuestion → the full Q+options+answer unit
    ///   ([`Record::auq_exchange`]);
    /// - a tool-use rejection-with-message → the user's typed instruction, optionally
    ///   suffixed with a `[plan: <path>]` pointer when `plan_index` resolves the rejected
    ///   `tool_use_id` to an ExitPlanMode plan (§4.2.4). `plan_index` may be `None` (no
    ///   plan resolution attempted), in which case the rejection text is returned alone.
    ///
    /// Returns `None` when this record does not open a turn. CODEPOINT-SAFE throughout
    /// (delegates to the codepoint-safe accessors).
    #[must_use]
    pub fn reconstructed_user_text(&self, plan_index: Option<&PlanIndex>) -> Option<String> {
        if let Some(text) = self.genuine_user_text() {
            return Some(text);
        }
        if let Some(unit) = self.auq_exchange() {
            return Some(normalize_line(&unit));
        }
        if let Some((rejected_id, msg)) = self.plan_rejection_message() {
            let mut out = normalize_line(&msg);
            if let (Some(idx), Some(id)) = (plan_index, rejected_id.as_deref()) {
                if let Some(path) = idx.plan_path(id) {
                    out.push_str(&format!(" [plan: {path}]"));
                }
            }
            return Some(out);
        }
        // A slash-command wrapper is NOT a turn boundary (§4.2.3), but when the user
        // typed prose after the command (`/compact <prose>`) that prose IS genuine user
        // input — surface it so `search -t user` still finds it within its turn.
        if let Some(args) = self.slash_command_args() {
            return Some(args);
        }
        None
    }

    /// The ExitPlanMode tool_use blocks carried by this (assistant) record, as
    /// `(tool_use_id, plan_file_path)` pairs — the raw material a [`PlanIndex`] is built
    /// from. `plan_file_path` prefers `input.planFilePath`; a block with no path yields
    /// an empty string (still indexed so the id is known to be an ExitPlanMode). Empty
    /// for any record carrying no ExitPlanMode tool_use.
    #[must_use]
    pub fn exit_plan_pointers(&self) -> Vec<(String, String)> {
        let Some(blocks) = self.blocks() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for b in blocks {
            if let Block::ToolUse {
                id: Some(id),
                name: Some(name),
                input,
            } = b
            {
                if name == "ExitPlanMode" {
                    let path = input
                        .as_ref()
                        .and_then(|v| v.get("planFilePath"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    out.push((id.clone(), path));
                }
            }
        }
        out
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

/// An index of ExitPlanMode `tool_use_id → planFilePath` built from a session's records
/// (§4.2.4). A tool-use rejection-with-message ([`Record::plan_rejection_message`])
/// resolves the rejected `tool_use_id` through this index to surface a `[plan: <path>]`
/// pointer so a consuming LLM can go Read the plan. Built once per session via
/// [`PlanIndex::from_records`]; cheap (one `BTreeMap` of the few ExitPlanMode calls).
#[derive(Debug, Clone, Default)]
pub struct PlanIndex {
    by_id: std::collections::BTreeMap<String, String>,
}

impl PlanIndex {
    /// Build the index from a session's records: every ExitPlanMode tool_use's
    /// `id → planFilePath` (see [`Record::exit_plan_pointers`]). A block with no
    /// `planFilePath` is skipped (an empty path is not a useful pointer).
    #[must_use]
    pub fn from_records<'a, I>(records: I) -> Self
    where
        I: IntoIterator<Item = &'a Record>,
    {
        let mut by_id = std::collections::BTreeMap::new();
        for rec in records {
            for (id, path) in rec.exit_plan_pointers() {
                if !path.is_empty() {
                    by_id.insert(id, path);
                }
            }
        }
        Self { by_id }
    }

    /// The plan file path an ExitPlanMode tool_use with `id` pointed to, if known.
    #[must_use]
    pub fn plan_path(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }
}

/// `"s"` for plural counts, `""` for exactly one — for the `N question(s)` label.
fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Group records (in file order) into TURNS, returning one `Vec<usize>` of record
/// indices per turn — the outer index IS the 0-based turn index (genuine-user order).
///
/// The single source of truth for turn delimiting (§6.4), shared by `search`'s
/// exchange reconstruction and `files`'s mutation attribution so the two never drift:
///
/// - A turn opens on a boundary record (`is_genuine`); every record after it, up to the
///   next boundary, belongs to that turn (a non-boundary `tool_result`-carrier, an
///   `isMeta` pseudo-turn, and a compaction summary are turn MEMBERS, never delimiters).
/// - Records before the first boundary (rare: leading tool noise) seed turn 0 so they
///   are never lost. When such a synthetic lead exists AND a real user turn follows, the
///   lead is folded into the first real turn so indices stay 0-based on boundary
///   openers. With NO boundary at all, the orphans are a standalone turn 0.
///
/// `is_genuine` is a closure (rather than calling [`Record::opens_turn`] directly) only
/// so callers can test the grouping over lightweight bool fixtures; in production it is
/// always [`Record::opens_turn`] — which opens on a genuine human message, an answered
/// AskUserQuestion (the answer is the user's message, §4.4), OR a tool-use
/// rejection-with-message (§4.2.4). An AUQ answer / plan rejection becoming a turn
/// boundary is the sanctioned correct behavior change (a previously-MISSED genuine user
/// message); interrupts / `<local-command-stdout>` / `<command-name>` wrappers, formerly
/// spurious boundaries, are excluded by `is_genuine_user` (regression fixes).
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

/// The TRUE class of a `<task-notification>` automation trigger, parsed from the leading
/// classifier of its `<summary>` (verified against real sessions: the summary opens with
/// `Background command "…"`, `Dynamic workflow "…"`, or `Agent …`). This is the attribution
/// the P2 turn-segmentation lens demands — the old code hardcoded the literal `workflow` for
/// EVERY trigger, mislabeling background-command + agent pulses (81% on the captured session).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationKind {
    /// A `Background command "…"` completion pulse (a `&`-detached shell command CC ran).
    BackgroundCommand,
    /// A `Dynamic workflow "…"` completion pulse (an OMC / dynamic workflow run).
    Workflow,
    /// An `Agent …` completion pulse (a spawned subagent).
    Agent,
    /// A `Monitor event: …` / `Monitor …` pulse — the ScheduleWakeup / monitor / cron-tick
    /// family the P2 turn-segmentation lens names as its own trigger class. Previously this
    /// collapsed into the opaque `Task` fallback (the only real `Task` instance in either
    /// oracle WAS a misclassified `Monitor event:` pulse), losing its attribution.
    Monitor,
    /// Any other / unrecognized classifier — the safe fallback (renders `task`).
    Task,
}

impl AutomationKind {
    /// Classify from the `<summary>`'s leading token. Case-insensitive on the known
    /// prefixes; anything else (or a missing summary) is [`AutomationKind::Task`]. The
    /// `monitor` / `scheduled` / `cron` prefixes route the ScheduleWakeup/monitor/cron-tick
    /// family to [`AutomationKind::Monitor`] (the lens demands it be a distinct, labeled
    /// class — verified `Monitor event:`×10 + `Monitor`×6 across the two oracles).
    #[must_use]
    pub fn from_summary(summary: Option<&str>) -> Self {
        let s = summary.unwrap_or("").trim_start();
        // The classifiers are a fixed leading phrase; match the longest-distinguishing
        // prefix case-insensitively so a `Background command "…"` is not mistaken for `task`.
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("background command") {
            AutomationKind::BackgroundCommand
        } else if lower.starts_with("dynamic workflow") || lower.starts_with("workflow") {
            AutomationKind::Workflow
        } else if lower.starts_with("monitor")
            || lower.starts_with("scheduled")
            || lower.starts_with("cron")
        {
            AutomationKind::Monitor
        } else if lower.starts_with("agent") {
            AutomationKind::Agent
        } else {
            AutomationKind::Task
        }
    }

    /// The stable lowercase slug rendered in the `[<kind> <id> <status>]` label.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            AutomationKind::BackgroundCommand => "background-command",
            AutomationKind::Workflow => "workflow",
            AutomationKind::Agent => "agent",
            AutomationKind::Monitor => "monitor",
            AutomationKind::Task => "task",
        }
    }
}

/// A parsed `<task-notification>` automation trigger — the stable inner tags of a
/// machine-injected background-command / workflow / spawned-task completion notice. Every
/// field is `Option` because a malformed / partial notification must degrade gracefully
/// (the label still renders with `?`/`completed` fallbacks) rather than be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTrigger {
    /// The TRUE trigger class (parsed from the `<summary>` classifier) — the attribution the
    /// label renders, replacing the prior hardcoded `workflow`.
    pub kind: AutomationKind,
    /// The `<task-id>` (the workflow / background-command id), if present.
    pub task_id: Option<String>,
    /// The `<status>` (`completed` / `failed` / …), if present.
    pub status: Option<String>,
    /// The `<summary>` (the human-readable "what completed" line), if present.
    pub summary: Option<String>,
    /// The `<event>` payload, if present — where a Monitor / ScheduleWakeup pulse carries its
    /// real outcome (`STAGE2_OUTPUT_READY`, `[Monitor timed out — re-arm if needed.]`). Often
    /// the only outcome signal on a Monitor pulse (which usually has no `<status>`), so the
    /// label falls back to it instead of fabricating `completed`.
    pub event: Option<String>,
}

/// Extract the text between `<tag>` and `</tag>` in `s`, trimmed, or `None` when the tag
/// is absent or empty. Codepoint-safe: `str::find` returns ASCII byte offsets of the
/// (ASCII) tag delimiters, and the slice is taken on those offsets only — never inside the
/// (possibly CJK) body. A missing close tag yields `None` (never a runaway slice).
fn extract_xml_tag(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let end_rel = s[start..].find(&close)?;
    let inner = s[start..start + end_rel].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
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
        // It is NOT a genuine user (it's a carrier) but IS an AUQ answer — and, per the
        // §6.4 behavior change, it DOES open a turn (the answer is the user's message).
        assert!(!r.is_genuine_user());
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
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
        // The dominant phrasing is also a turn boundary (§6.4).
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
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
    fn opens_turn_grouping_splits_on_auq_answer_and_skips_interrupt() {
        // A realistic record sequence over the PRODUCTION predicate (`opens_turn`):
        //   0 genuine user      → turn 0 opener
        //   1 assistant AUQ tool_use (member of turn 0)
        //   2 AUQ answer carrier → turn 1 opener (the behavior change)
        //   3 assistant reply   (member of turn 1)
        //   4 interrupt marker  → NOT a boundary (member of turn 1)
        //   5 genuine user      → turn 2 opener
        let records: Vec<Record> = [
            r#"{"type":"user","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","toolUseResult":{"answers":{"which?":"the bold one"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the bold one\"."}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"next question"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
        let turns = group_turn_indices(&records, |r| r.opens_turn());
        assert_eq!(
            turns,
            vec![vec![0, 1], vec![2, 3, 4], vec![5]],
            "AUQ answer opens turn 1; interrupt is a member, not a boundary"
        );
    }

    // ── §4.2.1 interrupts: synthesized markers are NOT genuine-user (spurious-boundary
    //    removal). Real shape: a text-block user record whose text is EXACTLY the marker
    //    (116 + 21 occurrences in the corpus, none isMeta, none carrying extra prose). ──

    #[test]
    fn interrupt_marker_plain_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
        );
        assert!(
            !r.is_genuine_user(),
            "interrupt marker must not open a turn"
        );
        assert!(!r.opens_turn());
    }

    #[test]
    fn interrupt_marker_for_tool_use_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(!r.opens_turn());
    }

    #[test]
    fn interrupt_marker_as_string_content_is_not_genuine_user() {
        // The same marker can arrive as bare-string content too — still excluded.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn text_that_merely_contains_interrupt_phrase_is_still_genuine() {
        // Exact-match only: a real message that QUOTES the phrase must stay genuine
        // (codepoint-safe `==`, never a substring/slice).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"why did i see [Request interrupted by user] earlier?"}}"#,
        );
        assert!(
            r.is_genuine_user(),
            "a message merely containing the phrase is still a human turn"
        );
    }

    // ── §4.2.2 / §4.2.3: <local-command-stdout> output + <command-name> slash wrapper
    //    are machine-templated, NOT genuine-user (string content, non-isMeta). ──

    #[test]
    fn local_command_stdout_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>✓ Updated oh-my-claudecode.</local-command-stdout>"}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(!r.opens_turn());
    }

    #[test]
    fn command_name_wrapper_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"}}"#,
        );
        assert!(
            !r.is_genuine_user(),
            "slash-command wrapper must not open a turn"
        );
        assert!(!r.opens_turn());
        // Empty args → no recoverable prose.
        assert!(r.slash_command_args().is_none());
    }

    #[test]
    fn command_name_wrapper_with_args_recovers_prose() {
        // Real shape: `/compact Just shipped spec-batch-14 …` — the typed prose lives in
        // <command-args>; it is recovered (and is the reconstructed user text), but the
        // wrapper itself is still not a standalone genuine-user record.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args> Just shipped spec-batch-14, summarize</command-args>"}}"#,
        );
        assert!(!r.is_genuine_user());
        assert_eq!(
            r.slash_command_args().as_deref(),
            Some("Just shipped spec-batch-14, summarize")
        );
        assert_eq!(
            r.reconstructed_user_text(None).as_deref(),
            Some("Just shipped spec-batch-14, summarize")
        );
    }

    #[test]
    fn command_name_wrapper_with_cjk_args_is_codepoint_safe() {
        // A CJK args body must be recovered whole (codepoint-safe slice on the ASCII tags
        // only) — the live panic class.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args>x</command-args>"}}"#,
        );
        assert_eq!(
            r.slash_command_args().as_deref(),
            Some("x")
        );
    }

    // ── §4.4 AskUserQuestion answer = a genuine-user TURN BOUNDARY (the sanctioned
    //    behavior change). Real shape verified on a captured sample: a user tool_result
    //    carrier with toolUseResult.answers + the synthesized marker, is_error absent. ──

    #[test]
    fn auq_answer_with_structured_answers_is_a_boundary() {
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"question":"which?","header":"FIX","options":[{"label":"opt A"},{"label":"opt B"}]}],"answers":{"which?":"go with opt A and also fix the prod gap"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"go with opt A and also fix the prod gap\". You can now continue."}]}}"#,
        );
        // It is still NOT a "genuine user" (it rides on a carrier) but IS a boundary.
        assert!(!r.is_genuine_user());
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
        // The reconstructed exchange carries Q + options + the answer prose.
        let unit = r.auq_exchange().expect("auq exchange");
        assert!(unit.contains("AskUserQuestion · 1 question"));
        assert!(unit.contains("which?"));
        assert!(unit.contains("FIX"));
        assert!(unit.contains("opt A | opt B"), "options rendered: {unit}");
        assert!(unit.contains("go with opt A and also fix the prod gap"));
        // reconstructed_user_text routes to the same unit.
        assert!(r
            .reconstructed_user_text(None)
            .unwrap()
            .contains("go with opt A"));
    }

    #[test]
    fn auq_answer_cjk_is_codepoint_safe_boundary() {
        // The exact AUQ answer that expanded the session scope — CJK answer prose. Must
        // reconstruct whole, no mid-codepoint slice.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"question":"STEP TWO x?","header":"STEP TWO x","options":[{"label":"x+x (x)"}]}],"answers":{"STEP TWO x?":"xsessionxscopex"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"STEP TWO x?\"=\"x\"."}]}}"#,
        );
        assert!(r.is_auq_answer_boundary());
        let unit = r.auq_exchange().expect("cjk auq exchange");
        assert!(unit.contains("xsessionxscopex"));
        assert!(unit.contains("STEP TWO x"));
        assert!(unit.contains("x+x (x)"));
    }

    #[test]
    fn auq_answer_marker_only_fallback_is_a_boundary() {
        // Older-shape carrier: the synthesized marker is present but there is no
        // toolUseResult.answers → the marker fallback still classifies it a boundary.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"q\"=\"chosen label\". You can now continue."}]}}"#,
        );
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
        let unit = r.auq_exchange().expect("fallback exchange");
        assert!(unit.contains("AskUserQuestion"));
        assert!(unit.contains("chosen label"));
    }

    #[test]
    fn cancelled_auq_is_not_a_boundary() {
        // A rejected/cancelled AUQ: is_error:true, no answers, the generic rejection
        // marker WITHOUT a "the user said" tail → must NOT open a turn (no user message).
        let r = parse(
            r#"{"type":"user","toolUseResult":"User rejected tool use","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}"#,
        );
        assert!(
            !r.is_auq_answer_boundary(),
            "cancelled AUQ is not a boundary"
        );
        assert!(
            !r.is_plan_rejection_boundary(),
            "no typed message → not a rejection boundary"
        );
        assert!(!r.opens_turn());
    }

    #[test]
    fn auq_validation_error_is_not_a_boundary() {
        // An InputValidationError AUQ result (is_error:true, <tool_use_error>…) is not an
        // answer → not a boundary.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"<tool_use_error>InputValidationError: AskUserQuestion failed…</tool_use_error>"}]}}"#,
        );
        assert!(!r.is_auq_answer_boundary());
        assert!(!r.opens_turn());
    }

    // ── §4.2.4 ExitPlanMode / tool-use rejection WITH a typed message = a genuine-user
    //    boundary + a plan pointer. Real shape from captured-c (CJK) + the English form. ──

    #[test]
    fn plan_rejection_with_cjk_message_is_a_boundary() {
        // Instance A (captured-c): the user rejects the plan and types a CJK instruction.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01KpYZsMm2SaKgw6Qvhd8ST8","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nxsmoke testxOKxscreenshotx"}]}}"#,
        );
        assert!(r.is_plan_rejection_boundary());
        assert!(r.opens_turn());
        let (id, msg) = r.plan_rejection_message().expect("rejection message");
        assert_eq!(id.as_deref(), Some("toolu_01KpYZsMm2SaKgw6Qvhd8ST8"));
        // The genuine message is ONLY the typed tail (CJK, whole), not the synthesized
        // prefix.
        assert_eq!(
            msg,
            "xsmoke testxOKxscreenshotx"
        );
        assert!(!msg.contains("doesn't want to proceed"));
    }

    #[test]
    fn plan_rejection_with_english_message_is_a_boundary() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01YKnvDN43RnTQMGcw18HShW","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nRound-4 sign-off: run the e2e once more before declaring done."}]}}"#,
        );
        let (_, msg) = r.plan_rejection_message().expect("english rejection");
        assert_eq!(
            msg,
            "Round-4 sign-off: run the e2e once more before declaring done."
        );
    }

    #[test]
    fn plan_rejection_without_message_is_not_a_boundary() {
        // The `STOP what you are doing and wait…` form carries NO typed message → no
        // boundary (36 such records in the corpus).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}"#,
        );
        assert!(r.plan_rejection_message().is_none());
        assert!(!r.is_plan_rejection_boundary());
        assert!(!r.opens_turn());
    }

    #[test]
    fn plan_approval_is_not_a_boundary() {
        // The approval path is the harness greenlight (no typed message, no is_error) —
        // must NOT become a turn boundary.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"User has approved your plan. You can now start coding. Your plan has been saved to: /Users/testuser/.claude/plans/elegant-scribbling-dream.md"}]}}"#,
        );
        assert!(!r.is_plan_rejection_boundary());
        assert!(!r.is_auq_answer_boundary());
        assert!(!r.opens_turn());
    }

    #[test]
    fn plan_rejection_surfaces_plan_pointer_via_index() {
        // The ExitPlanMode tool_use carries planFilePath; the rejection resolves it via
        // the PlanIndex → the reconstructed user text carries a `[plan: …]` pointer.
        let tool_use = parse(
            r##"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_PLAN","name":"ExitPlanMode","input":{"plan":"# the plan body","planFilePath":"/Users/testuser/.claude/plans/elegant-scribbling-dream.md"}}]}}"##,
        );
        let rejection = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLAN","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nadd a screenshot check first"}]}}"#,
        );
        let index = PlanIndex::from_records([&tool_use]);
        assert_eq!(
            index.plan_path("toolu_PLAN"),
            Some("/Users/testuser/.claude/plans/elegant-scribbling-dream.md")
        );
        let text = rejection
            .reconstructed_user_text(Some(&index))
            .expect("reconstructed");
        assert!(text.starts_with("add a screenshot check first"));
        assert!(
            text.contains("[plan: /Users/testuser/.claude/plans/elegant-scribbling-dream.md]"),
            "plan pointer surfaced: {text}"
        );
    }

    #[test]
    fn plan_rejection_of_non_exit_plan_tool_has_no_pointer() {
        // A rejection-with-message whose tool_use_id is NOT an ExitPlanMode → still a
        // genuine-user boundary, but no plan pointer (the index does not resolve it).
        let rejection = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_EDIT","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nuse a different file path"}]}}"#,
        );
        let index = PlanIndex::default(); // no ExitPlanMode indexed
        assert!(rejection.is_plan_rejection_boundary());
        let text = rejection.reconstructed_user_text(Some(&index)).unwrap();
        assert_eq!(text, "use a different file path");
        assert!(!text.contains("[plan:"));
    }

    #[test]
    fn exit_plan_pointers_extracts_id_and_path() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"x","planFilePath":"/plans/a.md"}}]}}"#,
        );
        assert_eq!(
            r.exit_plan_pointers(),
            vec![("p1".to_string(), "/plans/a.md".to_string())]
        );
        // A non-ExitPlanMode record yields nothing.
        let other = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/x"}}]}}"#,
        );
        assert!(other.exit_plan_pointers().is_empty());
    }

    #[test]
    fn plan_index_skips_pointer_without_path() {
        // An ExitPlanMode with NO planFilePath is not indexed (empty path → no pointer).
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"x"}}]}}"#,
        );
        let index = PlanIndex::from_records([&r]);
        assert!(index.plan_path("p1").is_none());
    }

    #[test]
    fn opens_turn_matches_genuine_user() {
        // A plain genuine user opens a turn; a plain tool_result carrier does not.
        let genuine = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(genuine.opens_turn());
        let carrier = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
        );
        assert!(!carrier.opens_turn());
    }

    // ── Automation-trigger classification (`<task-notification>`) ──

    #[test]
    fn automation_trigger_parses_task_notification() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>wh1it9jlj</task-id>\n<tool-use-id>toolu_x</tool-use-id>\n<output-file>/tmp/x.output</output-file>\n<status>completed</status>\n<summary>Dynamic workflow \"READ-ONLY: verify csift files\" completed</summary>\n</task-notification>"}}"#,
        );
        // It STILL opens a turn (it is a real boundary) — but is now classified.
        assert!(
            r.is_genuine_user(),
            "a task-notification passes the genuine-user gate"
        );
        assert!(r.opens_turn());
        let t = r.automation_trigger().expect("classified as automation");
        assert_eq!(t.task_id.as_deref(), Some("wh1it9jlj"));
        assert_eq!(t.status.as_deref(), Some("completed"));
        assert_eq!(
            t.kind,
            AutomationKind::Workflow,
            "Dynamic workflow → workflow"
        );
        assert_eq!(
            t.summary.as_deref(),
            Some("Dynamic workflow \"READ-ONLY: verify csift files\" completed")
        );
        // The rendered ATTRIBUTION label replaces the raw XML blob; a `Dynamic workflow`
        // summary keeps the `workflow` kind.
        let label = r.automation_label().unwrap();
        assert!(
            label.starts_with("[workflow wh1it9jlj completed]"),
            "got: {label}"
        );
        assert!(
            label.contains("Dynamic workflow"),
            "summary in label: {label}"
        );
        assert!(
            !label.contains("<task-id>"),
            "raw XML must not leak: {label}"
        );
    }

    #[test]
    fn automation_trigger_none_for_human_and_partial_graceful() {
        // A plain human message is NOT an automation trigger.
        let human =
            parse(r#"{"type":"user","message":{"role":"user","content":"please fix the bug"}}"#);
        assert!(human.automation_trigger().is_none());
        assert!(human.automation_label().is_none());
        // A partial notification (no summary/status) still labels gracefully with fallbacks.
        let partial = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>abc</task-id>\n</task-notification>"}}"#,
        );
        // No summary → kind falls back to `task` (NOT the old hardcoded `workflow`).
        let label = partial.automation_label().unwrap();
        assert_eq!(label, "[task abc completed]");
    }

    #[test]
    fn automation_kind_classifies_background_command_and_agent() {
        // The mislabel fix: a `Background command "…"` summary renders `background-command`,
        // an `Agent …` summary renders `agent` — NOT the old blanket `workflow`.
        let bg = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b497m4ncp</task-id>\n<status>completed</status>\n<summary>Background command \"build venvs\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        );
        let t = bg.automation_trigger().unwrap();
        assert_eq!(t.kind, AutomationKind::BackgroundCommand);
        assert!(
            bg.automation_label()
                .unwrap()
                .starts_with("[background-command b497m4ncp completed]"),
            "got: {:?}",
            bg.automation_label()
        );
        let ag = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>ag1</task-id>\n<status>completed</status>\n<summary>Agent executor finished its task</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(ag.automation_trigger().unwrap().kind, AutomationKind::Agent);
        assert!(ag
            .automation_label()
            .unwrap()
            .starts_with("[agent ag1 completed]"));
        // A failed background command keeps its kind + status.
        let bgf = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b0855naiz</task-id>\n<status>failed</status>\n<summary>Background command \"Launch the overnight guard\" failed with exit code 1</summary>\n</task-notification>"}}"#,
        );
        assert!(bgf
            .automation_label()
            .unwrap()
            .starts_with("[background-command b0855naiz failed]"));
    }

    #[test]
    fn monitor_cadence_event_replaces_fabricated_completed_status() {
        // The real captured monitor line-34408 shape: a Monitor pulse with NO <status> but a real
        // <event> outcome. The label must surface the EVENT (STAGE2_OUTPUT_READY), not fabricate
        // `completed` — which would invert a timed-out monitor's attribution.
        let mon = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b718g3gqq</task-id>\n<summary>Monitor event: \"full pulse suite re-run completion\"</summary>\n<event>STAGE2_OUTPUT_READY</event>\n</task-notification>"}}"#,
        );
        let t = mon.automation_trigger().expect("a monitor trigger");
        assert_eq!(t.kind, AutomationKind::Monitor);
        assert_eq!(t.status, None, "this monitor pulse carries no <status>");
        assert_eq!(t.event.as_deref(), Some("STAGE2_OUTPUT_READY"));
        let label = mon.automation_label().unwrap();
        assert!(
            label.starts_with("[monitor b718g3gqq STAGE2_OUTPUT_READY]"),
            "event must replace fabricated `completed`: {label}"
        );
        assert!(
            !label.contains("completed"),
            "no fabricated `completed` when an event is present: {label}"
        );

        // A timed-out monitor carries the timeout notice in <event> — also surfaced, never
        // inverted to `completed`.
        let timeout = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>q9</task-id>\n<summary>Monitor tick</summary>\n<event>[Monitor timed out — re-arm if needed.]</event>\n</task-notification>"}}"#,
        );
        let label2 = timeout.automation_label().unwrap();
        assert!(label2.contains("Monitor timed out"), "got: {label2}");
        assert!(!label2.contains("completed"), "got: {label2}");

        // When BOTH status and event are absent, the label still falls back to `completed`.
        let bare = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>z</task-id>\n<summary>Monitor tick</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            bare.automation_label().unwrap(),
            "[monitor z completed] Monitor tick"
        );

        // An explicit <status> still wins over <event> (status is the more authoritative slot).
        let both = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w</task-id>\n<status>failed</status>\n<summary>Monitor tick</summary>\n<event>SOME_EVENT</event>\n</task-notification>"}}"#,
        );
        assert!(both
            .automation_label()
            .unwrap()
            .starts_with("[monitor w failed]"));
    }

    #[test]
    fn automation_kind_from_summary_direct() {
        use AutomationKind::*;
        assert_eq!(
            AutomationKind::from_summary(Some("Background command \"x\"")),
            BackgroundCommand
        );
        assert_eq!(
            AutomationKind::from_summary(Some("Dynamic workflow \"x\"")),
            Workflow
        );
        assert_eq!(
            AutomationKind::from_summary(Some("workflow run done")),
            Workflow
        );
        assert_eq!(AutomationKind::from_summary(Some("Agent x done")), Agent);
        assert_eq!(
            AutomationKind::from_summary(Some("  background command y")),
            BackgroundCommand
        );
        // The ScheduleWakeup / monitor / cron-tick family is its own labeled class — the
        // real `Monitor event: "…"` pulse (10× across the oracles) must NOT fall to `task`.
        assert_eq!(
            AutomationKind::from_summary(Some("Monitor event: \"full pulse suite re-run\"")),
            Monitor
        );
        assert_eq!(AutomationKind::from_summary(Some("Monitor tick")), Monitor);
        assert_eq!(
            AutomationKind::from_summary(Some("Scheduled wakeup fired")),
            Monitor
        );
        assert_eq!(AutomationKind::from_summary(Some("cron run")), Monitor);
        assert_eq!(AutomationKind::from_summary(Some("something else")), Task);
        assert_eq!(AutomationKind::from_summary(None), Task);
        assert_eq!(AutomationKind::from_summary(Some("")), Task);
        // Slugs round-trip.
        assert_eq!(BackgroundCommand.slug(), "background-command");
        assert_eq!(Workflow.slug(), "workflow");
        assert_eq!(Agent.slug(), "agent");
        assert_eq!(Monitor.slug(), "monitor");
        assert_eq!(Task.slug(), "task");
    }

    #[test]
    fn automation_trigger_cjk_summary_codepoint_safe() {
        // A CJK summary body must not be split mid-codepoint by the tag extractor.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>zh1</task-id>\n<status>completed</status>\n<summary>x</summary>\n</task-notification>"}}"#,
        );
        let t = r.automation_trigger().unwrap();
        assert_eq!(t.summary.as_deref(), Some("x"));
    }

    #[test]
    fn extract_xml_tag_handles_missing_and_empty() {
        assert_eq!(extract_xml_tag("<a>x</a>", "a").as_deref(), Some("x"));
        assert_eq!(extract_xml_tag("<a></a>", "a"), None); // empty inner → None
        assert_eq!(extract_xml_tag("<a>x", "a"), None); // missing close → None
        assert_eq!(extract_xml_tag("no tags here", "a"), None);
    }

    #[test]
    fn automation_label_failed_status_and_no_summary() {
        // A non-`completed` status is rendered verbatim (the status arm is not hardcoded).
        let failed = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>bad7</task-id>\n<status>failed</status>\n<summary>The build broke</summary>\n</task-notification>"}}"#,
        );
        // "The build broke" carries no kind classifier → `task` fallback.
        assert_eq!(
            failed.automation_label().as_deref(),
            Some("[task bad7 failed] The build broke")
        );
        // A trigger with a status but EMPTY summary → the head-only arm (no trailing text).
        let no_sum = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>q9</task-id>\n<status>running</status>\n<summary></summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            no_sum.automation_label().as_deref(),
            Some("[task q9 running]")
        );
        // A trigger with NO task-id and NO status → both `?`/`completed` fallbacks fire.
        let bare = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>just a note</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            bare.automation_label().as_deref(),
            Some("[task ? completed] just a note")
        );
    }

    #[test]
    fn is_synthetic_user_marker_matches_each_form() {
        assert!(is_synthetic_user_marker("[Request interrupted by user]"));
        assert!(is_synthetic_user_marker(
            "[Request interrupted by user for tool use]"
        ));
        assert!(is_synthetic_user_marker("<local-command-stdout>anything"));
        assert!(is_synthetic_user_marker("<command-name>/x</command-name>"));
        assert!(!is_synthetic_user_marker("a normal human message"));
        // Exact-match for interrupts: a longer string merely starting with the marker is
        // NOT excluded as an interrupt.
        assert!(!is_synthetic_user_marker(
            "[Request interrupted by user] and then I said more"
        ));
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
