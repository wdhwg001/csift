//! `recover` subcommand — reconstruct a single file's history from a transcript.
//!
//! Where `files` only rolls up THAT a file was touched, `recover` rebuilds the file's
//! CONTENT, line by line, by replaying the transcript's Reads / Writes / Edits in
//! transcript order, and emits one of four views:
//!
//! - `--patches` (default) segmented unified-diff history of `--file`, split at INTEGRITY
//!   BOUNDARIES (a point where reconstruction across it is invalid: a `File has been
//!   modified since read` harness error, an `originalFile` that disagrees with the
//!   replayed buffer, an external `edited_text_file`, or a heuristic Bash mutation).
//! - `--at` the PARTIAL, line-numbered "in the LLM's eyes" snapshot as of a cutoff;
//!   unknown lines are EXPLICIT gaps, never fabricated.
//! - `--coverage` scope a recovery (recoverable ranges + boundaries + counts), no dump.
//!
//! ## The one new capability: jsonl line numbers
//!
//! No line-number tracking exists elsewhere in `src/`: [`crate::parse::scan_lines_bytes`]
//! hands the visitor only `&[u8]`. We add a LOCAL counter here (never touching the shared
//! signature, so `files`/`search` are unperturbed): `scan_lines_bytes` visits every
//! `\n`-delimited segment with no skipping, so incrementing on each visit yields an exact
//! 1:1 jsonl line map (blank + malformed lines are counted too). Every emitted reference
//! carries its `Lnnnnn` so a consumer can `Read` the raw jsonl directly.
//!
//! ## Never fabricate
//!
//! Reconstruction is necessarily PARTIAL. A line never Read/Edited is an explicit gap; an
//! edit whose `old_string` spans an unknown gap is an un-anchorable coverage hole; a Bash
//! touch is a HEURISTIC (soft) boundary, not authoritative. No silent truncation anywhere.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;

use crate::cli::{OutputFormat, RecoverArgs, RecoverMode};
use crate::model::{group_turn_indices_deduped, Block, Record};
use crate::parse::mmap_bytes;
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Max characters of inline content shown before an explicit `… (+N chars)` marker in
/// HUMAN text output (JSON + `--out` are verbatim). Mirrors `search::EXCERPT_MAX`.
const EXCERPT_MAX: usize = 400;

// ─────────────────────────────────────────────────────────────────────────────
// Data model
// ─────────────────────────────────────────────────────────────────────────────

/// A file-touching event extracted from ONE jsonl line, in transcript order.
#[derive(Debug, Clone)]
struct FileEvent {
    /// 1-based jsonl line (the new capability).
    line_no: usize,
    /// Genuine-user turn index (`group_turn_indices`).
    turn_index: usize,
    timestamp_utc: Option<String>,
    kind: EventKind,
}

/// What a [`FileEvent`] does to the reconstructed buffer.
#[derive(Debug, Clone)]
enum EventKind {
    /// Full ground-truth content (an anchor): a Write result, a full Read
    /// (`startLine==1 && numLines==totalLines`), or a `file` attachment.
    FullSnapshot {
        content: String,
        total_lines: usize,
        source: SnapSource,
    },
    /// Windowed Read: lines `[start_line, start_line+lines.len())` are known;
    /// `total_lines` is the file length the model saw (for gap detection).
    PartialRead {
        start_line: usize,
        lines: Vec<String>,
        total_lines: usize,
    },
    /// An Edit/MultiEdit applied old→new. `structured_patch` (when present) gives exact
    /// line positions; `original_file` (when present) is used ONLY to cross-check for a
    /// boundary, never to paper over drift.
    Edit {
        hunks: Vec<EditHunk>,
        original_file: Option<String>,
        structured_patch: Option<Vec<PatchHunk>>,
    },
    /// An integrity violation the harness surfaced.
    IntegrityError { kind: IntegrityKind, raw: String },
    /// A heuristic external mutation (Bash redirect/sed -i/tee/...). SOFT signal only.
    BashTouch { verb: String },
    /// An external/user edit captured as an `edited_text_file` attachment snippet.
    ExternalEdit { snippet: Vec<(usize, String)> },
    /// A `file-history-snapshot` recorded a disk backup of `--file` at this time. The
    /// on-disk blob name is NOT derivable from the record (the real `backupFileName` is
    /// frequently null), so this is a COVERAGE ANNOTATION only — never a content anchor.
    HistorySnapshotMarker,
}

/// The provenance of a [`EventKind::FullSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapSource {
    Write,
    FullRead,
    FileAttachment,
}

impl SnapSource {
    fn label(self) -> &'static str {
        match self {
            SnapSource::Write => "write",
            SnapSource::FullRead => "full-read",
            SnapSource::FileAttachment => "file-attachment",
        }
    }
}

/// The two harness integrity-error shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntegrityKind {
    /// "File has been modified since read, …" — a HARD boundary (disk drift detected).
    ModifiedSinceRead,
    /// "File has not been read yet. …" — the edit never landed; NOT a boundary.
    NotReadYet,
}

/// One hunk of an Edit (old→new strings), from the tool_use `input`.
#[derive(Debug, Clone)]
struct EditHunk {
    old_string: String,
    new_string: String,
    replace_all: bool,
}

/// One structured-patch hunk (`toolUseResult.structuredPatch[]`): a mirror of CC's
/// `{oldStart, oldLines, newStart, newLines, lines:[" ","-","+", …]}`. `newStart` is not
/// retained — replay derives the new position from `oldStart` + the running line offset.
#[derive(Debug, Clone)]
struct PatchHunk {
    old_start: usize,
    old_lines: usize,
    new_lines: usize,
    /// Each line prefixed by ` ` (context), `-` (removed), or `+` (added).
    lines: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-file scan
// ─────────────────────────────────────────────────────────────────────────────

/// Per-session scan result before global merge.
#[derive(Debug)]
struct ScanResult {
    session_id: String,
    /// True when this transcript is a SUBAGENT (so `session_id` is a bare hex, NOT a
    /// re-feedable `--session` target) — the r5 id-domain discriminator, now also on recover.
    is_subagent: bool,
    /// The re-feedable PARENT session uuid (= `session_id` for a top-level file).
    parent_session_id: String,
    events: Vec<FileEvent>,
    skipped_lines: usize,
}

/// Entry point for `csift recover`.
pub fn run_recover(args: &RecoverArgs) -> Result<()> {
    // Pointed error if the files-only `--subagents-only` was mistyped here.
    if let Some(msg) = args.span_flag_error() {
        bail!(msg);
    }
    // BATCH MODE: many files in one corpus scan (parse each transcript ONCE).
    if args.files_from.is_some() {
        return run_recover_batch(args);
    }
    // ── Validate window mutual-exclusion (same rule + wording as `files`/`search`) ──
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }
    let mode = args.mode();

    // ── `--out` is a no-op in `--coverage` mode (coverage is a scoping summary, not an
    //    artifact) — make the no-op VISIBLE at runtime so a "save the coverage report" call
    //    is not silently swallowed. The other three modes honor `--out` in render_text/json.
    if matches!(mode, RecoverMode::Coverage) && args.out.is_some() {
        eprintln!(
            "note: --out is ignored in --coverage mode (a scoping summary has no artifact \
             to write); use --patches / --at to write a file."
        );
    }

