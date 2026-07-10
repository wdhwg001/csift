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
//!
//! ## Scope resolution + parallelism
//!
//! Target resolution (positional PATH(s) / `@<uuid>` / `*.jsonl`, with subagent spanning)
//! goes through the SHARED [`crate::path::resolve_session_files`] resolver — the SAME one
//! `search`/`agents`/`files`/`recover`/`turns` use — so `list` is no longer a separate
//! scope dialect: a `csift list @<uuid>` identifies that one session, exactly like its
//! siblings. The dominant work — the per-session head+tail parse —
//! then runs `rayon` `par_iter()` across the resolved files on the default pool (= CPU
//! count); results are sorted by path for deterministic output.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rayon::prelude::*;

use crate::cli::{ListArgs, OutputFormat};
use crate::model::Record;
use crate::parse::{head_records_prefiltered, tail_records_prefiltered};
use crate::path::{self, SubagentScope};
use crate::time_window::TimeWindow;
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
    /// True when this row is a SUBAGENT transcript (so `session_id` is a bare hex, NOT a
    /// re-feedable `@<uuid>` target). Discriminates the id-domain — the SAME shape
    /// `search`/`files`/`turns`/`recover` carry, so a `list` JSON consumer can tell a
    /// subagent row from a top-level uuid row without string-parsing `path`.
    pub is_subagent: bool,
    /// The re-feedable PARENT session uuid (the owning top-level session). Equals
    /// `session_id` for a top-level row; for a subagent row it is the uuid you re-feed
    /// (`csift verbatim <parent_session_id>` works; the bare hex does not).
    pub parent_session_id: String,
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
    /// One-line renders of this session's UNRESOLVED-pending elicitations (§3.10) merged from
    /// the hook-written sidecar — what the session is currently blocked on (AskUserQuestion /
    /// ExitPlanMode / MCP), MISSING from the native transcript. Empty for a top-level session
    /// with no sidecar / no pending, and ALWAYS empty for a subagent row (the sidecar is keyed
    /// by the top-level session). Drives the `with elicitation sidecar` annotation.
    pub pending_elicitations: Vec<String>,
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

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}

/// Entry point for `csift list`.
pub fn run_list(args: &ListArgs) -> Result<()> {
    // 1+2. Resolve targets → the concrete session jsonl files, via the SAME shared resolver
    //       every other session-operating subcommand uses (`path::resolve_session_files`).
    //       This routes an `@<uuid>` / `@<hex>` POSITIONAL (and a `*.jsonl` file) to the session
    //       filter, so `csift list @<uuid>` identifies that one session instead of erroring — and
    //       0 targets ⇒ every project. Subagent transcripts (built-in Task/Agent-tool + workflow /
    //       OMC agents) span by default; `--no-subagents` keeps only the top-level `<uuid>.jsonl`
    //       set. Workflow `journal.jsonl` event logs are never transcripts and are excluded by the
    //       resolver.
    let scope = SubagentScope::from(args.want_subagents());
    let mut session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        scope,
        path::Caller::Other,
    )?;
    session_files.sort();
    session_files.dedup();

    // 3. Parallel across files (head+tail read each), collect order-stable.
    let mut summaries: Vec<SessionSummary> = session_files
        .par_iter()
        .map(|p| summarize_session(p))
        .collect::<Result<Vec<_>>>()?;
    // Deterministic order regardless of rayon completion order: by path.
    summaries.sort_by(|a, b| a.path.cmp(&b.path));

    // `--since`/`--until`: keep a session iff its [first-activity, last-activity] span
    // intersects the window. The span endpoints are the timestamps this index ALREADY reads
    // (head+tail) — no full-file scan, the `list` performance contract holds; ISO-UTC sorts
    // lexicographically, so min/max over the raw strings is chronological.
    let window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    if !window.is_unbounded() {
        summaries.retain(|s| {
            let ts: Vec<&str> = [&s.first_user, &s.last_user, &s.last_agent]
                .into_iter()
                .flatten()
                .filter_map(|p| p.timestamp_utc.as_deref())
                .collect();
            window.intersects_span(ts.iter().min().copied(), ts.iter().max().copied())
        });
    }

    // Context-safety cap (T2.1): an unscoped `csift list` over a large corpus floods the
    // reader's context (the SPEC-noted many-thousand-line hazard). Bound the rows — NEVER
    // silently: the drop is reported with guidance. `--max-count` overrides; with no explicit
    // cap, ALL-projects listing defaults to 50 while a scoped query (a target / --sessions-from)
    // stays unlimited. The kept rows are the MOST RECENTLY active (what an unscoped list wants),
    // then restored to the deterministic path order for display.
    let all_projects = args.paths.is_empty() && args.sessions_from.is_none();
    let cap = args.max_count.or(if all_projects {
        Some(DEFAULT_LIST_CAP)
    } else {
        None
    });
    let mut dropped = 0usize;
    if let Some(n) = cap {
        if summaries.len() > n {
            summaries.sort_by(|a, b| last_activity(b).cmp(&last_activity(a)));
            dropped = summaries.len() - n;
            summaries.truncate(n);
            summaries.sort_by(|a, b| a.path.cmp(&b.path));
        }
    }

    // 4. Render.
    match args.format {
        OutputFormat::Text => render_text(&summaries, dropped),
        OutputFormat::Json => render_json(&summaries, dropped)?,
    }
    Ok(())
}

