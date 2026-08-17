//! run_files: scope resolution, per-file scan, candidate prefilter.

use super::*;

/// Entry point for `csift files`.
pub fn run_files(args: &FilesArgs) -> Result<()> {
    // `--turn` and `--since`/`--until` INTERSECT (AND) - the one windowing rule every
    // command shares (the former mutual-exclusion bail was a leftover; search/recover/stats
    // already intersected).
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // ── Compile the optional path filters BEFORE any scan, so an invalid --regex/--glob
    //    fails fast (a hard error) rather than after a full pass ──
    let path_filter = PathFilter::from_args(args.regex.as_deref(), args.glob.as_deref())?;

    // ── Resolve targets → session files (subagent span per --no-subagents; default spans
    //    subagents, matching every other default-on command) ──
    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        path::SubagentScope::from(args.want_subagents()),
        path::Caller::Files,
    )?;

    // SCOPE span of the resolved set (every transcript, incl. mutation-free subagents) so the
    // fan-out is announced from the true file set, not just the mutation-bearing subset.
    let scope_sub = session_files
        .iter()
        .filter(|p| crate::subagent::is_subagent_path(p))
        .count();
    let scope_top = session_files.len() - scope_sub;

    // ── Parallel scan across files (default rayon pool = CPU count) ──
    let per_file: Vec<FileResult> = session_files
        .par_iter()
        .map(|p| scan_one_file(p))
        .collect::<Result<Vec<_>>>()?;

    // ── Merge + filter by turn / time-window per mutation + boundary ──
    let mut mutations: Vec<TaggedMutation> = Vec::new();
    let mut boundaries: Vec<TaggedBoundary> = Vec::new();
    let mut skipped_lines = 0usize;
    for fr in per_file {
        skipped_lines += fr.skipped_lines;
        // Resolve the `--turn` spec against THIS file's turn count (0-based), so
        // open/from-end forms (`N..`, `-3..` = the last 3) window each transcript's own turns.
        let turn_bounds = turn_range.map(|spec| spec.resolve(fr.turn_count, false));
        for tm in fr.mutations {
            if let Some((lo, hi)) = turn_bounds {
                if tm.turn_index < lo || tm.turn_index > hi {
                    continue;
                }
            }
            // A mutation with no timestamp never falls inside a BOUNDED window
            // (same rule as search/agents); an unbounded window admits it.
            if !time_window.contains(tm.mutation.timestamp_utc.as_deref()) {
                continue;
            }
            // --regex / --glob: keep only paths satisfying EVERY supplied filter, over the
            // FULL path, BEFORE the rollup so all --by views reflect the filtered set.
            if !path_filter.keeps(&tm.mutation.path) {
                continue;
            }
            mutations.push(tm);
        }
        // Boundaries obey the SAME turn / time-window / path filters as mutations.
        for tb in fr.boundaries {
            if let Some((lo, hi)) = turn_bounds {
                if tb.turn_index < lo || tb.turn_index > hi {
                    continue;
                }
            }
            if !time_window.contains(tb.timestamp_utc.as_deref()) {
                continue;
            }
            if !path_filter.keeps(&tb.path) {
                continue;
            }
            boundaries.push(tb);
        }
    }
    boundaries.sort_by(|a, b| a.line_no.cmp(&b.line_no));

    let outcome = Outcome {
        detail: args.detail(),
        mutations,
        boundaries,
        skipped_lines,
        turn_range: args.turn_range.clone(),
        time_window_bounded: !time_window.is_unbounded(),
        scope_top,
        scope_sub,
    };

    match args.format {
        OutputFormat::Text => render_text(&outcome),
        OutputFormat::Json => render_json(&outcome)?,
    }
    Ok(())
}

