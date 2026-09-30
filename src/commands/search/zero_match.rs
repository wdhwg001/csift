//! The zero-match self-diagnosis: what a DEFINITIVE absence says on stderr, and the probe
//! that names the labels a label-filtered miss DOES occur under.

use super::*;

/// Empty-result self-diagnosis (anti-slippage keystone, section T0.1). A zero-match result is a
/// DEFINITIVE absence, not a syntax error - but a bare "no matching exchanges" reads as
/// failure and drives a model back to hand-parsing. On zero hits we emit (to stderr;
/// stdout stays pure) what was searched + that this is exit-0-honest, and up to two probes,
/// each a re-scan of the same query with one filter dropped:
/// - the LABEL probe, run on a zero hit under a `-t`/`-T` filter: the filter dropped, it
///   names the label(s) the pattern DOES occur under;
/// - the TURN probe, run second on a zero hit under `--turn` (its line prints first, because
///   its remedy keeps the reader's label filter): the window dropped, it counts the records
///   the pattern and the label filter match outside turn numbering.
///
/// A query with neither filter pays for no re-scan, so the happy path is untouched.
#[allow(clippy::too_many_arguments)]
pub(crate) fn diagnose_zero_match(
    args: &SearchArgs,
    outcome: &SearchOutcome,
    session_files: &[PathBuf],
    matcher: &Matcher,
    turn_range: Option<crate::text::RangeSpec>,
    time_window: &TimeWindow,
    spawn_map: &HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>>,
    inner_parallel: bool,
    for_addr: Option<&ForAddr>,
    cross: CrossingBudget,
) -> Result<Option<EmptyDiagnosis>> {
    if !outcome.exchanges.is_empty() {
        return Ok(None);
    }
    let label_filtered = !args.labels.is_empty() || !args.labels_not.is_empty();
    // Both probes re-run the SAME query over the same scope with one filter changed, keeping
    // every other one - the reverse closure included, or a probe would name records outside
    // the set the reader asked about. They count LABELS, never edges, so no edge is built.
    let probe = |q: &SearchArgs, window: Option<crate::text::RangeSpec>| -> Result<Vec<Exchange>> {
        let files: Vec<FileResult> = session_files
            .par_iter()
            .map(|p| {
                search_one_file(
                    p,
                    q,
                    matcher,
                    window,
                    time_window,
                    None,
                    false,
                    spawn_map,
                    inner_parallel,
                    for_addr,
                    false,
                    cross,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(files.into_iter().flat_map(|fr| fr.exchanges).collect())
    };
    let excluded_by_label = if label_filtered {
        let mut unfiltered = args.clone();
        unfiltered.labels.clear();
        unfiltered.labels_not.clear();
        // The probe deliberately reports the FULL label sets - it exists to name
        // exactly what the dropped `-t`/`-T` filter excluded.
        census_rows(label_census(
            &probe(&unfiltered, turn_range)?,
            LabelFilter::all(),
        ))
    } else {
        None
    };
    // A `--turn` window leaves out every record outside turn numbering - a recalled draft, a
    // rewound opener, a record on an abandoned branch - so a window can empty a result the
    // label filter did not. The same pattern and label filter with the window dropped, kept
    // to those records, says how many it left out. Those records are exactly the ABANDONED
    // ones, which the first scan already counted over every transcript it read (a file the
    // whole-file gate skipped holds no match for the probe either, since the probe keeps the
    // pattern), so a scope with none needs no probe to answer zero.
    let outside_turns = if turn_range.is_some() && outcome.chain.abandoned_records == 0 {
        Some(None)
    } else if turn_range.is_some() {
        let unnumbered: Vec<Exchange> = probe(args, None)?
            .into_iter()
            .filter(|e| e.turn_index.is_none())
            .collect();
        Some(census_rows(label_census(&unnumbered, args.label_filter())))
    } else {
        None
    };
    let diag = EmptyDiagnosis {
        sessions_in_scope: outcome.scope_top + outcome.scope_sub,
        active_filters: active_filters_str(args),
        skipped_lines: outcome.skipped_lines,
        label_filtered,
        excluded_by_label,
        outside_turns,
        gated_unreached: args.gated_unreached(),
        attachment_unread: args
            .attachment_leaves_unread()
            .into_iter()
            .map(|c| c.path().to_string())
            .collect(),
        empty_visible_face: crate::cli::empty_visible_faces(&args.labels),
    };
    emit_empty_diagnosis(&args.pattern, &diag);
    Ok(Some(diag))
}

/// A label census as the diagnosis reports it: per-leaf rows, richest first, and the record
/// total; `None` when it counted no record.
fn census_rows((counts, recs): (BTreeMap<&'static str, usize>, usize)) -> Option<LabelRows> {
    (recs > 0).then(|| {
        let mut rows: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        (rows, recs)
    })
}
