//! Picking the leaf the walk starts from - four gates deep, and every one of them
//! decides which branch of a forked transcript IS the conversation.
//!
//! In order: a `last-prompt` line records a `leafUuid` with every write batch, and a
//! `compact_boundary` seen afterwards WIPES it; an `explicit:true` line with a null
//! leaf clears the leaf set outright, and an EMPTY set drops the pick to the newest
//! record by timestamp rather than to the file-order tail; a recorded leaf that is a proper ANCESTOR of the
//! file-order-last record is replaced by that record (which is why the leaf tracks
//! records appended after the last snapshot); a recorded leaf naming no record in this
//! file falls back to the file-order-last record, never to the next-newest leafUuid; and
//! a successful preserved-messages relink skips the recorded leaf entirely, leaving the
//! TIPS fallback - the records nothing points at, collapsed to their first conversational
//! ancestor - to choose.

use super::*;

/// What the `last-prompt` lines of a transcript accumulate to by its end.
struct Recorded<'a> {
    leaf: Option<&'a str>,
    explicit: bool,
    cleared: bool,
}

pub(crate) fn pick<'a>(
    b: &Builder<'a>,
    hint: Option<&'a str>,
    relink_anchor: Option<&'a str>,
) -> (Option<usize>, LeafSource) {
    let rec = match hint {
        Some(h) => Recorded {
            leaf: Some(h),
            explicit: false,
            cleared: false,
        },
        None => recorded(b),
    };
    let tail = tail_index(b);
    // An `explicit:true` line with a null leaf leaves the loader with an EMPTY leaf set,
    // and an empty set is not the same as never having had one: there is no recorded leaf
    // to resolve and no tail branch to fall into, so the pick drops straight to the
    // newest-by-timestamp record of any admitted type.
    if rec.cleared {
        return (newest(b), LeafSource::Cleared);
    }
    let resolved = rec.leaf.and_then(|u| b.resolve(u));
    let source = match (rec.leaf, resolved, rec.explicit) {
        (Some(_), Some(_), true) => LeafSource::Explicit,
        (Some(_), Some(_), false) => LeafSource::LastPrompt,
        (Some(_), None, _) => LeafSource::TailLeafAbsent,
        (None, _, _) => LeafSource::Tail,
    };
    // A relink anchor means the loader skips the recorded leaf unless it was pinned.
    let use_recorded = relink_anchor.is_none() || (rec.explicit && resolved.is_some());
    if use_recorded {
        let mut no = resolved;
        if let (Some(cur), Some(t)) = (no, tail) {
            if !rec.explicit && t != cur && is_ancestor_of(b, cur, t) {
                no = Some(t);
            }
        }
        if relink_anchor.is_none() {
            no = no.or(tail);
        }
        if let Some(i) = no.and_then(|i| walk_up_to_conv(b, i)) {
            return (Some(i), source);
        }
    }
    (tips(b, resolved, tail), source)
}

/// The `leafUuid` / `explicit` state the file's `last-prompt` lines leave behind, with a
/// `compact_boundary` wiping it exactly as the loader does.
fn recorded<'a>(b: &Builder<'a>) -> Recorded<'a> {
    let mut out = Recorded {
        leaf: None,
        explicit: false,
        cleared: false,
    };
    for i in 0..b.len() {
        let r = b.recs[i];
        if b.is_boundary(i) {
            out.leaf = None;
            out.explicit = false;
            continue;
        }
        if r.r#type.as_deref() != Some("last-prompt") {
            continue;
        }
        match r.leaf_uuid.as_deref() {
            Some(u) if !u.is_empty() => {
                out.explicit = r.explicit == Some(true) || (out.explicit && Some(u) == out.leaf);
                out.leaf = Some(u);
                out.cleared = false;
            }
            _ if r.explicit == Some(true) => {
                out.cleared = true;
                out.leaf = None;
                out.explicit = false;
            }
            _ => {}
        }
    }
    out
}

