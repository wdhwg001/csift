//! The file-history snapshot post-pass: attach mtime-VERIFIED store content to the
//! version-change markers so the replay layer can detect and rebase across
//! harness-side writes that left no tool record.
//!
//! Trust rules (each refusal degrades to the content-less marker, never a wrong
//! byte): content is fetched only for a marker whose `version` CHANGED versus the
//! previous marker of the same target (consecutive same-version snapshots are the
//! overwhelmingly common case and carry no new information); the store file must
//! exist under `<claude-home>/file-history/<top-level-session>/<backupFileName>`;
//! and its mtime must agree with the marker's `backupTime` within a tolerance -
//! the `@vN` file name COLLIDES across a mid-session generation reset (the version
//! counter restarts; measured 148 resets), so an unverified read can silently
//! return the WRONG generation's bytes. Non-UTF-8 store bytes are refused (the
//! replay buffer is line-textual).

use super::*;

/// mtime-vs-backupTime agreement window. Backups land within milliseconds of their
/// recorded instant; a colliding other-generation file is typically hours-to-days
/// away, so a wide-but-bounded window keeps clock skew tolerable and collisions out.
const MTIME_TOLERANCE_SECS: i64 = 120;

/// Attach verified snapshot content to `events` (in line order) for the transcript
/// at `session_path`. Best-effort: any failure leaves the marker content-less.
pub(crate) fn attach_snapshot_content(events: &mut [FileEvent], session_path: &Path) {
    let owner = crate::subagent::parent_session_id_from_path(session_path)
        .unwrap_or_else(|| crate::subagent::session_id_from_path(session_path));
    let Ok(home) = crate::path::claude_home() else {
        return;
    };
    let store = home.join("file-history").join(owner);
    let mut prev_version: Option<u64> = None;
    for e in events.iter_mut() {
        let EventKind::HistorySnapshotMarker {
            version,
            backup_file,
            backup_time,
            content,
        } = &mut e.kind
        else {
            continue;
        };
        let changed = match (prev_version, *version) {
            (Some(p), Some(v)) => v != p,
            (None, Some(_)) => true, // the first marker is a baseline worth checking
            _ => false,
        };
        if version.is_some() {
            prev_version = *version;
        }
        if !changed {
            continue;
        }
        let (Some(name), Some(bt)) = (backup_file.as_deref(), backup_time.as_deref()) else {
            continue;
        };
        *content = verified_store_content(&store.join(name), bt);
    }
}

/// Read the store file iff its mtime agrees with the recorded backup instant.
pub(crate) fn verified_store_content(path: &Path, backup_time: &str) -> Option<String> {
    let recorded: jiff::Timestamp = backup_time.parse().ok()?;
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let mtime_secs =
        i64::try_from(mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
    if (mtime_secs - recorded.as_second()).abs() > MTIME_TOLERANCE_SECS {
        return None; // a generation-collided (or otherwise re-written) store file.
    }
    String::from_utf8(std::fs::read(path).ok()?).ok()
}
