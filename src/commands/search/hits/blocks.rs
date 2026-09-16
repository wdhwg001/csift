//! The BLOCK loop of the emission engine, and the sink both halves emit through.
//!
//! Split from `hits.rs` at the 600-line structure gate, along the seam that was already
//! there: everything above decides what a RECORD says, and everything here decides what
//! one of its content BLOCKS says. The sink is what joins them, which is why it lives
//! with the loop that is its second caller.

use super::*;

/// The hit-emission sink shared by [`collect_record_hits`] and its block loop:
/// (class, text, tool_name, direction, tool_use_id, is_error, the source BLOCK).
///
/// The block is the last parameter and it is what makes a per-BLOCK family fact
/// possible: the record-level arms pass `None`, and the block loop passes the block the
/// unit came from, so a record carrying two thinking blocks emits two units whose facts
/// are each their own block's.
pub(crate) type EmitHit<'a> = dyn FnMut(
        Class,
        &str,
        Option<String>,
        Option<(String, String)>,
        Option<String>,
        Option<bool>,
        Option<&Block>,
    ) + 'a;

/// The block loop of [`collect_record_hits`]: one emission per selected block-bearing unit -
/// thinking (the four leaves, two of which render an EMPTY body and say what they are in
/// the label zone instead), assistant text, tool_use (richest comm view first), tool_result
/// (inbox > plain result; the user-facing dual, when SELECTED, was already emitted as the
/// richest view and suppresses the duplicate).
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_block_hits(
    rec: &Record,
    labels: &[Class],
    filter: LabelFilter<'_>,
    user_dual: Option<Class>,
    direction: &Option<(String, String)>,
    resolve_persisted: bool,
    tool_names: &HashMap<String, String>,
    emit: &mut EmitHit<'_>,
) {
    let sel = |c: Class| filter.selected(c.path());
    let has = |c: Class| labels.contains(&c);
    if let Some(blocks) = rec.blocks() {
        for block in blocks {
            match block {
                Block::Thinking { .. } | Block::RedactedThinking { .. } => {
                    // The four thinking leaves split on the BLOCK's own shape
                    // (`model/classify/thinking.rs`): the signature tag for a narration
                    // summary, the absent text for the two leaves that carry none.
                    // Adjacency is never consulted, and a mixed multi-block record splits
                    // per BLOCK.
                    let Some(class) = crate::model::thinking_block_leaf(block) else {
                        continue;
                    };
                    if !has(class) || !sel(class) {
                        continue;
                    }
                    // The BODY is the thinking family's call, not the loop's: what a
                    // thinking unit's text IS belongs with the four leaves that split on
                    // the block's own shape. `None` is a block shape the family does not
                    // own, and nothing is emitted for it rather than a fabricated body -
                    // unreachable today, since `thinking_block_leaf` already refused
                    // everything but these two above.
                    let Some(text) = record_text::thinking::block_body(block) else {
                        continue;
                    };
                    emit(class, &text, None, None, None, None, Some(block));
                }
                Block::Text { text }
                    if rec.is_type("assistant")
                        && has(Class::AgentMessagePlain)
                        && sel(Class::AgentMessagePlain) =>
                {
                    // Only assistant `text` blocks are the agent message; a user `text` block is
                    // a record-text unit (handled above), never agent.message.
                    emit(
                        Class::AgentMessagePlain,
                        text,
                        None,
                        None,
                        None,
                        None,
                        Some(block),
                    );
                }
                Block::ToolUse { id, name, input } => {
                    // Richest-selected for this tool_use: comm (sent/signal) > agent.tool.use.
                    let comm = tool_use_comm_class(name.as_deref(), input.as_ref());
                    let class = match comm {
                        Some(cc) if has(cc) && sel(cc) => Some(cc),
                        _ if has(Class::AgentToolUse) && sel(Class::AgentToolUse) => {
                            Some(Class::AgentToolUse)
                        }
                        _ => None,
                    };
                    if let Some(class) = class {
                        let rendered = render_tool_use(name.as_deref(), input.as_ref());
                        let dir = if is_comm_class(class) {
                            direction.clone()
                        } else {
                            None
                        };
                        emit(
                            class,
                            &rendered,
                            name.clone(),
                            dir,
                            id.clone(),
                            None,
                            Some(block),
                        );
                    }
                }
                Block::ToolResult {
                    content: Some(c),
                    tool_use_id,
                    is_error,
                    ..
                } => {
                    // C-13: every result hit states its error side explicitly - an absent
                    // field IS a clean result on the wire, so it maps to false, never null
                    // (pairing answers "did a result come back"; this answers "was it good").
                    let result_errored = is_error.unwrap_or(false);
                    // The user-facing dual was SELECTED + emitted as the richest view (§3 Q4) → skip
                    // the agent.tool.result duplicate. (When the dual is present but NOT selected -
                    // e.g. `-t agent.tool.result` alone - `user_dual` is None, so the plain result
                    // still surfaces and the answer is never lost.)
                    if user_dual.is_some() {
                        continue;
                    }
                    // Richest-selected: agent.communication.inbox (subagent return) > tool.result.
                    let class = if has(Class::CommInbox) && sel(Class::CommInbox) {
                        Class::CommInbox
                    } else if has(Class::AgentToolResult) && sel(Class::AgentToolResult) {
                        Class::AgentToolResult
                    } else {
                        continue;
                    };
                    let mut text = tool_result_content_text(c);
                    // §4.6: when asked, replace the inline persisted-output pointer with the real
                    // file content (matching runs against the resolved text).
                    if resolve_persisted {
                        if let Some(path) = rec.persisted_output_path() {
                            text = resolve_persisted_text(&path, &text);
                        }
                    }
                    let name = tool_use_id
                        .as_deref()
                        .and_then(|id| tool_names.get(id).cloned());
                    let dir = if class == Class::CommInbox {
                        direction.clone()
                    } else {
                        None
                    };
                    emit(
                        class,
                        &text,
                        name,
                        dir,
                        tool_use_id.clone(),
                        Some(result_errored),
                        Some(block),
                    );
                }
                _ => {}
            }
        }
    }
}