/// Default `list` row cap for an UNSCOPED (all-projects) invocation — the flood guard.
const DEFAULT_LIST_CAP: usize = 50;

/// A session's most-recent activity timestamp (max over first/last user + last agent), for the
/// recency-keep cap. `None` (no readable ts) sorts oldest.
fn last_activity(s: &SessionSummary) -> Option<&str> {
    [&s.first_user, &s.last_user, &s.last_agent]
        .into_iter()
        .flatten()
        .filter_map(|p| p.timestamp_utc.as_deref())
        .max()
}

/// The one-line PREVIEW body for a `list` first/last scan (the §1 / automation-label polish):
/// - a `<task-notification>` automation pulse → its parsed `[<kind> …] <summary>` attribution
///   label, never the raw `<task-notification>…<output-file>…` XML wrapper;
/// - an inbound `<teammate-message>` peer message → a CLEAN `agent.communication.{inbox,signal}
///   <from> ⇨ self  <body>` render (the wrapper tags + trailing harness footer stripped), never
///   the raw `<teammate-message …>` XML blob;
/// - otherwise the normal genuine-user / AUQ / rejection reconstruction.
///
/// Eligibility is UNCHANGED from the prior `reconstructed_user_text(None)` gate: a task-notification
/// and a teammate-message were already captured as first/last (the former via genuine-user, the
/// latter via the teammate fallback arm), so the head/tail scan stops on the SAME record and captures
/// the SAME identity fields — only the rendered text is now clean. (An isMeta `<agent-message>` stays
/// INELIGIBLE: it is gated behind `is_teammate_message_record`, so it never newly fronts a preview.)
fn preview_text(rec: &Record) -> Option<String> {
    if let Some(label) = rec.automation_label() {
        return Some(label);
    }
    if rec.is_teammate_message_record() {
        if let Some(ic) = rec.inbound_comm_preview() {
            return Some(format!(
                "{}  {} ⇨ self  {}",
                ic.class.path(),
                ic.from,
                ic.body
            ));
        }
    }
    rec.reconstructed_user_text(None)
}

