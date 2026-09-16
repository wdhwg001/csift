//! Per-session head/tail summarization + identity capture.
//!
//! A PAIRED ROOT: the head/tail summary and the clone probe live here, and the `/clear`
//! lineage join lives in `summarize/clear.rs` - the largest of the three lineage links a
//! row can carry, and the only one that has to JOIN two files rather than read a field.

mod clear;

pub(crate) use clear::*;

use super::*;

/// Build a [`SessionSummary`] for one session file via HEAD + TAIL reads only.
///
/// `want_lineage` adds the opt-in whole-file LINEAGE pass (`list --lineage`): the
/// `sessionKind` span and an exact `continued_in`, neither of which a head/tail window can
/// answer. It never changes any other field, so a caller that does not want it passes false
/// and gets exactly the head/tail answer.
pub fn summarize_session(path: &Path, want_lineage: bool) -> Result<SessionSummary> {
    // Run FIRST so the fold at the bottom is a plain `or`/`union` over a value that is
    // simply all-default when the flag is off.
    let lineage = if want_lineage {
        lineage_scan(path)?
    } else {
        LineageScan::default()
    };
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
    // The background handoff's parent-side line. File order in the head window, so the
    // LAST one seen is the newest of that window.
    let mut continued_in_head: Option<String> = None;
    // The background-lane stamp is a per-PROCESS fact, so the VALUES a window sees are the
    // answer even when the span is not; collected as a set because one transcript can carry
    // records from a background lane and from a later foreground resume.
    let mut kinds: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let (head_skipped, head_consumed) =
        head_records_prefiltered(path, line_is_list_candidate, |rec| {
            if let Some(child) = rec.continued_in_session_id.clone() {
                continued_in_head = Some(child);
            }
            if let Some(kind) = rec.session_kind.clone() {
                kinds.insert(kind);
            }
            // The FIRST non-isMeta user record decides the mint: the `/clear` wrapper
            // lands in the NEW transcript (the isMeta `<local-command-caveat>` record
            // may precede it), and any other opener means this file was not minted by a
            // clear. Decided once, never revisited.
            if !saw_first_user_record {
                if let Some(is_mint) = clear_mint_verdict(rec) {
                    saw_first_user_record = true;
                    if is_mint {
                        minted_by_clear = true;
                        clear_wrapper_ts = rec.timestamp.clone();
                    }
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
    // The handoff line is written AFTER the last conversation record of the session it
    // hands over, so the newest-first tail walk normally reaches it BEFORE the anchors
    // that stop the walk. First seen wins here: newest-first order makes it the newest.
    let mut continued_in_tail: Option<String> = None;
    // `head_consumed` as the floor keeps the two windows DISJOINT: a malformed line is
    // counted exactly once (R12 killed the head+tail double-book on files where both
    // scans used to walk the same region).
    let tail_skipped =
        tail_records_prefiltered(path, line_is_list_candidate, head_consumed, |rec| {
            if continued_in_tail.is_none() {
                continued_in_tail = rec.continued_in_session_id.clone();
            }
            if let Some(kind) = rec.session_kind.clone() {
                kinds.insert(kind);
            }
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
        let (pending, skipped) = crate::commands::elicitation::unresolved_pending(path)?;
        sidecar_skipped = skipped;
        pending
            .iter()
            .filter_map(crate::commands::elicitation::pending_text)
            .collect()
    };
    // File-existence = hook-installed evidence (resolved pairs stay in the file), the
    // machine-legible third state beside "pending" and "none pending".
    let sidecar_present = !is_subagent
        && crate::commands::elicitation::sidecar_path(path).is_some_and(|p| p.is_file());

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
        // The tail window is where the handoff line lands, so it answers first; the head
        // window covers a transcript short enough for the two to meet. A whole-file pass
        // outranks both, because it read every line either of them could have.
        continued_in: lineage
            .continued_in
            .or(continued_in_tail)
            .or(continued_in_head),
        // The window values and the whole-file values are the same KIND of answer, so they
        // union: a `--lineage` run can only add carriers a window missed.
        session_kind: kinds.union(&lineage.kinds).cloned().collect(),
        session_kind_first_line: lineage.first_line,
        session_kind_last_line: lineage.last_line,
        lineage_scanned: lineage.scanned,
    })
}

/// What one whole-file lineage pass found. All-default (and `scanned` false) when the pass
/// did not run, so the caller's `or`/`union` folds read the same either way.
#[derive(Debug, Default)]
pub(crate) struct LineageScan {
    pub(crate) kinds: std::collections::BTreeSet<String>,
    pub(crate) first_line: Option<usize>,
    pub(crate) last_line: Option<usize>,
    pub(crate) continued_in: Option<String>,
    pub(crate) scanned: bool,
}

/// The `list --lineage` pass: ONE walk over every line of the file, lifting the two
/// top-level lineage fields without parsing a single payload (`parse::lineage_fields`).
///
/// The byte prefilter means the walk itself runs only on the lines carrying one of the two
/// quoted keys, so a transcript with no background lane and no handoff pays the newline
/// scan and two memmem passes and nothing else. The walk cannot fail the way a parse can -
/// a torn line carries no top-level key - which is exactly why this pass contributes no
/// malformed count: `list`'s `skipped_lines` keeps meaning the head/tail lines it read
/// (R12), and `stats` stays the whole-file census authority.
pub(crate) fn lineage_scan(path: &Path) -> Result<LineageScan> {
    let mut out = LineageScan {
        scanned: true,
        ..LineageScan::default()
    };
    let Some(mmap) = crate::parse::mmap_bytes(path)? else {
        return Ok(out);
    };
    let bytes: &[u8] = &mmap;
    for (line_no, line) in bytes.split(|&b| b == b'\n').enumerate() {
        if !crate::parse::line_has_lineage_key(line) {
            continue;
        }
        let Some(f) = crate::parse::lineage_fields(line) else {
            continue;
        };
        if f.no_session_lineage() {
            continue;
        }
        // 1-based, the coordinate every csift surface addresses a line by.
        let n = line_no + 1;
        if let Some(kind) = f.session_kind {
            out.kinds.insert(kind);
            out.first_line.get_or_insert(n);
            out.last_line = Some(n);
        }
        // File order, so the LAST handoff line is the current child.
        if let Some(child) = f.continued_in_session_id {
            out.continued_in = Some(child);
        }
    }
    Ok(out)
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
