//! The `/clear` lineage link, end to end: is this transcript clear-minted, which sibling
//! preceded it, and how far back does the chain go.
//!
//! One concern and the largest of the three lineage links a `list` row can carry - and the
//! only one that is a JOIN rather than a field read, because a clear persists NO id: the
//! predecessor is found by matching the wrapper's instant against a sibling's cost
//! checkpoint, so the window, the tie rule and the chain walk all live here together
//! (AGENTS section 3.14).

use super::*;

/// The `/clear` mint decision for ONE record: `Some(true)` when this record is the
/// deciding one AND it is the `/clear` wrapper, `Some(false)` when it is the deciding
/// one and it is not, `None` while the scan should keep looking. The deciding record
/// is the first `type:"user"` record whose `isMeta` is not set - the isMeta
/// `<local-command-caveat>` record a clear writes above the wrapper is skipped, and a
/// `/clear` wrapper anywhere below the first user record belongs to the clear that
/// LEFT this file, never to the one that minted it. The ONE copy of the rule: the
/// `list` head scan and [`clear_mint_wrapper`] both read it.
pub(crate) fn clear_mint_verdict(rec: &Record) -> Option<bool> {
    (rec.r#type.as_deref() == Some("user") && rec.is_meta != Some(true)).then(|| {
        rec.slash_command_name()
            .is_some_and(|n| n.trim_start_matches('/') == "clear")
    })
}

/// The `/clear` wrapper's timestamp when `path` was minted by a clear, via its own
/// head read. `summarize_session` folds the same verdict into the head scan it already
/// runs; this entry exists for a caller that holds only a path (the `cleared_from`
/// chain walk), and it stops at the deciding record rather than the first genuine user.
pub(crate) fn clear_mint_wrapper(path: &Path) -> Result<Option<String>> {
    let mut ts: Option<String> = None;
    head_records_prefiltered(
        path,
        line_is_list_candidate,
        |rec| match clear_mint_verdict(rec) {
            Some(true) => {
                ts = rec.timestamp.clone();
                false
            }
            Some(false) => false,
            None => true,
        },
    )?;
    Ok(ts)
}

/// How many clears deep the chain walk follows before giving up. A session cleared
/// more often than this in one project directory is not a shape worth a longer walk,
/// and the bound plus the visited set make a cycle impossible.
const CLEAR_CHAIN_MAX: usize = 32;

/// The ROOT of the `cleared_from` chain: follow the join back from `path` until a
/// transcript that was not itself minted by a clear, and name it. `None` when `path`
/// is not clear-minted, or when the first hop finds no predecessor - the chain is only
/// as long as the checkpoints that back it.
pub(crate) fn cleared_from_root(path: &Path) -> Option<String> {
    let mut at = path.to_path_buf();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut root: Option<String> = None;
    for _ in 0..CLEAR_CHAIN_MAX {
        // A hop that finds no wrapper or no predecessor ENDS the walk and keeps what
        // the walk already reached - the root is the last file that answered, not a
        // condition every hop has to satisfy.
        let Some(ts) = clear_mint_wrapper(&at).ok().flatten() else {
            break;
        };
        let Some(prev) = cleared_from_origin(&at, &ts).cleared_from else {
            break;
        };
        if !seen.insert(prev.clone()) {
            break;
        }
        let next = at.with_file_name(format!("{prev}.jsonl"));
        if !next.is_file() {
            break;
        }
        root = Some(prev);
        at = next;
    }
    root
}

/// The window a clear's checkpoint may sit in, either side of the wrapper instant.
/// Measured distance on a live clear: 164 ms; the widest of the three opening records
/// is 200 ms. 2 s leaves an order of magnitude of headroom without admitting the next
/// session's own checkpoint (sessions are minutes apart).
pub(crate) const CLEAR_JOIN_WINDOW_MS: i64 = 2000;

/// What the `/clear` join concluded for one transcript.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClearJoin {
    /// The predecessor transcript's id - `None` when nothing qualified OR when two
    /// files tied (the tie is reported through `candidates` and joined to neither).
    pub(crate) cleared_from: Option<String>,
    /// Absolute distance in ms between the checkpoint's close instant and the wrapper.
    pub(crate) distance_ms: Option<i64>,
    /// True when the checkpoint closes AFTER the wrapper instant (the measured order).
    pub(crate) after: bool,
    /// The tied ids when two different files share the smallest distance.
    pub(crate) candidates: Vec<String>,
}

