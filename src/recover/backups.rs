//! `recover --list-backups`: list Claude Code's OWN file-history checkpoint store for
//! one absolute path.
//!
//! Store layout (verified against the live store): `<claude-home>/file-history/
//! <top-level-session-uuid>/<hash>@v<N>`, where `<hash>` is the first 16 hex chars of
//! sha256(absolute file path) and the file content is the PLAIN checkpoint snapshot
//! (no JSON wrapper). Facts that bound what a listing may claim:
//! - the write path is the STRUCTURED FILE LAYER (the rewind feature): the Edit, Write
//!   and NotebookEdit tools, plus ONE shell shape - an approved in-place `sed` preview,
//!   which the shell tool applies through the same checkpoint path instead of running
//!   the command (ledger FH-024); every other shell write and every manual edit never
//!   lands here, so absence proves nothing about a file's history;
//! - the store is PRUNED: old checkpoints get deleted wholesale (`@v1` often gone);
//! - `@vN` counters RESET per session dir and are reused across dirs, so `vN` is NOT
//!   an order key: only the backup instant (the store file's mtime) orders entries.
//!
//! csift therefore LISTS the store and never merges checkpoint content into a
//! reconstruction: a checkpoint has no transcript anchor, and the transcript stays the
//! ground truth (provenance honesty).

use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// One store entry for the requested path.
#[derive(Debug)]
struct BackupRow {
    session_id: String,
    /// The verbatim version token after `@` (`v3`).
    version: String,
    store_path: PathBuf,
    bytes: u64,
    /// RFC3339 UTC of the store file's mtime (the backup instant); `None` when the
    /// filesystem yields no usable mtime.
    backup_utc: Option<String>,
}

/// The store key: sha256 of the absolute path, first 16 hex chars.
fn path_hash(path: &str) -> String {
    let digest = Sha256::digest(path.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// mtime as an RFC3339 UTC instant (jiff renders `Z`-suffixed, sub-second precision).
fn mtime_utc(md: &std::fs::Metadata) -> Option<String> {
    let st = md.modified().ok()?;
    jiff::Timestamp::try_from(st).ok().map(|t| t.to_string())
}

pub(crate) fn run_list_backups(args: &RecoverArgs) -> Result<()> {
    let Some(file) = args.file.as_deref() else {
        bail!(
            "--list-backups requires --file <ABS_PATH> (the store is keyed by a hash of \
             the absolute path)"
        );
    };
    if file == "@plan" || !Path::new(file).is_absolute() {
        bail!(
            "--list-backups needs a literal ABSOLUTE --file path: the store key is \
             sha256 of the exact absolute path, so `@plan` and relative paths cannot \
             be hashed"
        );
    }
    let hash = path_hash(file);
    let store = crate::path::claude_home()?.join("file-history");

    // Optional session scoping: any positional target / --sessions-from narrows the
    // listing to those sessions' store dirs (the store is keyed by TOP-LEVEL session
    // uuid, so subagent targets fold to their parent). With no target, list every dir:
    // resolution is skipped entirely (the store scan needs no transcript).
    let scope: Option<std::collections::BTreeSet<String>> =
        if args.paths.is_empty() && args.sessions_from.is_none() {
            None
        } else {
            let files = crate::path::resolve_targets_with_session_list(
                &args.paths,
                args.sessions_from.as_deref(),
                args.want_subagents().into(),
                crate::path::Caller::Other,
            )?;
            Some(
                files
                    .iter()
                    .map(|p| {
                        crate::subagent::parent_session_id_from_path(p)
                            .unwrap_or_else(|| crate::subagent::session_id_from_path(p))
                    })
                    .collect(),
            )
        };

    let store_present = store.is_dir();
    let mut rows: Vec<BackupRow> = Vec::new();
    if store_present {
        let prefix = format!("{hash}@");
        for dir in std::fs::read_dir(&store)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue; // stray files at the store root (Finder debris) are not entries
            }
            let session_id = dir.file_name().to_string_lossy().to_string();
            if scope.as_ref().is_some_and(|s| !s.contains(&session_id)) {
                continue;
            }
            for entry in std::fs::read_dir(dir.path())? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(version) = name.strip_prefix(&prefix) else {
                    continue;
                };
                let md = entry.metadata()?;
                rows.push(BackupRow {
                    session_id: session_id.clone(),
                    version: version.to_string(),
                    store_path: entry.path(),
                    bytes: md.len(),
                    backup_utc: mtime_utc(&md),
                });
            }
        }
    }
    // Chronological: the backup instant is the ONLY order key (`vN` resets + reuses);
    // instant-less rows last, then deterministic tie-break.
    rows.sort_by(|a, b| {
        let key = |r: &BackupRow| (r.backup_utc.is_none(), r.backup_utc.clone());
        key(a)
            .cmp(&key(b))
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.version.cmp(&b.version))
    });

    let sessions: std::collections::BTreeSet<&str> =
        rows.iter().map(|r| r.session_id.as_str()).collect();
    match args.format {
        OutputFormat::Text => {
            render_text(file, &hash, &store, store_present, &rows, sessions.len())
        }
        OutputFormat::Json => {
            render_json(file, &hash, &store, store_present, &rows, sessions.len())?;
        }
    }
    Ok(())
}

