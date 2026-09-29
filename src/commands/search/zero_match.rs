//! The zero-match self-diagnosis: what a DEFINITIVE absence says on stderr, and the probe
//! that names the labels a label-filtered miss DOES occur under.

use super::*;

/// Empty-result self-diagnosis (anti-slippage keystone, section T0.1). A zero-match result is a
/// DEFINITIVE absence, not a syntax error - but a bare "no matching exchanges" reads as
/// failure and drives a model back to hand-parsing. On zero hits we emit (to stderr;
/// stdout stays pure) what was searched + that this is exit-0-honest, and - the killer -
/// when a `-t`/`-T` filter is active, a re-scan WITHOUT it that names the label(s) the
/// pattern DOES occur under. The re-scan is paid ONLY on a zero-hit + label-filtered
/// query, so the happy path is untouched.
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
                    matcher,
                    turn_range,
                    time_window,
                    None,
                    false,
                    spawn_map,
                    inner_parallel,
                    // The zero-match probe re-runs the SAME query with the label filter
                    // dropped, so it keeps every other filter - the reverse closure
                    // included, or the probe would name labels outside the set the
                    // reader asked about.
                    for_addr,
                    // The probe counts LABELS, never edges: it feeds `label_census`
                    // alone, so nothing it builds would be read.
                    false,
                    cross,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut probe_ex: Vec<Exchange> = Vec::new();
        for fr in probe_files {
            probe_ex.extend(fr.exchanges);
        }
        // The probe deliberately reports the FULL label sets - it exists to name
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
        gated_unreached: args.gated_unreached(),
        empty_visible_face: crate::cli::empty_visible_faces(&args.labels),
    };
    emit_empty_diagnosis(&args.pattern, &diag);
    Ok(Some(diag))
}