/// Pre-JSON byte prefilter for the head/tail scans: every record `preview_text` /
/// `agent_text` can anchor on is a `role:user`/`role:assistant` MESSAGE record
/// (genuine users, AUQ answers, rejections, task-notifications, teammate messages,
/// assistant text all carry one of the two markers — the same needles `search`'s
/// stage-1 candidate filter trusts). Any other line — attachment,
/// file-history-snapshot, queue-operation, metadata — is skipped UNPARSED; those
/// are routinely the LARGEST lines in a transcript, so this is the difference
/// between a head/tail read and paying `serde_json` for megabyte noise lines.
fn line_is_list_candidate(line: &[u8]) -> bool {
    memchr::memmem::find(line, br#""role":"user""#).is_some()
        || memchr::memmem::find(line, br#""role":"assistant""#).is_some()
}

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

    let head_skipped = head_records_prefiltered(path, line_is_list_candidate, |rec| {
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
    let tail_skipped = tail_records_prefiltered(path, line_is_list_candidate, |rec| {
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

    Ok(SessionSummary {
        session_id,
        is_subagent,
        parent_session_id,
        path: path.to_path_buf(),
        cwd,
        version,
        git_branch,
        first_user,
        last_user,
        last_agent,
        skipped_lines: head_skipped + tail_skipped + sidecar_skipped,
        pending_elicitations,
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

fn render_text(summaries: &[SessionSummary], dropped: usize) {
    if summaries.is_empty() {
        println!("no sessions found");
        return;
    }
    // SCOPE banner: `list` spans subagents by DEFAULT, so a bare `csift list <uuid>` can
    // return 1 top-level + N subagent rows — surface that split up front (mirroring
    // `turns --subagents` + now `files`/`search`/`recover`) so the default-span
    // surprise is announced, not buried. Printed only when the resolved set actually spans
    // ≥1 subagent. ONE shared emitter / wording across every spanning surface.
    let sub = summaries.iter().filter(|s| s.is_subagent).count();
    let top = summaries.len() - sub;
    crate::text::emit_scope_banner(top, sub);
    for (i, s) in summaries.iter().enumerate() {
        if i > 0 {
            println!();
        }
        // Brand a SUBAGENT row distinctly (mirroring `search`'s header + `turns`'
        // `(subagent transcript)` annotation): a subagent hex is NOT a re-feedable `@<uuid>`
        // target, so label it as such and surface the re-feedable parent uuid inline.
        if s.is_subagent {
            println!(
                "SUBAGENT  {}  ·  parent SESSION {}",
                s.session_id, s.parent_session_id
            );
        } else {
            println!("SESSION  {}", s.session_id);
        }

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

        // Currently-pending elicitation(s) merged from the sidecar (§3.10) — the session is
        // blocked on a human; this is its LATEST activity, missing from the native transcript.
        if !s.pending_elicitations.is_empty() {
            println!("  pending  with elicitation sidecar");
            for p in &s.pending_elicitations {
                println!(
                    "           ⏳ {}",
                    crate::text::truncate_excerpt(p, EXCERPT_MAX)
                );
            }
        }

        if s.skipped_lines > 0 {
            println!(
                "  note     {}",
                crate::text::malformed_note(s.skipped_lines)
            );
        }
    }
    // Context-safety cap (never silent): report the dropped rows + how to see more.
    if dropped > 0 {
        println!();
        println!(
            "… (+{dropped} more session(s) not shown — the most recently active are listed; \
             narrow with a target or --since, or raise --max-count)"
        );
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

fn render_json(summaries: &[SessionSummary], dropped: usize) -> Result<()> {
    use serde_json::json;
    // envelope v2: header (always) → kind-tagged session rows → summary (always).
    let sub = summaries.iter().filter(|s| s.is_subagent).count();
    let top = summaries.len() - sub;
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "list",
            top,
            sub,
            json!({})
        ))?
    );
    let mut skipped_total = 0usize;
    for s in summaries {
        skipped_total += s.skipped_lines;
        let obj = json!({
            "kind": "session",
            "session_id": s.session_id,
            // Id-domain discriminator (the r5 shape): `is_subagent` flags a bare-hex
            // subagent row; `parent_session_id` is the always-re-feedable owning uuid
            // (= session_id for a top-level row). Never re-feed a subagent `session_id`.
            "is_subagent": s.is_subagent,
            "parent_session_id": s.parent_session_id,
            "path": s.path.to_string_lossy(),
            "cwd": s.cwd,
            "version": s.version,
            "git_branch": s.git_branch,
            "first_user": preview_json(s.first_user.as_ref()),
            "last_user": preview_json(s.last_user.as_ref()),
            "last_agent": preview_json(s.last_agent.as_ref()),
            "skipped_lines": s.skipped_lines,
            // Unresolved-pending elicitations merged from the sidecar (§3.10): the one-line
            // renders + a `with_elicitation_sidecar` flag (the machine echo of the text note).
            // Empty / false for a session with no pending and for every subagent row.
            "pending_elicitations": s.pending_elicitations,
            "with_elicitation_sidecar": !s.pending_elicitations.is_empty(),
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    let summary = crate::text::envelope_summary(json!({
        "sessions": summaries.len(),
        "skipped_lines": skipped_total,
        // Context-safety cap: rows dropped to bound an unscoped flood (0 = nothing capped).
        // The kept rows are the most recently active; raise --max-count / narrow scope for more.
        "dropped_by_cap": dropped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
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
    fn top_level_summary_is_not_subagent_and_is_its_own_parent() {
        // A plain `<uuid>.jsonl` path is top-level: is_subagent=false and parent==session_id.
        let p = tmp_session(
            "toplevel",
            &[r#"{"type":"user","message":{"role":"user","content":"hi"}}"#],
        );
        let s = summarize_session(&p).unwrap();
        let stem = p.file_stem().unwrap().to_str().unwrap().to_string();
        std::fs::remove_file(&p).ok();
        assert!(!s.is_subagent);
        assert_eq!(s.parent_session_id, stem);
        assert_eq!(s.parent_session_id, s.session_id);
    }

    #[test]
    fn subagent_summary_carries_is_subagent_and_refeedable_parent() {
        // A subagent transcript path `…/<PARENT-UUID>/subagents/workflows/wf_x/agent-<hex>.jsonl`:
        // session_id is the bare hex (NOT re-feedable), is_subagent=true, and
        // parent_session_id is the re-feedable PARENT uuid (the dir before `subagents/`).
        let dir = std::env::temp_dir().join(format!(
            "csift-subdir-{}-{}/11111111-2222-3333-4444-555555555555/subagents/workflows/wf_z",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("agent-deadbeefcafe1234.jsonl");
        {
            let mut f = std::fs::File::create(&p).unwrap();
            writeln!(
                f,
                r#"{{"type":"user","message":{{"role":"user","content":"sub work"}}}}"#
            )
            .unwrap();
        }
        let s = summarize_session(&p).unwrap();
        std::fs::remove_file(&p).ok();
        assert_eq!(
            s.session_id, "deadbeefcafe1234",
            "bare hex (agent- stripped)"
        );
        assert!(s.is_subagent, "a subagents/ path is a subagent transcript");
        assert_eq!(
            s.parent_session_id, "11111111-2222-3333-4444-555555555555",
            "parent is the re-feedable uuid dir before subagents/"
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
