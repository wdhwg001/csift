//! The classify engine: multi-label role.category.leaf assignment + direction.
//!
//! The engine here is the DISPATCH; the leaf families live in the children, one
//! module per shape axis, each a MECHANICAL table from an on-disk value to a leaf:
//! [`bookkeeping`] (top-level line types), [`system`] (`type:"system"` subtypes),
//! [`attachments`] (attachment payload types), [`thinking`] (thinking-block shapes)
//! and [`injections`] (the isMeta-user templates). Beside them sit [`support`] (the
//! shared predicates, re-exported from `model` so a `crate::model::` path still
//! resolves), [`channel`] (the csift-channel delivery leaf), [`resume`] (the loader's
//! resume repair pair) and [`direction`] (who was talking to whom, a separate
//! question from which leaves a record carries).

pub(super) mod attachments;
pub(super) mod bookkeeping;
pub(super) mod channel;
pub(super) mod direction;
pub(super) mod injections;
pub(super) mod resume;
pub(super) mod support;
pub(super) mod system;
pub(super) mod thinking;

use super::*;

#[allow(dead_code)]
impl Record {
    /// True when this record is ANY inbound PEER message (GOLD §1 + FINDING-2 + C-30) - a
    /// `<teammate-message>`, `<agent-message>` OR `<cross-session-message>` at a section boundary. The predicate
    /// [`Record::is_genuine_user`] EXCLUDES and [`Record::opens_turn`] INCLUDES (a peer message is
    /// not the operator, but it still delimits a turn). Reads the raw (un-normalized) message text so
    /// the relay preamble's `\n` survives; gated to `type:"user"` (the only place a peer message
    /// arrives). The body render for a peer-opened turn comes from
    /// [`Record::inbound_comm_preview`] (`turns`/`list`) / `record_text_sections` (`search`).
    #[must_use]
    pub fn is_peer_message_record(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        match self.raw_message_text() {
            Some(text) => is_peer_message(&text),
            None => false,
        }
    }

    /// The raw textual body of this message for MARKER detection - the bare string, or text
    /// blocks joined with `\n` (NOT whitespace-normalized, so `\n`-bearing markers survive).
    /// `None` when there is no message / no text. (Distinct from [`flatten_content_text`],
    /// which normalizes whitespace for display.)
    pub(crate) fn raw_message_text(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        match content {
            Content::Text(s) => Some(s.clone()),
            Content::Blocks(blocks) => {
                let parts: Vec<&str> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            }
        }
    }

    /// True when this carrier is a TEAMMATE-SPAWN ACK, not a child return (edge-fixtures
    /// correction): a persistent teammate `Agent` spawn's tool_result is an immediate
    /// `toolUseResult.status == "teammate_spawned"` acknowledgement - the teammate's actual
    /// work returns LATER as inbound `<teammate-message>`s, never via this tool_result. So an
    /// ACK is `agent.tool.result` ONLY, never `…inbox` (unlike a one-shot Task return).
    pub(crate) fn is_teammate_spawn_ack(&self) -> bool {
        self.tur_probe()
            .as_ref()
            .and_then(|p| p.status.as_ref())
            .and_then(serde_json::Value::as_str)
            == Some("teammate_spawned")
    }

