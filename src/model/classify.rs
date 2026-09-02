//! The classify engine: multi-label role.class.sub assignment + direction.

use super::*;

#[allow(dead_code)]
impl Record {
    /// True when this is the `system`/`compact_boundary` metrics record (GOLD §5) -
    /// `harness.compaction.boundary`.
    #[must_use]
    pub fn is_compact_boundary(&self) -> bool {
        self.is_type("system") && self.subtype.as_deref() == Some("compact_boundary")
    }

    /// True when this record is ANY inbound PEER message (GOLD §1 + FINDING-2) - a
    /// `<teammate-message>` OR `<agent-message>` at a section boundary. The predicate
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

    /// Classify this record into ALL applicable leaf [`Class`]es (GOLD §3, multi-label,
    /// deduped, richest-first order). Pure + tolerant + no `unwrap` - an unmodeled record
    /// yields an empty `Vec`. Cross-record facts come from `ctx` (see [`ClassifyCtx`]).
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

        // Hook-injected additionalContext (a `type:"attachment"` record): harness machinery,
        // not a message - labeled `harness.meta.hook`. Only `search --additional-context`
        // (or an explicit `show --line`/`--uuid` address) ever parses these lines, so the
        // label is unreachable elsewhere; the record never opens a turn.
        if self.hook_additional_context_text().is_some() {
            push_unique(&mut out, Class::MetaHook);
            return out;
        }

        // Any OTHER `type:"attachment"` record (edited_text_file, compact_file_reference,
        // file snapshots, …): harness sidecar payload - labeled `harness.meta.attachment`.
        // Only `search --attachments` / `--count-by attachment` (or an explicit `show`
        // address) ever parses these lines; the record never opens a turn.
        if self.attachment_payload_text().is_some() {
            push_unique(&mut out, Class::MetaAttachment);
            return out;
        }

        // v0.9.5: a promoted NON-message line (queue-operation / turn_duration /
        // away_summary / stop_hook_summary / file-history-*) carries exactly one leaf
        // (`classify_promoted.rs`); every one of them is LLM-invisible.
        if let Some(c) = self.promoted_class() {
            push_unique(&mut out, c);
            return out;
        }

