//! Terminal census modes (-c / -l / --count-by) + the zero-match self-diagnosis.

use super::*;

impl AddressSet {
    pub(crate) fn addresses(&self, kept: &Kept) -> bool {
        (!self.lines.is_empty() && self.lines.contains(&kept.line_no))
            || (!self.uuids.is_empty()
                && kept
                    .rec
                    .uuid
                    .as_deref()
                    .is_some_and(|u| self.uuids.contains(u)))
    }
}

/// Iterate an exchange's hits as RECORD groups. One record can emit SEVERAL hits — one per
/// matching section (GOLD §3 G4/G5: a batched notification surfaces each section, a
/// text+tool_use assistant record surfaces both views) — as a run of consecutive hits
/// sharing a physical jsonl line ([`collect_record_hits`] emits per record, in order). A
/// sidecar-merged hit has no physical line (`line == 0`) and a sidecar record emits exactly
/// ONE hit, so it forms its own group. Censuses count RECORDS, never sections — without
/// this grouping a leaf tally drifted above what `-t <leaf>` surfaces (the documented
/// invariant), by exactly the multi-section overlap.
pub(crate) fn record_groups(hits: &[Hit]) -> Vec<&[Hit]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < hits.len() {
        let mut j = i + 1;
        if hits[i].line > 0 {
            while j < hits.len() && hits[j].line == hits[i].line {
                j += 1;
            }
        }
        out.push(&hits[i..j]);
        i = j;
    }
    out
}

/// Per-leaf record census of a matched exchange set (GOLD §5): each matched RECORD (a
/// multi-section record is grouped by [`record_groups`], never multi-counted) contributes
/// to every leaf in its label set that SURVIVES the active `-t`/`-T` filter, so a leaf's
/// tally is exactly how many records `-t <leaf>` (composed with your other filters) would
/// surface. With no filter that is the record's FULL label set — but under `-t`/`-T` a
/// dual-labeled record must not leak its filtered-out twin into the census (R7 §2.3: a
/// `-t user -T user.message` census showing `agent.tool.result`, or a `-t harness` census
/// dominated by `agent.communication.inbox` keys, reads as the filter not working). The
/// filter decides membership per-VIEW already; the census keys follow the same predicate.
/// Returns the per-leaf counts and the distinct matched-record total. Shared by
/// `--count-by label` and the zero-hit label probe (the probe passes
/// [`LabelFilter::all`] — it deliberately reports what the DROPPED filter excluded).
pub(crate) fn label_census(
    exchanges: &[Exchange],
    filter: LabelFilter<'_>,
) -> (BTreeMap<&'static str, usize>, usize) {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut records = 0usize;
    for ex in exchanges {
        for group in record_groups(&ex.hits) {
            records += 1;
            // The label set is per-RECORD (classify output), identical across the
            // record's section hits — read it off the first.
            for &leaf in &group[0].labels {
                if filter.selected(leaf) {
                    *counts.entry(leaf).or_insert(0) += 1;
                }
            }
        }
    }
    (counts, records)
}

