//! run_image: listing + extraction drive.

use super::*;

pub fn run_image(args: &ImageArgs) -> Result<()> {
    let extracting = args.out.is_some();
    let selection = parse_id_selection(&args.id)?;

    let session_files = crate::path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        args.want_subagents().into(),
        crate::path::Caller::Other,
    )?;

    // Scan across files (rayon — each is an independent mmap + prefilter scan).
    use rayon::prelude::*;
    let per_file: Vec<(Vec<ImageRef>, usize)> = session_files
        .par_iter()
        .map(|p| images_in_file(p, extracting))
        .collect::<Result<Vec<_>>>()?;

    let mut images: Vec<ImageRef> = Vec::new();
    let mut skipped_lines = 0usize;
    for (refs, skipped) in per_file {
        images.extend(refs);
        skipped_lines += skipped;
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

    // ── Scope filters: each NARROWS the image set, so an ambiguous `#N` can resolve in a
    //    window / turn / uuid where it is unique — the disambiguators the ambiguity error
    //    names, and pre-applyable up front (`--since 1h` so `#N` is unique in the last hour). ──
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
            let turn_of = transcript_line_info(path)?.turn_of;
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
    // a multi-transcript scope is ambiguous → require a single transcript (like `show`).
    let selected: Vec<&ImageRef> = if selection.is_empty() {
        // List/extract ALL → dedup the SAME image re-injected across context windows (by
        // content fingerprint). Two DISTINCT-content images that share a `#N` both survive, so
        // the listing shows the reuse — and an `--id #N` against it ERRORS (never silent-picks).
        dedup_latest(&images)
    } else {
        if distinct_transcripts > 1 {
            bail!(
                "--id is per-transcript (line numbers and `#N` are), but the scope resolves to \
                 {distinct_transcripts} transcripts — pin it with `@<uuid> --no-subagents`"
            );
        }
        resolve_selection(&selection, &images, &session_files)?
    };

    if let Some(out_dir) = args.out.as_deref() {
        extract(&selected, out_dir, args, skipped_lines)
    } else {
        match args.format {
            OutputFormat::Json => render_json(&selected, distinct_transcripts, skipped_lines),
            OutputFormat::Text => render_text(&selected, distinct_transcripts, skipped_lines),
        }
        Ok(())
    }
}