        match self.r#type.as_deref() {
            Some("system") => {
                if self.is_compact_boundary() {
                    push_unique(&mut out, Class::CompactionBoundary);
                }
            }
            Some("assistant") => self.classify_assistant(&mut out),
            Some("user") => self.classify_user(ctx, &mut out),
            _ => {}
        }
        out
    }

    /// Classify an `assistant` record's blocks (GOLD §2/§3): visible text → `agent.message`;
    /// thinking → `agent.thinking`; tool_use → `agent.tool.use` (+ `…sent`/`…signal` for a
    /// `SendMessage`/spawn); tool_result (rare on assistant) → `agent.tool.result`.
    pub(crate) fn classify_assistant(&self, out: &mut Vec<Class>) {
        let Some(blocks) = self.blocks() else {
            // A bare-string assistant body (rare) is a visible message (§ `agent_text`).
            if self.agent_text().is_some() {
                push_unique(out, Class::AgentMessage);
            }
            return;
        };
        for b in blocks {
            match b {
                Block::Text { text } if !text.trim().is_empty() => {
                    push_unique(out, Class::AgentMessage);
                }
                Block::Text { .. } => {}
                Block::Thinking { signature, .. } => {
                    push_unique(out, thinking_block_class(signature.as_deref()));
                }
                Block::RedactedThinking { .. } => {
                    push_unique(out, Class::AgentThinking);
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
                _ => {}
            }
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
            if classify_batched_sections(&raw, out) {
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
        if s.trim_start().starts_with(SCHEDULE_CONTINUATION_MARKER) {
            push_unique(out, Class::ScheduleContinuation);
            return;
        }
        // harness.schedule.wakeup: the FIRED autonomous-loop / ScheduleWakeup timer tick. Three
        // fixed markers (P1c M2a): the `<<autonomous-loop-dynamic>>` sentinel, the `# Autonomous
        // loop check` header, and the `You're being invoked on a timer` body sentence. Matched
        // BEFORE the meta.loop arm - `check` ≠ `tick`, so the loop-DRIVER prefix never collides.
        if s.contains(SCHEDULE_WAKEUP_MARKER)
            || s.starts_with(SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX)
            || s.contains(SCHEDULE_WAKEUP_TIMER_MARKER)
        {
            push_unique(out, Class::ScheduleWakeup);
            return;
        }
        // harness.meta.hook (G2): hook-injected feedback - stop-hook, <local-command-caveat>,
        // or the edit-failed-retry notice. (These are isMeta records that would otherwise fall
        // through to user.message.)
        if s.starts_with(STOP_HOOK_FEEDBACK_PREFIX)
            || s.starts_with(LOCAL_COMMAND_CAVEAT_PREFIX)
            || s.contains(EDIT_RETRY_MARKER)
        {
            push_unique(out, Class::MetaHook);
            return;
        }
        // harness.meta.loop (G2): autonomous-loop DRIVER ticks (`# Autonomous loop tick` /
        // `Run the autonomous check`), distinct from the schedule.wakeup fired tick above.
        if s.starts_with(AUTONOMOUS_LOOP_TICK_PREFIX) || s.contains(AUTONOMOUS_CHECK_MARKER) {
            push_unique(out, Class::MetaLoop);
            return;
        }
        // isMeta "[Image: source:…]" pseudo-record (G2): EXCLUDED - emit no label rather than
        // mislabel it user.message.
        if s.starts_with(IMAGE_SOURCE_PREFIX) {
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
        // M2b ROOT FIX: a genuine `user.message` is NEVER isMeta. An isMeta record that matched
        // no marker above is a harness-injected pseudo-turn (a generic cron/monitor tick, a
        // novel hook wrapper), NOT the operator - emit NOTHING rather than mislabel it
        // `user.message` (the role-level isMeta gate `is_genuine_user` already applies). Only
        // genuine, non-isMeta unmarked prose is `user.message`.
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
            return; // image-only / empty → nothing in the taxonomy
        }
        self.classify_user_string(ctx, &joined, out);
    }

    /// The FROM id of the FIRST inbound peer section (a `<teammate-message>` or `<agent-message>`)
    /// in this `type:"user"` record - the comm FROM for [`Record::direction`] (GOLD §4 + P1c M1).
    /// `None` when this is not a peer record; a section with no sender attribute degrades to the
    /// literal `"peer"`. Reads the raw (un-normalized) text so the relay preamble's `\n` survives.
    pub(crate) fn first_peer_from(&self) -> Option<String> {
        if !self.is_type("user") {
            return None;
        }
        let text = self.raw_message_text()?;
        let first = parse_all_peer_sections(&text).into_iter().next()?;
        Some(first.from.unwrap_or_else(|| "peer".to_string()))
    }

    /// The CLEAN inbound-comm preview of this record when it is (or leads with) an inbound peer
    /// message - a `<teammate-message …>` or `<agent-message from="…">` (GOLD §1/§5). Returns the
    /// FIRST inbound peer section's class + sender + tag/footer-stripped body, so `turns` / `list`
    /// render `agent.communication.inbox  <from> ⇨ self  <body>` instead of the raw `<teammate-message
    /// …>` XML blob a peer opener used to show. `None` for a non-peer record. RENDER-ONLY (does not
    /// affect [`Record::classify`] / [`Record::opens_turn`]). Pure + tolerant + codepoint-safe
    /// (delegates to the ASCII-offset peer-section scan).
    #[must_use]
    pub fn inbound_comm_preview(&self) -> Option<InboundComm> {
        let text = self.raw_message_text()?;
        let first = parse_all_peer_sections(&text).into_iter().next()?;
        let class = if first.is_signal {
            Class::CommSignal
        } else {
            Class::CommInbox
        };
        Some(InboundComm {
            class,
            from: first.from.unwrap_or_else(|| "peer".to_string()),
            body: normalize_line(peer_section_body(&first.text)),
        })
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
            |offset, section| {
                let kind =
                    AutomationKind::from_summary(extract_xml_tag(section, "summary").as_deref());
                let label = automation_label_for_section(section);
                out.push(RecordTextSection {
                    class: notification_class(kind),
                    text: label.clone(),
                    direction: None,
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
                    });
                }
                notif_spans.push((offset, offset + section.len()));
            },
        );
        // (b) inbound peer sections OUTSIDE every notification span (precedence + cross-family).
        for peer in parse_all_peer_sections(&raw) {
            if notif_spans
                .iter()
                .any(|&(s, e)| peer.offset >= s && peer.offset < e)
            {
                continue;
            }
            let from = peer.from.clone().unwrap_or_else(|| "peer".to_string());
            out.push(RecordTextSection {
                class: if peer.is_signal {
                    Class::CommSignal
                } else {
                    Class::CommInbox
                },
                text: normalize_line(&peer.text),
                direction: Some((from, owner())),
            });
        }
        out
    }

    /// The comm direction `(from, to)` for a communication record (GOLD §4), or `None` for a
    /// non-comm record. The `self` side is `ctx.owner_id` (falls back to the literal `"self"`
    /// so direction is testable without a real id); record-supplied ids (teammate_id, the
    /// SendMessage recipient) are used verbatim; a spawn child / subagent-return FROM is
    /// resolved via the [`SpawnLookup`], degrading to the raw spawn name or `"?"`. A record
    /// carrying multiple comm blocks returns the FIRST (most salient) direction.
    #[must_use]
    pub fn direction(&self, ctx: &ClassifyCtx) -> Option<(String, String)> {
        let owner = || ctx.owner_id.unwrap_or("self").to_string();

        // M3 precedence: a <task-notification> record is resolved FIRST - BEFORE the peer scan -
        // so a notification whose <result> merely QUOTES a "<teammate-message" tag never takes
        // the peer direction. A G1 <result>-bearing pulse is the bg-agent's report (child ⇨
        // self), the child resolved via the embedded <tool-use-id> spawn id (degrading to "?"
        // without a lookup); a bare launch-ack pulse (no <result>) carries no comm direction.
        if let Some(Content::Text(s)) = self.message.as_ref().and_then(|m| m.content.as_ref()) {
            if s.starts_with(TASK_NOTIFICATION_PREFIX) {
                if s.contains(NOTIFICATION_RESULT_TAG) {
                    let child = extract_xml_tag(s, "tool-use-id")
                        .and_then(|id| ctx.spawn.and_then(|sp| sp.child_for_spawn_tool_use_id(&id)))
                        .unwrap_or_else(|| "?".to_string());
                    return Some((child, owner()));
                }
                return None;
            }
        }

        // Inbound peer message (teammate-message / agent-message): from ⇨ self. The FIRST
        // section's sender (most-salient), per the multi-section rule (P1c M1 folds the
        // <agent-message> peer form in alongside <teammate-message>).
        if let Some(from) = self.first_peer_from() {
            return Some((from, owner()));
        }

        if let Some(blocks) = self.blocks() {
            for b in blocks {
                match b {
                    Block::ToolUse { id, name, input } => {
                        let Some(name) = name.as_deref() else {
                            continue;
                        };
                        if name == "SendMessage" {
                            let to = send_message_recipient(input.as_ref())
                                .unwrap_or_else(|| "?".to_string());
                            return Some((owner(), to));
                        }
                        if is_spawn_tool_name(name) {
                            // TO = the spawned child: id-join first, then the name-join, else
                            // the raw spawn name, else `?`.
                            let to = id
                                .as_deref()
                                .and_then(|i| {
                                    ctx.spawn.and_then(|s| s.child_for_spawn_tool_use_id(i))
                                })
                                .or_else(|| {
                                    spawn_target_name(input.as_ref()).map(|n| {
                                        ctx.spawn
                                            .and_then(|s| s.child_for_spawn_name(&n))
                                            .unwrap_or(n)
                                    })
                                })
                                .unwrap_or_else(|| "?".to_string());
                            return Some((owner(), to));
                        }
                    }
                    // Subagent return: child ⇨ self (the Task tool_result of a one-shot spawn).
                    // A spawn LAUNCH ACK (teammate OR async/background Agent) shares the spawn id
                    // but is NOT a return → no direction (the real reply comes later - a teammate
                    // via a teammate-message, an async agent via the <task-notification> result).
                    Block::ToolResult {
                        tool_use_id: Some(tid),
                        ..
                    } if !self.is_spawn_launch_ack() => {
                        if let Some(child) =
                            ctx.spawn.and_then(|s| s.child_for_spawn_tool_use_id(tid))
                        {
                            return Some((child, owner()));
                        }
                    }
                    _ => {}
                }
            }
        }

        // Subagent transcript opener (the spawn-prompt seed): parent ⇨ self.
        if ctx.is_subagent && ctx.is_transcript_opener && self.opens_turn() {
            let from = ctx.parent_id.unwrap_or("parent").to_string();
            return Some((from, owner()));
        }

        None
    }
}