/// Per-AXIS record census of a matched exchange set — the `--count-by <axis>` engine.
/// `label` multi-counts (every leaf a record carries; exactly [`label_census`]'s numbers);
/// every other axis counts each matched record ONCE under its single key, and records
/// OUTSIDE the axis's domain (no tool name / no pairing / no model) are excluded AND
/// tallied so the caller can report them (no silent drop). A multi-section record is ONE
/// record ([`record_groups`]); its axis value is the first `Some` among its section hits
/// (the tool.use view carries the tool/pairing its sibling message-view hit does not).
/// Returns the rows already in output order — `turn` ascending on (transcript, turn index)
/// so it reads as a histogram, every other axis richest-count first — plus the
/// matched-record total and the excluded count.
pub(crate) fn axis_census(
    exchanges: &[Exchange],
    axis: crate::cli::CountAxis,
    filter: LabelFilter<'_>,
) -> (Vec<(String, usize)>, usize, usize) {
    use crate::cli::CountAxis as A;
    let multi_transcript = distinct_session_count(exchanges) > 1;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut turn_counts: BTreeMap<(String, usize), usize> = BTreeMap::new();
    let mut records = 0usize;
    let mut excluded = 0usize;
    for ex in exchanges {
        for group in record_groups(&ex.hits) {
            records += 1;
            match axis {
                A::Label => {
                    // Keys pass the SAME `-t`/`-T` predicate that admitted the record's
                    // views (see [`label_census`] — R7 §2.3).
                    for &leaf in &group[0].labels {
                        if filter.selected(leaf) {
                            *counts.entry(leaf.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                A::Tool => match group.iter().find_map(|h| h.tool_name.clone()) {
                    Some(t) => *counts.entry(t).or_insert(0) += 1,
                    None => excluded += 1,
                },
                A::Turn => {
                    *turn_counts
                        .entry((ex.session_id.clone(), ex.turn_index))
                        .or_insert(0) += 1;
                }
                A::Session => *counts.entry(ex.session_id.clone()).or_insert(0) += 1,
                A::Pairing => match group.iter().find_map(|h| h.pair) {
                    Some(Pairing::Paired) => *counts.entry("paired".to_string()).or_insert(0) += 1,
                    Some(Pairing::PendingNoResult) => {
                        *counts.entry("pending".to_string()).or_insert(0) += 1;
                    }
                    Some(Pairing::OrphanResult) => {
                        *counts.entry("orphan".to_string()).or_insert(0) += 1;
                    }
                    None => excluded += 1,
                },
                A::Model => match group.iter().find_map(|h| h.model.clone()) {
                    Some(m) => *counts.entry(m).or_insert(0) += 1,
                    None => excluded += 1,
                },
            }
        }
    }
    let rows: Vec<(String, usize)> = if matches!(axis, A::Turn) {
        // The turn axis reads as a HISTOGRAM: ascending (transcript, turn) order; the key
        // carries the transcript id only when >1 transcript is in scope (kept FULL — a
        // truncated id would not round-trip as an `@` target).
        turn_counts
            .into_iter()
            .map(|((sid, t), n)| {
                let key = if multi_transcript {
                    format!("{sid}\u{b7}t{t}")
                } else {
                    format!("t{t}")
                };
                (key, n)
            })
            .collect()
    } else {
        let mut v: Vec<(String, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    };
    (rows, records, excluded)
}

/// A zero-match search's self-diagnosis (the anti-slippage keystone). A bare "no matching
/// exchanges" reads to a model as a syntax failure and drives it back to hand-parsing jsonl;
/// this makes the empty result SAY it is a definitive, honest, exit-0 absence, echoes the
/// filters that constrained it, and — when a `-t`/`-T` filter was active — names the label(s)
/// the pattern DOES occur under (the exact L74681 trap: a tool-name searched under
/// `-t user.message`). Emitted to stderr so stdout stays a pure stream.
#[derive(Debug)]
pub(crate) struct EmptyDiagnosis {
    pub(crate) sessions_in_scope: usize,
    pub(crate) active_filters: String,
    /// Malformed lines skipped during the scan — an absence claim over a corpus with skipped
    /// lines must DISCLOSE them (the claim is definitive only for the parseable lines).
    pub(crate) skipped_lines: usize,
    pub(crate) label_filtered: bool,
    /// `Some((per-leaf rows richest-first, total records))` when a `-t`/`-T` filter was active
    /// AND the pattern matches under OTHER labels; `None` when no label filter, or the pattern
    /// is genuinely absent even unfiltered.
    pub(crate) excluded_by_label: Option<(Vec<(String, usize)>, usize)>,
}

/// Render the active `-t`/`-T`/time/turn filters as a compact echo (`none` when unfiltered) —
/// so a zero-result diagnosis shows exactly what constrained the query.
pub(crate) fn active_filters_str(args: &SearchArgs) -> String {
    let mut parts: Vec<String> = Vec::new();
    for l in &args.labels {
        parts.push(format!("-t {l}"));
    }
    for l in &args.labels_not {
        parts.push(format!("-T {l}"));
    }
    if let Some(s) = &args.since {
        parts.push(format!("--since {s}"));
    }
    if let Some(u) = &args.until {
        parts.push(format!("--until {u}"));
    }
    if let Some(t) = &args.turn_range {
        parts.push(format!("--turn {t}"));
    }
    if args.additional_context {
        parts.push("--additional-context".to_string());
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(" ")
    }
}

/// Emit the zero-match diagnosis to stderr (stdout stays pure for the caller's pipe).
pub(crate) fn emit_empty_diagnosis(pattern: &str, diag: &EmptyDiagnosis) {
    eprintln!(
        "csift: 0 matches — a DEFINITIVE absence (exit 0), NOT an error. \
         Scope: {} session(s). Active filters: {}.",
        diag.sessions_in_scope, diag.active_filters
    );
    if diag.skipped_lines > 0 {
        // Integrity caveat: skipped lines were never matched, so the absence claim spans
        // only the parseable corpus — an honest zero must say so.
        eprintln!(
            "csift: caveat: {} — the absence is definitive for parseable lines only.",
            crate::text::malformed_note(diag.skipped_lines)
        );
    }
    let quoted = if pattern.is_empty() {
        "the filter".to_string()
    } else {
        format!("\"{pattern}\"")
    };
    match &diag.excluded_by_label {
        Some((rows, recs)) => {
            let shown: Vec<String> = rows
                .iter()
                .take(6)
                .map(|(l, n)| format!("{l} ×{n}"))
                .collect();
            let more = rows.len().saturating_sub(6);
            let tail = if more > 0 {
                format!(" (+{more} more label(s))")
            } else {
                String::new()
            };
            eprintln!(
                "csift: ⚠ but {quoted} DOES occur — {recs} record(s) under: {}{tail}. \
                 Your -t/-T excluded them; drop -t/-T or select one of those labels.",
                shown.join(" · ")
            );
        }
        None if diag.label_filtered => {
            eprintln!(
                "csift: (even without the -t/-T filter, {quoted} has 0 matches here — \
                 genuinely absent in this scope, not a label mistake.)"
            );
        }
        None => {
            eprintln!(
                "csift: to see what a scope holds before guessing a filter, run \
                 `csift search \"\" <target> --count-by label`."
            );
        }
    }
}
