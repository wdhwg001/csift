//! The pre-walk relink: what a `compact_boundary` carrying `compactMetadata`
//! `preservedMessages` / `preservedSegment` does to the map BEFORE any leaf is picked.
//!
//! Mirrors the loader hop that runs first on every load. When the NEWEST boundary
//! carries a preserved set it (1) re-chains those uuids into a path hanging off
//! `anchorUuid`, (2) re-parents anything that pointed at the anchor to the LAST
//! preserved uuid, (3) DELETES every record whose first line sits before that boundary
//! and is not preserved, and (4) re-parents any user/assistant record orphaned by (3) to
//! the last preserved uuid. Anything that cannot be resolved aborts the whole pass and
//! leaves the map untouched - measured on a real corpus, 7 of 21 compacted files relink
//! and 14 abort.
//!
//! csift does not delete: a removed record is marked `PreCut`, which keeps it readable
//! and selectable while stating that the loader would not carry it forward.

use super::*;

/// A boundary's record index plus the insertion position of its uuid (the FIRST line
/// carrying it - what the loader's Map-order comparison actually uses).
#[derive(Clone, Copy)]
struct Bnd {
    idx: usize,
    first: usize,
}

/// The resolved preserved set: the anchor the segment hangs off, and its uuids in order.
struct Preserved {
    anchor: String,
    uuids: Vec<String>,
}

/// Returns the ANCHOR the relink produced (the last preserved uuid) when it ran: a
/// truthy anchor makes the loader skip the recorded-leaf branch entirely, so the leaf
/// then comes from the tips fallback.
pub(crate) fn apply<'a>(b: &mut Builder<'a>) -> Option<&'a str> {
    let (newest, with_meta) = boundaries(b);
    let (Some(newest), Some(meta)) = (newest, with_meta) else {
        return None;
    };
    // The preserved metadata must sit on the NEWEST boundary; otherwise nothing is
    // re-chained and only the cut runs.
    let preserved = if meta.first == newest.first {
        match resolve(b, meta.idx) {
            Some(p) => Some(p),
            None => return None, // unresolvable segment: the whole pass aborts
        }
    } else {
        None
    };
    let preserved = preserved.filter(|p| !p.uuids.is_empty());
    if let Some(p) = &preserved {
        if p.uuids.iter().any(|u| b.resolve(u).is_none()) {
            return None; // a listed uuid is not in this file: abort, map untouched
        }
    }
    // The cut runs FIRST here, where the loader deletes first and re-parents afterwards:
    // same order of effect, and it keeps a removed record's own `parentUuid` intact.
    // Claude Code never looks at a deleted record again, but csift still reads it - and
    // re-pointing every cut record at the last preserved uuid would collapse a whole
    // session's openers onto ONE parent, which the blind-region draft rule then reads as
    // hundreds of same-parent resends.
    cut(b, newest.first, preserved.as_ref());
    let mut anchor = None;
    if let Some(p) = &preserved {
        rechain(b, p);
        anchor = keyed(b, &p.uuids[p.uuids.len() - 1]).map(|(k, _)| k);
        reparent_orphans(b, p);
    }
    anchor
}

/// A surviving user/assistant record whose parent the cut removed is re-parented to the
/// last preserved uuid - the loader's fourth step. Records the cut removed are left
/// alone: they are out of the map, and csift keeps their real parent for its own reading.
fn reparent_orphans(b: &mut Builder<'_>, p: &Preserved) {
    let Some(last_uuid) = keyed(b, &p.uuids[p.uuids.len() - 1]).map(|(k, _)| k) else {
        return;
    };
    for i in 0..b.len() {
        if b.removed[i] || !b.is_conv(i) {
            continue;
        }
        // The parent is in the map but the cut removed it: `resolve` is the one lookup
        // that answers both halves.
        let orphaned =
            b.parent[i].is_some_and(|pu| b.map.get(pu).is_some_and(|slot| b.removed[slot.last]));
        if orphaned {
            b.parent[i] = Some(last_uuid);
        }
    }
}