    // ── `--file` is required for every mode (an absolute path, or the `@plan` sigil) ──
    if args.file.is_none() {
        bail!("--file <ABS_PATH> (or `@plan`) is required for --patches / --at / --coverage");
    }

    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let line_range = args
        .line_range
        .as_deref()
        .map(parse_line_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // ── Resolve targets → session files (spanning subagents by default) ──
    let session_files = path::resolve_session_files(
        &args.paths,
        args.session.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;

    // ── `--file @plan`: substitute the session-bound plan file (the `plan_mode` attachment's
    //    `planFilePath`) as the real target, then reconstruct it exactly like any file — every
    //    Write/Edit on it counts, not just the latest Write. Owns the plan-locating concern so
    //    a deleted plan is recoverable from the transcript alone. ──
    let plan_target: Option<String> = match args.file.as_deref() {
        Some(f) if f == crate::plan::PLAN_SIGIL => {
            let pref = crate::plan::resolve_plan_target(&session_files)?;
            eprintln!(
                "note: {} resolved to {} (bound to session {}{})",
                crate::plan::PLAN_SIGIL,
                pref.plan_file,
                pref.session_id,
                if pref.is_subagent { ", subagent" } else { "" }
            );
            Some(pref.plan_file)
        }
        _ => None,
    };
    let target_file = plan_target.as_deref().or(args.file.as_deref());

    // ── Parallel scan across files (default rayon pool = CPU count) ──
    let per_file: Vec<ScanResult> = session_files
        .par_iter()
        .map(|p| scan_one_file(p, target_file))
        .collect::<Result<Vec<_>>>()?;

    // ── Merge, keeping each session's events grouped (ordering inside a session is the
    //    jsonl line order; across sessions we sort sessions by id for determinism). ──
    let mut skipped_lines = 0usize;
    let mut sessions: Vec<ScanResult> = Vec::new();
    for sr in per_file {
        skipped_lines += sr.skipped_lines;
        sessions.push(sr);
    }
    sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    // ── Apply the turn / time window to events per session ──
    for sr in &mut sessions {
        sr.events.retain(|e| {
            window_admits(
                e.turn_index,
                e.timestamp_utc.as_deref(),
                turn_range,
                &time_window,
            )
        });
    }

    // Scope banner reflects the TRUE in-scope transcript set, captured BEFORE the
    // reconstruction merge folds subagents into their parent.
    let (scope_top, scope_sub) = scope_span(&sessions);

    // ── Cross-session reconstruction merge (At / Coverage) ──
    // A file's history spans a top-level session AND its own subagents, interleaved by
    // wall-clock: main edits a few lines, a subagent edits some, main edits again, a
    // subagent edits again. Per-session replay sees only fragments — a subagent that
    // partial-reads then edits is un-anchorable in its OWN transcript (sparse buffer), and
    // no single transcript holds the whole file. Merge each top-level GROUP (keyed by
    // parent_session_id — a top-level's own id, shared by its subagents) into one
    // timestamp-ordered timeline so the file reconstructs as a single COMPLETE artifact.
    // UNRELATED top-level sessions (different parents) are never merged: separate histories.
    // Patches stays per-session (per-transcript diff-history provenance is the right view).
    let sessions = if matches!(
        mode,
        RecoverMode::At | RecoverMode::Coverage | RecoverMode::Restore | RecoverMode::Salvage
    ) {
        merge_groups_for_reconstruction(sessions)
    } else {
        sessions
    };

    let ctx = RenderCtx {
        mode,
        file: target_file.map(str::to_string),
        line_range,
        at: args.at.clone(),
        skipped_lines,
        scope_top,
        scope_sub,
    };

    match args.format {
        OutputFormat::Text => render_text(&ctx, &sessions, args.out.as_deref())?,
        OutputFormat::Json => render_json(&ctx, &sessions, args.out.as_deref())?,
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Batch reconstruction (`--files-from` / `--out-dir`)
//
// The dominant cost of recovering MANY files one-by-one is re-parsing the same huge
// transcripts on every `recover --file` call (a session that wrote — or merely mentions —
// hundreds of files is re-parsed hundreds of times). Batch mode parses + turn-groups each
// transcript ONCE and extracts every listed file it touched, so the corpus is walked a
// single time regardless of manifest size.
// ─────────────────────────────────────────────────────────────────────────────

/// One target's batch outcome, for the TSV report + the summary line.
struct BatchOutcome {
    target: String,
    status: &'static str, // "complete" | "partial" | "no-history" | "skipped-exists"
    known: usize,
    total: usize,
    written: Option<std::path::PathBuf>,
}

/// The last path component (the filename) — the distinctive token a transcript carries for
/// every op on the file, and the [`aho_corasick`] pattern that gates parsing.
fn basename_of(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn run_recover_batch(args: &RecoverArgs) -> Result<()> {
    let manifest = args
        .files_from
        .as_deref()
        .expect("batch mode requires --files-from");
    let Some(out_dir) = args.out_dir.as_deref() else {
        bail!("--files-from requires --out-dir <DIR> (where to write the recovered files)");
    };
    if args.file.is_some() {
        bail!("--files-from (batch) is mutually exclusive with --file (single-file mode)");
    }
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }

    // ── Manifest: one absolute path per line; blank lines and `#` comments ignored; deduped. ──
    let raw = std::fs::read_to_string(manifest)
        .with_context(|| format!("cannot read manifest {}", manifest.display()))?;
    let mut seen = std::collections::HashSet::new();
    let targets: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .filter(|t| seen.insert(t.clone()))
        .collect();
    if targets.is_empty() {
        bail!("manifest {} lists no files", manifest.display());
    }

    // ── Multi-pattern byte prefilter: which manifest BASENAMES a transcript mentions. A
    //    transcript matching none is skipped without parsing (the single-file prefilter,
    //    generalized to the whole manifest in one Aho-Corasick pass). ──
    let basenames: Vec<String> = targets.iter().map(|t| basename_of(t).to_string()).collect();
    let ac = aho_corasick::AhoCorasick::new(&basenames)
        .context("building the manifest basename matcher")?;

    let when = args.at.clone().unwrap_or_default();
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    let session_files = path::resolve_session_files(
        &args.paths,
        args.session.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;
    eprintln!(
        "recover --files-from: {} file(s), scanning {} transcript(s) once…",
        targets.len(),
        session_files.len()
    );

    // ── ONE parse per transcript; extract every present target from its shared turn grouping. ──
    let per_file: Vec<Vec<(usize, ScanResult)>> = session_files
        .par_iter()
        .map(|p| scan_one_file_multi(p, &targets, &ac))
        .collect::<Result<Vec<_>>>()?;

    let mut by_target: Vec<Vec<ScanResult>> = (0..targets.len()).map(|_| Vec::new()).collect();
    for file_results in per_file {
        for (ti, sr) in file_results {
            by_target[ti].push(sr);
        }
    }

    // ── Reconstruct + write each target (the file's final state, honoring any window). ──
    let mut outcomes: Vec<BatchOutcome> = Vec::with_capacity(targets.len());
    for (ti, mut scans) in by_target.into_iter().enumerate() {
        let target = targets[ti].clone();
        for sr in &mut scans {
            sr.events.retain(|e| {
                window_admits(
                    e.turn_index,
                    e.timestamp_utc.as_deref(),
                    turn_range,
                    &time_window,
                )
            });
        }
        scans.retain(|s| !s.events.is_empty());
        let Some((content, known, total)) = reconstruct_best(scans, &when)? else {
            outcomes.push(BatchOutcome {
                target,
                status: "no-history",
                known: 0,
                total: 0,
                written: None,
            });
            continue;
        };
        // Mirror the absolute path under out_dir (strip leading separators → safe join).
        let dest = out_dir.join(target.trim_start_matches('/'));
        if dest.exists() && !args.force {
            outcomes.push(BatchOutcome {
                target,
                status: "skipped-exists",
                known,
                total,
                written: None,
            });
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(&dest, &content)
            .with_context(|| format!("cannot write {}", dest.display()))?;
        let status = if total > 0 && known >= total {
            "complete"
        } else {
            "partial"
        };
        outcomes.push(BatchOutcome {
            target,
            status,
            known,
            total,
            written: Some(dest),
        });
    }

    write_batch_report(out_dir, &outcomes)
}

/// Scan ONE transcript for ALL manifest targets it mentions: AC-gate, parse + turn-group once,
/// then extract each present target. Returns `(target_index, ScanResult)` for every target with
/// at least one event in this transcript.
fn scan_one_file_multi(
    path: &Path,
    targets: &[String],
    ac: &aho_corasick::AhoCorasick,
) -> Result<Vec<(usize, ScanResult)>> {
    let session_id = crate::subagent::session_id_from_path(path);
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(Vec::new());
    };
    let bytes: &[u8] = &mmap;
    if !ac.is_match(bytes) {
        return Ok(Vec::new());
    }
    // Which manifest basenames appear (overlapping, so a basename that is a substring of
    // another match region is still detected) → the targets worth extracting from this file.
    let mut present: Vec<usize> = ac
        .find_overlapping_iter(bytes)
        .map(|m| m.pattern().as_usize())
        .collect();
    present.sort_unstable();
    present.dedup();

    let (records, skipped) =
        crate::parse::parse_candidates_parallel(bytes, line_is_recover_candidate);
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices_deduped(&recs, |r| *r);

    let mut out = Vec::new();
    for ti in present {
        let events = extract_with_turns(&records, &turns, Some(&targets[ti]));
        if !events.is_empty() {
            out.push((
                ti,
                ScanResult {
                    session_id: session_id.clone(),
                    is_subagent,
                    parent_session_id: parent_session_id.clone(),
                    events,
                    skipped_lines: skipped,
                },
            ));
        }
    }
    Ok(out)
}

/// Reconstruct a target's FINAL content (or its `--at`/window snapshot) as RAW bytes — the
/// restorable file, not the line-numbered diff view. Cross-session writes are merged per
/// top-level group; when unrelated sessions each hold a version, the FRESHEST (latest-write)
/// candidate wins. Returns `(content, known_lines, total_lines)`, or `None` when nothing is
/// recoverable. A partial reconstruction (`known < total`) joins the known lines in order.
fn reconstruct_best(scans: Vec<ScanResult>, when: &str) -> Result<Option<(String, usize, usize)>> {
    let merged = merge_groups_for_reconstruction(scans);
    let mut best: Option<(String, usize, usize, Option<String>)> = None;
    for s in &merged {
        if s.events.is_empty() {
            continue;
        }
        let cutoff = resolve_cutoff(when, &s.events)?;
        let rep = replay(&s.events, cutoff);
        let known = rep.final_buffer.known_lines();
        if known.is_empty() {
            continue;
        }
        let total = rep
            .final_buffer
            .seen_total_lines
            .unwrap_or_else(|| known.last().map(|(n, _)| *n).unwrap_or(0));
        let latest_ts = s
            .events
            .iter()
            .filter_map(|e| e.timestamp_utc.clone())
            .max();
        let mut content = known
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if !content.is_empty() {
            content.push('\n');
        }
        let fresher = match &best {
            None => true,
            Some((_, bk, _, bts)) => (&latest_ts, known.len()) > (bts, *bk),
        };
        if fresher {
            best = Some((content, known.len(), total, latest_ts));
        }
    }
    Ok(best.map(|(c, k, t, _)| (c, k, t)))
}

/// Write `recovery-report.tsv` under `out_dir` and print the one-line summary.
fn write_batch_report(out_dir: &Path, outcomes: &[BatchOutcome]) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let mut body = String::from("status\tknown_lines\ttotal_lines\ttarget\twritten_to\n");
    let (mut complete, mut partial, mut none, mut skipped) = (0usize, 0usize, 0usize, 0usize);
    for o in outcomes {
        match o.status {
            "complete" => complete += 1,
            "partial" => partial += 1,
            "no-history" => none += 1,
            "skipped-exists" => skipped += 1,
            _ => {}
        }
        body.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            o.status,
            o.known,
            o.total,
            o.target,
            o.written
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ));
    }
    let report = out_dir.join("recovery-report.tsv");
    std::fs::write(&report, &body).with_context(|| format!("cannot write {}", report.display()))?;
    println!(
        "recovered {} file(s): {complete} complete · {partial} partial · {none} no-history · \
         {skipped} skipped (already present)",
        outcomes.len()
    );
    println!("report: {}", report.display());
    Ok(())
}

/// True when an event at `turn_index` / `ts` is admitted by the active window.
/// A timestamp-less item never falls inside a BOUNDED time window (same rule as `files`).
fn window_admits(
    turn_index: usize,
    ts: Option<&str>,
    turn_range: Option<(usize, usize)>,
    time_window: &TimeWindow,
) -> bool {
    if let Some((lo, hi)) = turn_range {
        if turn_index < lo || turn_index > hi {
            return false;
        }
    }
    time_window.contains(ts)
}

/// Scan one session file: mmap → forward line-numbered scan → extract `--file` events.
/// The forward `scan_lines_bytes` path is mandatory (NOT head/tail): it visits every line
/// including blanks, so the local counter == the true jsonl line.
fn scan_one_file(path: &Path, target_file: Option<&str>) -> Result<ScanResult> {
    // Bare-hex canonical id for a subagent transcript (strip the `agent-` filename
    // prefix) so a recovered subagent row's `session_id` matches the `agents` topology id
    // — id-form unification (a top-level session uuid is unaffected: no `agent-` prefix).
    let session_id = crate::subagent::session_id_from_path(path);
    // Id-domain discriminator (the r5 shape, now on recover): a subagent transcript's
    // `session_id` is a non-re-feedable bare hex; carry `is_subagent` + the re-feedable
    // parent uuid (the dir before `subagents/`) onto every emitted recover record.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(ScanResult {
            session_id,
            is_subagent,
            parent_session_id,
            events: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // ── File-level prefilter (the dominant cost of an UNSCOPED recover). A transcript that
    //    never mentions the target's BASENAME holds no events for it: every Read/Edit/Write/Bash
    //    op and result `extract` replays carries the path literal somewhere in that transcript,
    //    and `extract` matches by full path OR basename-suffix — so the basename is the correct
    //    SUPERSET gate. One SIMD `memmem` over the mmap lets us skip PARSING the file entirely,
    //    turning an unscoped recover from a whole-corpus JSON parse into a parse of only the few
    //    transcripts that touched the file. (No target ⇒ no gate; behaviour unchanged.)
    if let Some(t) = target_file {
        let base = t.rsplit('/').next().unwrap_or(t);
        if !base.is_empty() && memmem::find(bytes, base.as_bytes()).is_none() {
            return Ok(ScanResult {
                session_id,
                is_subagent,
                parent_session_id,
                events: Vec::new(),
                skipped_lines: 0,
            });
        }
    }

    // Parse all recover-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool),
    // preserving each record's exact 1-based line number (counts EVERY visited line, 1:1 with
    // jsonl) — a single giant transcript is no longer scanned on one core.
    let (records, skipped) =
        crate::parse::parse_candidates_parallel(bytes, line_is_recover_candidate);

    let events = extract(&records, target_file);
    Ok(ScanResult {
        session_id,
        is_subagent,
        parent_session_id,
        events,
        skipped_lines: skipped,
    })
}

/// Pre-JSON byte prefilter — a SUPERSET of `files`' (we need Reads, tool_result bodies,
/// integrity errors, attachments, history snapshots — not just mutations).
/// Coarse by design; the structural parse decides what each line really is.
fn line_is_recover_candidate(line: &[u8]) -> bool {
    memmem::find(line, br#""role":"user""#).is_some()
        || memmem::find(line, b"toolUseResult").is_some()
        || memmem::find(line, b"Edit").is_some()
        || memmem::find(line, b"Write").is_some()
        || memmem::find(line, b"Read").is_some()
        || memmem::find(line, b"Bash").is_some()
        || memmem::find(line, b"filePath").is_some()
        || memmem::find(line, b"file_path").is_some()
        || memmem::find(line, b"file-history-snapshot").is_some()
        || memmem::find(line, b"edited_text_file").is_some()
        || memmem::find(line, b"tool_use_error").is_some()
}

// ─────────────────────────────────────────────────────────────────────────────
// Extraction: (line_no, Record) → FileEvent
// ─────────────────────────────────────────────────────────────────────────────

/// Extract `--file` events from a session's line-numbered records.
///
/// Intent↔result is joined by `tool_use_id` WITHIN a turn (never by adjacency — an
/// integrity error can precede its own tool_use line). We build, per turn, two maps:
/// `tool_use_id → file_path` (from the originating Read/Edit/Write tool_use) so an
/// integrity-error carrier with no inline path can be attributed to `--file`.
fn extract(records: &[(usize, Record)], target_file: Option<&str>) -> Vec<FileEvent> {
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    // Turn delimiting keys on the shared boundary predicate (§6.4) so file-event
    // attribution lines up with `turns`/`search`: an answered AskUserQuestion / a
    // tool-use rejection-with-message opens a turn, an interrupt / local-command-stdout
    // does not.
    let turns = group_turn_indices_deduped(&recs, |r| *r);
    extract_with_turns(records, &turns, target_file)
}

/// Extract `--file` events given PRE-COMPUTED turn groups. Batch reconstruction groups each
/// transcript ONCE and calls this per target, so a transcript mentioning many manifest files
/// is grouped a single time rather than once per file.
fn extract_with_turns(
    records: &[(usize, Record)],
    turns: &[Vec<usize>],
    target_file: Option<&str>,
) -> Vec<FileEvent> {
    let mut events: Vec<FileEvent> = Vec::new();

    for (turn_index, idxs) in turns.iter().enumerate() {
        // tool_use_id → file_path for THIS turn's Read/Edit/Write/MultiEdit tool_uses.
        let mut id_to_path: BTreeMap<String, String> = BTreeMap::new();
        // tool_use_ids whose result carrier carries the structured `toolUseResult` echo —
        // i.e. the ops `extract_from_tool_use_result` can reconstruct from. SUBAGENT and
        // workflow-agent transcripts OMIT `toolUseResult` (the tool_result is just a
        // `"File created successfully…"` string), so those ids are absent here and the
        // input-side fallback below supplies their content (§ subagent recover).
        let mut ids_with_result: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // tool_use_ids whose RESULT was an error (`is_error:true`) — e.g. a failed Edit
        // ("String to replace not found in file", "File has not been read yet"). The op did
        // NOT mutate the file, so its tool_use INPUT must never be replayed as if it landed.
        // A failed Edit also has NO `toolUseResult` echo, so its id is absent from
        // `ids_with_result` — without this set the input-side fallback below would apply the
        // ghost edit. Captured per-turn; the result block sits in the same turn as its call.
        let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &i in idxs {
            let rec = &records[i].1;
            collect_tool_use_paths(rec.blocks(), &mut id_to_path);
            let has_structured_result = rec.tool_use_result.is_some();
            if let Some(blocks) = rec.blocks() {
                for b in blocks {
                    if let Block::ToolResult {
                        tool_use_id: Some(id),
                        is_error,
                        ..
                    } = b
                    {
                        if has_structured_result {
                            ids_with_result.insert(id.clone());
                        }
                        if *is_error == Some(true) {
                            failed_ids.insert(id.clone());
                        }
                    }
                }
            }
        }

        for &i in idxs {
            let (line_no, rec) = (&records[i].0, &records[i].1);
            extract_from_record(
                *line_no,
                turn_index,
                rec,
                target_file,
                &id_to_path,
                &mut events,
            );
            // Carrier-less ops (subagent/workflow): reconstruct content from the tool_use
            // INPUT. Gated on `ids_with_result` so it NEVER double-emits in a top-level
            // session (whose carriers always carry `toolUseResult`) — main-session
            // reconstruction stays byte-identical — and on `failed_ids` so a failed Edit's
            // input is never applied as a phantom mutation.
            extract_input_fallback(
                *line_no,
                turn_index,
                rec,
                target_file,
                &ids_with_result,
                &failed_ids,
                &mut events,
            );
        }
    }

    events
}

/// Record, for each Read/Edit/Write/MultiEdit tool_use block, its `tool_use_id →
/// file_path` so a later integrity-error carrier (which has no inline path) can be
/// attributed by id.
fn collect_tool_use_paths(blocks: Option<&[Block]>, out: &mut BTreeMap<String, String>) {
    let Some(blocks) = blocks else { return };
    for b in blocks {
        if let Block::ToolUse {
            id: Some(id),
            name: Some(name),
            input: Some(input),
        } = b
        {
            let key = match name.as_str() {
                "Read" | "Edit" | "Write" | "MultiEdit" => "file_path",
                "NotebookEdit" => "notebook_path",
                _ => continue,
            };
            if let Some(p) = input.get(key).and_then(serde_json::Value::as_str) {
                if !p.is_empty() {
                    out.insert(id.clone(), p.to_string());
                }
            }
        }
    }
}

/// Reconstruct a Write/Edit/MultiEdit content event from the tool_use INPUT when the op
/// has NO `toolUseResult` carrier (its id is absent from `ids_with_result`).
///
/// WHY: a subagent (built-in Task/Agent-tool) and a workflow-agent transcript record the
/// tool RESULT as a bare `tool_result` string (`"File created successfully at: …"`) with
/// NO structured `toolUseResult` echo — unlike a top-level session, whose carrier carries
/// `{type:create, filePath, content, …}`. `extract_from_tool_use_result` reads that echo,
/// so without this fallback a file WRITTEN BY A SUBAGENT is invisible to `recover`
/// (`no recoverable history`) even though `files`/`search` see it (they read the tool_use
/// input directly). The authoritative content IS in the input — `Write.content`,
/// `Edit.{old_string,new_string,replace_all}`, `MultiEdit.edits[]` — present in EVERY
/// transcript. An Edit reconstructs via `apply_string_edit` (old→new), so the missing
/// `structuredPatch` is not needed.
///
/// Gated on `ids_with_result` so it never double-emits in a top-level session.
fn extract_input_fallback(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: Option<&str>,
    ids_with_result: &std::collections::HashSet<String>,
    failed_ids: &std::collections::HashSet<String>,
    events: &mut Vec<FileEvent>,
) {
    let ts = rec.timestamp.clone();
    let Some(blocks) = rec.blocks() else { return };
    for b in blocks {
        let Block::ToolUse {
            id,
            name: Some(name),
            input: Some(input),
        } = b
        else {
            continue;
        };
        // Skip when this op already has a `toolUseResult` carrier to reconstruct from, OR
        // when its result was an ERROR (a failed Edit/Write never mutated the file, so its
        // input is a phantom — `is_error:true` covers both "String to replace not found"
        // and the Edit-before-Read "File has not been read yet" wall, incl. the
        // Bash-created-then-directly-Edited and the must-re-Read-a-plan cases).
        if let Some(id) = id {
            if ids_with_result.contains(id) || failed_ids.contains(id) {
                continue;
            }
        }
        let path = input
            .get("file_path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !path_matches(target_file, path) {
            continue;
        }
        match name.as_str() {
            "Write" => {
                if let Some(content) = input.get("content").and_then(serde_json::Value::as_str) {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: ts.clone(),
                        kind: EventKind::FullSnapshot {
                            content: content.to_string(),
                            total_lines: line_count(content),
                            source: SnapSource::Write,
                        },
                    });
                }
            }
            "Edit" => {
                let hunks = vec![EditHunk {
                    old_string: input
                        .get("old_string")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    new_string: input
                        .get("new_string")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    replace_all: input
                        .get("replace_all")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                }];
                events.push(FileEvent {
                    line_no,
                    turn_index,
                    timestamp_utc: ts.clone(),
                    kind: EventKind::Edit {
                        hunks,
                        original_file: None,
                        structured_patch: None,
                    },
                });
            }
            "MultiEdit" => {
                let hunks: Vec<EditHunk> = input
                    .get("edits")
                    .and_then(serde_json::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .map(|e| EditHunk {
                                old_string: e
                                    .get("old_string")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                new_string: e
                                    .get("new_string")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                replace_all: e
                                    .get("replace_all")
                                    .and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !hunks.is_empty() {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: ts.clone(),
                        kind: EventKind::Edit {
                            hunks,
                            original_file: None,
                            structured_patch: None,
                        },
                    });
                }
            }
            _ => {}
        }
    }
}

/// True when `path` matches `--file`: exact raw-string match, or a basename-suffix
/// fallback (so a user may pass a short path). `None` target matches nothing (handled
/// by callers that gate on the mode).
fn path_matches(target: Option<&str>, path: &str) -> bool {
    let Some(t) = target else { return false };
    if t == path {
        return true;
    }
    // Basename-suffix fallback: the target is a trailing path segment of the record's
    // path (component-aligned, so `b.rs` does not match `/x/ab.rs`).
    path.strip_suffix(t)
        .map(|prefix| prefix.is_empty() || prefix.ends_with('/'))
        .unwrap_or(false)
}

/// Extract every `--file` event carried by ONE record.
fn extract_from_record(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: Option<&str>,
    id_to_path: &BTreeMap<String, String>,
    events: &mut Vec<FileEvent>,
) {
    let ts = rec.timestamp.clone();

    // ── (8) file-history-snapshot marker (a top-level sibling, no `message`) ──
    if let Some(snap) = rec.snapshot.as_ref() {
        if let Some(tfb) = snap.get("trackedFileBackups").and_then(|v| v.as_object()) {
            for path in tfb.keys() {
                if path_matches(target_file, path) {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: snap
                            .get("timestamp")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                            .or_else(|| ts.clone()),
                        kind: EventKind::HistorySnapshotMarker,
                    });
                }
            }
        }
    }

