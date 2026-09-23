//! The file-level TOOL JOIN: the `tool_use_id` indexes one transcript is scanned for,
//! and the `agent.tool.use` -> `agent.tool.result` verdict a hit reads off them.
//!
//! Split from `turns_match.rs` at the 600-line structure gate, along the seam already in
//! the file: everything there reconstructs TURNS, and everything here answers one
//! question about a pair of BLOCKS that may sit in different turns entirely - which is
//! exactly why the join is global rather than per turn.

use super::*;

/// Everything ONE walk over a file's `tool_use` / `tool_result` blocks answers.
///
/// Three questions about one block - what is this tool called, did a result come back, and
/// which line carries the call (the last is what the `result_of` provenance edge needs) -
/// answered by ONE walk. Three separate walks would pay three passes over the same blocks
/// and buy nothing: the questions are asked together, at the same place, about the same
/// blocks.
#[derive(Debug, Default)]
pub(crate) struct ToolIndex {
    /// `tool_use_id -> tool name`. A later `tool_result` carries only the id, so a
    /// `tool-response` row looks its tool up here to say WHICH tool it answers. First write
    /// wins (ids are unique).
    pub(crate) names: HashMap<String, String>,
    /// Every `tool_use` block's `id` in this transcript - the use side of the use/result
    /// pairing join, which is membership and not contiguity so a use pairs with its result
    /// across records and parallel calls.
    pub(crate) uses: HashSet<String>,
    /// Every `tool_result` block's `tool_use_id` - the result side of the same join.
    pub(crate) results: HashSet<String>,
    /// `tool_use_id -> the LINE carrying the call`, the one join the chain's uuid map
    /// cannot make (a `tool_use_id` is a wire id and appears in no `uuid` field).
    pub(crate) prov: crate::model::ProvIndex,
}

/// Walk a file's blocks ONCE and answer all four.
pub(crate) fn build_tool_index(rows: &[Row<'_>]) -> ToolIndex {
    let mut ix = ToolIndex::default();
    for k in rows.iter().filter_map(|r| r.kept()) {
        let Some(blocks) = k.rec.blocks() else {
            continue;
        };
        for b in blocks {
            match b {
                Block::ToolUse {
                    id: Some(id), name, ..
                } => {
                    ix.uses.insert(id.clone());
                    // A sidecar-merged elicitation marker has no physical line, so it
                    // records none: an edge pointing at line 0 would be an address that
                    // fetches nothing.
                    if k.line_no > 0 {
                        ix.prov.insert_tool_use(id, k.line_no);
                    }
                    if let Some(name) = name {
                        ix.names.entry(id.clone()).or_insert_with(|| name.clone());
                    }
                }
                Block::ToolResult {
                    tool_use_id: Some(id),
                    ..
                } => {
                    ix.results.insert(id.clone());
                }
                _ => {}
            }
        }
    }
    ix
}

/// Resolve a tool hit's [`Pairing`] against the file-level id sets. Pairing is a property
/// of the underlying tool_use/tool_result BLOCK, so it rides EVERY view of that block - the
/// plain `agent.tool.*` views AND the communication views that supersede them under the
/// richest-view law (a SendMessage/spawn `agent.communication.sent`/`.signal` rides a
/// tool_use block; a subagent-return `agent.communication.inbox` rides a tool_result
/// block). Read off the VIEW instead, a FROZEN SendMessage (the dominant stuck-lane shape
/// in a teams session) falls outside the `pairing` census whenever its comm view wins the
/// dedup. A use-side hit is paired iff its result-id is present (else pending - frozen /
/// elicitation / unreturned); a result-side hit is paired iff its use-id is present (else
/// orphan - compacted / sliced away). A hit with no `tool_use_id` (a record-text unit: an
/// inbound teammate-message, an idle signal section) stays `None` - outside the axis.
pub(crate) fn set_pairing(h: &mut Hit, ix: &ToolIndex) {
    let (use_ids, result_ids) = (&ix.uses, &ix.results);
    let Some(id) = h.tool_use_id.as_deref() else {
        return;
    };
    h.pair = match h.class {
        Some(Class::AgentToolUse | Class::CommSent | Class::CommSignal) => {
            Some(if result_ids.contains(id) {
                Pairing::Paired
            } else {
                Pairing::PendingNoResult
            })
        }
        Some(Class::AgentToolResult | Class::CommInbox) => Some(if use_ids.contains(id) {
            Pairing::Paired
        } else {
            Pairing::OrphanResult
        }),
        _ => None,
    };
}
