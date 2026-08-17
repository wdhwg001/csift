//! run_search: resolve scope, fan out, merge the chronological timeline, cap + render.

use super::*;

/// Pre-scan advisory notes (stderr only): a uuid-shaped PATTERN with no session target, and
/// the truly-unbounded-search warning (empty pattern + no category/time/turn/session filter).
fn emit_advisory_notes(
    args: &SearchArgs,
    matcher: &Matcher,
    has_turn_range: bool,
    time_window: &TimeWindow,
    has_session_filter: bool,
) {
    if path::is_uuid(&args.pattern) && !has_session_filter {
        eprintln!(
            "csift: note: searching for this uuid as TEXT across the scope; to scope the \
             search TO that session, pass it as a target: `csift search <PATTERN> @{}`",
            args.pattern
        );
    }
    if matcher.is_pure_filter()
        && args.labels.is_empty()
        && args.labels_not.is_empty()
        && !has_turn_range
        && time_window.is_unbounded()
        && !has_session_filter
    {
        eprintln!(
            "csift: warning: empty pattern with no category/time/turn/session filter \
             matches every exchange in scope — this may emit a lot."
        );
    }
}

/// `--count-only`: emit only the TRUE total of matching exchanges (add back any capped by
/// `--max-count`), the ripgrep `-c` idiom — no per-exchange output.
fn emit_count_only(outcome: &SearchOutcome, format: OutputFormat) -> Result<()> {
    let total = outcome.exchanges.len() + outcome.dropped_by_cap;
    match format {
        OutputFormat::Text => println!("{total}"),
        OutputFormat::Json => {
            // envelope v2 even here: header + summary (no rows) — one reading idiom.
            let header = crate::text::envelope_scope_header(
                "search",
                outcome.scope_top,
                outcome.scope_sub,
                serde_json::json!({}),
            );
            println!("{}", serde_json::to_string(&header)?);
            let summary = crate::text::envelope_summary(serde_json::json!({ "matched": total }));
            println!("{}", serde_json::to_string(&summary)?);
        }
    }
    Ok(())
}