    // ── (7) attachment (edited_text_file external edit / file snapshot) ──
    if let Some(att) = rec.attachment.as_ref() {
        extract_from_attachment(line_no, turn_index, &ts, att, target_file, events);
    }

    // ── toolUseResult-bearing carriers: Read / Write / Edit results ──
    if let Some(tur) = rec.tool_use_result.as_ref() {
        extract_from_tool_use_result(line_no, turn_index, &ts, tur, target_file, events);
    }

    // ── Per-block extraction over message.content[] ──
    let Some(blocks) = rec.blocks() else { return };
    for b in blocks {
        match b {
            // (5) integrity error on a tool_result carrier (no inline path → id-join).
            Block::ToolResult {
                tool_use_id,
                content: Some(content),
                is_error: Some(true),
            } => {
                if let Some(kind) = classify_integrity_error(content) {
                    let attributed = tool_use_id
                        .as_ref()
                        .and_then(|id| id_to_path.get(id))
                        .map(String::as_str);
                    if path_matches(target_file, attributed.unwrap_or_default()) {
                        events.push(FileEvent {
                            line_no,
                            turn_index,
                            timestamp_utc: ts.clone(),
                            kind: EventKind::IntegrityError {
                                kind,
                                raw: crate::model::tool_result_content_text(content),
                            },
                        });
                    }
                }
            }
            // (6) Bash heuristic mutation touching `--file`.
            Block::ToolUse {
                name: Some(name),
                input: Some(input),
                ..
            } if name == "Bash" => {
                if let Some(cmd) = input.get("command").and_then(serde_json::Value::as_str) {
                    for bm in crate::bash_mutations::parse_bash_mutations(cmd) {
                        if path_matches(target_file, &bm.path) {
                            events.push(FileEvent {
                                line_no,
                                turn_index,
                                timestamp_utc: ts.clone(),
                                kind: EventKind::BashTouch {
                                    verb: bm.verb.to_string(),
                                },
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract a `FullSnapshot` / `PartialRead` (Read) or a `FullSnapshot` (Write) / `Edit`
/// from a `toolUseResult` carrier.
fn extract_from_tool_use_result(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    tur: &serde_json::Value,
    target_file: Option<&str>,
    events: &mut Vec<FileEvent>,
) {
    // ── (1a) Read result: toolUseResult.file = {filePath, content, startLine, …} ──
    if let Some(file) = tur.get("file").and_then(|v| v.as_object()) {
        let path = file.get("filePath").and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            let content = file
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let start_line = file
                .get("startLine")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1) as usize;
            let total_lines = file
                .get("totalLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            let num_lines = file
                .get("numLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            push_read_event(
                line_no,
                turn_index,
                ts,
                &content,
                start_line,
                num_lines,
                total_lines,
                SnapSource::FullRead,
                events,
            );
            return;
        }
    }

    // ── (2) Write result: {type:create|update, filePath, content, …} ──
    // ── (3) Edit result: {filePath, oldString, newString, structuredPatch, …} (no type) ──
    let path = tur.get("filePath").and_then(serde_json::Value::as_str);
    if !path_matches(target_file, path.unwrap_or_default()) {
        return;
    }
    let has_edit_strings = tur.get("oldString").is_some() || tur.get("newString").is_some();
    let structured_patch = parse_structured_patch(tur.get("structuredPatch"));

    if has_edit_strings {
        // An Edit (carrier side): keep the strings + structuredPatch + originalFile.
        let hunks = vec![EditHunk {
            old_string: tur
                .get("oldString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            new_string: tur
                .get("newString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            replace_all: tur
                .get("replaceAll")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }];
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::Edit {
                hunks,
                original_file: tur
                    .get("originalFile")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                structured_patch,
            },
        });
        return;
    }

    // A Write result: full-content anchor.
    if let Some(content) = tur.get("content").and_then(serde_json::Value::as_str) {
        let total = line_count(content);
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::FullSnapshot {
                content: content.to_string(),
                total_lines: total,
                source: SnapSource::Write,
            },
        });
    }
}

/// Push a Read event as either a `FullSnapshot` (whole file seen) or a `PartialRead`.
#[allow(clippy::too_many_arguments)]
fn push_read_event(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    content: &str,
    start_line: usize,
    num_lines: Option<usize>,
    total_lines: Option<usize>,
    source: SnapSource,
    events: &mut Vec<FileEvent>,
) {
    let lines: Vec<String> = split_lines(content);
    let observed = num_lines.unwrap_or(lines.len());
    let total = total_lines.unwrap_or(observed.max(start_line + lines.len().saturating_sub(1)));
    let is_full = start_line == 1 && observed >= total && total > 0;
    if is_full {
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::FullSnapshot {
                content: content.to_string(),
                total_lines: total.max(lines.len()),
                source,
            },
        });
    } else {
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::PartialRead {
                start_line: start_line.max(1),
                lines,
                total_lines: total,
            },
        });
    }
}

/// Extract `edited_text_file` (external edit) or `file` (snapshot) from an attachment.
fn extract_from_attachment(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    att: &serde_json::Value,
    target_file: Option<&str>,
    events: &mut Vec<FileEvent>,
) {
    let atype = att.get("type").and_then(serde_json::Value::as_str);

    // (7a) edited_text_file → an external edit (hard boundary).
    if atype == Some("edited_text_file") {
        let path = att
            .get("filename")
            .or_else(|| att.get("filePath"))
            .and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            let snippet_text = att
                .get("snippet")
                .or_else(|| att.get("content"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let snippet = strip_gutter(snippet_text);
            events.push(FileEvent {
                line_no,
                turn_index,
                timestamp_utc: ts.clone(),
                kind: EventKind::ExternalEdit { snippet },
            });
        }
        return;
    }

    // (7b) a `file` attachment → same shape as a structured Read.
    if let Some(file) = att
        .get("content")
        .and_then(|c| c.get("file"))
        .or_else(|| att.get("file"))
    {
        let path = file.get("filePath").and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            let content = file
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let start_line = file
                .get("startLine")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1) as usize;
            let total_lines = file
                .get("totalLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            let num_lines = file
                .get("numLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            push_read_event(
                line_no,
                turn_index,
                ts,
                &content,
                start_line,
                num_lines,
                total_lines,
                SnapSource::FileAttachment,
                events,
            );
        }
    }
}

/// Classify a tool_result error body as an integrity error, or `None` if it is some
/// other tool error (which is not a content boundary).
fn classify_integrity_error(content: &serde_json::Value) -> Option<IntegrityKind> {
    let text = crate::model::tool_result_content_text(content);
    if text.contains("has been modified since read") || text.contains("File has been modified") {
        Some(IntegrityKind::ModifiedSinceRead)
    } else if text.contains("has not been read yet") || text.contains("Read it first") {
        Some(IntegrityKind::NotReadYet)
    } else {
        None
    }
}

/// Parse `toolUseResult.structuredPatch` (an array of hunks) into [`PatchHunk`]s.
fn parse_structured_patch(v: Option<&serde_json::Value>) -> Option<Vec<PatchHunk>> {
    let arr = v?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for h in arr {
        let old_start = h.get("oldStart").and_then(serde_json::Value::as_u64)? as usize;
        let old_lines = h.get("oldLines").and_then(serde_json::Value::as_u64)? as usize;
        let new_lines = h.get("newLines").and_then(serde_json::Value::as_u64)? as usize;
        let lines = h
            .get("lines")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push(PatchHunk {
            old_start,
            old_lines,
            new_lines,
            lines,
        });
    }
    Some(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Reconstruction — the sparse line-keyed buffer
// ─────────────────────────────────────────────────────────────────────────────

/// One known file line + the jsonl line that last set it.
#[derive(Debug, Clone)]
struct LineCell {
    text: String,
    last_line_no: usize,
}

/// The "in the LLM's eyes" model: a SPARSE map of known file lines. A line absent from
/// `known` is an EXPLICIT gap (unknown — never fabricated).
#[derive(Debug, Default, Clone)]
struct SparseBuffer {
    known: BTreeMap<usize, LineCell>,
    /// The file length the model last observed (bounds the trailing gap).
    seen_total_lines: Option<usize>,
    /// Whether the last full-content anchor ended in a file-final newline. The Read tool
    /// reports `totalLines` by SEPARATOR count (so a newline-terminated file gets a phantom
    /// empty last line: 12 content lines → totalLines 13), while `split_lines` uses
    /// TERMINATOR count (12). When this is set, a windowed read's `total_lines` is
    /// normalised down by that phantom so the two conventions agree and no spurious trailing
    /// `??? line N+1 unknown` gap is reported for a fully-recovered file.
    content_ends_with_newline: bool,
}

impl SparseBuffer {
    /// Reset to a full snapshot's lines (1..=N). Supersedes all prior state.
    fn reset_to_full(&mut self, content: &str, total_lines: usize, line_no: usize) {
        self.known.clear();
        for (i, text) in split_lines(content).into_iter().enumerate() {
            self.known.insert(
                i + 1,
                LineCell {
                    text,
                    last_line_no: line_no,
                },
            );
        }
        // A full-content anchor is the authority on trailing-newline status (used to
        // normalise later windowed reads' separator-counted totals).
        self.content_ends_with_newline = content.ends_with('\n');
        self.seen_total_lines = Some(total_lines.max(self.known.len()));
    }

    /// Convert a tool-reported `total_lines` (SEPARATOR count) to the TERMINATOR count used
    /// by `split_lines`, dropping the phantom empty last line a file-final newline adds.
    /// A no-op until a full-content anchor has confirmed the trailing newline.
    fn normalize_total(&self, total_lines: usize) -> usize {
        if self.content_ends_with_newline {
            total_lines.saturating_sub(1)
        } else {
            total_lines
        }
    }

    /// Splice a windowed read: set `known[start+i]` for each line, leave the rest as-is.
    /// Gaps are NOT padded (padding would fabricate unseen lines).
    fn splice(&mut self, start_line: usize, lines: &[String], total_lines: usize, line_no: usize) {
        for (i, text) in lines.iter().enumerate() {
            self.known.insert(
                start_line + i,
                LineCell {
                    text: text.clone(),
                    last_line_no: line_no,
                },
            );
        }
        let norm_total = self.normalize_total(total_lines);
        self.seen_total_lines = Some(norm_total.max(self.seen_total_lines.unwrap_or(0)));
    }

    /// Contiguous runs of currently-known lines, as inclusive `(start, end)` spans.
    fn covered_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for &k in self.known.keys() {
            match ranges.last_mut() {
                Some(last) if last.1 + 1 == k => last.1 = k,
                _ => ranges.push((k, k)),
            }
        }
        ranges
    }

    /// The known lines as a dense `(line_no, text)` vector (gaps omitted), ascending.
    fn known_lines(&self) -> Vec<(usize, String)> {
        self.known
            .iter()
            .map(|(k, c)| (*k, c.text.clone()))
            .collect()
    }

    /// The known lines with provenance: `(file_line, text, jsonl_line_that_set_it)`.
    /// Surfaces `LineCell::last_line_no` so a consumer can `Read` the exact jsonl line a
    /// reconstructed line came from.
    fn known_lines_with_provenance(&self) -> Vec<(usize, String, usize)> {
        self.known
            .iter()
            .map(|(k, c)| (*k, c.text.clone(), c.last_line_no))
            .collect()
    }
}

/// The result of applying an [`EditKind::Edit`] to the buffer: did it anchor cleanly?
#[derive(Debug, PartialEq, Eq)]
enum EditOutcome {
    Applied,
    /// The edit could not be anchored (old_string over an unknown gap / not found).
    UnAnchorable,
}

/// Apply an Edit to the buffer. Preferred: structured-patch by exact line position;
/// fallback: string replacement over the contiguous known text. Returns whether it
/// anchored (an un-anchorable edit is a coverage hole, never a fabrication).
fn apply_edit(
    buf: &mut SparseBuffer,
    hunks: &[EditHunk],
    structured_patch: &Option<Vec<PatchHunk>>,
    line_no: usize,
) -> EditOutcome {
    if let Some(patches) = structured_patch {
        if !patches.is_empty() {
            return apply_structured_patch(buf, patches, line_no);
        }
    }
    // Fallback: string replacement over the dense known text.
    apply_string_edit(buf, hunks, line_no)
}

/// Apply structured-patch hunks by exact line position, shifting subsequent keys.
///
/// A patch hunk replaces `oldLines` source lines starting at `oldStart` with `newLines`
/// lines starting at `newStart`. We process hunks high-to-low so earlier hunks' indices
/// stay valid, rebuilding the dense line vector each time (the file is small enough — a
/// single tool result — that an O(n) rebuild per hunk is fine and keeps the logic exact).
fn apply_structured_patch(
    buf: &mut SparseBuffer,
    patches: &[PatchHunk],
    line_no: usize,
) -> EditOutcome {
    // Work on a dense snapshot ONLY if the affected region is fully known; otherwise the
    // edit is un-anchorable (we will not fabricate the missing context).
    // Build the current dense vector over the min..max line span the patches touch.
    let mut applied_any = false;
    // Materialize the whole known buffer as a dense vector indexed from line 1, padding
    // unknown interior lines with a sentinel we refuse to emit (tracked separately).
    let max_line = buf.known.keys().copied().max().unwrap_or(0);
    let patch_max = patches
        .iter()
        .map(|h| h.old_start + h.old_lines)
        .max()
        .unwrap_or(0);
    let span = max_line.max(patch_max);
    // dense[i] = Some(text) if known, None if a gap.
    let mut dense: Vec<Option<String>> = vec![None; span + 1]; // 1-based; index 0 unused
    for (k, c) in &buf.known {
        if *k <= span {
            dense[*k] = Some(c.text.clone());
        }
    }

    // Apply hunks low-to-high but accumulate a running offset (newLines-oldLines) so each
    // subsequent hunk's oldStart maps onto the already-shifted dense vector.
    let mut offset: isize = 0;
    for h in patches {
        // The OLD region content is the patch's context (` `) + removed (`-`) lines, in
        // order. The NEW region content is the context (` `) + added (`+`) lines.
        let old_region: Vec<String> = h
            .lines
            .iter()
            .filter(|l| l.starts_with('-') || l.starts_with(' '))
            .map(|l| l[1.min(l.len())..].to_string())
            .collect();
        let added: Vec<String> = h
            .lines
            .iter()
            .filter(|l| l.starts_with('+') || l.starts_with(' '))
            .map(|l| l[1.min(l.len())..].to_string())
            .collect();

        let start = (h.old_start as isize + offset).max(1) as usize;
        let end = start + h.old_lines; // exclusive
                                       // Defensive grow: `dense` is pre-sized to `span + 1` (≥ every hunk's
                                       // `old_start + old_lines`) and each splice grows it by the running offset, so with
                                       // well-formed ascending hunks `end` never exceeds `dense.len()`. We keep the guard
                                       // anyway because the hunk stream is untrusted transcript data — a pathological
                                       // (e.g. non-ascending) `structuredPatch` must not index out of bounds below.
        if end > dense.len() {
            dense.resize(end, None);
        }
        // ANCHOR CHECK (anti-fabrication): an edit's absolute `oldStart` is only
        // trustworthy if it lands ON or ADJACENT TO currently-known content. A hunk
        // whose entire neighbourhood is an unknown gap is position-drifted — applying it
        // would fabricate island lines at a wrong absolute number (the heavily-edited
        // file built without a clean full anchor is the real-data failure mode). Refuse
        // it as un-anchorable rather than asserting a wrong "known" line.
        let neighbourhood_known = (start.saturating_sub(1)..=end)
            .any(|i| dense.get(i).map(Option::is_some).unwrap_or(false));
        if !neighbourhood_known {
            return EditOutcome::UnAnchorable;
        }
        // Verify the removed region is fully known (anchorable). If any line in the old
        // range is an unknown gap, we cannot safely re-anchor → un-anchorable.
        let region_known = (start..end).all(|i| dense.get(i).map(Option::is_some).unwrap_or(false));
        if h.old_lines > 0 && !region_known {
            return EditOutcome::UnAnchorable;
        }
        // CONTEXT VERIFICATION (anti-fabrication): the patch's old-region lines must
        // match what the buffer currently holds at the anchored position. If they
        // DISAGREE, the edit is mis-anchored (the buffer drifted out of sync with the
        // real file — e.g. an earlier un-anchorable edit), so applying it would corrupt
        // known lines. Refuse: report un-anchorable rather than assert a wrong line.
        if h.old_lines > 0 && old_region.len() == h.old_lines {
            let matches = (0..h.old_lines).all(|k| {
                dense
                    .get(start + k)
                    .and_then(|c| c.as_ref())
                    .map(|t| t == &old_region[k])
                    .unwrap_or(false)
            });
            if !matches {
                return EditOutcome::UnAnchorable;
            }
        }
        // Splice: replace dense[start..end] with the new region content.
        let tail: Vec<Option<String>> = dense.split_off(end.min(dense.len()));
        dense.truncate(start.min(dense.len()));
        for a in &added {
            dense.push(Some(a.clone()));
        }
        dense.extend(tail);
        offset += h.new_lines as isize - h.old_lines as isize;
        applied_any = true;
    }

    // Rebuild the sparse buffer from the dense vector (1-based; skip the unused index 0
    // and any remaining gaps).
    buf.known.clear();
    for (i, cell) in dense.iter().enumerate().skip(1) {
        if let Some(text) = cell {
            buf.known.insert(
                i,
                LineCell {
                    text: text.clone(),
                    last_line_no: line_no,
                },
            );
        }
    }
    let max_known = buf.known.keys().copied().max().unwrap_or(0);
    // A structured patch is AUTHORITATIVE about the file's new length: adjust the
    // previously-seen total by this edit's net line delta (`offset` = added − removed
    // accumulated over the hunks) rather than monotonically maxing it. Maxing left a
    // phantom trailing gap after a net deletion (e.g. an insert that grew the file to N+1
    // followed by a delete back to N would still report N+1, emitting a spurious
    // `??? line N+1 unknown`). Clamp to ≥ the max known line and ≥ 0.
    let prev_total = buf.seen_total_lines.unwrap_or(0) as isize;
    let adjusted = (prev_total + offset).max(max_known as isize).max(0) as usize;
    buf.seen_total_lines = Some(adjusted);

    if applied_any {
        EditOutcome::Applied
    } else {
        EditOutcome::UnAnchorable
    }
}

/// Fallback string-replacement edit over the dense contiguous known text. If the buffer
/// is not a single contiguous run (or `old_string` is not found), the edit is
/// un-anchorable (we never guess across a gap).
fn apply_string_edit(buf: &mut SparseBuffer, hunks: &[EditHunk], line_no: usize) -> EditOutcome {
    // Only safe when the known lines are one contiguous run starting at line 1.
    let ranges = buf.covered_ranges();
    let contiguous_from_one = matches!(ranges.first(), Some(&(1, _))) && ranges.len() == 1;
    if !contiguous_from_one {
        return EditOutcome::UnAnchorable;
    }
    let mut text = buf
        .known
        .values()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let mut any = false;
    for h in hunks {
        if h.old_string.is_empty() || !text.contains(&h.old_string) {
            return EditOutcome::UnAnchorable;
        }
        if h.replace_all {
            text = text.replace(&h.old_string, &h.new_string);
        } else {
            text = text.replacen(&h.old_string, &h.new_string, 1);
        }
        any = true;
    }
    if !any {
        return EditOutcome::UnAnchorable;
    }

    buf.known.clear();
    for (i, line) in text.split('\n').enumerate() {
        buf.known.insert(
            i + 1,
            LineCell {
                text: line.to_string(),
                last_line_no: line_no,
            },
        );
    }
    let total = buf.known.len();
    buf.seen_total_lines = Some(total.max(buf.seen_total_lines.unwrap_or(0)));
    EditOutcome::Applied
}

// ─────────────────────────────────────────────────────────────────────────────
// Boundaries + segmentation
// ─────────────────────────────────────────────────────────────────────────────

/// The confidence of an integrity boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confidence {
    Authoritative,
    Heuristic,
}

impl Confidence {
    fn label(self) -> &'static str {
        match self {
            Confidence::Authoritative => "AUTHORITATIVE",
            Confidence::Heuristic => "HEURISTIC",
        }
    }
    fn json(self) -> &'static str {
        match self {
            Confidence::Authoritative => "authoritative",
            Confidence::Heuristic => "heuristic",
        }
    }
}

