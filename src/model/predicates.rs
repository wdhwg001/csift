//! Record predicates: genuine-user, slash wrappers, automation labels, attachments.

use super::*;

impl Record {
    /// True when this record is `type == "<t>"`.
    #[must_use]
    pub fn is_type(&self, t: &str) -> bool {
        self.r#type.as_deref() == Some(t)
    }

    /// True when this record is a csift ELICITATION-SIDECAR marker (§3.10) - it carries
    /// `csift:"elicitation-marker-v1"`. Distinguishes a hook-backfilled record (merged
    /// into search/turns/list) from a native CC transcript record.
    #[must_use]
    pub fn is_elicitation_marker(&self) -> bool {
        self.csift.as_deref() == Some(ELICITATION_MARKER)
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
        // `isSidechain:true` user seed. It is NOT gated out here on purpose - `list`'s
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
        // GOLD §1 BUG FIX + FINDING-2: an inbound PEER message - `<teammate-message>` OR
        // `<agent-message from="…">` - is `type:user`/`role:user`/string content and matches no
        // synthetic marker, so it used to slip through as a GENUINE HUMAN turn (106 peer messages
        // mislabeled as the user in one real session). Both are PEER-AGENT messages - never the
        // operator. Excluded here via [`is_peer_message`] (each still OPENS a turn via
        // [`Record::opens_turn`], but classifies `agent.communication.inbox`, not `user`). The check
        // is on the borrowed content (no allocation on the common path) and is BOUNDARY-anchored
        // (FINDING-1), so a genuine message merely QUOTING the tag stays `user.message`.
        match &msg.content {
            Some(Content::Text(s)) => !is_synthetic_user_marker(s) && !is_peer_message(s),
            Some(Content::Blocks(blocks)) => {
                let has_tool_result = blocks.iter().any(|b| matches!(b, Block::ToolResult { .. }));
                let has_text = blocks.iter().any(|b| matches!(b, Block::Text { .. }));
                if !has_text || has_tool_result {
                    return false;
                }
                // §4.2.1: an interrupt marker arrives as a single `text` block whose text
                // is EXACTLY the marker - exclude it (exact match, codepoint-safe).
                let joined = flatten_content_text(msg.content.as_ref().unwrap());
                !is_synthetic_user_marker(&joined) && !is_peer_message(&joined)
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
        if !is_slash_command_wrapper(s) {
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

    /// The invoked slash command's name (e.g. `/csift`) from the wrapper's
    /// `<command-name>…</command-name>` tag - EITHER tag order. `None` when the record
    /// is not a slash-command wrapper or the tag is absent/empty. Same codepoint-safety
    /// as [`Record::slash_command_args`] (slices only on ASCII tag offsets).
    #[must_use]
    pub fn slash_command_name(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !is_slash_command_wrapper(s) {
            return None;
        }
        const OPEN: &str = "<command-name>";
        const CLOSE: &str = "</command-name>";
        let start = s.find(OPEN)? + OPEN.len();
        let end = s[start..].find(CLOSE).map_or(s.len(), |rel| start + rel);
        let name = s[start..end].trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// Classify this record as a MACHINE-INJECTED automation trigger, if it is one.
    ///
    /// A `<task-notification>` record is a `type:"user"`, non-`isMeta`, STRING-content
    /// record CC inserts when a background command / spawned task / workflow completes. It
    /// passes every [`Record::is_genuine_user`] gate (so it opens a turn like a human
    /// message), but it is an automation pulse - surfacing its raw `<task-id>` /
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
        // `STAGE2_OUTPUT_READY`, `[Monitor timed out - re-arm if needed.]`) and frequently have
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
    /// `workflow` / `agent` / fallback `task`) - NOT the hardcoded literal `workflow` that
    /// mislabeled 81% of triggers on a captured session (85 background-command + 2 agent). A missing
    /// field is elided gracefully. This is what `turns` / `search` render as the segment
    /// opener in place of the raw `<task-notification>` XML blob.
    #[must_use]
    pub fn automation_label(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        // The agents-stopped kill notice (no XML, no id): `[subagent stopped] <notice>`.
        if is_agents_stopped_notice(s) {
            return Some(format!("[subagent stopped] {}", normalize_line(s)));
        }
        if !s.starts_with(TASK_NOTIFICATION_PREFIX) {
            return None;
        }
        Some(automation_label_for_section(s))
    }

    /// Plain-text rendering of a GENUINE user message for the `list`/`search`
    /// excerpt: the raw string, or the concatenation of all `text` blocks. Returns
    /// `None` for non-user / non-genuine records. Whitespace-normalized to a single
    /// line (callers truncate explicitly - never silently).
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
    /// (any marker in `AUQ_ANSWER_MARKERS`, §4.4 - the three shipped phrasings; the
    /// unanswered branch is never one). Such a record is surfaced under the
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
    /// `is_error` is not true, AND it is a real answer - signalled by a non-empty
    /// `toolUseResult.answers` object (the clean, structured source) OR, as a fallback
    /// for an older record without `toolUseResult`, the synthesized AUQ-answer marker in
    /// the tool_result content. The answer is a genuine USER message (the user's
    /// selection + prose reasoning), so it is a turn boundary.
    ///
    /// A CANCELLED / rejected / validation-errored AUQ (no `answers`, `is_error:true`,
    /// or a `Cancelled…` / `<tool_use_error>…` body) is NOT a boundary - those carry no
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
        if self.has_auq_answers() {
            return true;
        }
        // Fallback (older records without `toolUseResult`): the synthesized marker.
        self.is_auq_answer()
    }

    /// Parse the raw `toolUseResult` blob into a full `Value` tree ON DEMAND - for the
    /// few DEEP consumers (`recover`'s event extraction, the AUQ exchange
    /// reconstruction). Each call re-parses, so a caller needing several reads parses
    /// once and shares the local value. `None` when absent or unparseable (the raw text
    /// was validated as part of the line's JSON, so unparseable never happens in
    /// practice - the guard is tolerance, not control flow).
    #[must_use]
    pub fn tool_use_result_value(&self) -> Option<serde_json::Value> {
        let raw = self.tool_use_result.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// Parse the raw `attachment` blob into a full `Value` tree ON DEMAND (`recover`'s
    /// external-edit/file-snapshot reader, `plan`'s `plan_mode` binding reader).
    #[must_use]
    pub fn attachment_value(&self) -> Option<serde_json::Value> {
        let raw = self.attachment.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// The attachment payload's own `type` value (`hook_additional_context`,
    /// `edited_text_file`, `compact_file_reference`, …) - the `--count-by attachment`
    /// census key. `None` off a non-attachment record or when the payload has no type.
    #[must_use]
    pub fn attachment_type(&self) -> Option<String> {
        if !self.is_type("attachment") {
            return None;
        }
        let v = self.attachment_value()?;
        v.as_object()?
            .get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    /// The VERBATIM payload JSON of a `type:"attachment"` record - the matchable/rendered
    /// text under `search --attachments` for every non-hook payload. Verbatim by design:
    /// the text is a byte substring of the source line, so the §7d literal prefilter and
    /// the §7f whole-file gate stay sound with no synthesized-marker machinery.
    #[must_use]
    pub fn attachment_payload_text(&self) -> Option<String> {
        if !self.is_type("attachment") {
            return None;
        }
        self.attachment.as_ref().map(|raw| raw.get().to_string())
    }

    /// Hook-injected `additionalContext` text: a `type:"attachment"` record whose payload is
    /// `{"type":"hook_additional_context","content":[…],…}` - the context a SessionStart /
    /// UserPromptSubmit / … hook injected into the turn. `content` is a string ARRAY in real
    /// data (one element per injected block; joined with `\n`), tolerated as a bare string.
    /// `None` for every other record shape. Cheap for non-attachment records (one type
    /// compare before any parse).
    #[must_use]
    pub fn hook_additional_context_text(&self) -> Option<String> {
        if !self.is_type("attachment") {
            return None;
        }
        let v = self.attachment_value()?;
        let att = v.as_object()?;
        if att.get("type").and_then(serde_json::Value::as_str) != Some("hook_additional_context") {
            return None;
        }
        match att.get("content")? {
            serde_json::Value::String(s) => {
                let s = s.trim();
                (!s.is_empty()).then(|| s.to_string())
            }
            serde_json::Value::Array(parts) => {
                let texts: Vec<&str> = parts.iter().filter_map(serde_json::Value::as_str).collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            }
            _ => None,
        }
    }

    /// Cheap typed probe of the SMALL `toolUseResult` fields the hot paths consult -
    /// deserializing it skips the huge content values (file bodies, stdout echoes)
    /// without allocating them. `None` when there is no `toolUseResult` or it is not a
    /// JSON object (e.g. a subagent's bare-string echo) - exactly the cases where every
    /// former `.get(…)` probe answered `None` too.
    pub(crate) fn tur_probe(&self) -> Option<TurProbe> {
        let raw = self.tool_use_result.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// The structured `toolUseResult.answers` object test (§4.4): present AND non-empty.
    /// `false` for a cancelled/rejected AUQ (no answers) or a non-AUQ carrier.
    pub(crate) fn has_auq_answers(&self) -> bool {
        self.tur_probe()
            .as_ref()
            .and_then(|p| p.answers.as_ref())
            .and_then(serde_json::Value::as_object)
            .is_some_and(|m| !m.is_empty())
    }
}
