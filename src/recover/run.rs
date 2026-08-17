//! run_recover + the --files-from/--out-dir batch driver.

use super::*;

/// Entry point for `csift recover`.
pub fn run_recover(args: &RecoverArgs) -> Result<()> {
    // BATCH MODE: many files in one corpus scan (parse each transcript ONCE).
    if args.files_from.is_some() {
        return run_recover_batch(args);
    }
    // ── Validate window mutual-exclusion (same rule + wording as `files`/`search`) ──
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
    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
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

    // ── Apply the turn / time window to events per session. The `--turn` spec resolves
    //    its open/from-end forms against THIS session's own turns (max event turn + 1). ──
    for sr in &mut sessions {
        let bounds = turn_range.map(|spec| {
            let tc = sr
                .events
                .iter()
                .map(|e| e.turn_index)
                .max()
                .map_or(0, |m| m + 1);
            spec.resolve(tc, false)
        });
        sr.events.retain(|e| {
            window_admits(
                e.turn_index,
                e.timestamp_utc.as_deref(),
                bounds,
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
pub(crate) struct BatchOutcome {
    pub(crate) target: String,
    pub(crate) status: &'static str, // "complete" | "partial" | "no-history" | "skipped-exists"
    pub(crate) known: usize,
    pub(crate) total: usize,
    pub(crate) written: Option<std::path::PathBuf>,
}

/// The last path component (the filename) — the distinctive token a transcript carries for
/// every op on the file, and the [`aho_corasick`] pattern that gates parsing.
pub(crate) fn basename_of(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

pub(crate) fn run_recover_batch(args: &RecoverArgs) -> Result<()> {
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

    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
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
            let bounds = turn_range.map(|spec| {
                let tc = sr
                    .events
                    .iter()
                    .map(|e| e.turn_index)
                    .max()
                    .map_or(0, |m| m + 1);
                spec.resolve(tc, false)
            });
            sr.events.retain(|e| {
                window_admits(
                    e.turn_index,
                    e.timestamp_utc.as_deref(),
                    bounds,
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