    /// True when this carrier is an ASYNC-LAUNCH ACK, not a child return (smoke-found bug): an
    /// ASYNC/background `Agent` spawn's tool_result is the immediate launch confirmation
    /// (`"Async agent launched successfully.\nagentId: …"`), shaped on disk as
    /// `toolUseResult.{isAsync:true, status:"async_launched"}`. It shares the spawn
    /// `tool_use_id`, so the [`SpawnLookup`] WOULD resolve it - but it is the LAUNCH ack, not
    /// the work product. The async child's real report arrives LATER via the
    /// `<task-notification>` `<result>` pulse (G1 → `agent.communication.inbox`), never via this
    /// tool_result. So a launch ack is `agent.tool.result` ONLY (unlike a SYNC one-shot Task
    /// return, which IS the child's reply → `…inbox`). Robust dual detection: the structured
    /// `toolUseResult` shape first, then the content prefix ([`ASYNC_LAUNCH_ACK_PREFIX`]) for a
    /// record lacking the structured field.
    pub(crate) fn is_async_launch_ack(&self) -> bool {
        if let Some(probe) = self.tur_probe() {
            if probe.status.as_ref().and_then(serde_json::Value::as_str) == Some("async_launched")
                || probe.is_async.as_ref().and_then(serde_json::Value::as_bool) == Some(true)
            {
                return true;
            }
        }
        self.blocks().is_some_and(|blocks| {
            blocks.iter().any(|b| match b {
                Block::ToolResult {
                    content: Some(c), ..
                } => tool_result_content_text(c).starts_with(ASYNC_LAUNCH_ACK_PREFIX),
                _ => false,
            })
        })
    }

    /// True when this carrier is a spawn LAUNCH ACK rather than a child RETURN - either a
    /// persistent teammate spawn ([`Record::is_teammate_spawn_ack`]) or an async/background
    /// `Agent` launch ([`Record::is_async_launch_ack`]). Both share the spawn `tool_use_id`
    /// (so the [`SpawnLookup`] would resolve them) yet are the launch confirmation, not the
    /// work product → `agent.tool.result` ONLY, never `…inbox`/a child ⇨ self direction.
    pub(crate) fn is_spawn_launch_ack(&self) -> bool {
        self.is_teammate_spawn_ack() || self.is_async_launch_ack()
    }

    /// True when this record is a SUBAGENT RETURN (GOLD §3) - a tool_result whose
    /// `tool_use_id` the spawn lookup resolves to a spawned child (the Task tool_result of a
    /// ONE-SHOT spawn = the child's return, child ⇨ self). `false` without a [`SpawnLookup`]
    /// in `ctx`, AND `false` for a spawn LAUNCH ACK ([`Record::is_spawn_launch_ack`] - teammate
    /// or async) - the ACK shares the spawn `tool_use_id` so the lookup WOULD resolve it, but it
    /// is not a return.
    pub(crate) fn is_subagent_return(&self, ctx: &ClassifyCtx) -> bool {
        let Some(spawn) = ctx.spawn else {
            return false;
        };
        if self.is_spawn_launch_ack() {
            return false;
        }
        let Some(blocks) = self.blocks() else {
            return false;
        };
        blocks.iter().any(|b| match b {
            Block::ToolResult {
                tool_use_id: Some(id),
                ..
            } => spawn.child_for_spawn_tool_use_id(id).is_some(),
            _ => false,
        })
    }

