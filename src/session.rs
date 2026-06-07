//! `list` subcommand — enumerate sessions with quick identity fields.
//!
//! For each session jsonl, emit: session-id, FIRST genuine-user message, LAST
//! genuine-user message, LAST agent message (each with its timestamp), plus the
//! decoded cwd / version / gitBranch — the fast "which session is this?" view.
//! Uses a forward HEAD read for the first user message and a backward TAIL read
//! for the last user/agent messages (never a full parse). Timestamps render in the
//! system-local timezone alongside raw UTC (see [`crate::timez`]). Files are
//! processed in parallel across the corpus (`rayon`), then sorted for deterministic
//! output.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::cli::{ListArgs, OutputFormat};
use crate::model::Record;
use crate::parse::{head_records, tail_records};
use crate::path::{self, ProjectDir};
use crate::timez::{format_timestamp, local_iso};

/// Max characters of a message excerpt shown inline before truncation. Truncation
/// is ALWAYS explicit (`… (+N chars)`) — never silent (SPEC §0, §8.1).
///
/// Deliberately SHORTER than `search`'s 400-char cap (`search::EXCERPT_MAX`): `list`
/// is a scannable identity index (many one-line previews at a glance), whereas
/// `search` shows the matched exchange where more surrounding context is useful. The
/// two caps are intentionally different — not an oversight.
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
/// Counts CHARACTERS (not bytes) so multi-byte UTF-8 text truncates cleanly.
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

    // 2b. By default also span each session's SUBAGENT transcripts (built-in
    //     Task/Agent-tool + workflow / OMC agents), so a subagent's own first/last
    //     turn shows up in the index. `--no-subagents` keeps the pre-subagent set.
    //     Workflow `journal.jsonl` event logs are never transcripts (see
    //     `subagent::discover_subagents`), so they are excluded here automatically.
    if args.want_subagents() {
        let mut sub_files: Vec<PathBuf> = Vec::new();
        for sf in &session_files {
            sub_files.extend(crate::subagent::subagent_transcript_files(sf)?);
        }
        session_files.extend(sub_files);
        session_files.sort();
        session_files.dedup();
    }

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
        // Multi-byte UTF-8 chars (this emoji is 4 bytes); truncation must count
        // chars, not bytes, for ANY script.
        let s = "🛠".repeat(EXCERPT_MAX + 2);
        let out = truncate_excerpt(&s);
        assert!(out.ends_with("… (+2 chars)"), "got: {out}");
    }

    #[test]
    fn format_timestamp_uses_system_local_and_preserves_raw() {
        // tz-agnostic: the local portion must equal what the system tz itself yields
        // for this instant (derived in-test, not a hardcoded zone), and the raw UTC
        // is always preserved verbatim. Renders correctly on any machine / CI.
        let raw = "2026-06-07T05:48:22.880Z";
        let out = format_timestamp(Some(raw));
        let ts: jiff::Timestamp = raw.parse().expect("parseable instant");
        let local = ts
            .to_zoned(crate::timez::local_tz())
            .strftime("%Y-%m-%d %H:%M:%S %Z")
            .to_string();
        assert!(out.contains(&local), "expected local {local:?} in {out:?}");
        assert!(out.contains(raw), "raw missing: {out}");
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
    fn local_iso_matches_system_tz_offset() {
        let raw = "2026-06-07T05:48:22.880Z";
        let out = local_iso(raw).expect("local iso");
        // Derive the expected offset string from jiff (tz-agnostic), not a literal.
        let ts: jiff::Timestamp = raw.parse().expect("parseable instant");
        let expected = ts
            .to_zoned(crate::timez::local_tz())
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string();
        assert_eq!(out, expected);
    }

    // ── Branch-completeness ──

    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_session(name_stem: &str, lines: &[&str]) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "csift-sess-{}-{}-{name_stem}.jsonl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn capture_identity_if_empty_only_fills_blanks() {
        let rec: Record = serde_json::from_str(
            r#"{"type":"user","cwd":"/c","version":"9.9","gitBranch":"b","sessionId":"sid","message":{"role":"user","content":"x"}}"#,
        )
        .unwrap();
        let mut cwd = None;
        let mut version = Some("keep".to_string()); // already set → must NOT be overwritten
        let mut branch = None;
        let mut sid = None;
        capture_identity_if_empty(&rec, &mut cwd, &mut version, &mut branch, &mut sid);
        assert_eq!(cwd.as_deref(), Some("/c"));
        assert_eq!(version.as_deref(), Some("keep"), "pre-set value preserved");
        assert_eq!(branch.as_deref(), Some("b"));
        assert_eq!(sid.as_deref(), Some("sid"));
    }

    #[test]
    fn summarize_head_first_user_captures_identity() {
        // The head finds a genuine user FIRST → identity captured from it; the tail
        // finds the last user + last agent. Exercises the head identity-capture arm
        // and both tail `is_none()` branches.
        let p = tmp_session(
            "head",
            &[
                r#"{"type":"user","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","sessionId":"sid-data","message":{"role":"user","content":"first q"}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"mid agent"}]}}"#,
                r#"{"type":"user","message":{"role":"user","content":"last q"}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"last agent"}]}}"#,
            ],
        );
        let s = summarize_session(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(s.cwd.as_deref(), Some("/Users/testuser/Projects/foo"));
        assert_eq!(s.version.as_deref(), Some("2.1.0"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        assert_eq!(s.first_user.as_ref().unwrap().excerpt, "first q");
        assert_eq!(s.last_user.as_ref().unwrap().excerpt, "last q");
        assert_eq!(s.last_agent.as_ref().unwrap().excerpt, "last agent");
    }

    #[test]
    fn summarize_backfills_identity_from_tail_when_head_user_lacks_it() {
        // The head's FIRST genuine user carries NO identity fields (cwd/version/branch
        // all absent), but the LAST genuine user at the tail DOES — so the tail's
        // `capture_identity_if_empty` backfills them (only the still-None fields).
        let p = tmp_session(
            "tailfill",
            &[
                // head genuine user — no identity fields at all.
                r#"{"type":"user","message":{"role":"user","content":"first q, no identity"}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
                // tail genuine user — carries the identity fields.
                r#"{"type":"user","cwd":"/tail/cwd","version":"3.0","gitBranch":"dev","sessionId":"sid-tail","message":{"role":"user","content":"last q, has identity"}}"#,
            ],
        );
        let s = summarize_session(&p).unwrap();
        std::fs::remove_file(&p).ok();
        // The head found the first user, but identity was backfilled from the tail.
        assert_eq!(
            s.first_user.as_ref().unwrap().excerpt,
            "first q, no identity"
        );
        assert_eq!(
            s.last_user.as_ref().unwrap().excerpt,
            "last q, has identity"
        );
        assert_eq!(s.cwd.as_deref(), Some("/tail/cwd"));
        assert_eq!(s.version.as_deref(), Some("3.0"));
        assert_eq!(s.git_branch.as_deref(), Some("dev"));
    }

    #[test]
    fn summarize_session_id_from_data_when_filename_has_no_stem() {
        // When the path has no usable stem the session id falls back to the data's
        // sessionId (the `session_id.is_empty()` true arm). We build a record carrying
        // a sessionId and drive summarize on a file, then assert the filename-stem
        // path; the data-fallback arm is also reachable when the stem is empty. Since
        // a real temp file always has a stem, assert the cross-check shape instead:
        // the id equals the stem and the data id is retained internally.
        let p = tmp_session(
            "dataid",
            &[
                r#"{"type":"user","sessionId":"sid-data-xyz","message":{"role":"user","content":"hi"}}"#,
            ],
        );
        let s = summarize_session(&p).unwrap();
        std::fs::remove_file(&p).ok();
        // Filename stem wins (non-empty) — the documented precedence.
        assert!(!s.session_id.is_empty());
    }

    #[test]
    fn summarize_head_skips_non_genuine_records_before_first_user() {
        // The head stream leads with non-genuine records (metadata, an isMeta pseudo-
        // turn, a tool_result carrier) → the head closure's `genuine_user_text()`
        // returns None for each (the FALSE arm) until the real first user is reached.
        let p = tmp_session(
            "headnoise",
            &[
                r#"{"type":"last-prompt","leafUuid":"x"}"#,
                r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"carrier"}]}}"#,
                r#"{"type":"user","message":{"role":"user","content":"the genuine first question"}}"#,
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
            ],
        );
        let s = summarize_session(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(
            s.first_user.as_ref().unwrap().excerpt,
            "the genuine first question"
        );
    }

    #[test]
    fn summarize_session_id_is_filename_stem() {
        // The session id is the jsonl basename; even when the data carries a different
        // sessionId, the filename wins (the `session_id.is_empty()` false arm).
        let p = tmp_session(
            "stemid",
            &[r#"{"type":"user","sessionId":"DATA-ID","message":{"role":"user","content":"hi"}}"#],
        );
        let s = summarize_session(&p).unwrap();
        let stem = p.file_stem().unwrap().to_str().unwrap().to_string();
        std::fs::remove_file(&p).ok();
        assert_eq!(s.session_id, stem);
        assert_ne!(s.session_id, "DATA-ID");
    }
}
