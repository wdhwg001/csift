//! The file-level TOOL JOIN: the `tool_use_id` indexes one transcript is scanned for,
//! and the `agent.tool.use` -> `agent.tool.result` verdict a hit reads off them.
//!
//! Split from `turns_match.rs` at the 600-line structure gate, along the seam already in
//! the file: everything there reconstructs TURNS, and everything here answers one
//! question about a pair of BLOCKS that may sit in different turns entirely - which is
//! exactly why the join is global rather than per turn.

use super::*;

/// Build the `tool_use_id → tool name` index for a file's records: every `tool_use` block's
/// `{id, name}`. A later `tool_result` (which carries only the `tool_use_id`) looks its tool up
/// here so a `tool-response` row can say WHICH tool it answers. First write wins (ids are unique).
pub(crate) fn build_tool_name_index(rows: &[Row<'_>]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for k in rows.iter().filter_map(|r| r.kept()) {
        if let Some(blocks) = k.rec.blocks() {
            for b in blocks {
                if let Block::ToolUse {
                    id: Some(id),
                    name: Some(name),
                    ..
                } = b
                {
                    map.entry(id.clone()).or_insert_with(|| name.clone());
                }
            }
        }
    }
    map
}

/// The `▹` pairing id sets for a file (GOLD §7): every `tool_use` block's `id` and every
/// `tool_result` block's `tool_use_id`. Joined GLOBALLY (membership, not contiguity) so a use
/// pairs with its result across records / parallel calls.
pub(crate) fn tool_pair_ids(rows: &[Row<'_>]) -> (HashSet<String>, HashSet<String>) {
    let mut uses = HashSet::new();
    let mut results = HashSet::new();
    for k in rows.iter().filter_map(|r| r.kept()) {
        if let Some(blocks) = k.rec.blocks() {
            for b in blocks {
                match b {
                    Block::ToolUse { id: Some(id), .. } => {
                        uses.insert(id.clone());
                    }
                    Block::ToolResult {
                        tool_use_id: Some(id),
                        ..
                    } => {
                        results.insert(id.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    (uses, results)
}

/// Resolve a tool hit's [`Pairing`] against the file-level id sets (GOLD §7). Pairing is a
/// property of the underlying tool_use/tool_result BLOCK, so it rides EVERY view of that
/// block - the plain `agent.tool.*` views AND the communication views that supersede them
/// under the richest-view law (a SendMessage/spawn `agent.communication.sent`/`.signal`
/// rides a tool_use block; a subagent-return `agent.communication.inbox` rides a
/// tool_result block). Without this, a FROZEN SendMessage (the dominant stuck-lane shape
/// in a teams session) fell outside the `pairing` census whenever its comm view won the
/// dedup. A use-side hit is paired iff its result-id is present (else pending - frozen /
/// elicitation / unreturned); a result-side hit is paired iff its use-id is present (else
/// orphan - compacted / sliced away). A hit with no `tool_use_id` (a record-text unit:
/// an inbound teammate-message, an idle signal section) stays `None` - outside the axis.
pub(crate) fn set_pairing(h: &mut Hit, use_ids: &HashSet<String>, result_ids: &HashSet<String>) {
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