/// Scan one session file: mmap → prefilter → parse → delimit turns → extract + join.
pub(crate) fn scan_one_file(path: &Path) -> Result<FileResult> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(FileResult {
            mutations: Vec::new(),
            boundaries: Vec::new(),
            skipped_lines: 0,
            turn_count: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // The session id is the jsonl basename, but for a SUBAGENT transcript the on-disk
    // stem is `agent-<hex>` whereas the canonical id (the record `agentId`, and what the
    // `agents` topology prints) is the BARE hex. The shared helper strips the prefix so a
    // `files` subagent row's `session_id` is joinable to `agents` (id-form unification).
    let session_id = crate::subagent::session_id_from_path(path);

    // Retain transcript records in file order. The mutation prefilter gates the parse
    // on raw bytes (pre-JSON): a line is kept only if it could carry a mutation
    // (`Edit`/`Write`/`NotebookEdit`/`MultiEdit`/`Bash`/`filePath`) OR it is a genuine-
    // user delimiter (`"role":"user"`, needed so turns can still be delimited). Skipped
    // malformed lines are counted, never hidden.
    // Parse all files-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool) so a
    // single giant transcript is not scanned on one core. KEEP the parallel scan's exact jsonl
    // line numbers (aligned with `records` by index) - every `files` row + Edit-before-Read
    // boundary carries its `Lnnnn` so it joins back to the raw transcript like recover/search.
    let (recs, skipped) = crate::parse::parse_candidates_parallel(bytes, line_is_files_candidate);
    let line_nos: Vec<usize> = recs.iter().map(|(ln, _)| *ln).collect();
    let records: Vec<Record> = recs.into_iter().map(|(_, rec)| rec).collect();

    // A subagent transcript's `session_id` is a non-re-feedable bare hex; stamp the
    // id-domain discriminator + the re-feedable parent uuid (the dir before `subagents/`)
    // onto every mutation, so the timeline JSON can distinguish a parent UUID from a
    // subagent transcript hex. A top-level file is its own parent.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());
    let mut mutations = extract_mutations(&session_id, &records, &line_nos);
    let mut boundaries = extract_boundaries(&session_id, &records, &line_nos);
    if is_subagent {
        for tm in &mut mutations {
            tm.is_subagent = true;
            tm.parent_session_id = parent_session_id.clone();
        }
        for tb in &mut boundaries {
            tb.is_subagent = true;
            tb.parent_session_id = parent_session_id.clone();
        }
    }
    // Per-file turn count for resolving `--turn` open/from-end forms (same grouping
    // `extract_mutations`/`extract_boundaries` used to assign each `turn_index`).
    let turn_count = group_turn_indices_deduped(&records, |r| r).len();
    Ok(FileResult {
        mutations,
        boundaries,
        skipped_lines: skipped,
        turn_count,
    })
}

/// Pre-JSON byte prefilter: keep a line if it could carry a file mutation OR is a
/// genuine-user delimiter (so turns can still be delimited even when a turn opens with
/// no mutation in it). Broad-by-design (substring, not structural) so no mutation is
/// lost. Like `search`'s prefilter, this only gates the parse.
pub(crate) fn line_is_files_candidate(line: &[u8]) -> bool {
    // R13: the genuine-user hook is serialization-tolerant (user-only - assistant
    // coverage rides the tool-name needles below, so admitting every assistant
    // text record here would repeal this prefilter). Finders built ONCE (per-line
    // hot path - the stateless form rebuilt its searcher every call).
    static NEEDLES: std::sync::LazyLock<[memmem::Finder<'static>; 5]> =
        std::sync::LazyLock::new(|| {
            [
                memmem::Finder::new(b"Edit"),
                memmem::Finder::new(b"Write"),
                memmem::Finder::new(b"Bash"),
                memmem::Finder::new(b"filePath"),
                // Keep tool_result ERROR carriers - they carry the Edit-before-Read boundaries
                // (and drive `failed_ids`, so a cancelled/errored op is never miscounted as a
                // real mutation), and an error carrier may not otherwise match (its
                // `"role":"user"` is its only hook).
                memmem::Finder::new(b"is_error"),
            ]
        });
    crate::parse::line_has_user_role_marker(line) || NEEDLES.iter().any(|f| f.find(line).is_some())
}
