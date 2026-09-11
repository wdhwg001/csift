//! Per-session head/tail summarization + identity capture.

use super::*;

/// Build a [`SessionSummary`] for one session file via HEAD + TAIL reads only.
pub fn summarize_session(path: &Path) -> Result<SessionSummary> {
    // The session id is authoritatively the jsonl basename (== uuid; verified the
    // env var CLAUDE_CODE_SESSION_ID equals it). For a SUBAGENT transcript the stem is
    // `agent-<hex>`; the shared helper strips the prefix to the bare-hex canonical id
    // (the record `agentId`, what `agents` prints) so a `list` subagent row is joinable.
    let session_id = crate::subagent::session_id_from_path(path);

    // ── HEAD read: first genuine-user message + identity fields ──
    let mut first_user: Option<MessagePreview> = None;
    let mut cwd: Option<String> = None;
    let mut version: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut data_session_id: Option<String> = None;

    // C-44: the `/clear` mint probe rides the SAME head scan (no second read) - the
    // wrapper is a role-bearing user record, so the list prefilter already keeps it.
    let mut minted_by_clear = false;
    let mut clear_wrapper_ts: Option<String> = None;
    let mut saw_first_user_record = false;

    let (head_skipped, head_consumed) =
        head_records_prefiltered(path, line_is_list_candidate, |rec| {
            // The FIRST non-isMeta user record decides the mint: the `/clear` wrapper
            // lands in the NEW transcript (the isMeta `<local-command-caveat>` record
            // may precede it), and any other opener means this file was not minted by a
            // clear. Decided once, never revisited.
            if !saw_first_user_record
                && rec.r#type.as_deref() == Some("user")
                && rec.is_meta != Some(true)
            {
                saw_first_user_record = true;
                if rec
                    .slash_command_name()
                    .is_some_and(|n| n.trim_start_matches('/') == "clear")
                {
                    minted_by_clear = true;
                    clear_wrapper_ts = rec.timestamp.clone();
                }
            }
            // First user message = a genuine human turn, an answered AskUserQuestion, or a
            // tool-use rejection-with-message (§4.1/§4.4/§4.2.4). No PlanIndex in this
            // single-record head scan, so a rejection surfaces its typed instruction without
            // the `[plan: …]` pointer (the pointer is a turns/search affordance). A
            // `<task-notification>` / inbound `<teammate-message>` renders its clean label /
            // inbound-comm form via `preview_text` rather than the raw XML it used to show.
            if let Some(text) = preview_text(rec) {
                // Capture identity off the first user record (it carries cwd / version /
                // gitBranch / sessionId in real data).
                cwd = rec.cwd.clone();
                version = rec.version.clone();
                git_branch = rec.git_branch.clone();
                data_session_id = rec.session_id.clone();
                first_user = Some(MessagePreview::from(rec.timestamp.clone(), &text));
                return false; // stop the head scan
            }
            true
        })?;

    // ── TAIL read: last genuine-user + last agent message (newest-first) ──
    let mut last_user: Option<MessagePreview> = None;
    let mut last_agent: Option<MessagePreview> = None;
    // LAST-seen identity: the tail walks newest-first, so the first version/branch-
    // bearing record it visits IS the newest one. Costs nothing - the same records
    // are already being read for last_user/last_agent.
    let mut version_last: Option<String> = None;
    let mut git_branch_last: Option<String> = None;
    // `head_consumed` as the floor keeps the two windows DISJOINT: a malformed line is
    // counted exactly once (R12 killed the head+tail double-book on files where both
    // scans used to walk the same region).
    let tail_skipped =
        tail_records_prefiltered(path, line_is_list_candidate, head_consumed, |rec| {
            if version_last.is_none() {
                version_last = rec.version.clone();
            }
            if git_branch_last.is_none() {
                git_branch_last = rec.git_branch.clone();
            }
            if last_agent.is_none() {
                if let Some(text) = rec.agent_text() {
                    last_agent = Some(MessagePreview::from(rec.timestamp.clone(), &text));
                }
            }
            if last_user.is_none() {
                if let Some(text) = preview_text(rec) {
                    last_user = Some(MessagePreview::from(rec.timestamp.clone(), &text));
                    // Backfill identity from the tail if the head never found a genuine
                    // user (e.g. a session whose only user turns are near the end).
                    capture_identity_if_empty(
                        rec,
                        &mut cwd,
                        &mut version,
                        &mut git_branch,
                        &mut data_session_id,
                    );
                }
            }
            last_user.is_none() || last_agent.is_none()
        })?;

    // Prefer the filename-derived id; cross-check with the data id (§2.4 spirit).
    let session_id = if session_id.is_empty() {
        data_session_id.unwrap_or_default()
    } else {
        session_id
    };

    // Id-domain discriminator: a subagent transcript's `session_id` is a non-re-feedable
    // bare hex; carry `is_subagent` + the re-feedable parent uuid (the dir before
    // `subagents/`) so a `list` consumer can distinguish + re-feed. A top-level file is its
    // own parent (the same r5 shape `search`/`files`/`turns`/`recover` carry).
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    // ── Transparent elicitation-sidecar merge (§3.10) ──
    // A TOP-LEVEL session's unresolved-pending elicitations (the latest activity, MISSING from
    // the native transcript) annotate the row with `with elicitation sidecar` + the pending
    // kind. A subagent transcript has no sidecar (keyed by the top-level session). The sidecar
    // is tiny → a plain read; its malformed-line count folds into `skipped_lines` (never silent).
    let mut sidecar_skipped = 0usize;
    let pending_elicitations = if is_subagent {
        Vec::new()
    } else {
        let (pending, skipped) = crate::elicitation::unresolved_pending(path)?;
        sidecar_skipped = skipped;
        pending
            .iter()
            .filter_map(crate::elicitation::pending_text)
            .collect()
    };
    // File-existence = hook-installed evidence (resolved pairs stay in the file), the
    // machine-legible third state beside "pending" and "none pending".
    let sidecar_present =
        !is_subagent && crate::elicitation::sidecar_path(path).is_some_and(|p| p.is_file());

    // ── C-19 clone lineage (top-level rows only; forks copy whole session files) ──
    let clone_boundary_uuid = if is_subagent {
        None
    } else {
        clone_head_boundary(path)?
    };
    let clone_of = clone_boundary_uuid
        .as_deref()
        .and_then(|u| clone_origin(path, u));

    // ── C-44 `/clear` lineage (top-level rows only; a subagent is never cleared) ──
    let minted_by_clear = minted_by_clear && !is_subagent;
    let clear_join = match clear_wrapper_ts.as_deref() {
        Some(ts) if minted_by_clear => cleared_from_origin(path, ts),
        _ => ClearJoin::default(),
    };

    Ok(SessionSummary {
        session_id,
        is_subagent,
        parent_session_id,
        path: path.to_path_buf(),
        cwd,
        // The base fields are LAST-seen (what the session is on NOW); the head
        // capture becomes the *_first pair. Either window can be empty - fall back
        // to the other so a one-record session reports the same value everywhere.
        version: version_last.clone().or_else(|| version.clone()),
        version_first: version.or(version_last),
        git_branch: git_branch_last.clone().or_else(|| git_branch.clone()),
        git_branch_first: git_branch.or(git_branch_last),
        first_user,
        last_user,
        last_agent,
        skipped_lines: head_skipped + tail_skipped + sidecar_skipped,
        pending_elicitations,
        sidecar_present,
        clone_boundary_uuid,
        clone_of,
        minted_by_clear,
        cleared_from: clear_join.cleared_from,
        cleared_from_distance_ms: clear_join.distance_ms,
        cleared_from_after: clear_join.after,
        cleared_from_candidates: clear_join.candidates,
    })
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
/// ARE the line's only instants; a line of any other type yields `None`.
pub(crate) fn checkpoint_close_ms(line: &[u8]) -> Option<i64> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    if v.get("type").and_then(serde_json::Value::as_str) != Some("cost-state") {
        return None;
    }
    let num = |k: &str| v.get(k).and_then(serde_json::Value::as_f64);
    #[allow(clippy::cast_possible_truncation)]
    Some((num("startTime")? + num("totalDuration")?) as i64)
}

