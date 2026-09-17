//! The walk itself, its two repairs and the two membership rules that widen it past
//! plain ancestry.
//!
//! `parentUuid` FIRST - that is the only link Claude Code's own loader reads, so a
//! `compact_boundary` (whose `parentUuid` is null on 260 of 260 corpus specimens) ends
//! the loader's reconstruction and every record above it is gone from the resumed
//! conversation. csift climbs the same link and then asks one more question the loader
//! never asks: the boundary names the record it re-linked from, in `logicalParentUuid`,
//! and under a CROSSING BUDGET ([`CrossingBudget`]) the walk steps over the cut and
//! carries on. What it reaches above a crossed boundary is resolved by the ordinary rules
//! - Live where the chain reaches it, Abandoned where it does not.
//!
//! The walk ends on a boundary anyway in two cases, and both leave a FLOOR: the budget ran
//! out, or the `logicalParentUuid` names no record the map still holds (claim MISC-043: 52
//! of 255 boundary records, the pre-compaction lines having been deleted from disk; a
//! relink cut removes more). Everything physically above the floor is unresolved - never
//! abandoned, only `PreCut` - and [`CrossingReport`] states which of the two it was, how
//! many boundaries are still above, and the flag value that would reach them.
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
    /// The boundary the walk STOPPED at - the floor, when the floor is a boundary - and
    /// `None` when it ended at a head, a dangling parent or a cycle. A boundary the walk
    /// stepped OVER is counted in `crossed` and never named here: this is the record the
    /// surviving conversation is cut at, and a crossed one is a cut csift got past.
    pub(crate) boundary: Option<usize>,
    /// Compaction boundaries stepped over.
    pub(crate) crossed: usize,
    /// The walk met a boundary with no budget left.
    pub(crate) exhausted: bool,
    /// Why the walk ended where it did.
    pub(crate) cause: FloorCause,
}

pub(crate) fn run(b: &mut Builder<'_>, leaf: Option<usize>, budget: CrossingBudget) -> Walked {
    let mut out = Walked {
        floor: leaf.unwrap_or(0),
        boundary: None,
        crossed: 0,
        exhausted: false,
        cause: FloorCause::Head,
    };
    let Some(leaf) = leaf else {
        // No leaf: nothing is claimed. The floor stays at 0 only in the sense that no
        // record is on the chain, and the survival pass turns that into PreCut.
        out.floor = b.len();
        out.cause = FloorCause::NoLeaf;
        return out;
    };
    let mut cur = Some(leaf);
    while let Some(i) = cur {
        if b.visited[i] {
            out.cause = FloorCause::Cycle;
            break;
        }
        b.visited[i] = true;
        b.on_chain[i] = true;
        out.floor = i;
        // The parent link, which is all the loader reads. A null `parentUuid` (every
        // corpus boundary) and one naming no record the repair could stand in for both
        // arrive here as None.
        let climbed = match b.parent[i] {
            Some(pu) => {
                b.chain_child.entry(pu).or_insert(i);
                match b.resolve(pu) {
                    Some(j) if !b.visited[j] => Some(j),
                    _ => repair(b, i),
                }
            }
            None => None,
        };
        if let Some(j) = climbed {
            cur = Some(j);
            continue;
        }
        // Nothing above by the parent link. Only a compaction boundary offers the second
        // door, and only while the budget lasts.
        if !b.is_boundary(i) {
            out.cause = if b.parent[i].is_some() {
                FloorCause::DanglingParent
            } else {
                FloorCause::Head
            };
            break;
        }
        if !budget.allows(out.crossed) {
            out.exhausted = true;
            out.cause = FloorCause::BudgetExhausted;
            out.boundary = Some(i);
            break;
        }
        let over = b.recs[i]
            .logical_parent_uuid()
            .and_then(|u| b.resolve(u))
            .filter(|&j| !b.visited[j]);
        let Some(j) = over else {
            out.cause = FloorCause::UnresolvableBoundary;
            out.boundary = Some(i);
            break;
        };
        out.crossed += 1;
        cur = Some(j);
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
