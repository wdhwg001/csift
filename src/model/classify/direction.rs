//! `Record::direction`: the from-and-to a communication record carries.
//!
//! Split out of the engine because it answers a different question - `classify`
//! says which LEAVES a record carries, this says who was talking to whom - and
//! because the engine sits at the 600-line structure cap.

use super::*;

impl Record {
    /// The comm direction `(from, to)` for a communication record, or `None` for a
    /// non-comm record. The `self` side is `ctx.owner_id` (falls back to the literal `"self"`
    /// so direction is testable without a real id); record-supplied ids (teammate_id, the
    /// SendMessage recipient) are used verbatim; a spawn child / subagent-return FROM is
    /// resolved via the [`SpawnLookup`], degrading to the raw spawn name or `"?"`. A record
    /// carrying multiple comm blocks returns the FIRST (most salient) direction.
    #[must_use]
    pub fn direction(&self, ctx: &ClassifyCtx) -> Option<(String, String)> {
        let owner = || ctx.owner_id.unwrap_or("self").to_string();

        // NOTIFICATION PRECEDENCE: a <task-notification> record is resolved FIRST - BEFORE the peer scan -
        // so a notification whose <result> merely QUOTES a "<teammate-message" tag never takes
        // the peer direction. A <result>-bearing pulse is the bg-agent's report (child ⇨
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
        // section's sender (most-salient), per the multi-section rule; the <agent-message> peer
        // form is an inbound peer message alongside <teammate-message>.
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

        self.csift_channel_direction(ctx)
            .or_else(|| self.coordinator_direction(ctx))
    }

    /// The COORDINATOR relay's direction: `coordinator ⇨ self`.
    ///
    /// Its leaf is `agent.communication.inbox` like any other inbound message, and every
    /// other comm leaf renders an arrow, so without this one the relay is the one inbound
    /// message whose hit says nothing about where it came from.
    ///
    /// The FROM is the literal word and not a read of some sender field, because there is
    /// no sender field to read: measured over one projects root, every one of the 147
    /// coordinator relays carries an `origin` object whose key set is exactly `{kind}`,
    /// and all 147 are `isMeta`. A relay that later gains a sender key would be a
    /// different shape and would need a different arm; inventing an id from this one
    /// would be inventing it (claim CLS-046).
    fn coordinator_direction(&self, ctx: &ClassifyCtx) -> Option<(String, String)> {
        (self.origin_kind()? == "coordinator").then(|| {
            (
                "coordinator".to_string(),
                ctx.owner_id.unwrap_or("self").to_string(),
            )
        })
    }
}