fn render_text(
    file: &str,
    hash: &str,
    store: &Path,
    store_present: bool,
    rows: &[BackupRow],
    sessions: usize,
) {
    println!("BACKUPS  {file}");
    println!(
        "  store  {}  ·  key {hash}@vN (sha256 of the absolute path, first 16 hex)",
        store.display()
    );
    if rows.is_empty() {
        if store_present {
            println!("  no checkpoints for this path in the store");
        } else {
            println!("  store directory absent (the file-history feature may be disabled)");
        }
        println!(
            "  the store is written by the structured file tools and by an approved in-place \
             sed preview only (other shell writes and manual edits never land here) and \
             PRUNED: absence here is NOT evidence the file was never edited. The \
             transcript is the ground truth: csift recover <target> --file {file} --coverage"
        );
        return;
    }
    println!();
    for r in rows {
        println!(
            "  {}  {:>9} B  @{}  {}",
            crate::timez::format_timestamp(r.backup_utc.as_deref()),
            r.bytes,
            r.version,
            r.store_path.display()
        );
    }
    println!();
    println!(
        "  {} checkpoint(s) across {} session dir(s) · ordered by backup instant (store \
         mtime; @vN counters reset per session dir and are NOT an order key)",
        rows.len(),
        sessions
    );
    println!(
        "  provenance: Claude Code's own rewind checkpoint store, written by the structured \
         file tools and by an approved in-place sed preview only (other shell writes and \
         manual edits never land here) and pruned over time, so this listing is NOT \
         a history. csift never merges checkpoint content into a reconstruction; copy a \
         store path yourself to inspect one."
    );
}

fn render_json(
    file: &str,
    hash: &str,
    store: &Path,
    store_present: bool,
    rows: &[BackupRow],
    sessions: usize,
) -> Result<()> {
    let header = crate::text::envelope_header(
        "recover",
        json!({
            "mode": "list-backups",
            "file": file,
            "hash": hash,
            "store": store.display().to_string(),
            "store_present": store_present,
        }),
    );
    println!("{}", serde_json::to_string(&header)?);
    for r in rows {
        let obj = json!({
            "kind": "backup",
            "session_id": r.session_id,
            "version": r.version,
            "path": r.store_path.display().to_string(),
            "bytes": r.bytes,
            "backup_utc": r.backup_utc,
            "backup_local": r.backup_utc.as_deref().and_then(crate::timez::local_iso),
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    let summary = crate::text::envelope_summary(json!({
        "backups": rows.len(),
        "sessions": sessions,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