/// A detected integrity boundary — a point where reconstruction across it is invalid.
#[derive(Debug, Clone)]
struct Boundary {
    line_no: usize,
    turn_index: usize,
    timestamp_utc: Option<String>,
    kind: &'static str,
    confidence: Confidence,
    detail: String,
}

/// Per-op counts for the coverage report.
#[derive(Debug, Default, Clone)]
struct EventCounts {
    read_full: usize,
    read_windowed: usize,
    edit: usize,
    edit_unanchorable: usize,
    write: usize,
    bash: usize,
    external_edit: usize,
    history_snapshot: usize,
    integrity_error: usize,
}

/// One reconstructed segment (a maximal run of events with no hard boundary inside).
#[derive(Debug, Clone)]
struct Segment {
    index: usize,
    line_no_start: usize,
    line_no_end: usize,
    turn_start: usize,
    turn_end: usize,
    ts_start: Option<String>,
    ts_end: Option<String>,
    /// The buffer state at the END of this segment.
    end_buffer: SparseBuffer,
    /// The buffer state at the START of this segment (its pre-state).
    start_buffer: SparseBuffer,
    /// False when this segment opened after a boundary with no fresh full anchor.
    pre_state_known: bool,
    /// The kind of full anchor (if any) that opened/seeded this segment.
    anchor_source: Option<SnapSource>,
}

