//! `list` subcommand — enumerate sessions with quick identity fields.
//!
//! For each session jsonl, emit: session-id, FIRST genuine-user message, LAST
//! genuine-user message, LAST agent message (each with its timestamp), plus the
//! decoded cwd / version / gitBranch — the fast "which session is this?" view.
//! Uses a forward HEAD read for the first user message and a backward TAIL read
//! for the last user/agent messages (never a full parse). Timestamps render in
//! Australia/Sydney local alongside raw UTC. Files are processed in parallel
//! across the corpus (`rayon`), then sorted for deterministic output.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::cli::{ListArgs, OutputFormat};
use crate::model::Record;
use crate::parse::{head_records, tail_records};
use crate::path::{self, ProjectDir};

/// Max characters of a message excerpt shown inline before truncation. Truncation
/// is ALWAYS explicit (`… (+N chars)`) — never silent (SPEC §0, §8.1).
const EXCERPT_MAX: usize = 200;

/// One row of `list` output.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    /// Absolute path to the session jsonl.
    pub path: PathBuf,
    /// Decoded human-readable cwd (read from the data, §2.4), if present.
    pub cwd: Option<String>,
    pub version: Option<String>,
    pub git_branch: Option<String>,
    pub first_user: Option<MessagePreview>,
    pub last_user: Option<MessagePreview>,
    pub last_agent: Option<MessagePreview>,
    /// Count of malformed lines skipped while reading this session (never hidden).
    pub skipped_lines: usize,
}

/// A short, timestamped preview of one message for the `list` view.
#[derive(Debug, Clone)]
pub struct MessagePreview {
    /// Raw ISO8601 UTC timestamp, if the record had one.
    pub timestamp_utc: Option<String>,
    /// One-line excerpt of the message text (already whitespace-normalized).
    pub excerpt: String,
}

impl MessagePreview {
    fn from(timestamp_utc: Option<String>, text: &str) -> Self {
        MessagePreview {
            timestamp_utc,
            excerpt: truncate_excerpt(text),
        }
    }
}

/// Truncate to [`EXCERPT_MAX`] chars with an explicit `… (+N chars)` marker.
/// Counts CHARACTERS (not bytes) so multi-byte CJK text truncates cleanly.
fn truncate_excerpt(s: &str) -> String {
    let total = s.chars().count();
    if total <= EXCERPT_MAX {
        return s.to_string();
    }
    let kept: String = s.chars().take(EXCERPT_MAX).collect();
    let dropped = total - EXCERPT_MAX;
    format!("{kept}… (+{dropped} chars)")
}

/// Entry point for `csift list`.
pub fn run_list(args: &ListArgs) -> Result<()> {
    // 1. Resolve targets → concrete project dirs. 0 args ⇒ every project.
    let project_dirs = resolve_list_targets(&args.paths)?;

    // 2. Enumerate every top-level *.jsonl session file under each dir. A dir with
    //    no session files (childless / subagent-only / memory-only, §1) is fine —
    //    it simply contributes nothing.
    let mut session_files: Vec<PathBuf> = Vec::new();
    for pd in &project_dirs {
        session_files.extend(session_files_in(&pd.dir)?);
    }
    session_files.sort();
    session_files.dedup();

    // 3. Parallel across files (head+tail read each), collect order-stable.
    let mut summaries: Vec<SessionSummary> = session_files
        .par_iter()
        .map(|p| summarize_session(p))
        .collect::<Result<Vec<_>>>()?;
    // Deterministic order regardless of rayon completion order: by path.
    summaries.sort_by(|a, b| a.path.cmp(&b.path));

    // 4. Render.
    match args.format {
        OutputFormat::Text => render_text(&summaries),
        OutputFormat::Json => render_json(&summaries)?,
    }
    Ok(())
}

/// Resolve the `list` positional targets into project dirs (0 args ⇒ all).
fn resolve_list_targets(paths: &[PathBuf]) -> Result<Vec<ProjectDir>> {
    if paths.is_empty() {
        return path::all_project_dirs();
    }
    let mut dirs = Vec::with_capacity(paths.len());
    for p in paths {
        dirs.push(path::resolve_target(p)?);
    }
    Ok(dirs)
}