/// `-l`: only WHICH sessions matched, one OWNING id (`parent_session_id`) per line — sorted,
/// deduped, UNCAPPED (the grep idiom, built to pipe into `--sessions-from -`). A
/// `--max-count` drop could hide sessions, so it is disclosed on stderr (stdout stays a pure
/// id stream) — no silent truncation.
fn emit_sessions_with_matches(outcome: &SearchOutcome, format: OutputFormat) -> Result<()> {
    if format == OutputFormat::Json {
        bail!("-l prints a plain id stream; with --format json read the summary's `transcript_ids` instead");
    }
    let mut ids: Vec<&str> = outcome
        .exchanges
        .iter()
        .map(|e| e.parent_session_id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    for id in &ids {
        println!("{id}");
    }
    if outcome.dropped_by_cap > 0 {
        eprintln!(
            "csift: note: {} exchange(s) dropped by --max-count — this session listing \
             may be incomplete; raise --max-count",
            outcome.dropped_by_cap
        );
    }
    Ok(())
}

pub fn run_search(args: &SearchArgs) -> Result<()> {
    // ── PATTERN-position traps (the ONE place csift's positional grammar differs:
    //    search's first positional is the PATTERN, not a target) ──
    if args.pattern.starts_with('@') {
        bail!(
            "search's FIRST positional is the regex PATTERN — targets come AFTER it: \
             `csift search <PATTERN> {0}`. To literally match '{0}', escape the @: '\\{0}'.",
            args.pattern
        );
    }

    // Unlike files/turns/list/agents/recover (whose first positional is the PATH/`@<uuid>`
    // target), search's FIRST positional is PATTERN — so a bare uuid here is a LITERAL pattern,
    // searched verbatim across scope. To scope to a session, pass it as an `@<uuid>` POSITIONAL
    // (a PATH target), exactly like every sibling (`csift search PATTERN @<uuid>`).
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // A truly unbounded search (empty pattern + no filters) will emit a lot. Warn,
    // but do not refuse — SPEC §6.2 explicitly allows it. An `@<uuid>` / `@<hex>` / `*.jsonl`
    // POSITIONAL pins a single session (via resolve_session_files), so it counts as a session
    // filter here too — otherwise the warning would falsely claim "no session filter" on a run
    // that is in fact scoped to one session.
    let has_session_filter = args.sessions_from.is_some()
        || args
            .targets()
            .iter()
            .filter_map(|p| p.to_str())
            .any(path::pins_single_session);
    // A `-t`/`-T` combination that excludes everything it includes can never match — a
    // statically-detectable mistake, so fail loud (never an honest-looking empty result).
    if args.label_filter().is_statically_empty() {
        bail!(
            "-T excludes every label the -t selection includes (-t {:?} -T {:?}) — this \
             filter can never match anything. Loosen -T or widen -t.",
            args.labels,
            args.labels_not
        );
    }

    let matcher = build_matcher(args)?;

    // ── Resolve targets → session files via the shared (optionally subagent-spanning)
    //    resolver. (Record FETCHING by line/uuid is `csift show`'s job, not search's.)
    //    Resolution runs BEFORE the advisory notes below: an unreachable target must fail
    //    first, not after a scope warning about a run that was never going to happen (R9). ──
    let session_files = path::resolve_targets_with_session_list(
        &args.targets(),
        args.sessions_from.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;

    emit_advisory_notes(
        args,
        &matcher,
        turn_range.is_some(),
        &time_window,
        has_session_filter,
    );

    // `--siblings <SPEC>`: parse the repeatable caps ONCE here (a malformed spec is a hard
    // error, surfaced before any scan). `None` ⇒ siblings off. Parsed up front so the per-file
    // parallel scan just borrows the result.
    let want_siblings = args.siblings;

    // ── Spawn-lookup hoist (GOLD §3): build each DISTINCT discovery-root's DiscoveredSpawns
    //    ONCE, then share it across the whole par_iter. A subagent's discovery-root is its parent
    //    top-level `.jsonl` (the SAME for all its siblings), so building the lookup per file made
    //    `search .` O(subagent_count²) — 3290 files each re-running `discover_subagents` over the
    //    parent's 3290-entry `subagents/` tree (~66s on a 1.4 GB corpus). Distinct roots number
    //    only a handful (one per top-level session in scope), so `discover_subagents` now runs
    //    ~7× total instead of once per file. The lookup values are IDENTICAL — only WHEN/how
    //    often they are built changes — so output is byte-for-byte unchanged. Sequential build is
    //    fine: distinct-root count is tiny. `None` ⇒ that root has no resolvable spawns. ──
    let mut spawn_map: HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>> = HashMap::new();
    for p in &session_files {
        spawn_map
            .entry(discovery_root_for(p))
            .or_insert_with_key(|root| build_spawn_lookup(root).map(Arc::new));
    }

    // ── Parallel scan across files; collect order-stable, then merge ──
    // Inner per-turn fan-out is enabled only when the resolved scope is too SMALL to fill
    // the pool from the outside (a scoped one/few-transcript query, `show`'s fetch): with
    // thousands of files the across-files fan-out already saturates every worker, and
    // nested fan-out just adds steal churn (measured: broad-match wall +0.2s, sys +0.13s).
    let inner_parallel = session_files.len() <= rayon::current_num_threads() * 2;
    let per_file: Vec<FileResult> = session_files
        .par_iter()
        .map(|p| {
            search_one_file(
                p,
                args,
                &matcher,
                turn_range,
                &time_window,
                None,
                want_siblings,
                &spawn_map,
                inner_parallel,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    // SCOPE span of the resolved set (every transcript, incl. hit-free subagents).
    let scope_sub = session_files
        .iter()
        .filter(|p| crate::subagent::is_subagent_path(p))
        .count();
    let scope_top = session_files.len() - scope_sub;

    // ── Combined STABLE chronological timeline (SPEC §6.2) ──
    // Flatten every file's exchanges into ONE timeline, then stable-sort by the
    // turn-opening timestamp so subagent exchanges INTERLEAVE with top-level ones by
    // absolute time (both clocks are the same machine's UTC). ISO-8601 sorts as text;
    // timestamp-less exchanges sort LAST (mirrors `files --timeline`). The pre-sort
    // order — sorted file order, then turn order — is deterministic, and a stable sort
    // keeps it as the tie-break, so the timeline is fully reproducible. The GLOBAL
    // --max-count cap is applied AFTER the sort (keeping the EARLIEST N), never
    // silently — the dropped remainder is reported in the footer.
    let mut outcome = SearchOutcome {
        scope_top,
        scope_sub,
        ..SearchOutcome::default()
    };
    let mut all: Vec<Exchange> = Vec::new();
    for fr in per_file {
        outcome.skipped_lines += fr.skipped_lines;
        all.extend(fr.exchanges);
    }
    all.sort_by(|a, b| {
        timestamp_sort_key(a.started_utc.as_deref())
            .cmp(&timestamp_sort_key(b.started_utc.as_deref()))
    });
    // TRUE totals, captured BEFORE the cap window — the head banner and tail footer report
    // these; the JSON summary keeps its post-cap `matched` + `dropped_by_cap` pair unchanged.
    outcome.total_matched = all.len();
    outcome.total_sessions = distinct_session_count(&all);

    // `--max-count 0` = uncapped (the crate-wide convention). SIGNED: a positive N keeps the
    // EARLIEST N of the chronological stream, a negative N the LATEST N — the kept exchanges
    // still emit oldest-first among themselves (ONE ordering rule; the sign only selects a
    // prefix or suffix of the sorted timeline, mirroring the range grammar's `-k` from-end).
    if let Some(cap) = args.max_count.filter(|&n| n != 0) {
        let keep = usize::try_from(cap.unsigned_abs()).unwrap_or(usize::MAX);
        if all.len() > keep {
            outcome.dropped_by_cap = all.len() - keep;
            if cap > 0 {
                all.truncate(keep);
            } else {
                all.drain(..all.len() - keep);
            }
        }
    }
    outcome.exchanges = all;

    // `--count-only`: emit only the TRUE total of matching exchanges (add back any capped by
    // `--max-count`), the ripgrep `-c` idiom — no per-exchange output.
    if args.count_only {
        return emit_count_only(&outcome, args.format);
    }

    // `-l`: only WHICH sessions matched, one id per line — sorted, deduped, UNCAPPED (the
    // grep idiom, built to pipe into `--sessions-from -`). Ids are the OWNING sessions
    // (`parent_session_id`) — the scope-token domain: re-targeting is scope-level, so a
    // subagent hit lists its parent uuid (always re-feedable; a per-transcript detail id is
    // the JSON summary's `transcript_ids` / a hit's `refetch`). A `--max-count` drop could hide
    // sessions, so it is disclosed on stderr (stdout stays a pure id stream) — no silent
    // truncation.
    if args.sessions_with_matches {
        return emit_sessions_with_matches(&outcome, args.format);
    }

    // `--count-by <axis>`: a per-KEY census of the matched records along ONE closed axis
    // (label/tool/turn/session/pairing/model) — the exploration on-ramp so an empty
    // `-t <leaf>` result is never mistaken for a typo, and the 1-command answer to
    // "any pending tools?" / "which model?" / "hits per turn?". stdout = the census; the
    // accounting note goes to stderr (text) so `<count> <key>` stays pipe-clean. Records
    // outside the axis's domain are excluded AND reported (never silent).
    if let Some(axis) = args.count_by {
        let (rows, records, excluded) = axis_census(
            &outcome.exchanges,
            axis,
            LabelFilter::new(&args.labels, &args.labels_not),
        );
        let slug = axis.slug();
        match args.format {
            OutputFormat::Text => {
                for (key, n) in &rows {
                    println!("{n:>7}  {key}");
                }
                let excl = if excluded > 0 {
                    format!(" · {excluded} record(s) have no {slug} (outside this axis)")
                } else {
                    String::new()
                };
                let drop = if outcome.dropped_by_cap > 0 {
                    format!(
                        " · {} exchange(s) dropped by --max-count (census incomplete; raise --max-count)",
                        outcome.dropped_by_cap
                    )
                } else {
                    String::new()
                };
                eprintln!(
                    "csift: {records} matched record(s) across {} {slug} key(s){excl}{drop}",
                    rows.len()
                );
            }
            OutputFormat::Json => {
                let header = crate::text::envelope_scope_header(
                    "search",
                    outcome.scope_top,
                    outcome.scope_sub,
                    serde_json::json!({}),
                );
                println!("{}", serde_json::to_string(&header)?);
                for (key, n) in &rows {
                    let obj = serde_json::json!({
                        "kind": "census",
                        "axis": slug,
                        "key": key,
                        "records": n,
                    });
                    println!("{}", serde_json::to_string(&obj)?);
                }
                let summary = crate::text::envelope_summary(serde_json::json!({
                    "axis": slug,
                    "matched_records": records,
                    "distinct_keys": rows.len(),
                    "excluded_records": excluded,
                    "dropped_by_cap": outcome.dropped_by_cap,
                    "skipped_lines": outcome.skipped_lines,
                }));
                println!("{}", serde_json::to_string(&summary)?);
            }
        }
        return Ok(());
    }

    // `--raw`: the found records' VERBATIM jsonl lines — stdout is a pure jsonl stream (for
    // `jq`); scope/accounting notes go to stderr. One line per matched RECORD (a record hit
    // under several labels emits once); a sidecar-merged record has no physical line and is
    // omitted WITH a stderr note (disclosed, never silent).
    if args.raw {
        if args.format == OutputFormat::Json {
            bail!("--raw IS the machine output (verbatim jsonl lines) — drop --format json");
        }
        let mut skipped_sidecar = 0usize;
        let mut seen: std::collections::BTreeSet<(&str, usize)> = std::collections::BTreeSet::new();
        for ex in &outcome.exchanges {
            for h in &ex.hits {
                if h.from_sidecar {
                    skipped_sidecar += 1;
                    continue;
                }
                if let Some(raw) = &h.raw {
                    if seen.insert((ex.session_id.as_str(), h.line)) {
                        println!("{raw}");
                    }
                }
            }
        }
        if skipped_sidecar > 0 {
            eprintln!(
                "csift: note: {skipped_sidecar} sidecar-merged record(s) have no physical \
                 transcript line — omitted under --raw"
            );
        }
        if outcome.dropped_by_cap > 0 {
            eprintln!(
                "csift: note: {} {} exchange(s) dropped by --max-count",
                outcome.dropped_by_cap,
                dropped_side(args)
            );
        }
        if outcome.skipped_lines > 0 {
            eprintln!(
                "csift: note: {}",
                crate::text::malformed_note(outcome.skipped_lines)
            );
        }
        return Ok(());
    }

    // ── Empty-result self-diagnosis (anti-slippage keystone, §T0.1). A zero-match result is a
    //    DEFINITIVE absence, not a syntax error — but a bare "no matching exchanges" reads as
    //    failure and drives a model back to hand-parsing. On zero hits we emit (to stderr;
    //    stdout stays pure) what was searched + that this is exit-0-honest, and — the killer —
    //    when a `-t`/`-T` filter is active, a re-scan WITHOUT it that names the label(s) the
    //    pattern DOES occur under. The re-scan is paid ONLY on a zero-hit + label-filtered
    //    query, so the happy path is untouched. ──
    let diagnosis = if outcome.exchanges.is_empty() {
        let label_filtered = !args.labels.is_empty() || !args.labels_not.is_empty();
        let excluded_by_label = if label_filtered {
            let mut probe = args.clone();
            probe.labels.clear();
            probe.labels_not.clear();
            let probe_files: Vec<FileResult> = session_files
                .par_iter()
                .map(|p| {
                    search_one_file(
                        p,
                        &probe,
                        &matcher,
                        turn_range,
                        &time_window,
                        None,
                        false,
                        &spawn_map,
                        inner_parallel,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let mut probe_ex: Vec<Exchange> = Vec::new();
            for fr in probe_files {
                probe_ex.extend(fr.exchanges);
            }
            // The probe deliberately reports the FULL label sets — it exists to name
            // exactly what the dropped `-t`/`-T` filter excluded.
            let (counts, recs) = label_census(&probe_ex, LabelFilter::all());
            (recs > 0).then(|| {
                let mut rows: Vec<(String, usize)> = counts
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect();
                rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                (rows, recs)
            })
        } else {
            None
        };
        let diag = EmptyDiagnosis {
            sessions_in_scope: outcome.scope_top + outcome.scope_sub,
            active_filters: active_filters_str(args),
            skipped_lines: outcome.skipped_lines,
            label_filtered,
            excluded_by_label,
        };
        emit_empty_diagnosis(&args.pattern, &diag);
        Some(diag)
    } else {
        None
    };

    match args.format {
        OutputFormat::Text => render_text(&outcome, args),
        OutputFormat::Json => render_json(&outcome, diagnosis.as_ref())?,
    }
    Ok(())
}

/// True when ANY emitted hit/sibling came from the elicitation sidecar (§3.10) — drives the
/// `with elicitation sidecar` note so a consumer knows the output includes hook-backfilled
/// records, not raw native jsonl.
pub(crate) fn merged_any_sidecar(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| h.from_sidecar)
    })
}

/// True when ANY emitted hit/sibling excerpt was CLIPPED to the default cap — drives the
/// trailing reader-caution note (text) / the `excerpts_truncated` JSON flag. Always false under
/// `--no-truncate` and in `--line`/`--uuid` fetch mode (the cap is lifted to
/// `usize::MAX`, so no hit can be truncated), so a single check both detects truncation AND
/// auto-suppresses the note exactly when the reader already asked for whole records.
pub(crate) fn any_truncated_excerpt(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| h.truncated)
    })
}

/// Count of DISTINCT sessions among these exchanges (by transcript `session_id`, in
/// first-seen order). One cheap always-on number — surfaced in every search footer.
pub(crate) fn distinct_session_count(exchanges: &[Exchange]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for ex in exchanges {
        if !seen.contains(&ex.session_id.as_str()) {
            seen.push(&ex.session_id);
        }
    }
    seen.len()
}

/// Sort key that places timestamp-less exchanges LAST (after all timestamped ones) and
/// orders timestamped ones chronologically (ISO-8601 UTC sorts as text). Same shape as
/// `files::timestamp_sort_key`, so `search` and `files --timeline` order identically.
pub(crate) fn timestamp_sort_key(ts: Option<&str>) -> (bool, &str) {
    match ts {
        Some(t) => (false, t),
        None => (true, ""),
    }
}

/// Per-file scan result before the global cap is applied.
pub(crate) struct FileResult {
    pub(crate) exchanges: Vec<Exchange>,
    pub(crate) skipped_lines: usize,
    /// This transcript's genuine-turn count — the domain a `--turn` spec resolves
    /// against. Consumed by `show`'s turn address-miss reporting (`no such turn: t99 —
    /// the transcript has N turn(s)`); 0 on the early-return paths (empty / gated file).
    pub(crate) turn_count: usize,
}

/// A retained record. `can_hit` is the §7d keyword-prefilter verdict on the raw
/// line: when `false`, the line provably lacks the required literal, so it can
/// never be a regex hit and we skip the (more expensive) per-block regex matching
/// on it — but it is STILL retained so it can appear as a sibling record in a
/// matched turn's complete round-trip (SPEC §6.4). When the matcher has no
/// anchorable literal (case-insensitive or regex-with-metachars) every record is
/// `can_hit`.
pub(crate) struct Kept {
    pub(crate) rec: Record,
    pub(crate) can_hit: bool,
    /// 1-based PHYSICAL line number of this record in its source jsonl (from the scanner) —
    /// a stable address (jsonl is append-only), surfaced per hit so `csift show --line N` (and
    /// raw `sed -n 'Np'`) can re-fetch the exact record. `0` for a merged elicitation-sidecar
    /// record (it has no physical transcript line — see `from_sidecar`).
    pub(crate) line_no: usize,
    /// True when this record was merged from the elicitation SIDECAR (§3.10), not scanned from
    /// the native jsonl. Such a record has no physical `line_no` (0); its hits render
    /// `(elicitation sidecar)` instead of `Lnnnn`.
    pub(crate) from_sidecar: bool,
}
