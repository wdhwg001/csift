//! The walk itself, its two repairs and the two membership rules that widen it past
//! plain ancestry.
//!
//! `parentUuid` ONLY - `logicalParentUuid` is never read by the loader, so a
//! `compact_boundary` (whose own `parentUuid` is null) ends the reconstruction and every
//! record above it is gone from the resumed conversation. csift's reading is forensic,
//! not a replay, so it STEPS OVER that cut through the boundary's own
//! `logicalParentUuid` - the predecessor the compaction recorded - and flags everything
//! it then reaches as `PreCut`. Without that step a compacted transcript would report its
//! whole history as unresolved.
//!
//! The step can FAIL, and that failure is the reason the floor exists (claim MISC-043):
//! the loader never reads the field, so nothing keeps its target on disk, and 52 of 245
//! boundary records in a real corpus name a `logicalParentUuid` that matches no `uuid` on
//! any line of their own file. `resolve` then yields nothing, the walk ends on the
//! boundary, and everything physically above it is a region csift could not resolve -
//! never abandoned, only `PreCut`.
//!
//! Repairs and rescues, each mirroring one loader hop: a parent uuid that names no
//! record (or one already visited) falls back to the nearest UNVISITED record with the
//! same `isSidechain` whose timestamp is at most 5 s earlier; a cycle stops the walk;
//! same-`message.id` assistant siblings of a chain assistant - and the `tool_result`
//! carriers parented to them - join the conversation; and every non-conversation
//! descendant of the leaf joins it too.

use super::*;

/// The 5 s window the parent repair searches, in milliseconds.
const REPAIR_WINDOW_MS: i64 = 5_000;

/// What the walk leaves behind for the survival pass.
pub(crate) struct Walked {
    /// The last record the walk reached - the FLOOR below which abandonment is claimed.
    pub(crate) floor: usize,
    /// The newest boundary the walk stopped at or stepped over.
    pub(crate) boundary: Option<usize>,
}

pub(crate) fn run(b: &mut Builder<'_>, leaf: Option<usize>) -> Walked {
    let mut out = Walked {
        floor: leaf.unwrap_or(0),
        boundary: None,
    };
    let Some(leaf) = leaf else {
        // No leaf: nothing is claimed. The floor stays at 0 only in the sense that no
        // record is on the chain, and the survival pass turns that into PreCut.
        out.floor = b.len();
        return out;
    };
    let mut crossed = false;
    let mut cur = Some(leaf);
    while let Some(i) = cur {
        if b.visited[i] {
            break; // cycle guard
        }
        b.visited[i] = true;
        b.on_chain[i] = true;
        b.precut[i] = crossed;
        out.floor = i;
        let Some(pu) = b.parent[i] else {
            if b.is_boundary(i) {
                out.boundary.get_or_insert(i);
            }
            break;
        };
        b.chain_child.entry(pu).or_insert(i);
        let next = match b.resolve(pu) {
            Some(j) if !b.visited[j] => Some(j),
            _ => repair(b, i),
        };
        let next = match next {
            Some(j) => Some(j),
            None if b.is_boundary(i) => {
                out.boundary.get_or_insert(i);
                crossed = true;
                b.recs[i]
                    .logical_parent_uuid()
                    .and_then(|u| b.resolve(u))
                    .filter(|&j| !b.visited[j])
            }
            None => None,
        };
        cur = next;
    }
    rescue_message_id(b);
    rescue_leaf_descendants(b, leaf);
    out
}

/// The parent repair: among UNVISITED records with the same `isSidechain`, the one whose
/// timestamp is closest at or before this record's, within 5 s. Measured to fire zero
/// times over a 75-file corpus, so the sorted table it needs is built only on first use.
fn repair(b: &mut Builder<'_>, i: usize) -> Option<usize> {
    let now = b.recs[i].timestamp().and_then(crate::timez::epoch_ms)?;
    let side = b.recs[i].is_sidechain();
    if b.ts_sorted.is_none() {
        let mut v: Vec<(i64, usize)> = (0..b.len())
            .filter(|&k| b.admit[k] && !b.removed[k] && b.is_survivor(k))
            .filter_map(|k| {
                b.recs[k]
                    .timestamp()
                    .and_then(crate::timez::epoch_ms)
                    .map(|ms| (ms, k))
            })
            .collect();
        v.sort_unstable();
        b.ts_sorted = Some(v);
    }
    let table = b.ts_sorted.as_ref()?;
    let mut at = table.partition_point(|&(ms, _)| ms <= now);
    while at > 0 {
        at -= 1;
        let (ms, k) = table[at];
        if now - ms > REPAIR_WINDOW_MS {
            return None;
        }
        if !b.visited[k] && b.recs[k].is_sidechain() == side {
            return Some(k);
        }
    }
    None
}

/// Claude Code writes one record per content block, so ONE assistant message can span
/// several lines sharing a `message.id`. The chain threads through one of them; the rest
/// (and the `tool_result` carriers parented to any of them) are part of the same turn and
/// are re-interleaved on load, so they are Live.
fn rescue_message_id(b: &mut Builder<'_>) {
    let ids: HashSet<&str> = (0..b.len())
        .filter(|&i| b.on_chain[i] && b.recs[i].kind() == Some("assistant"))
        .filter_map(|i| b.recs[i].message_id())
        .collect();
    if ids.is_empty() {
        return;
    }
    let siblings: Vec<usize> = (0..b.len())
        .filter(|&i| {
            !b.visited[i]
                && b.admit[i]
                && !b.removed[i]
                && b.is_survivor(i)
                && b.recs[i].kind() == Some("assistant")
                && b.recs[i].message_id().is_some_and(|id| ids.contains(id))
        })
        .collect();
    let mut group: HashSet<&str> = (0..b.len())
        .filter(|&i| b.on_chain[i] && b.recs[i].kind() == Some("assistant"))
        .filter_map(|i| b.uuid(i))
        .collect();
    for i in siblings {
        b.visited[i] = true;
        b.on_chain[i] = true;
        if let Some(u) = b.uuid(i) {
            group.insert(u);
        }
    }
    let carriers: Vec<usize> = (0..b.len())
        .filter(|&i| {
            !b.visited[i]
                && b.admit[i]
                && !b.removed[i]
                && b.is_survivor(i)
                && b.recs[i].kind() == Some("user")
                && b.parent[i].is_some_and(|p| group.contains(p))
                && b.recs[i].has_tool_result()
        })
        .collect();
    for i in carriers {
        b.visited[i] = true;
        b.on_chain[i] = true;
    }
}

/// Everything hanging BELOW the leaf that is not itself a conversation record - the
/// attachments and telemetry a turn trails - is appended to the loaded transcript, so it
/// is Live.
fn rescue_leaf_descendants<'a>(b: &mut Builder<'a>, leaf: usize) {
    let Some(root) = b.uuid(leaf) else {
        return;
    };
    let mut kids: HashMap<&'a str, Vec<usize>> = HashMap::new();
    for i in 0..b.len() {
        if !b.admit[i] || b.removed[i] || !b.is_survivor(i) || b.is_conv(i) {
            continue;
        }
        if let Some(p) = b.parent[i] {
            kids.entry(p).or_default().push(i);
        }
    }
    let mut queue: Vec<&'a str> = vec![root];
    while let Some(u) = queue.pop() {
        let Some(children) = kids.get(u) else {
            continue;
        };
        let children = children.clone();
        for i in children {
            if b.visited[i] {
                continue;
            }
            b.visited[i] = true;
            b.on_chain[i] = true;
            if let Some(cu) = b.uuid(i) {
                queue.push(cu);
            }
        }
    }
}