    /// Classify this record into ALL applicable leaf [`Class`]es (multi-label, deduped,
    /// richest-first order). Pure + tolerant + no `unwrap` - a record whose shape csift
    /// models no leaf for yields an empty `Vec`. Cross-record facts come from `ctx`
    /// (see [`ClassifyCtx`]).
    ///
    /// The dispatch order is fixed and each step is exhaustive for its own shape axis:
    /// the elicitation sidecar, then the csift CHANNEL and the rest of the attachment
    /// payload types, then the non-transcript line types (system subtypes included),
    /// then the three transcript record types.
    #[must_use]
    pub fn classify(&self, ctx: &ClassifyCtx) -> Vec<Class> {
        let mut out: Vec<Class> = Vec::new();

        // Elicitation sidecar markers (§3.10): a PENDING marker stands in for the native
        // tool_use CC has not yet written → agent.tool.use (covers the MCP `system` form too,
        // which carries no tool_use block). A RESOLVED marker is a pairing artifact → no label.
        if self.is_elicitation_marker() {
            if self.csift_phase.as_deref() == Some("pending") {
                push_unique(&mut out, Class::AgentToolUse);
            }
            return out;
        }

        // Hook-injected additionalContext (a `type:"attachment"` record): the injected
        // text the model reads - `harness.hook.context`, LED by
        // `agent.communication.channel` when that text is a csift-channel delivery. The
        // channel view is one of THREE attachment leaves a default scan reaches; the record
        // never opens a turn either way.
        if self.hook_additional_context_text().is_some() {
            self.push_hook_classes(&mut out);
            return out;
        }

        // Every OTHER `type:"attachment"` record, by its payload's own `type` value
        // (`classify/attachments.rs`). A `queued_command` the HUMAN queued mid-turn is
        // the person's message rather than harness context, and the PAYLOAD's `origin`
        // - not its type - is what says so.
        //
        // ONE parse answers both questions. Reading the type through `attachment_type()`
        // and the origin through a second accessor would parse the same bytes twice, and
        // a queued payload carries the operator's whole typed prompt.
        if self.is_type("attachment") {
            if let Some(payload) = self.attachment_value() {
                if let Some(ty) = payload.get("type").and_then(serde_json::Value::as_str) {
                    if ty == "queued_command" && attachments::payload_origin_is_human(&payload) {
                        push_unique(&mut out, Class::UserMessage);
                        return out;
                    }
                    if let Some(c) = attachments::classify(ty) {
                        push_unique(&mut out, c);
                    }
                }
            }
            return out;
        }

        // A NON-TRANSCRIPT line: a bookkeeping line type, a `type:"system"` subtype, or
        // the human's own queued text (`classify/bookkeeping.rs` + `classify/system.rs`).
        // Every leaf below is gated - a selector has to name it - except the compaction
        // boundary, which a match-all scan reaches.
        if !self.is_type("assistant") && !self.is_type("user") {
            if let Some(c) = self.promoted_class() {
                push_unique(&mut out, c);
            }
            return out;
        }

        match self.r#type.as_deref() {
            // The loader's resume repair PLACEHOLDER wears an assistant record's clothes but
            // carries the `<synthetic>` model sentinel: it is harness text, so it takes the
            // harness leaf INSTEAD of the per-block agent classes (`agent.message.plain`
            // would claim the model wrote it). Tested before the block walk, never inside it.
            Some("assistant") if self.is_resume_placeholder() => {
                push_unique(&mut out, Class::ResumePlaceholder);
            }
            Some("assistant") => self.classify_assistant(&mut out),
            Some("user") => self.classify_user(ctx, &mut out),
            _ => {}
        }
        out
    }

    /// Classify an `assistant` record's blocks: visible text → `agent.message.plain`;
    /// a thinking-family block → its own leaf (`classify/thinking.rs`); tool_use →
    /// `agent.tool.use` (+ `…sent`/`…signal` for a `SendMessage`/spawn); tool_result
    /// (rare on assistant) → `agent.tool.result`.
    pub(crate) fn classify_assistant(&self, out: &mut Vec<Class>) {
        let Some(blocks) = self.blocks() else {
            // A bare-string assistant body (rare) is a visible message (§ `agent_text`).
            if self.agent_text().is_some() {
                push_unique(out, Class::AgentMessagePlain);
            }
            return;
        };
        for b in blocks {
            match b {
                Block::Text { text } if !text.trim().is_empty() => {
                    push_unique(out, Class::AgentMessagePlain);
                }
                Block::Text { .. } => {}
                Block::Thinking { .. } | Block::RedactedThinking { .. } => {
                    if let Some(c) = thinking::classify(b) {
                        push_unique(out, c);
                    }
                }
                Block::ToolUse { name, input, .. } => {
                    push_unique(out, Class::AgentToolUse);
                    match name.as_deref() {
                        Some("SendMessage") => {
                            if send_message_is_signal(input.as_ref()) {
                                push_unique(out, Class::CommSignal);
                            } else {
                                push_unique(out, Class::CommSent);
                            }
                        }
                        Some(n) if is_spawn_tool_name(n) => push_unique(out, Class::CommSent),
                        _ => {}
                    }
                }
                Block::ToolResult { .. } => push_unique(out, Class::AgentToolResult),
                // An IMAGE block on an ASSISTANT record carries no leaf. `harness.context
                // .image` is a SHAPE the harness writes - the isMeta image-only user
                // record, and the `[Image: ` reference in its text form - not a property
                // of the block wherever it lands. Giving the block its own leaf made a
                // text+image turn emit TWO hits with the harness view first, while
                // `labels[]` ordered richest-first put the plain message first: the
                // emission order and the label order disagreeing about one record. The
                // corpus has no subject for it either - an image block rides an assistant
                // record 0 times over two projects roots - so an assistant record with
                // text stays `agent.message.plain`, and an image-ONLY one is an unmodeled
                // unit under an address.
                _ => {}
            }
        }
        // The `fallback` block - what the API writes in place of the text when it served
        // the turn from a fallback model - is the assistant's TURN, so the record takes
        // the plain message leaf with an empty body; naming the fallback in the render is
        // a family lane's work, not the engine's.
        //
        // The test is the BLOCK, never "nothing else matched". A blanket arm answers for
        // every OTHER leafless shape an assistant record can carry - a `document` block,
        // which the inventory gives no leaf, and a whitespace-only text block - and
        // silently overrides those rows.
        if out.is_empty() && blocks.iter().any(|b| matches!(b, Block::Fallback { .. })) {
            push_unique(out, Class::AgentMessagePlain);
        }
    }

    /// Classify a `user` record (GOLD §2/§3): compaction summary (by the `isCompactSummary`
    /// FLAG, not text - G9); BATCHED mixed-family sections - `<task-notification>` pulse(s) and/or
    /// inbound peer message(s) `<teammate-message>` / `<agent-message>` (the §1 fix, the G4/G5
    /// union, and P1c M1/M3 cross-family/precedence, via [`classify_batched_sections`]); then the
    /// string-content vs block-content sub-cases.
    pub(crate) fn classify_user(&self, ctx: &ClassifyCtx, out: &mut Vec<Class>) {
        if self.is_compact_summary.unwrap_or(false) {
            push_unique(out, Class::CompactionSummary);
            return;
        }
        // BATCHED mixed-family sections: a `<task-notification>` automation pulse and/or an
        // inbound peer message can be concatenated in ONE record. Scan ALL sections and UNION
        // their labels, with notification precedence over a peer tag quoted inside a
        // notification span (P1c M3). When ≥1 section matches, that fully classifies the record.
        if let Some(raw) = self.raw_message_text() {
            if classify_batched_sections(&raw, self.is_meta.unwrap_or(false), out) {
                return;
            }
        }
        match self.message.as_ref().and_then(|m| m.content.as_ref()) {
            Some(Content::Text(s)) => self.classify_user_string(ctx, s, out),
            Some(Content::Blocks(blocks)) => self.classify_user_blocks(ctx, blocks, out),
            None => {}
        }
    }

    /// Classify the string body of a `user` record (also reused for the joined text of a
    /// no-tool_result block record): the harness markers (interrupts, `<local-command-stdout>`,
    /// `<command-name>`, schedule ticks, meta hook/loop), else genuine prose - or
    /// `agent.communication.inbox` when this is a subagent transcript opener (parent ⇨ self).
    /// (Batched `<task-notification>` / peer-message sections are handled UPSTREAM by
    /// [`classify_batched_sections`], so they never reach here.)
    pub(crate) fn classify_user_string(&self, ctx: &ClassifyCtx, s: &str, out: &mut Vec<Class>) {
        if s == INTERRUPT_MARKERS[0] {
            push_unique(out, Class::InterruptUser);
            return;
        }
        if s == INTERRUPT_MARKERS[1] {
            push_unique(out, Class::InterruptTool);
            return;
        }
        if s.starts_with(LOCAL_COMMAND_STDOUT_PREFIX) {
            push_unique(out, Class::CommandStdout);
            return;
        }
        if is_slash_command_wrapper(s) {
            // Prose typed after the slash command (`<command-args>`) IS genuine user
            // input - and the RICHER view (richest-view law: the prose beats the
            // wrapper), so it is pushed FIRST: the unfiltered record-text emission
            // renders `/name args`, never the wrapper XML. An explicit
            // `-t harness.command.invocation` still reaches the wrapper form.
            if self.slash_command_args().is_some() {
                push_unique(out, Class::UserMessage);
            }
            push_unique(out, Class::CommandInvocation);
            return;
        }
        // The isMeta-USER INJECTION templates (`classify/injections.rs`): the resume
        // repair prompt, the two schedule ticks, the stop-hook feedback, the local-command
        // caveat and the edit-retry nudge. One module owns their order and their
        // content-start anchoring; `None` means the text matched no template csift models.
        if let Some(c) = injections::classify(self, s) {
            push_unique(out, c);
            return;
        }
        // The harness's agents-stopped notice (v0.10.0): a kill notice, not the human.
        if is_agents_stopped_notice(s) {
            push_unique(out, Class::NotificationSubagent);
            return;
        }
        // The spawn-prompt seed of a subagent transcript is an inbound comm (parent ⇨ self),
        // not the operator (GOLD §3) - unchanged, regardless of isMeta.
        if ctx.is_subagent && ctx.is_transcript_opener {
            push_unique(out, Class::CommInbox);
            return;
        }
        // harness.schedule.fire: the PROMPT a scheduled task fired. LAST of the harness arms
        // by construction - the fired text is whatever the operator armed, so every marker
        // above must get first refusal, and the two tick prompts that DO carry a marker keep
        // their own leaves. Before this arm such a record matched nothing and fell straight
        // into the isMeta exclusion below: unsearchable, and absent from every census.
        if self.is_scheduled_fire_prompt(ctx) {
            push_unique(out, Class::ScheduleFire);
            return;
        }
        // M2b ROOT FIX: a genuine `user.message` is NEVER isMeta. An isMeta record that matched
        // no marker above is a harness-injected pseudo-turn (a novel hook wrapper), NOT the
        // operator - emit NOTHING rather than mislabel it `user.message` (the role-level isMeta
        // gate `is_genuine_user` already applies). Only genuine, non-isMeta unmarked prose is
        // `user.message`.
        if !self.is_meta.unwrap_or(false) {
            push_unique(out, Class::UserMessage);
        }
    }

    /// Classify a block-content `user` record (GOLD §3). A tool_result carrier →
    /// `agent.tool.result`, plus the dual labels (`user.answer` for an AUQ answer,
    /// `user.rejection` for a typed rejection, `agent.communication.inbox` for a subagent
    /// return). A no-tool_result block record is routed through the string classifier on its
    /// joined text (interrupt markers / genuine prose can ride on a text block).
    pub(crate) fn classify_user_blocks(
        &self,
        ctx: &ClassifyCtx,
        blocks: &[Block],
        out: &mut Vec<Class>,
    ) {
        let has_tool_result = blocks.iter().any(|b| matches!(b, Block::ToolResult { .. }));
        if has_tool_result {
            // Order follows the GOLD §3 table (label A then label B): the user-facing dual
            // label (`user.answer`/`user.rejection`) leads its carrier; a subagent return
            // leads with its `agent.tool.result` base then the `…inbox` comm view. These
            // dual-label shapes are mutually exclusive, so an if/else chain is deterministic.
            if self.is_auq_answer_boundary() {
                push_unique(out, Class::UserAnswer);
                push_unique(out, Class::AgentToolResult);
            } else if self.is_plan_rejection_boundary() {
                push_unique(out, Class::UserRejection);
                push_unique(out, Class::AgentToolResult);
            } else if self.is_subagent_return(ctx) {
                push_unique(out, Class::AgentToolResult);
                push_unique(out, Class::CommInbox);
            } else {
                push_unique(out, Class::AgentToolResult);
            }
            return;
        }
        let joined = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if joined.trim().is_empty() {
            // harness.context.image: an image-ONLY `type:"user"` record - the block form
            // of the attached image the `[Image: ` reference names in text form, and the
            // shape the inventory's `Block("image")` row records. Measured over two
            // projects roots, every such record is isMeta (94 and 75) and every
            // non-isMeta record carrying an image carries the person's text beside it
            // (802 and 539), which is why the leaf is a statement about THIS shape and
            // not about an image block wherever it lands. Any other textless block
            // record (a `document`, a bare `fallback`) still carries nothing, which is
            // what sends it to the unmodeled unit.
            if blocks.iter().any(|b| matches!(b, Block::Image { .. })) {
                push_unique(out, Class::ContextImage);
            }
            return;
        }
        self.classify_user_string(ctx, &joined, out);
    }

    /// The per-section record-level text emissions of a BATCHED `type:"user"` record (≥1
    /// `<task-notification>` and/or inbound peer `<teammate-message>` / `<agent-message>` section)
    /// -- GOLD §3 G4/G5 per-section render. One [`RecordTextSection`] per section's label, MIRRORING
    /// [`classify_batched_sections`] EXACTLY (same notification-span precedence/masking) so the text
    /// render never drifts from the classification: each `<task-notification>` yields its
    /// `harness.notification.<kind>` (text = the per-section automation label) PLUS, when it carries
    /// a `<result>` (G1), an `agent.communication.inbox` section (child ⇨ self via the embedded
    /// `<tool-use-id>`, degrading to `?` without a [`SpawnLookup`]); each inbound peer section
    /// outside every notification span yields `agent.communication.{inbox,signal}` (sender ⇨ self).
    /// EMPTY when the record carries no such section - the caller then falls back to the single
    /// richest-label record-text emission.
    #[must_use]
    pub fn record_text_sections(&self, ctx: &ClassifyCtx) -> Vec<RecordTextSection> {
        let mut out: Vec<RecordTextSection> = Vec::new();
        let Some(raw) = self.raw_message_text() else {
            return out;
        };
        let owner = || ctx.owner_id.unwrap_or("self").to_string();
        // (a) <task-notification> sections (+ the G1 inbox view of a <result>-bearing pulse),
        //     recording each span to mask a peer tag quoted inside it.
        let mut notif_spans: Vec<(usize, usize)> = Vec::new();
        scan_tag_sections(
            &raw,
            TASK_NOTIFICATION_PREFIX,
            TASK_NOTIFICATION_CLOSE,
            self.is_meta.unwrap_or(false),
            |offset, section| {
                let kind =
                    AutomationKind::from_summary(extract_xml_tag(section, "summary").as_deref());
                let label = automation_label_for_section(section);
                out.push(RecordTextSection {
                    class: notification_class(kind),
                    text: label.clone(),
                    direction: None,
                    // Every task THIS section closes, plus the orphan-reconciliation kind
                    // when it is one - the structured form of the ids the label renders.
                    task_ids: section_task_ids(section),
                });
                if section.contains(NOTIFICATION_RESULT_TAG) {
                    let child = extract_xml_tag(section, "tool-use-id")
                        .and_then(|id| ctx.spawn.and_then(|sp| sp.child_for_spawn_tool_use_id(&id)))
                        .unwrap_or_else(|| "?".to_string());
                    // The inbox view excerpts the child's REPORT (the <result> body); fall back to
                    // the attribution label when the body is absent/empty.
                    let report = extract_xml_tag(section, "result")
                        .map(|r| normalize_line(&r))
                        .filter(|r| !r.is_empty())
                        .unwrap_or(label);
                    out.push(RecordTextSection {
                        class: Class::CommInbox,
                        text: report,
                        direction: Some((child, owner())),
                        task_ids: TaskIds::default(),
                    });
                }
                notif_spans.push((offset, offset + section.len()));
            },
        );
        // (b) inbound peer sections OUTSIDE every notification span (precedence + cross-family).
        let peers = parse_all_peer_sections(&raw);
        let single = peers.len() == 1;
        for peer in &peers {
            if notif_spans
                .iter()
                .any(|&(s, e)| peer.offset >= s && peer.offset < e)
            {
                continue;
            }
            out.push(RecordTextSection {
                class: if peer.is_signal {
                    Class::CommSignal
                } else {
                    Class::CommInbox
                },
                text: peer_render_body(self, peer, single),
                direction: Some((peer_sender(self, peer, single), owner())),
                task_ids: TaskIds::default(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests;