/// The full replay outcome for one session's `--file` events.
#[derive(Debug, Default)]
struct Replay {
    segments: Vec<Segment>,
    boundaries: Vec<Boundary>,
    counts: EventCounts,
    /// The final buffer after replaying ALL events (used by `--coverage`/`--at`).
    final_buffer: SparseBuffer,
    coverage_holes: Vec<(usize, usize, usize)>, // (line_no, turn, jsonl-ish marker)
}

/// Replay a session's ordered `--file` events into segments + boundaries + counts.
/// `cutoff_line` (when `Some`) stops replay at jsonl line ≤ cutoff (for `--at`).
fn replay(events: &[FileEvent], cutoff_line: Option<usize>) -> Replay {
    let mut out = Replay::default();
    let mut buf = SparseBuffer::default();
    let mut seg_start = buf.clone();
    let mut seg_open: Option<(usize, usize, Option<String>)> = None; // (line_no, turn, ts)
    let mut seg_last: Option<(usize, usize, Option<String>)> = None;
    let mut pre_state_known = true;
    let mut had_full_anchor = false;
    let mut anchor_source: Option<SnapSource> = None;

    #[allow(clippy::too_many_arguments)]
    let close_segment = |out: &mut Replay,
                         start_buf: &SparseBuffer,
                         end_buf: &SparseBuffer,
                         open: &Option<(usize, usize, Option<String>)>,
                         last: &Option<(usize, usize, Option<String>)>,
                         pre_known: bool,
                         anchor: Option<SnapSource>| {
        if let (Some((ls, tts, tss)), Some((le, tte, tse))) = (open, last) {
            out.segments.push(Segment {
                index: out.segments.len() + 1,
                line_no_start: *ls,
                line_no_end: *le,
                turn_start: *tts,
                turn_end: *tte,
                ts_start: tss.clone(),
                ts_end: tse.clone(),
                start_buffer: start_buf.clone(),
                end_buffer: end_buf.clone(),
                pre_state_known: pre_known,
                anchor_source: anchor,
            });
        }
    };

    for e in events {
        if let Some(c) = cutoff_line {
            if e.line_no > c {
                break;
            }
        }
        let here = (e.line_no, e.turn_index, e.timestamp_utc.clone());

        match &e.kind {
            EventKind::FullSnapshot {
                content,
                total_lines,
                source,
            } => {
                match source {
                    SnapSource::FullRead => out.counts.read_full += 1,
                    SnapSource::Write => out.counts.write += 1,
                    SnapSource::FileAttachment => out.counts.read_full += 1,
                }
                let opened_here = seg_open.is_none();
                // A WRITE is a creation/whole-file event → its segment's pre-state is the
                // buffer BEFORE the write (so the diff shows the write as a real change). A
                // full READ / file attachment is an OBSERVATION of existing state → its
                // segment's pre-state is the anchor content itself (post-read), so the diff
                // shows only the edits made AFTER the read, not a spurious "creation".
                let pre_before_reset = buf.clone();
                anchor_source = Some(*source);
                buf.reset_to_full(content, *total_lines, e.line_no);
                had_full_anchor = true;
                if opened_here {
                    seg_start = match source {
                        SnapSource::Write => pre_before_reset,
                        SnapSource::FullRead | SnapSource::FileAttachment => buf.clone(),
                    };
                    seg_open = Some(here.clone());
                    pre_state_known = true;
                }
                seg_last = Some(here);
            }
            EventKind::PartialRead {
                start_line,
                lines,
                total_lines,
            } => {
                out.counts.read_windowed += 1;
                if seg_open.is_none() {
                    seg_start = buf.clone();
                    seg_open = Some(here.clone());
                }
                buf.splice(*start_line, lines, *total_lines, e.line_no);
                seg_last = Some(here);
            }
            EventKind::Edit {
                hunks,
                original_file,
                structured_patch,
            } => {
                out.counts.edit += 1;
                if seg_open.is_none() {
                    seg_start = buf.clone();
                    seg_open = Some(here.clone());
                }
                // Boundary cross-check: originalFile vs replayed buffer.
                if let Some(orig) = original_file {
                    if had_full_anchor && buffer_disagrees_with_original(&buf, orig) {
                        out.boundaries.push(Boundary {
                            line_no: e.line_no,
                            turn_index: e.turn_index,
                            timestamp_utc: e.timestamp_utc.clone(),
                            kind: "original_file_disagreement",
                            confidence: Confidence::Authoritative,
                            detail: "edit originalFile disagrees with replayed buffer".to_string(),
                        });
                    }
                }
                let outcome = apply_edit(&mut buf, hunks, structured_patch, e.line_no);
                if outcome == EditOutcome::UnAnchorable {
                    out.counts.edit_unanchorable += 1;
                    out.coverage_holes
                        .push((e.line_no, e.turn_index, e.line_no));
                }
                seg_last = Some(here);
            }
            EventKind::IntegrityError { kind, raw } => {
                out.counts.integrity_error += 1;
                match kind {
                    IntegrityKind::ModifiedSinceRead => {
                        // HARD boundary: close the current segment.
                        close_segment(
                            &mut out,
                            &seg_start,
                            &buf,
                            &seg_open,
                            &seg_last,
                            pre_state_known,
                            anchor_source,
                        );
                        out.boundaries.push(Boundary {
                            line_no: e.line_no,
                            turn_index: e.turn_index,
                            timestamp_utc: e.timestamp_utc.clone(),
                            kind: "modified_since_read",
                            confidence: Confidence::Authoritative,
                            detail: first_line(raw),
                        });
                        seg_open = None;
                        seg_last = None;
                        pre_state_known = false;
                        had_full_anchor = false;
                        anchor_source = None;
                        // The file changed out from under us — the harness rejected the edit and
                        // demanded a fresh Read. Everything known so far is now SUSPECT, so
                        // invalidate the buffer: only content RE-READ / re-written after this
                        // point counts toward the final state. Pre-boundary lines become explicit
                        // gaps, never silently-stale lines presented as "current".
                        buf.known.clear();
                        buf.seen_total_lines = None;
                    }
                    IntegrityKind::NotReadYet => { /* not a boundary; the edit never landed */ }
                }
            }
            EventKind::ExternalEdit { snippet } => {
                out.counts.external_edit += 1;
                // HARD boundary: close current segment, then splice the external snippet.
                close_segment(
                    &mut out,
                    &seg_start,
                    &buf,
                    &seg_open,
                    &seg_last,
                    pre_state_known,
                    anchor_source,
                );
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "external_edit",
                    confidence: Confidence::Authoritative,
                    detail: "edited_text_file attachment (file changed outside the tool stream)"
                        .to_string(),
                });
                for (n, text) in snippet {
                    buf.known.insert(
                        *n,
                        LineCell {
                            text: text.clone(),
                            last_line_no: e.line_no,
                        },
                    );
                }
                // Open a fresh segment anchored on the external snippet.
                seg_start = buf.clone();
                seg_open = Some(here.clone());
                seg_last = Some(here);
                pre_state_known = false;
                had_full_anchor = false;
                anchor_source = None;
            }
            EventKind::BashTouch { verb } => {
                out.counts.bash += 1;
                // SOFT boundary: close current segment + flag heuristic.
                close_segment(
                    &mut out,
                    &seg_start,
                    &buf,
                    &seg_open,
                    &seg_last,
                    pre_state_known,
                    anchor_source,
                );
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "bash_mutation",
                    confidence: Confidence::Heuristic,
                    detail: format!(
                        "bash `{verb}` (reconstruction across this point may be invalid)"
                    ),
                });
                seg_open = None;
                seg_last = None;
                pre_state_known = false;
                had_full_anchor = false;
                anchor_source = None;
            }
            EventKind::HistorySnapshotMarker => {
                out.counts.history_snapshot += 1;
                // A coverage annotation only — not an anchor, not a boundary.
            }
        }
    }

    // Close the trailing open segment.
    close_segment(
        &mut out,
        &seg_start,
        &buf,
        &seg_open,
        &seg_last,
        pre_state_known,
        anchor_source,
    );
    out.final_buffer = buf;
    out
}

/// True when the replayed buffer, over the lines `originalFile` describes, DISAGREES with
/// `originalFile` (an out-of-band change happened). Compares the contiguous known region
/// that overlaps `originalFile`'s first lines; if nothing is comparably known, returns
/// false (we cannot prove a disagreement, so we never flag a false boundary).
fn buffer_disagrees_with_original(buf: &SparseBuffer, original_file: &str) -> bool {
    let orig_lines = split_lines(original_file);
    if orig_lines.is_empty() {
        return false;
    }
    // Compare line-by-line over the known cells that fall within the original's length.
    let mut compared = 0usize;
    let mut mismatches = 0usize;
    for (k, cell) in &buf.known {
        if *k >= 1 && *k <= orig_lines.len() {
            compared += 1;
            if orig_lines[*k - 1] != cell.text {
                mismatches += 1;
            }
        }
    }
    // Require a reasonable comparison base (≥1 line) and ANY mismatch to flag — but only
    // when we compared enough to be meaningful (avoid a single fluke). A mismatch ratio
    // over a small threshold is a real disagreement.
    compared > 0 && mismatches > 0 && (mismatches * 4 >= compared)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified diff (in-crate, safe Rust)
// ─────────────────────────────────────────────────────────────────────────────

/// Compute a unified diff between two line vectors, emitting `@@ -a,b +c,d @@` hunks with
/// ` `/`-`/`+` prefixes. A compact LCS-based diff (O(n·m) DP) — fine for single-file
/// reconstruction sizes and fully unit-testable. Returns an empty string when identical.
/// `context` = number of equal lines to keep around each change. `usize::MAX` ⇒ FULL context:
/// every line of `old`/`new` is shown. `--patches` passes MAX on purpose — `old`/`new` are the
/// segment's READ-covered lines, and CC's strict Read-before-Edit means each of those lines was
/// genuinely observed, so showing them all is valid, high-quality context (a fully-read,
/// barely-edited file then reproduces in full, not just a 3-line window around the one change).
fn unified_diff(old: &[String], new: &[String], context: usize) -> String {
    if old == new {
        return String::new();
    }
    let ops = lcs_diff(old, new);
    format_unified(&ops, old, new, context)
}

/// A single edit op in the diff script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffOp {
    Equal,
    Delete,
    Insert,
}

/// LCS-based diff script over two line slices (classic DP backtrace).
fn lcs_diff(old: &[String], new: &[String]) -> Vec<(DiffOp, usize, usize)> {
    let n = old.len();
    let m = new.len();
    // dp[i][j] = LCS length of old[i..] and new[j..].
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push((DiffOp::Equal, i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push((DiffOp::Delete, i, j));
            i += 1;
        } else {
            ops.push((DiffOp::Insert, i, j));
            j += 1;
        }
    }
    while i < n {
        ops.push((DiffOp::Delete, i, j));
        i += 1;
    }
    while j < m {
        ops.push((DiffOp::Insert, i, j));
        j += 1;
    }
    ops
}