/// `(the newest boundary, the last boundary carrying a preserved set)`.
fn boundaries(b: &Builder<'_>) -> (Option<Bnd>, Option<Bnd>) {
    let mut newest = None;
    let mut with_meta = None;
    for idx in 0..b.len() {
        if !b.admit[idx] || !b.is_boundary(idx) {
            continue;
        }
        let slot = b.uuid(idx).and_then(|u| b.map.get(u));
        let bnd = Bnd {
            idx,
            first: slot.map_or(idx, |s| s.first),
        };
        newest = Some(bnd);
        let last = slot.map_or(idx, |s| s.last);
        if b.recs[last].compact_metadata.as_ref().is_some_and(|m| {
            m.get("preservedMessages").is_some() || m.get("preservedSegment").is_some()
        }) {
            with_meta = Some(bnd);
        }
    }
    (newest, with_meta)
}

/// The preserved uuids: a listed `preservedMessages`, else a `preservedSegment` walked
/// from its tail up to its head.
fn resolve(b: &Builder<'_>, idx: usize) -> Option<Preserved> {
    let last = b
        .uuid(idx)
        .and_then(|u| b.map.get(u))
        .map_or(idx, |s| s.last);
    let meta = b.recs[last].compact_metadata.as_ref()?;
    if let Some(list) = meta.get("preservedMessages") {
        let anchor = list.get("anchorUuid")?.as_str()?.to_string();
        let uuids = list
            .get("uuids")?
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        return Some(Preserved { anchor, uuids });
    }
    let seg = meta.get("preservedSegment")?;
    let anchor = seg.get("anchorUuid")?.as_str()?.to_string();
    let head = seg.get("headUuid")?.as_str()?;
    let tail = seg.get("tailUuid")?.as_str()?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut cur = b.resolve(tail);
    while let Some(i) = cur {
        let u = b.uuid(i)?.to_string();
        if !seen.insert(u.clone()) {
            return None;
        }
        out.push(u);
        if b.uuid(i) == Some(head) {
            out.reverse();
            return Some(Preserved { anchor, uuids: out });
        }
        cur = b.parent[i].and_then(|p| b.resolve(p));
    }
    None
}

/// The map's own key for a uuid (a slice of the caller's records, so it outlives the
/// builder) plus the line that uuid resolves to.
fn keyed<'a>(b: &Builder<'a>, uuid: &str) -> Option<(&'a str, usize)> {
    b.map.get_key_value(uuid).map(|(k, s)| (*k, s.last))
}

/// Re-chain the preserved uuids into a path off the anchor, then point everything that
/// referenced the anchor at the path's tail.
fn rechain<'a>(b: &mut Builder<'a>, p: &Preserved) {
    let first_uuid = keyed(b, &p.uuids[0]).map(|(k, _)| k);
    let last_uuid = keyed(b, &p.uuids[p.uuids.len() - 1]).map(|(k, _)| k);
    let anchor = keyed(b, &p.anchor).map(|(k, _)| k);
    let mut prev: Option<&'a str> = anchor;
    for u in &p.uuids {
        let Some((key, last)) = keyed(b, u) else {
            continue;
        };
        b.parent[last] = prev;
        prev = Some(key);
    }
    let (Some(anchor), Some(last_uuid)) = (anchor, last_uuid) else {
        return;
    };
    for i in 0..b.len() {
        if !b.removed[i] && b.parent[i] == Some(anchor) && b.uuid(i) != first_uuid {
            b.parent[i] = Some(last_uuid);
        }
    }
}

/// Remove every record whose first line sits before the newest boundary and is not
/// preserved. csift marks rather than deletes: a removed record is `PreCut`, still
/// readable, still selectable, and it keeps its own `parentUuid`.
fn cut(b: &mut Builder<'_>, newest_first: usize, preserved: Option<&Preserved>) {
    let keep: HashSet<&str> = preserved
        .map(|p| p.uuids.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let dropped: HashSet<usize> = b
        .map
        .iter()
        .filter(|(u, slot)| slot.first < newest_first && !keep.contains(*u))
        .map(|(_, slot)| slot.last)
        .collect();
    if dropped.is_empty() {
        return;
    }
    // Mark by INDEX (the map already knows every line a uuid occupies): a uuid-keyed
    // re-scan would hash every record's uuid a second time.
    for &i in &dropped {
        b.removed[i] = true;
    }
    let copies: Vec<usize> = b
        .replay_of
        .iter()
        .filter(|(_, &s)| b.removed[s])
        .map(|(&i, _)| i)
        .collect();
    for i in copies {
        b.removed[i] = true;
    }
}