/// Top-level `*.jsonl` files directly under a project dir (no recursion into the
/// per-session sidecar dirs — those hold subagent/tool-result artifacts, §1).
fn session_files_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read project dir {}", dir.display()))?;
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", dir.display()))?;
        let p = entry.path();
        let is_file = match entry.file_type() {
            Ok(ft) => ft.is_file(),
            Err(_) => p.is_file(),
        };
        if is_file && p.extension().is_some_and(|e| e == "jsonl") {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Build a [`SessionSummary`] for one session file via HEAD + TAIL reads only.
pub fn summarize_session(path: &Path) -> Result<SessionSummary> {
    // The session id is authoritatively the jsonl basename (== uuid; verified the
    // env var CLAUDE_CODE_SESSION_ID equals it). Fall back to the data only if the
    // filename is somehow not a stem.
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_default();

    // ── HEAD read: first genuine-user message + identity fields ──
    let mut first_user: Option<MessagePreview> = None;
    let mut cwd: Option<String> = None;
    let mut version: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut data_session_id: Option<String> = None;

    let head_skipped = head_records(path, |rec| {
        if let Some(text) = rec.genuine_user_text() {
            // Capture identity off the first genuine-user record (it carries cwd /
            // version / gitBranch / sessionId in real data).
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
    let tail_skipped = tail_records(path, |rec| {
        if last_agent.is_none() {
            if let Some(text) = rec.agent_text() {
                last_agent = Some(MessagePreview::from(rec.timestamp.clone(), &text));
            }
        }
        if last_user.is_none() {
            if let Some(text) = rec.genuine_user_text() {
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

    Ok(SessionSummary {
        session_id,
        path: path.to_path_buf(),
        cwd,
        version,
        git_branch,
        first_user,
        last_user,
        last_agent,
        skipped_lines: head_skipped + tail_skipped,
    })
}

fn capture_identity_if_empty(
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

// ── Timestamp rendering (Australia/Sydney local + raw UTC, via jiff) ──

/// Render a raw ISO8601 UTC timestamp as `YYYY-MM-DD HH:MM:SS <TZ> (RAW_UTC)` in
/// Australia/Sydney local time. If the timestamp is absent or unparseable, the
/// raw string (or `—`) is shown — never a panic, never a fabricated time.
fn format_timestamp(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "—".to_string();
    };
    match raw.parse::<jiff::Timestamp>() {
        Ok(ts) => match jiff::tz::TimeZone::get("Australia/Sydney") {
            Ok(tz) => {
                let zoned = ts.to_zoned(tz);
                let local = zoned.strftime("%Y-%m-%d %H:%M:%S %Z");
                format!("{local} ({raw})")
            }
            // tzdb missing the zone (extremely unusual): show UTC only, labelled.
            Err(_) => format!("{raw} (UTC; Australia/Sydney tz unavailable)"),
        },
        // Unparseable timestamp: surface the raw bytes rather than drop them.
        Err(_) => format!("{raw} (unparsed)"),
    }
}

// ── Text rendering ──

fn render_text(summaries: &[SessionSummary]) {
    if summaries.is_empty() {
        println!("no sessions found");
        return;
    }
    for (i, s) in summaries.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("SESSION  {}", s.session_id);

        // cwd + (branch, version) on one meta line.
        let cwd = s.cwd.as_deref().unwrap_or("<unknown cwd>");
        let mut meta = String::new();
        if let Some(b) = &s.git_branch {
            meta.push_str(&format!("branch {b}"));
        }
        if let Some(v) = &s.version {
            if !meta.is_empty() {
                meta.push_str(", ");
            }
            meta.push_str(&format!("CC {v}"));
        }
        if meta.is_empty() {
            println!("  cwd      {cwd}");
        } else {
            println!("  cwd      {cwd}   ({meta})");
        }

        print_preview("first ◂", s.first_user.as_ref());
        print_preview("last ◂ ", s.last_user.as_ref());
        print_preview("last ▸ ", s.last_agent.as_ref());

        if s.skipped_lines > 0 {
            println!("  note     {} malformed line(s) skipped", s.skipped_lines);
        }
    }
}

fn print_preview(label: &str, preview: Option<&MessagePreview>) {
    match preview {
        Some(p) => {
            println!(
                "  {label}  {}",
                format_timestamp(p.timestamp_utc.as_deref())
            );
            println!("           {}", p.excerpt);
        }
        None => {
            println!("  {label}  —");
        }
    }
}

// ── JSON rendering (deterministic, one object per session) ──

fn render_json(summaries: &[SessionSummary]) -> Result<()> {
    use serde_json::json;
    for s in summaries {
        let obj = json!({
            "session_id": s.session_id,
            "path": s.path.to_string_lossy(),
            "cwd": s.cwd,
            "version": s.version,
            "git_branch": s.git_branch,
            "first_user": preview_json(s.first_user.as_ref()),
            "last_user": preview_json(s.last_user.as_ref()),
            "last_agent": preview_json(s.last_agent.as_ref()),
            "skipped_lines": s.skipped_lines,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    Ok(())
}

fn preview_json(preview: Option<&MessagePreview>) -> serde_json::Value {
    use serde_json::json;
    match preview {
        None => serde_json::Value::Null,
        Some(p) => {
            let ts_local = p.timestamp_utc.as_deref().and_then(local_iso);
            json!({
                "ts_utc": p.timestamp_utc,
                "ts_local": ts_local,
                "excerpt": p.excerpt,
            })
        }
    }
}

/// Australia/Sydney local time as an ISO8601-ish string for JSON, or `None` if the
/// raw UTC is missing/unparseable.
fn local_iso(raw: &str) -> Option<String> {
    let ts = raw.parse::<jiff::Timestamp>().ok()?;
    let tz = jiff::tz::TimeZone::get("Australia/Sydney").ok()?;
    Some(ts.to_zoned(tz).strftime("%Y-%m-%dT%H:%M:%S%:z").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_excerpt_unchanged() {
        assert_eq!(truncate_excerpt("hello"), "hello");
    }

    #[test]
    fn truncate_long_excerpt_marks_dropped_count() {
        let s = "x".repeat(EXCERPT_MAX + 5);
        let out = truncate_excerpt(&s);
        assert!(out.ends_with("… (+5 chars)"), "got: {out}");
        assert!(out.starts_with(&"x".repeat(EXCERPT_MAX)));
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        // CJK chars are 3 bytes in UTF-8; truncation must count chars.
        let s = "x".repeat(EXCERPT_MAX + 2);
        let out = truncate_excerpt(&s);
        assert!(out.ends_with("… (+2 chars)"), "got: {out}");
    }

    #[test]
    fn format_timestamp_renders_sydney_local_and_raw() {
        // 2026-06-07 is winter in Sydney → AEST (UTC+10). 05:48 UTC → 15:48 AEST.
        let out = format_timestamp(Some("2026-06-07T05:48:22.880Z"));
        assert!(out.contains("2026-06-07 15:48:22"), "got: {out}");
        assert!(
            out.contains("2026-06-07T05:48:22.880Z"),
            "raw missing: {out}"
        );
    }

    #[test]
    fn format_timestamp_missing_is_em_dash() {
        assert_eq!(format_timestamp(None), "—");
    }

    #[test]
    fn format_timestamp_unparseable_surfaces_raw() {
        let out = format_timestamp(Some("not-a-time"));
        assert!(out.contains("not-a-time"));
        assert!(out.contains("unparsed"));
    }

    #[test]
    fn local_iso_winter_offset() {
        let out = local_iso("2026-06-07T05:48:22.880Z").expect("local iso");
        // AEST = +10:00 in June.
        assert!(out.contains("15:48:22"), "got: {out}");
        assert!(out.ends_with("+10:00"), "got: {out}");
    }
}