/// Format an LCS op-script as unified-diff hunks. `context` equal lines surround each change
/// (`usize::MAX` ⇒ full context: all lines emitted, every change merged into one spanning hunk).
fn format_unified(
    ops: &[(DiffOp, usize, usize)],
    old: &[String],
    new: &[String],
    context: usize,
) -> String {
    // Mark which op indices are changes vs equal.
    let is_change: Vec<bool> = ops.iter().map(|(o, _, _)| *o != DiffOp::Equal).collect();
    let mut out = String::new();

    let mut idx = 0usize;
    while idx < ops.len() {
        if !is_change[idx] {
            idx += 1;
            continue;
        }
        // A change run: extend backward/forward by CONTEXT equal ops.
        let mut start = idx;
        let mut ctx_back = 0;
        while start > 0 && !is_change[start - 1] && ctx_back < context {
            start -= 1;
            ctx_back += 1;
        }
        let mut end = idx;
        // Walk to the end of this hunk: include changes + up to CONTEXT trailing equals,
        // but merge adjacent change runs separated by ≤ 2*CONTEXT equals.
        while end < ops.len() {
            if is_change[end] {
                end += 1;
                continue;
            }
            // Count equals run; if a change follows within 2*CONTEXT, keep going.
            let mut run = end;
            while run < ops.len() && !is_change[run] {
                run += 1;
            }
            let equal_len = run - end;
            if run < ops.len() && equal_len <= context.saturating_mul(2) {
                end = run; // absorb the gap
            } else {
                end += equal_len.min(context);
                break;
            }
        }

        // Compute hunk header line numbers (1-based) from the first/last op in [start,end).
        let slice = &ops[start..end];
        let (mut old_lo, mut new_lo) = (usize::MAX, usize::MAX);
        let (mut old_count, mut new_count) = (0usize, 0usize);
        let mut body = String::new();
        for (op, oi, nj) in slice {
            match op {
                DiffOp::Equal => {
                    old_lo = old_lo.min(*oi);
                    new_lo = new_lo.min(*nj);
                    old_count += 1;
                    new_count += 1;
                    body.push_str(&format!(" {}\n", old[*oi]));
                }
                DiffOp::Delete => {
                    old_lo = old_lo.min(*oi);
                    old_count += 1;
                    body.push_str(&format!("-{}\n", old[*oi]));
                }
                DiffOp::Insert => {
                    new_lo = new_lo.min(*nj);
                    new_count += 1;
                    body.push_str(&format!("+{}\n", new[*nj]));
                }
            }
        }
        if old_lo == usize::MAX {
            old_lo = 0;
        }
        if new_lo == usize::MAX {
            new_lo = 0;
        }
        // Unified-diff headers are 1-based; a zero-length side uses the 0,0 form.
        let old_start = if old_count == 0 { old_lo } else { old_lo + 1 };
        let new_start = if new_count == 0 { new_lo } else { new_lo + 1 };
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        out.push_str(&body);
        idx = end;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared text helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Split content into lines WITHOUT a trailing empty element for a final newline (so a
/// file `"a\nb\n"` is `["a","b"]`, matching how CC numbers lines).
fn split_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').map(str::to_string).collect()
}

/// Count the lines a content blob represents (== `split_lines(content).len()`).
fn line_count(content: &str) -> usize {
    split_lines(content).len()
}

/// The first line of a (possibly multi-line) string, trimmed.
fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// Strip a leading line-number gutter from each line of a cat -n style snippet. Handles
/// BOTH the TAB gutter (`\d+\t<text>`, what current CC Read content uses) and the arrow
/// gutter (`\d+→<text>`, an older form). Returns `(file_line_no, text)` pairs; a line
/// with no recognizable gutter is skipped (we never fabricate a number).
fn strip_gutter(snippet: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for raw in snippet.split('\n') {
        let line = raw;
        // Find the gutter separator: a tab or the U+2192 arrow after leading digits.
        let trimmed = line.trim_start();
        let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let rest = &trimmed[digits.len()..];
        let text = if let Some(t) = rest.strip_prefix('\t') {
            t
        } else if let Some(t) = rest.strip_prefix('\u{2192}') {
            t
        } else {
            continue;
        };
        if let Ok(n) = digits.parse::<usize>() {
            out.push((n, text.to_string()));
        }
    }
    out
}

/// Parse a `--turn-range START..END` into an inclusive 0-based `(lo, hi)` (shared parser).
fn parse_turn_range(s: &str) -> Result<(usize, usize)> {
    crate::text::parse_range(s, "--turn-range", false)
}

/// Parse a `--line-range START..END` into an inclusive 1-based `(lo, hi)` (shared parser;
/// the 1-based variant rejects a 0 start — file lines are 1-based).
fn parse_line_range(s: &str) -> Result<(usize, usize)> {
    crate::text::parse_range(s, "--line-range", true)
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}

// ─────────────────────────────────────────────────────────────────────────────
// At-cutoff resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve an `--at <WHEN>` spec to a cutoff jsonl line over a session's events.
/// Supports `@latest` (no cutoff → the file's FINAL state), `@line:<N>`, `@turn:<N>` (first line
/// strictly after turn N), an ISO8601 / relative datetime (events with ts ≤ the bound — the bound
/// is INCLUSIVE), or returns `None` (no cutoff → replay everything) when `when` is empty.
fn resolve_cutoff(when: &str, events: &[FileEvent]) -> Result<Option<usize>> {
    let when = when.trim();
    // `@latest` / empty → the final reconstructed state (replay every event, no cutoff). The
    // clean way to ask for "the file's last form" without guessing a timestamp past the last
    // write (a datetime cutoff is ≤-inclusive, so a too-early ts simply yields less).
    if when.is_empty() || when == "@latest" {
        return Ok(None);
    }
    if let Some(rest) = when.strip_prefix("@line:") {
        let n: usize = rest
            .trim()
            .parse()
            .with_context(|| format!("--at @line:<N> needs an integer, got {rest:?}"))?;
        return Ok(Some(n));
    }
    if let Some(rest) = when.strip_prefix("@turn:") {
        let target: usize = rest
            .trim()
            .parse()
            .with_context(|| format!("--at @turn:<N> needs an integer, got {rest:?}"))?;
        // Cutoff = the last jsonl line whose turn_index ≤ target.
        let cutoff = events
            .iter()
            .filter(|e| e.turn_index <= target)
            .map(|e| e.line_no)
            .max();
        // If nothing is at/below the target turn, cutoff at 0 (empty snapshot).
        return Ok(Some(cutoff.unwrap_or(0)));
    }
    // Datetime bound: the cutoff is the highest line_no whose ts ≤ bound.
    let window = TimeWindow::from_args(None, Some(when))?;
    let cutoff = events
        .iter()
        .filter(|e| window.contains(e.timestamp_utc.as_deref()))
        .map(|e| e.line_no)
        .max();
    Ok(Some(cutoff.unwrap_or(0)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Render context + line-range filtering
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the renderers need beyond the per-session scan results.
#[derive(Debug)]
struct RenderCtx {
    mode: RecoverMode,
    file: Option<String>,
    line_range: Option<(usize, usize)>,
    at: Option<String>,
    skipped_lines: usize,
    /// SCOPE-span counts of the resolved transcript set, captured BEFORE the
    /// reconstruction merge folds subagents into their parent (so the banner still
    /// announces the true `1 top-level + N subagent` fan-out).
    scope_top: usize,
    scope_sub: usize,
}

/// Restrict a `(line_no, text)` known-line vector to the `--line-range`, if any.
fn apply_line_range(
    lines: Vec<(usize, String)>,
    line_range: Option<(usize, usize)>,
) -> Vec<(usize, String)> {
    match line_range {
        Some((lo, hi)) => lines
            .into_iter()
            .filter(|(n, _)| *n >= lo && *n <= hi)
            .collect(),
        None => lines,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Text rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Write the `--out` artifact ONLY when it has content. An EMPTY reconstruction (no
/// recoverable history / over-budget) must NOT clobber the destination — a user reusing a
/// scratch path (the advertised `--out /tmp/restored.md` idiom) would otherwise lose
/// pre-existing content AND read a false `(wrote …)` success line. Returns `true` when a
/// write happened (so the caller prints its `(wrote …)` line) and `false` (with a stderr
/// note) when the blob was empty and the file was left untouched. Uniform across patches/at
/// + their JSON twins + turns.
pub(crate) fn write_out_guarded(p: &Path, blob: &str) -> Result<bool> {
    if blob.is_empty() {
        eprintln!(
            "note: nothing reconstructed in range — --out file {} left untouched",
            p.display()
        );
        return Ok(false);
    }
    std::fs::write(p, blob).with_context(|| format!("cannot write --out file {}", p.display()))?;
    Ok(true)
}

/// SCOPE-span counts of the resolved transcript set (one `ScanResult` per resolved file,
/// incl. empty/no-history subagents) — `(top_level, subagent)`. Drives the shared SCOPE
/// banner / JSON header so a bare `csift recover <uuid>` fan-out is announced like list/turns.
fn scope_span(sessions: &[ScanResult]) -> (usize, usize) {
    let sub = sessions.iter().filter(|s| s.is_subagent).count();
    (sessions.len() - sub, sub)
}

/// Chronological order of two optional ISO-8601 timestamps. Timestamped events sort before
/// timestamp-less ones; a stable sort then keeps the within-session order for ties/absent ts.
fn cmp_ts(a: &Option<String>, b: &Option<String>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Fold each top-level group (a session + ITS OWN subagents, keyed by `parent_session_id`)
/// into one cross-session reconstruction timeline. A group whose target-file events come
/// from <2 transcripts is returned unchanged (byte-identical legacy single-session path).
/// A group with ≥2 contributing transcripts is collapsed into ONE synthetic `ScanResult`
/// whose events are the union, stably sorted by wall-clock timestamp, with `line_no`
/// re-stamped to a monotonic 1..N over the merged order so `--at` cutoffs (`@line` / `@turn`
/// / datetime) and `replay`'s cutoff stay well-defined across the merged stream.
fn merge_groups_for_reconstruction(sessions: Vec<ScanResult>) -> Vec<ScanResult> {
    // Group by parent id, preserving BTreeMap key order (== session_id order for top-level
    // sessions, since a top-level's parent_session_id is its own id) for determinism.
    let mut groups: BTreeMap<String, Vec<ScanResult>> = BTreeMap::new();
    for s in sessions {
        groups
            .entry(s.parent_session_id.clone())
            .or_default()
            .push(s);
    }

    let mut out: Vec<ScanResult> = Vec::new();
    for (parent_key, group) in groups {
        let contributing = group.iter().filter(|s| !s.events.is_empty()).count();
        if contributing < 2 {
            // 0 or 1 transcript touched the file in this group → no merge; the renderers
            // skip the empty members exactly as before.
            out.extend(group);
            continue;
        }
        // Prefer the top-level session's own uuid as the merged id (re-feedable `--session`
        // target); fall back to the shared parent key if the group is subagent-only.
        let merged_id = group
            .iter()
            .find(|s| !s.is_subagent)
            .map(|s| s.session_id.clone())
            .unwrap_or_else(|| parent_key.clone());
        let mut events: Vec<FileEvent> = group
            .iter()
            .flat_map(|s| s.events.iter().cloned())
            .collect();
        events.sort_by(|a, b| cmp_ts(&a.timestamp_utc, &b.timestamp_utc));
        for (i, e) in events.iter_mut().enumerate() {
            e.line_no = i + 1;
        }
        out.push(ScanResult {
            session_id: merged_id.clone(),
            is_subagent: false,
            parent_session_id: merged_id,
            events,
            skipped_lines: 0,
        });
    }
    out
}

fn render_text(ctx: &RenderCtx, sessions: &[ScanResult], out_path: Option<&Path>) -> Result<()> {
    // Restore writes the RAW file to stdout (for piping) — no scope banner to pollute it.
    if matches!(ctx.mode, RecoverMode::Restore) {
        return render_restore(ctx, sessions, out_path, false);
    }
    crate::text::emit_scope_banner(ctx.scope_top, ctx.scope_sub);
    match ctx.mode {
        RecoverMode::Restore => unreachable!("handled above"),
        RecoverMode::Coverage => render_coverage_text(ctx, sessions),
        // Salvage == `--at @latest`: render_at_text reads ctx.at (None here) → empty `when`
        // → resolve_cutoff returns None → the final-state best-effort fragment.
        RecoverMode::At | RecoverMode::Salvage => render_at_text(ctx, sessions, out_path),
        RecoverMode::Patches => render_patches_text(ctx, sessions, out_path),
    }
}

/// DEFAULT `recover` (no mode flag): hand back the file's FINAL content as RAW restorable bytes
/// — but ONLY when it is fully recoverable. When the session saw just PART of the file (a
/// windowed read + edits), ERROR (never a holey file), naming the recoverable + missing line
/// ranges and pointing at the other modes. Across unrelated session groups the freshest,
/// most-complete candidate wins. Raw content goes to STDOUT (so `recover --file X > X` restores
/// it); the status note goes to STDERR.
fn render_restore(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
    json: bool,
) -> Result<()> {
    /// The freshest, most-complete restore candidate so far. Newer ts — then more lines — wins.
    /// Carries the source group's events + boundaries so a partial result can re-derive the
    /// richest pre-change state and list every external-change boundary.
    struct RestoreCandidate<'a> {
        known: Vec<(usize, String)>,
        total: usize,
        ts: Option<String>,
        events: &'a [FileEvent],
        boundaries: Vec<Boundary>,
    }
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let mut best: Option<RestoreCandidate> = None;
    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        let rep = replay(&s.events, None); // final state — no cutoff
        let known = rep.final_buffer.known_lines();
        if known.is_empty() {
            continue;
        }
        let total = rep
            .final_buffer
            .seen_total_lines
            .unwrap_or_else(|| known.last().map(|(n, _)| *n).unwrap_or(0));
        let ts = s
            .events
            .iter()
            .filter_map(|e| e.timestamp_utc.clone())
            .max();
        let better = match &best {
            None => true,
            Some(b) => (&ts, known.len()) > (&b.ts, b.known.len()),
        };
        if better {
            best = Some(RestoreCandidate {
                known,
                total,
                ts,
                events: &s.events,
                boundaries: rep.boundaries,
            });
        }
    }
    let Some(cand) = best else {
        bail!(
            "no recoverable history for {file} in this scope — it was never Read/Written/Edited \
             here. Widen the scope (more sessions/transcripts) or check the path."
        );
    };
    let RestoreCandidate {
        known,
        total,
        events,
        boundaries,
        ..
    } = cand;
    // Every line_no is ≤ total, so knowing `total` distinct lines ⇒ the whole 1..=total is known.
    let complete = total > 0 && known.len() == total;
    if !complete {
        bail!(
            "{}",
            restore_partial_message(file, &known, total, &boundaries, events)
        );
    }
    let mut content = known
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    if let Some(p) = out_path {
        let wrote = write_out_guarded(p, &content)?;
        if json {
            println!(
                "{}",
                serde_json::json!({"file": file, "complete": true, "lines": known.len(), "path": p.to_string_lossy(), "wrote": wrote})
            );
        } else if wrote {
            eprintln!(
                "(recovered {file} → {}, {} lines)",
                p.display(),
                known.len()
            );
        }
    } else if json {
        println!(
            "{}",
            serde_json::json!({"file": file, "complete": true, "lines": known.len(), "content": content})
        );
    } else {
        print!("{content}");
        eprintln!("(recovered {file}: {} lines, complete)", known.len());
    }
    Ok(())
}

/// The smart "can't fully restore the LATEST state" diagnostic. Beyond the covered/missing
/// ranges it (1) lists EVERY external-change boundary (Edit-before-Read / external edit — the
/// file changed outside the tool stream), (2) when a richer state existed BEFORE the first such
/// change, surfaces it (complete in the session-authored case, a fuller salvage otherwise) with
/// a dump-pre-change + dump-patches-since + reconcile-by-hand recipe, and (3) ALWAYS appends the
/// caveat that csift cannot see changes made outside the visible Read/Write/Edit stream and does
/// NOT hunt for hidden boundaries (escalated when a bash mutation may have touched the file).
fn restore_partial_message(
    file: &str,
    known: &[(usize, String)],
    total: usize,
    boundaries: &[Boundary],
    events: &[FileEvent],
) -> String {
    let covered = ranges_str(&known.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    let missing = missing_ranges_str(known, total);
    let mut m = format!(
        "cannot fully recover the LATEST {file} from this scope: recovered {}/{} line(s) \
         [{covered}], MISSING [{missing}].",
        known.len(),
        total
    );

    // External-change boundaries: the file changed OUTSIDE the tool stream and a fresh Read was
    // forced. Across these, pre-change content is no longer part of "latest".
    let ext: Vec<&Boundary> = boundaries
        .iter()
        .filter(|b| {
            matches!(
                b.kind,
                "modified_since_read" | "external_edit" | "original_file_disagreement"
            )
        })
        .collect();
    if let Some(first) = ext.iter().min_by_key(|b| b.line_no) {
        m.push_str(&format!(
            " The file changed OUTSIDE the Read/Write/Edit stream at {} point(s) (so latest can't \
             include the pre-change lines):",
            ext.len()
        ));
        for b in &ext {
            m.push_str(&format!(
                "\n  - jsonl L{} · turn {} · {} · {}",
                b.line_no,
                b.turn_index,
                format_timestamp(b.timestamp_utc.as_deref()),
                b.kind
            ));
        }
        // Richest pre-change state = just before the FIRST external boundary (before any
        // invalidation). A second replay with a cutoff there never trips the invalidation.
        let cutoff = first.line_no.saturating_sub(1);
        let pre = replay(events, Some(cutoff));
        let pre_known = pre.final_buffer.known_lines();
        let pre_total = pre
            .final_buffer
            .seen_total_lines
            .unwrap_or_else(|| pre_known.last().map(|(n, _)| *n).unwrap_or(0));
        if pre_known.len() > known.len() {
            let pre_complete = pre_total > 0 && pre_known.len() == pre_total;
            let since = first
                .timestamp_utc
                .as_deref()
                .map(|t| format!("--since '{t}'"))
                .unwrap_or_else(|| format!("(events after L{})", first.line_no));
            if pre_complete {
                m.push_str(&format!(
                    "\nBUT BEFORE that first change the file is COMPLETELY recoverable ({} lines, \
                     as of {}). Recommended (reconcile by hand): dump the pre-change version with \
                     `recover --file {file} --at @line:{cutoff}`, then the changes since with \
                     `recover --file {file} --patches {since}`.",
                    pre_known.len(),
                    format_timestamp(first.timestamp_utc.as_deref())
                ));
            } else {
                m.push_str(&format!(
                    "\nBUT BEFORE that first change MORE survives ({}/{} lines, vs {}/{} at \
                     latest). Recommended (reconcile by hand): dump that fuller fragment with \
                     `recover --file {file} --at @line:{cutoff}` (line-numbered, gaps explicit), \
                     then the changes since with `recover --file {file} --patches {since}`.",
                    pre_known.len(),
                    pre_total,
                    known.len(),
                    total
                ));
            }
        }
    } else {
        m.push_str(" This session only observed PART of the file, so a complete file can't be rebuilt here.");
    }

    m.push_str(
        " For the best-effort LATEST fragment (survivors numbered, gaps explicit) use `--salvage`; \
         for the changes use `--patches`; to scope what's recoverable use `--coverage`; or widen \
         the scope.",
    );

    // Always-on caveat — csift does NOT hunt for hidden boundaries.
    m.push_str(
        "\nNote: recovery can't fully guarantee a match to disk — anything that changed this file \
         OUTSIDE the visible Read/Write/Edit stream (a formatter like prettier, a husky/pre-commit \
         hook, git, an external editor, a bash mutation) may be invisible here; csift does not hunt \
         for hidden changes.",
    );
    let bash: Vec<&Boundary> = boundaries
        .iter()
        .filter(|b| b.kind == "bash_mutation")
        .collect();
    if let Some(b0) = bash.first() {
        m.push_str(&format!(
            " In fact this session ran {} bash command(s) that may have touched the file (first at \
             L{}) — treat the result as suspect.",
            bash.len(),
            b0.line_no
        ));
    }
    m
}

/// Compress sorted line numbers to a compact `1-50, 52, 60-72` range string (`none` when empty).
fn ranges_str(nums: &[usize]) -> String {
    if nums.is_empty() {
        return "none".to_string();
    }
    let mut sorted: Vec<usize> = nums.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let fmt = |a: usize, b: usize| {
        if a == b {
            a.to_string()
        } else {
            format!("{a}-{b}")
        }
    };
    let mut parts = Vec::new();
    let (mut start, mut prev) = (sorted[0], sorted[0]);
    for &n in &sorted[1..] {
        if n == prev + 1 {
            prev = n;
        } else {
            parts.push(fmt(start, prev));
            start = n;
            prev = n;
        }
    }
    parts.push(fmt(start, prev));
    parts.join(", ")
}

/// The `1..=total` line numbers NOT present in `known`, as a compact range string.
fn missing_ranges_str(known: &[(usize, String)], total: usize) -> String {
    let present: std::collections::HashSet<usize> = known.iter().map(|(n, _)| *n).collect();
    let missing: Vec<usize> = (1..=total).filter(|n| !present.contains(n)).collect();
    ranges_str(&missing)
}

/// Print the per-transcript header. A SUBAGENT transcript is branded
/// `SUBAGENT <hex> · parent SESSION <uuid>` (mirroring list/files/search/turns text) — its
/// `session_id` is a bare hex, NOT a re-feedable `--session` target, so it must never be
/// tokened a bare `SESSION`. A top-level transcript prints `SESSION <uuid>`.
fn session_header(first: &mut bool, s: &ScanResult) {
    if !*first {
        println!();
    }
    *first = false;
    if s.is_subagent {
        println!(
            "SUBAGENT {}  ·  parent SESSION {}",
            s.session_id, s.parent_session_id
        );
    } else {
        println!("SESSION {}", s.session_id);
    }
}

fn render_coverage_text(ctx: &RenderCtx, sessions: &[ScanResult]) -> Result<()> {
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let mut first = true;
    let mut any = false;
    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        any = true;
        let rep = replay(&s.events, None);
        session_header(&mut first, s);
        println!("  file: {file}");
        let ranges = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
        let known = ranges.len();
        let total = rep.final_buffer.seen_total_lines.unwrap_or(known);
        let pct = if total > 0 { known * 100 / total } else { 0 };
        let fragments = rep.boundaries_hard_count() + 1;
        println!("  recoverable: {known}/{total} lines ({pct}%)  fragments: {fragments}");
        let covered = covered_spans(&ranges);
        println!("  covered line ranges: {}", fmt_spans(&covered));
        println!("  events: {}", fmt_counts(&rep.counts));
        if rep.boundaries.is_empty() {
            println!("  integrity boundaries: (none)");
        } else {
            println!("  integrity boundaries:");
            for b in &rep.boundaries {
                let sym = if b.confidence == Confidence::Authoritative {
                    "⚠"
                } else {
                    "~"
                };
                println!(
                    "    {sym} L{}  turn {}  {}  {} ({})",
                    b.line_no,
                    b.turn_index,
                    format_timestamp(b.timestamp_utc.as_deref()),
                    b.detail,
                    b.confidence.label()
                );
            }
        }
        if rep.counts.edit_unanchorable > 0 {
            println!(
                "  un-anchorable edits (coverage holes): {}",
                rep.counts.edit_unanchorable
            );
        }
    }
    if !any {
        println!("no recoverable history for {file} in range");
    }
    print_footer(ctx);
    Ok(())
}

fn render_patches_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
) -> Result<()> {
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let mut first = true;
    let mut any = false;
    let mut out_blob = String::new();

    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        let rep = replay(&s.events, None);
        if rep.segments.is_empty() && rep.boundaries.is_empty() {
            continue;
        }
        any = true;
        session_header(&mut first, s);
        println!("  file: {file}");

        // Interleave segments + boundaries in jsonl-line order.
        let mut items: Vec<TimelineItem> = Vec::new();
        for seg in &rep.segments {
            items.push(TimelineItem::Seg(seg.clone()));
        }
        for b in &rep.boundaries {
            items.push(TimelineItem::Bound(b.clone()));
        }
        items.sort_by_key(TimelineItem::sort_key);

        for item in &items {
            match item {
                TimelineItem::Seg(seg) => {
                    let pre = match (seg.pre_state_known, seg.anchor_source) {
                        (_, Some(src)) => format!("pre-state: {} anchor", src.label()),
                        (true, None) => "pre-state: known".to_string(),
                        (false, None) => "pre-state PARTIALLY UNKNOWN after boundary".to_string(),
                    };
                    println!(
                        "  ─ SEGMENT {}  L{}..L{}  turns {}..{}  {}..{}  ({pre}) ─",
                        seg.index,
                        seg.line_no_start,
                        seg.line_no_end,
                        seg.turn_start,
                        seg.turn_end,
                        format_timestamp(seg.ts_start.as_deref()),
                        format_timestamp(seg.ts_end.as_deref()),
                    );
                    let old = filter_lines(&seg.start_buffer, ctx.line_range);
                    let new = filter_lines(&seg.end_buffer, ctx.line_range);
                    let diff = unified_diff(&old, &new, usize::MAX);
                    if diff.is_empty() {
                        println!("  (no change in this segment)");
                    } else {
                        for line in diff.lines() {
                            println!("  {line}");
                        }
                    }
                    out_blob.push_str(&diff);
                }
                TimelineItem::Bound(b) => {
                    let sym = if b.confidence == Confidence::Authoritative {
                        "⚠"
                    } else {
                        "~"
                    };
                    println!(
                        "  {sym} INTEGRITY BOUNDARY  L{}  turn {}  {}  {} ({})",
                        b.line_no,
                        b.turn_index,
                        format_timestamp(b.timestamp_utc.as_deref()),
                        b.detail,
                        b.confidence.label()
                    );
                }
            }
        }
    }

    if !any {
        println!("no recoverable history for {file} in range");
    }
    if let Some(p) = out_path {
        if write_out_guarded(p, &out_blob)? {
            println!();
            println!("(wrote concatenated patches to {})", p.display());
        }
    }
    print_footer(ctx);
    Ok(())
}

fn render_at_text(ctx: &RenderCtx, sessions: &[ScanResult], out_path: Option<&Path>) -> Result<()> {
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let when = ctx.at.as_deref().unwrap_or("");
    let mut first = true;
    let mut any = false;
    let mut out_blob = String::new();

    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        let cutoff = resolve_cutoff(when, &s.events)?;
        let rep = replay(&s.events, cutoff);
        let known = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
        if known.is_empty() && rep.final_buffer.seen_total_lines.is_none() {
            continue;
        }
        any = true;
        session_header(&mut first, s);
        println!("  file: {file}");
        if let Some(c) = cutoff {
            println!("  as of: jsonl line {c}");
        }
        let total = rep.final_buffer.seen_total_lines.unwrap_or(0);
        let rendered = render_snapshot_body(&known, total, true);
        for line in rendered.lines() {
            println!("  {line}");
        }
        // The --out artifact: known lines + explicit gap markers (honest).
        out_blob.push_str(&render_snapshot_body(&known, total, false));
        out_blob.push('\n');
    }

    if !any {
        println!("no recoverable history for {file} as of {when}");
    }
    if let Some(p) = out_path {
        if write_out_guarded(p, &out_blob)? {
            println!();
            println!("(wrote partial snapshot to {})", p.display());
        }
    }
    print_footer(ctx);
    Ok(())
}

/// Render a partial snapshot body: known lines numbered, gaps explicit. `inline_trunc`
/// truncates long lines for human stdout (false for the verbatim `--out` artifact).
fn render_snapshot_body(known: &[(usize, String)], total: usize, inline_trunc: bool) -> String {
    let mut out = String::new();
    let mut prev: usize = 0;
    let last_known = known.last().map(|(n, _)| *n).unwrap_or(0);
    for (n, text) in known {
        if *n > prev + 1 {
            out.push_str(&format!("??? lines {}..{} unknown\n", prev + 1, n - 1));
        }
        let body = if inline_trunc {
            truncate_excerpt(text)
        } else {
            text.clone()
        };
        out.push_str(&format!("{n:>5}  {body}\n"));
        prev = *n;
    }
    // Trailing gap up to the last-seen total.
    if total > last_known && last_known > 0 {
        out.push_str(&format!(
            "??? lines {}..{} unknown\n",
            last_known + 1,
            total
        ));
    } else if known.is_empty() {
        if total > 0 {
            out.push_str(&format!(
                "??? lines 1..{total} unknown (no content seen in range)\n"
            ));
        } else {
            out.push_str("(no content seen for this file in range)\n");
        }
    }
    out
}

/// A timeline item for `--patches` ordering (segments + boundaries interleaved).
#[derive(Debug)]
enum TimelineItem {
    Seg(Segment),
    Bound(Boundary),
}

impl TimelineItem {
    fn sort_key(&self) -> usize {
        match self {
            TimelineItem::Seg(s) => s.line_no_start,
            TimelineItem::Bound(b) => b.line_no,
        }
    }
}

/// Dense line vector of a buffer (gaps omitted) restricted to `--line-range`.
fn filter_lines(buf: &SparseBuffer, line_range: Option<(usize, usize)>) -> Vec<String> {
    apply_line_range(buf.known_lines(), line_range)
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

/// The contiguous spans of a `(line_no, text)` vector, as inclusive `(lo, hi)`.
fn covered_spans(lines: &[(usize, String)]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (n, _) in lines {
        match spans.last_mut() {
            Some(last) if last.1 + 1 == *n => last.1 = *n,
            _ => spans.push((*n, *n)),
        }
    }
    spans
}

fn fmt_spans(spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return "(none)".to_string();
    }
    spans
        .iter()
        .map(|(a, b)| format!("[{a}..{b}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn fmt_counts(c: &EventCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    let reads = c.read_full + c.read_windowed;
    if reads > 0 {
        parts.push(format!(
            "{reads} read ({} full, {} windowed)",
            c.read_full, c.read_windowed
        ));
    }
    if c.edit > 0 {
        let unanch = if c.edit_unanchorable > 0 {
            format!(" ({} un-anchorable)", c.edit_unanchorable)
        } else {
            String::new()
        };
        parts.push(format!("{} edit{unanch}", c.edit));
    }
    if c.write > 0 {
        parts.push(format!("{} write", c.write));
    }
    if c.bash > 0 {
        parts.push(format!("{} bash (heuristic)", c.bash));
    }
    if c.external_edit > 0 {
        parts.push(format!("{} external-edit", c.external_edit));
    }
    if c.history_snapshot > 0 {
        parts.push(format!("{} history-snapshot", c.history_snapshot));
    }
    if c.integrity_error > 0 {
        parts.push(format!("{} integrity-error", c.integrity_error));
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        parts.join(" · ")
    }
}

fn print_footer(ctx: &RenderCtx) {
    let mode = match ctx.mode {
        RecoverMode::Patches => "patches",
        RecoverMode::At => "at",
        RecoverMode::Salvage => "salvage",
        RecoverMode::Coverage => "coverage",
        RecoverMode::Restore => "restore", // never reached — render_restore returns before the footer
    };
    println!();
    println!(
        "mode={mode}  (reconstruction is partial — unknown lines are explicit, never fabricated)"
    );
    if ctx.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(ctx.skipped_lines));
    }
}

impl Replay {
    /// Count of HARD (authoritative + heuristic-promoted) boundaries for fragment math.
    fn boundaries_hard_count(&self) -> usize {
        self.boundaries.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON rendering (NDJSON — one object per line, trailing summary)
// ─────────────────────────────────────────────────────────────────────────────

fn render_json(ctx: &RenderCtx, sessions: &[ScanResult], out_path: Option<&Path>) -> Result<()> {
    use serde_json::json;
    if matches!(ctx.mode, RecoverMode::Restore) {
        return render_restore(ctx, sessions, out_path, true);
    }
    let mut session_count = 0usize;

    // Leading `{kind:"session_header", …}` scope record (same three span fields as turns),
    // emitted only when the scope spans ≥1 subagent — uniform JSON scope disclosure.
    let (scope_top, scope_sub) = (ctx.scope_top, ctx.scope_sub);
    if scope_sub > 0 {
        println!(
            "{}",
            serde_json::to_string(&crate::text::scope_header_json(scope_top, scope_sub))?
        );
    }

    match ctx.mode {
        RecoverMode::Restore => unreachable!("Restore handled above in render_json"),
        RecoverMode::Coverage => {
            for s in sessions {
                if s.events.is_empty() {
                    continue;
                }
                session_count += 1;
                let rep = replay(&s.events, None);
                let known = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
                let spans = covered_spans(&known);
                let obj = json!({
                    "session_id": s.session_id,
                    "is_subagent": s.is_subagent,
                    "parent_session_id": s.parent_session_id,
                    "file": ctx.file,
                    "recoverable_lines": known.len(),
                    "seen_total_lines": rep.final_buffer.seen_total_lines,
                    "covered_ranges": spans.iter().map(|(a,b)| [*a,*b]).collect::<Vec<_>>(),
                    "fragments": rep.boundaries_hard_count() + 1,
                    "events": counts_json(&rep.counts),
                    "boundaries": rep.boundaries.iter().map(boundary_json).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string(&obj)?);
            }
        }
        RecoverMode::Patches => {
            let mut out_blob = String::new();
            for s in sessions {
                if s.events.is_empty() {
                    continue;
                }
                let rep = replay(&s.events, None);
                if rep.segments.is_empty() && rep.boundaries.is_empty() {
                    continue;
                }
                session_count += 1;
                let mut items: Vec<TimelineItem> = Vec::new();
                for seg in &rep.segments {
                    items.push(TimelineItem::Seg(seg.clone()));
                }
                for b in &rep.boundaries {
                    items.push(TimelineItem::Bound(b.clone()));
                }
                items.sort_by_key(TimelineItem::sort_key);
                for item in &items {
                    match item {
                        TimelineItem::Seg(seg) => {
                            let old = filter_lines(&seg.start_buffer, ctx.line_range);
                            let new = filter_lines(&seg.end_buffer, ctx.line_range);
                            let diff = unified_diff(&old, &new, usize::MAX);
                            out_blob.push_str(&diff);
                            let obj = json!({
                                "session_id": s.session_id,
                                "is_subagent": s.is_subagent,
                                "parent_session_id": s.parent_session_id,
                                "type": "segment",
                                "segment_index": seg.index,
                                "line_no": seg.line_no_start,
                                "line_no_start": seg.line_no_start,
                                "line_no_end": seg.line_no_end,
                                "turn_start": seg.turn_start,
                                "turn_end": seg.turn_end,
                                "ts_utc": seg.ts_start,
                                "ts_local": seg.ts_start.as_deref().and_then(local_iso),
                                "pre_state_known": seg.pre_state_known,
                                "anchor_source": seg.anchor_source.map(SnapSource::label),
                                "unified_diff": diff,
                            });
                            println!("{}", serde_json::to_string(&obj)?);
                        }
                        TimelineItem::Bound(b) => {
                            let mut obj = boundary_json(b);
                            obj["type"] = json!("boundary");
                            obj["session_id"] = json!(s.session_id);
                            obj["is_subagent"] = json!(s.is_subagent);
                            obj["parent_session_id"] = json!(s.parent_session_id);
                            println!("{}", serde_json::to_string(&obj)?);
                        }
                    }
                }
            }
            if let Some(p) = out_path {
                write_out_guarded(p, &out_blob)?;
            }
        }
        RecoverMode::At | RecoverMode::Salvage => {
            // Salvage feeds an empty `when` (ctx.at is None) → @latest (no cutoff).
            let when = ctx.at.as_deref().unwrap_or("");
            let mut out_blob = String::new();
            for s in sessions {
                if s.events.is_empty() {
                    continue;
                }
                let cutoff = resolve_cutoff(when, &s.events)?;
                let rep = replay(&s.events, cutoff);
                let known = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
                if known.is_empty() && rep.final_buffer.seen_total_lines.is_none() {
                    continue;
                }
                session_count += 1;
                let total = rep.final_buffer.seen_total_lines;
                let gaps = gap_ranges(&known, total.unwrap_or(0));
                // Provenance: which jsonl line last set each known file line.
                let prov: BTreeMap<usize, usize> = rep
                    .final_buffer
                    .known_lines_with_provenance()
                    .into_iter()
                    .map(|(n, _, set_at)| (n, set_at))
                    .collect();
                let obj = json!({
                    "session_id": s.session_id,
                    "is_subagent": s.is_subagent,
                    "parent_session_id": s.parent_session_id,
                    "type": "snapshot",
                    "file": ctx.file,
                    "line_no": cutoff,
                    "line_no_cutoff": cutoff,
                    "lines": known.iter().map(|(n,t)| json!({
                        "n": n,
                        "text": t,
                        "set_at_line": prov.get(n),
                    })).collect::<Vec<_>>(),
                    "gaps": gaps.iter().map(|(a,b)| [*a,*b]).collect::<Vec<_>>(),
                    "seen_total_lines": total,
                });
                println!("{}", serde_json::to_string(&obj)?);
                out_blob.push_str(&render_snapshot_body(&known, total.unwrap_or(0), false));
                out_blob.push('\n');
            }
            if let Some(p) = out_path {
                write_out_guarded(p, &out_blob)?;
            }
        }
    }

    // Trailing summary line.
    let summary = json!({
        "summary": {
            "sessions": session_count,
            "file": ctx.file,
            "mode": match ctx.mode {
                RecoverMode::Patches => "patches",
                RecoverMode::At => "at",
                RecoverMode::Salvage => "salvage",
                RecoverMode::Coverage => "coverage",
                RecoverMode::Restore => "restore",
            },
            "skipped_lines": ctx.skipped_lines,
        }
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

fn counts_json(c: &EventCounts) -> serde_json::Value {
    serde_json::json!({
        "read_full": c.read_full,
        "read_windowed": c.read_windowed,
        "edit": c.edit,
        "edit_unanchorable": c.edit_unanchorable,
        "write": c.write,
        "bash": c.bash,
        "external_edit": c.external_edit,
        "history_snapshot": c.history_snapshot,
        "integrity_error": c.integrity_error,
    })
}

fn boundary_json(b: &Boundary) -> serde_json::Value {
    serde_json::json!({
        "line_no": b.line_no,
        "turn_index": b.turn_index,
        "ts_utc": b.timestamp_utc,
        "ts_local": b.timestamp_utc.as_deref().and_then(local_iso),
        "kind": b.kind,
        "confidence": b.confidence.json(),
        "detail": b.detail,
    })
}

/// The explicit gap ranges of a known-line vector up to `total` (1-based, inclusive).
fn gap_ranges(known: &[(usize, String)], total: usize) -> Vec<(usize, usize)> {
    let mut gaps = Vec::new();
    let mut prev = 0usize;
    let last = known.last().map(|(n, _)| *n).unwrap_or(0);
    for (n, _) in known {
        if *n > prev + 1 {
            gaps.push((prev + 1, n - 1));
        }
        prev = *n;
    }
    if total > last && last > 0 {
        gaps.push((last + 1, total));
    } else if known.is_empty() && total > 0 {
        gaps.push((1, total));
    }
    gaps
}

#[cfg(test)]
mod tests;
