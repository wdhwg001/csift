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

    let (head_skipped, head_consumed) =
        head_records_prefiltered(path, line_is_list_candidate, |rec| {
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
    })
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