/// The file-order-last non-sidechain record still in the map.
fn tail_index(b: &Builder<'_>) -> Option<usize> {
    (0..b.len())
        .rev()
        .find(|&i| b.admit[i] && !b.removed[i] && b.is_survivor(i) && !b.is_sidechain(i))
}

/// Is `anc` a proper `parentUuid` ancestor of `start`? The step budget is the record
/// count: a longer walk can only be a cycle, and it costs no allocation to bound it that
/// way (these walks run once per candidate tip, so a per-call set would dominate).
fn is_ancestor_of(b: &Builder<'_>, anc: usize, start: usize) -> bool {
    let mut cur = Some(start);
    for _ in 0..=b.len() {
        let Some(i) = cur else { return false };
        if i == anc {
            return true;
        }
        cur = b.parent[i].and_then(|p| b.resolve(p));
    }
    false
}

/// Climb `parentUuid` until a `user`/`assistant` record - the loader never starts the
/// walk on an attachment or a system record. Same step budget as above.
fn walk_up_to_conv(b: &Builder<'_>, start: usize) -> Option<usize> {
    let mut cur = Some(start);
    for _ in 0..=b.len() {
        let i = cur?;
        if b.is_conv(i) {
            return Some(i);
        }
        cur = b.parent[i].and_then(|p| b.resolve(p));
    }
    None
}

/// The TIPS fallback: every record nothing points at, collapsed to its first
/// conversational ancestor and kept only when that ancestor has no conversational child
/// of its own. One candidate wins outright; several are resolved by the recorded leaf,
/// else by the file tail; none leaves the max-timestamp non-sidechain record.
fn tips(b: &Builder<'_>, recorded: Option<usize>, tail: Option<usize>) -> Option<usize> {
    // Marked by INDEX rather than by uuid: one resolve per record instead of two string
    // hashes, and no set to grow on a transcript with tens of thousands of records.
    let mut referenced = vec![false; b.len()];
    let mut conv_referenced = vec![false; b.len()];
    for i in 0..b.len() {
        if !b.admit[i] || b.removed[i] {
            continue;
        }
        if let Some(p) = b.parent[i].and_then(|p| b.resolve(p)) {
            referenced[p] = true;
            if b.is_conv(i) {
                conv_referenced[p] = true;
            }
        }
    }
    let mut candidates: Vec<usize> = Vec::new();
    for (i, &is_referenced) in referenced.iter().enumerate() {
        if !b.admit[i] || b.removed[i] || !b.is_survivor(i) || is_referenced {
            continue;
        }
        let Some(c) = walk_up_to_conv(b, i) else {
            continue;
        };
        if !conv_referenced[c] && !candidates.contains(&c) {
            candidates.push(c);
        }
    }
    match candidates.len() {
        1 => return candidates.first().copied(),
        0 => {}
        _ => {
            let pick = recorded
                .filter(|i| candidates.contains(i))
                .or(tail)
                .or_else(|| candidates.first().copied());
            return pick.and_then(|i| walk_up_to_conv(b, i));
        }
    }
    // Nothing looked like a tip: the max-timestamp non-sidechain record of ANY admitted
    // type, which is what the loader falls back to.
    newest(b).or_else(|| tail.and_then(|i| walk_up_to_conv(b, i)))
}

/// The loader's last resort: the max-`timestamp` non-sidechain record of ANY admitted
/// type, climbed to its first conversational ancestor. File order is deliberately NOT
/// consulted - a write that landed out of order would otherwise decide the whole chain.
fn newest(b: &Builder<'_>) -> Option<usize> {
    let mut best: Option<(i64, usize)> = None;
    for i in 0..b.len() {
        if !b.admit[i] || b.removed[i] || !b.is_survivor(i) || b.is_sidechain(i) {
            continue;
        }
        let Some(ms) = b.recs[i]
            .timestamp
            .as_deref()
            .and_then(crate::timez::epoch_ms)
        else {
            continue;
        };
        if best.is_none_or(|(bm, _)| ms > bm) {
            best = Some((ms, i));
        }
    }
    best.and_then(|(_, i)| walk_up_to_conv(b, i))
}
