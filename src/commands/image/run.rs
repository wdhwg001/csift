//! run_image: listing + extraction drive.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use super::*;

pub fn run_image(args: &ImageArgs, cross: CrossingBudget) -> Result<()> {
    let extracting = args.out.is_some();
    let selection = parse_id_selection(&args.id)?;

    let resolved = crate::path::resolve_targets_with_session_list_reported(
        &args.paths,
        args.sessions_from.as_deref(),
        args.want_subagents().into(),
        crate::path::Caller::Other,
    )?;
    let session_files = resolved.files;
    // The span header's scope counts, read off the RESOLVED transcript set before any filter
    // narrows the images - the set every other span command's header counts.
    let scope = crate::subagent::scope_span_files(&session_files);

    // Scan across files (rayon - each is an independent mmap + prefilter scan).
    use rayon::prelude::*;
    let per_file: Vec<(Vec<ImageRef>, usize, crate::model::CrossingReport)> = session_files
        .par_iter()
        .map(|p| images_in_file(p, extracting, cross))
        .collect::<Result<Vec<_>>>()?;

    let mut images: Vec<ImageRef> = Vec::new();
    let mut skipped_lines = 0usize;
    let mut crossing = crate::model::CrossingReport::new(cross);
    for (fi, (refs, skipped, rep)) in per_file.into_iter().enumerate() {
        images.extend(refs);
        skipped_lines += skipped;
        crossing.add(&rep, fi == 0);
    }
    // Combined stable order: chronological by record ts, then line/index; ts-less last.
    images.sort_by(|a, b| {
        match (a.ts_utc.as_deref(), b.ts_utc.as_deref()) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then((a.line_no, a.img_index).cmp(&(b.line_no, b.img_index)))
    });

    // -- Scope filters: each NARROWS the image set, so an ambiguous `#N` can resolve in a
    //    window / turn / uuid where it is unique - the disambiguators the ambiguity error
    //    names, and pre-applyable up front (`--since 1h` so `#N` is unique in the last hour). --
    let time_window =
        crate::time_window::TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    images.retain(|i| time_window.contains(i.ts_utc.as_deref()));
    if let Some(prefix) = args.uuid.as_deref() {
        images.retain(|i| {
            i.record_uuid
                .as_deref()
                .is_some_and(|u| u.starts_with(prefix))
        });
    }
    let mut distinct_transcripts = count_distinct_transcripts(&images);

    // `--turn` is per-transcript (turn indices are), so it needs a single transcript.
    if let Some(spec_str) = args.turn_range.as_deref() {
        let spec = crate::text::parse_range_spec(spec_str, "--turn", false)?;
        if distinct_transcripts > 1 {
            bail!(
                "--turn is per-transcript (turn indices are), but the scope resolves to \
                 {distinct_transcripts} transcripts — pin it with `@<uuid> --no-subagents`"
            );
        }
        if let Some(path) = pinned_path(&session_files, &images) {
            let turn_of = transcript_line_info(path, cross)?.turn_of;
            // Resolve open/from-end forms against this transcript's turn count.
            let turn_count = turn_of.values().copied().max().map_or(0, |m| m + 1);
            let (lo, hi) = spec.resolve(turn_count, false);
            images.retain(|i| {
                turn_of
                    .get(&i.line_no)
                    .is_some_and(|t| *t >= lo && *t <= hi)
            });
        } else {
            images.clear();
        }
        distinct_transcripts = count_distinct_transcripts(&images);
    }

    // Apply the `--id` selection (if any). `#N` and `L<line>i<n>` are both per-transcript, so
    // a multi-transcript scope is ambiguous -> require a single transcript (like `show`).
    let selected: Vec<&ImageRef> = if selection.is_empty() {
        // List/extract ALL -> dedup the SAME image re-injected across context windows (by
        // content fingerprint). Two DISTINCT-content images that share a `#N` both survive, so
        // the listing shows the reuse - and an `--id #N` against it ERRORS (never silent-picks).
        dedup_latest(&images)
    } else {
        if distinct_transcripts > 1 {
            bail!(
                "--id is per-transcript (line numbers and `#N` are), but the scope resolves to \
                 {distinct_transcripts} transcripts — pin it with `@<uuid> --no-subagents`"
            );
        }
        resolve_selection(&selection, &images, &session_files, cross)?
    };

    if let Some(out_dir) = args.out.as_deref() {
        extract(
            &selected,
            out_dir,
            args,
            skipped_lines,
            scope,
            &resolved.duplicates,
        )
    } else {
        match args.format {
            OutputFormat::Json => render_json(
                &selected,
                distinct_transcripts,
                skipped_lines,
                scope,
                &resolved.duplicates,
                &crossing,
            ),
            OutputFormat::Text => render_text(
                &selected,
                distinct_transcripts,
                skipped_lines,
                &resolved.duplicates,
                &crossing,
            ),
        }
    }
}