/// The C-19 clone law: a transcript whose FIRST TIMESTAMPED record is a
/// system/`compact_boundary` was minted by copying another session at a compaction
/// point. Measured on a real 61-file project dir: exactly the one known fork
/// detected, zero false positives; file-birthtime rules were REFUTED (filesystem
/// copies and migrations move birthtimes days past the records). Walks head lines
/// until the first record carrying a timestamp and early-exits - near-free on a
/// normal transcript (a handful of bookkeeping lines lead the file).
pub(crate) fn clone_head_boundary(path: &Path) -> Result<Option<String>> {
    static TS: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"timestamp\""));
    let Some(mmap) = crate::parse::mmap_bytes(path)? else {
        return Ok(None);
    };
    for line in mmap.split(|&b| b == b'\n') {
        if TS.find(line).is_none() {
            continue;
        }
        if let Ok(Some(rec)) = crate::parse::parse_line(line) {
            if rec.timestamp.is_some() {
                let hit = rec.is_compact_boundary();
                return Ok(hit
                    .then(|| rec.uuid.clone().unwrap_or_default())
                    .filter(|u| !u.is_empty()));
            }
        }
    }
    Ok(None)
}

/// Join a detected clone to its ORIGIN: the sibling transcript where the boundary
/// record NATIVELY lives. A prose mention of the uuid parses to a record whose own
/// uuid differs; a co-clone's head probe returns the same boundary uuid and is
/// skipped. Cost (one memmem sweep over the project dir's siblings) is paid ONLY
/// when a clone was detected.
pub(crate) fn clone_origin(path: &Path, boundary_uuid: &str) -> Option<String> {
    let dir = path.parent()?;
    let finder = memchr::memmem::Finder::new(boundary_uuid.as_bytes());
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let sib = entry.path();
        if sib == *path
            || sib.extension().and_then(|e| e.to_str()) != Some("jsonl")
            || !sib.is_file()
        {
            continue;
        }
        let Ok(Some(mmap)) = crate::parse::mmap_bytes(&sib) else {
            continue;
        };
        let bytes: &[u8] = &mmap;
        let mut at = 0usize;
        let mut carrier = false;
        while let Some(pos) = finder.find(&bytes[at..]) {
            let abs = at + pos;
            let start = memchr::memrchr(b'\n', &bytes[..abs]).map_or(0, |i| i + 1);
            let end = memchr::memchr(b'\n', &bytes[abs..]).map_or(bytes.len(), |i| abs + i);
            if let Ok(Some(rec)) = crate::parse::parse_line(&bytes[start..end]) {
                if rec.uuid.as_deref() == Some(boundary_uuid) && rec.is_compact_boundary() {
                    carrier = true;
                    break;
                }
            }
            at = end.min(bytes.len());
            if at >= bytes.len() {
                break;
            }
        }
        if carrier && clone_head_boundary(&sib).ok().flatten().as_deref() != Some(boundary_uuid) {
            return Some(crate::subagent::session_id_from_path(&sib));
        }
    }
    None
}

pub(crate) fn capture_identity_if_empty(
    rec: &Record,
    cwd: &mut Option<String>,
    version: &mut Option<String>,
    git_branch: &mut Option<String>,
    data_session_id: &mut Option<String>,
) {
    if cwd.is_none() {
        *cwd = rec.cwd.clone();
    }
    if version.is_none() {
        *version = rec.version.clone();
    }
    if git_branch.is_none() {
        *git_branch = rec.git_branch.clone();
    }
    if data_session_id.is_none() {
        *data_session_id = rec.session_id.clone();
    }
}

// ── Text rendering ──
