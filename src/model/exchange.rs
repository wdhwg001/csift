//! AUQ exchange reconstruction, plan rejection, opens_turn, reconstructed user text.

use super::*;

impl Record {
    /// Reconstruct the COMPLETE AskUserQuestion exchange (§4.4) as one genuine-user unit:
    /// `[AskUserQuestion · N questions]` followed by, per question, the header, the
    /// question, each option WITH its description (supplementary note), the user's answer, and any
    /// free-text `annotations.notes` attached to that answer (the `"(notes only)"` path,
    /// where the user's real message lives - never dropped). Built from the structured
    /// `toolUseResult.questions[]` zipped with `toolUseResult.answers{}` + `.annotations{}`;
    /// falls back to the synthesized `tool_result` string (parsed for `"<q>"="<a>"`) when
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
        // Parse the raw blob ONCE here (this runs only on an actual answered-AUQ carrier)
        // and read answers/annotations/questions from the shared local tree.
        let tur = self.tool_use_result_value();
        let answers = tur
            .as_ref()
            .and_then(|t| t.get("answers"))
            .and_then(serde_json::Value::as_object)
            .filter(|m| !m.is_empty());
        if let Some(answers) = answers {
            let questions = tur
                .as_ref()
                .and_then(|t| t.get("questions"))
                .and_then(serde_json::Value::as_array);
            // `annotations` map (§4.4) - per-question `{notes?, preview?}`; when the answer
            // is the `"(notes only)"` placeholder the user's ENTIRE real message lives here.
            let annotations = tur
                .as_ref()
                .and_then(|t| t.get("annotations"))
                .and_then(serde_json::Value::as_object)
                .filter(|m| !m.is_empty());
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
                    // Each option is a (label, description) pair - BOTH surfaced; the
                    // description (supplementary note) is the per-option detail the user wants kept,
                    // and is what was being dropped (only the label survived).
                    let opts: Vec<(String, Option<String>)> = q
                        .get("options")
                        .and_then(serde_json::Value::as_array)
                        .map(|os| {
                            os.iter()
                                .filter_map(|o| {
                                    let label =
                                        o.get("label").and_then(serde_json::Value::as_str)?;
                                    let desc = o
                                        .get("description")
                                        .and_then(serde_json::Value::as_str)
                                        .filter(|s| !s.is_empty())
                                        .map(str::to_string);
                                    Some((label.to_string(), desc))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    // The answer is keyed by the (verbatim) question string in `answers`.
                    let answer = answers
                        .get(question)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    // Free-text notes the user attached to THIS answer. When the answer is
                    // the `"(notes only)"` placeholder, the notes ARE the user's message -
                    // dropping them silently swallowed the whole turn (the common path,
                    // since the user routinely answers AUQs with typed prose, not a click).
                    let note = annotations
                        .and_then(|a| a.get(question))
                        .and_then(|v| v.get("notes"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.is_empty());
                    out.push_str(&format!("\nQ{} ", i + 1));
                    if let Some(h) = header {
                        out.push_str(&format!("({}): ", normalize_line(h)));
                    }
                    out.push_str(&normalize_line(question));
                    for (label, desc) in &opts {
                        out.push_str(&format!("\n  - {}", normalize_line(label)));
                        if let Some(d) = desc {
                            out.push_str(&format!(": {}", normalize_line(d)));
                        }
                    }
                    out.push_str(&format!("\nA{}: {}", i + 1, normalize_line(answer)));
                    if let Some(n) = note {
                        out.push_str(&format!("\n   note: {}", normalize_line(n)));
                    }
                }
            } else {
                // No questions[] array (rare): list the answers map directly, still
                // surfacing any notes attached to each answer.
                for (i, (q, a)) in answers.iter().enumerate() {
                    out.push_str(&format!(
                        "\nQ{}: {}\nA{}: {}",
                        i + 1,
                        normalize_line(q),
                        i + 1,
                        normalize_line(a.as_str().unwrap_or_default())
                    ));
                    if let Some(n) = annotations
                        .and_then(|an| an.get(q))
                        .and_then(|v| v.get("notes"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        out.push_str(&format!("\n   note: {}", normalize_line(n)));
                    }
                }
            }
            return Some(out);
        }
        // Fallback path: the synthesized marker string is the whole exchange.
        self.auq_answer_marker_text()
            .map(|t| format!("[AskUserQuestion] {}", normalize_line(&t)))
    }

    /// The synthesized AUQ-answer string from this carrier's `tool_result` (§4.4) - the
    /// fallback content when `toolUseResult.answers` is absent. `None` if no AUQ marker.
    pub(crate) fn auq_answer_marker_text(&self) -> Option<String> {
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
    /// (the `STOP what you are doing and wait…` form - the user clicked reject but typed
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
    /// message), a tool-use rejection carrying a typed user instruction, OR an inbound
    /// teammate/peer message. Every surface (turns / search / recover / files) keys turn
    /// delimiting on THIS predicate so they never drift.
    ///
    /// GOLD §1 + FINDING-2: an inbound PEER message (`<teammate-message>` OR `<agent-message>`) is
    /// no longer [`Record::is_genuine_user`] (it is a peer, not the operator), but it MUST still
    /// delimit a turn - so the dedicated [`Record::is_peer_message_record`] clause keeps `opens_turn`
    /// firing for peer records (true before and after the fix for the non-isMeta teammate/agent
    /// forms), leaving turn grouping byte-identical where peers already opened turns while the `user`
    /// mislabel is removed.
    #[must_use]
    pub fn opens_turn(&self) -> bool {
        self.is_genuine_user()
            || self.is_auq_answer_boundary()
            || self.is_plan_rejection_boundary()
            || self.is_peer_message_record()
    }

    /// The rendered genuine-user text for any boundary-opening record, normalized to a
    /// single line - the unified opener body used by `turns` / `search` / `list` /
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
        // input - surface it as `/name args` so `search -t user` still finds it within
        // its turn and the wrapper XML never masquerades as prose. Prefilter/gate note:
        // both the name and the args are VERBATIM raw-line substrings, and the seam
        // between them is a space (a whitespace-bearing pattern is never
        // prefilter-eligible), so no synth needle is needed for this render.
        if let Some(args) = self.slash_command_args() {
            return Some(match self.slash_command_name() {
                Some(name) => format!("{name} {args}"),
                None => args,
            });
        }
        // GOLD §1: an inbound TEAMMATE message opens a turn but is NOT genuine-user, so the
        // genuine-user arm above no longer yields its body. Render the message text here so a
        // teammate-opened turn is not BLANK - preserving the exact text `turns`/`search`/`list`
        // produced before the `is_genuine_user` fix. This stays TEAMMATE-specific on purpose: the
        // `<agent-message>` peer form (FINDING-2) opens a turn too, but every surface that renders an
        // opener body catches it FIRST via [`Record::inbound_comm_preview`] (`turns`/`list`) or
        // `record_text_sections` (`search`), and `list` deliberately keeps an `<agent-message>`
        // INELIGIBLE to front a preview (session.rs `preview_text`) - so widening this arm would only
        // change that decision, never prevent a blank.
        if self.is_teammate_message_record() {
            if let Some(content) = self.message.as_ref().and_then(|m| m.content.as_ref()) {
                return Some(flatten_content_text(content));
            }
        }
        None
    }

    /// The ExitPlanMode tool_use blocks carried by this (assistant) record, as
    /// `(tool_use_id, plan_file_path)` pairs - the raw material a [`PlanIndex`] is built
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
    /// structured `toolUseResult.persistedOutputPath` (exact - no regex) and falling
    /// back to scraping the inline `Full output saved to: <path>` marker from a
    /// `tool_result` block. Returns `None` when there is no persisted pointer.
    #[must_use]
    pub fn persisted_output_path(&self) -> Option<String> {
        // Structured field first (SPEC §4.6 resolution rule).
        if let Some(probe) = self.tur_probe() {
            if let Some(p) = probe
                .persisted_output_path
                .as_ref()
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

    /// Plain-text rendering of the assistant's VISIBLE end-of-turn message - the
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
            // would be a genuine surprise - surface it rather than silently drop.
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