/// Join a `/clear`-minted transcript to the one it was cleared FROM.
///
/// Claude Code writes NO lineage at a clear: the old id survives only in process
/// memory, and the new transcript carries no `parentSessionId` / `clearedFrom` field.
/// The one on-disk bridge is the cost ledger's CHECKPOINT, written on the OLD session
/// (so it carries the OLD `sessionId`) just before the id swap: `startTime +
/// totalDuration` is the instant the old session closed, and the new file's opening
/// records sit within 200 ms of it. So the rule is that SUM against the `/clear`
/// wrapper's own timestamp, inside [`CLEAR_JOIN_WINDOW_MS`] - never file mtimes, and
/// never the time adjacency of ordinary records, which any two busy sessions share.
/// A tie between two DIFFERENT files is reported and joined to neither (the same
/// fail-loud posture [`clone_origin`] takes). Cost is paid only for a transcript that
/// opens with the wrapper, and only over its siblings' `cost-state` lines.
pub(crate) fn cleared_from_origin(path: &Path, wrapper_ts: &str) -> ClearJoin {
    let mut join = ClearJoin::default();
    let Some(t_wrapper) = crate::timez::epoch_ms(wrapper_ts) else {
        return join;
    };
    let Some(dir) = path.parent() else {
        return join;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return join;
    };
    // (distance, signed delta, id) - the best line of each sibling that qualifies.
    let mut best: Vec<(i64, i64, String)> = Vec::new();
    for entry in entries.flatten() {
        let sib = entry.path();
        if sib == *path
            || sib.extension().and_then(|e| e.to_str()) != Some("jsonl")
            || !sib.is_file()
        {
            continue;
        }
        if let Some((dist, delta)) = nearest_checkpoint_close(&sib, t_wrapper) {
            if dist <= CLEAR_JOIN_WINDOW_MS {
                best.push((dist, delta, crate::subagent::session_id_from_path(&sib)));
            }
        }
    }
    best.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
    let Some((dist, delta, id)) = best.first().cloned() else {
        return join;
    };
    let tied: Vec<String> = best
        .iter()
        .filter(|(d, _, _)| *d == dist)
        .map(|(_, _, i)| i.clone())
        .collect();
    join.distance_ms = Some(dist);
    join.after = delta < 0;
    if tied.len() > 1 {
        join.candidates = tied;
    } else {
        join.cleared_from = Some(id);
    }
    join
}

/// The smallest `|wrapper - (startTime + totalDuration)|` over one transcript's
/// `cost-state` lines, with the signed delta of that line. `None` when the file carries
/// no readable checkpoint. Only the rare lines carrying the literal are parsed.
fn nearest_checkpoint_close(sib: &Path, t_wrapper: i64) -> Option<(i64, i64)> {
    static COST: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"cost-state\""));
    let mmap = crate::parse::mmap_bytes(sib).ok().flatten()?;
    let bytes: &[u8] = &mmap;
    let mut at = 0usize;
    let mut best: Option<(i64, i64)> = None;
    while let Some(pos) = COST.find(&bytes[at..]) {
        let abs = at + pos;
        let start = memchr::memrchr(b'\n', &bytes[..abs]).map_or(0, |i| i + 1);
        let end = memchr::memchr(b'\n', &bytes[abs..]).map_or(bytes.len(), |i| abs + i);
        if let Some(close) = checkpoint_close_ms(&bytes[start..end]) {
            let delta = t_wrapper - close;
            let dist = delta.abs();
            if best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, delta));
            }
        }
        at = end.min(bytes.len());
        if at >= bytes.len() {
            break;
        }
    }
    best
}

/// `startTime + totalDuration` of one `cost-state` line, in epoch ms. The schema is
/// twelve keys with no `uuid`, no `timestamp` and no `message{}`, so the two numbers
/// ARE the line's only instants; a line of any other type yields `None`. The pair is
/// read by [`crate::model::Record::checkpoint_close_ms`], the reading the search fact
/// shares.
pub(crate) fn checkpoint_close_ms(line: &[u8]) -> Option<i64> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    if v.get("type").and_then(serde_json::Value::as_str) != Some("cost-state") {
        return None;
    }
    crate::model::Record::checkpoint_close_ms(v.as_object()?)
}
