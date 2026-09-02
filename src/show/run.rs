//! run_show: raw + rendered fetch drives.

use super::*;

/// Entry point for `csift show`.
pub fn run_show(args: &ShowArgs) -> Result<()> {
    // Reject the (hidden, no-op) span pair with the pointed rule instead of letting
    // `allow_hyphen_values` feed it to the TARGET parser as a mistyped-flag guess (R7 §2.3).
    if let Some(msg) = args.span_flag_error() {
        bail!(msg);
    }
    if args.raw && args.format == OutputFormat::Json {
        bail!("--raw is mutually exclusive with --format json (raw IS the file's own JSON)");
    }
    let parsed = parse_line_specs(&args.line)?;
    let uuids: BTreeSet<String> = args
        .uuid
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();
    // `--turn` addresses by turn index (0-based, the `·tN` search's headers print) rather than by
    // jsonl line - the SAME shared range grammar (`N`/`A..B`/`N..`/`-k`). Clap forbids mixing
    // it with `--line`/`--uuid`, so it is a self-contained addressing mode.
    let turn_spec = args
        .turn
        .as_deref()
        .map(|s| crate::text::parse_range_spec(s, "--turn", false))
        .transpose()?;

    // The clap-side Vec is an error-attribution device (see `ShowArgs::target`); the
    // command contract is still exactly ONE transcript per call.
    let target = match args.target.as_slice() {
        [one] => one,
        many => bail!(
            "show reads exactly ONE transcript per call; got {} targets: {} — record \
             addresses (`--line`/`--uuid`/`--turn`) are per-FILE, so fetch each \
             transcript with its own `csift show` call",
            many.len(),
            many.iter()
                .map(|p| format!("'{}'", p.display()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let file = resolve_single_transcript(target)?;
    if args.branch_points {
        return run_branch_points(&file, args.format);
    }
    if parsed.is_empty() && uuids.is_empty() && turn_spec.is_none() {
        bail!(
            "show needs an address: `--line <N|A..B|N..|-k>` (1-based jsonl lines, the `Lnnnn` \
             other csift commands print), `--turn <N|A..B|-k>` (0-based turn index, the `tN` \
             search prints), and/or `--uuid <U>`. The transcript is {} — csift never dumps a \
             whole transcript into your context.",
            file.display()
        );
    }
    // Resolve `--line` open/from-end forms (`N..`, `-20..` = the last 20) against this
    // transcript's line count, materializing the concrete explicit/range addresses.
    let specs = resolve_line_specs(&parsed, count_lines(&file)?);

    if args.raw {
        return run_raw(&file, &specs, &uuids, turn_spec, args.max_count);
    }
    run_rendered(
        &file,
        &specs,
        &uuids,
        turn_spec,
        args.format,
        args.max_count,
    )
}

/// `--raw`: emit the verbatim bytes of each addressed jsonl line, ascending, exactly
/// as stored (a blank or torn line included - that is the point).
pub(crate) fn run_raw(
    file: &std::path::Path,
    specs: &LineSpecs,
    uuids: &BTreeSet<String>,
    turn_range: Option<crate::text::RangeSpec>,
    max_count: Option<usize>,
) -> Result<()> {
    let Some(mmap) = mmap_bytes(file)? else {
        bail!("transcript {} is empty", file.display());
    };
    let bytes: &[u8] = &mmap;

    // uuid → line resolution: parse only the lines whose raw bytes contain a wanted
    // uuid (cheap memmem prefilter), then confirm against the parsed record's own uuid.
    let mut uuid_line: std::collections::BTreeMap<&String, Option<usize>> =
        uuids.iter().map(|u| (u, None)).collect();
    let mut wanted_lines = specs.all();
    // `--turn --raw`: resolve the turn range to its records' physical lines (via the shared
    // grouping, so the turn numbering matches `search`), then emit those lines verbatim.
    if let Some(spec) = turn_range {
        let (exchanges, _, turn_count) =
            fetch_records(file, BTreeSet::new(), BTreeSet::new(), Some(spec))?;
        // An EXPLICIT turn (`N` / `A..B`) is an address - zero records = miss (law 1).
        if turn_spec_is_explicit(&spec) && exchanges.is_empty() {
            return Err(turn_miss_error(&spec, turn_count));
        }
        for ex in &exchanges {
            for h in &ex.hits {
                if h.line > 0 {
                    wanted_lines.insert(h.line);
                }
            }
        }
    }
    let mut total_lines = 0usize;
    let mut keep: std::collections::BTreeMap<usize, Vec<u8>> = std::collections::BTreeMap::new();

    // One owned finder per addressed uuid, built ONCE before the line walk (the
    // stateless per-line `memmem::find` rebuilt its searcher on every call). The
    // zip below is order-safe: `uuid_line` is a BTreeMap, so `keys()` here and
    // `iter_mut()` below walk the SAME sorted order.
    let uuid_finders: Vec<memchr::memmem::Finder<'static>> = uuid_line
        .keys()
        .map(|u| memchr::memmem::Finder::new(u.as_bytes()).into_owned())
        .collect();
    let mut line_no = 0usize;
    scan_lines_bytes(bytes, |line| {
        line_no += 1;
        total_lines = line_no;
        let mut want = wanted_lines.contains(&line_no);
        if !want && !uuids.is_empty() {
            for ((u, slot), finder) in uuid_line.iter_mut().zip(&uuid_finders) {
                if slot.is_none() && finder.find(line).is_some() {
                    // Confirm structurally: the record's OWN uuid must equal it (a body
                    // merely quoting the uuid must not satisfy the address).
                    if let Ok(Some(rec)) = crate::parse::parse_line(line) {
                        if rec.uuid.as_deref() == Some(u.as_str()) {
                            *slot = Some(line_no);
                            want = true;
                        }
                    }
                }
            }
        }
        if want {
            keep.insert(line_no, line.to_vec());
        }
    })?;

    // Address-miss = error: explicit lines beyond EOF, unresolved uuids, empty ranges.
    let mut misses: Vec<String> = specs
        .explicit
        .iter()
        .filter(|&&n| n > total_lines)
        .map(|n| format!("L{n} (file has {total_lines} lines)"))
        .collect();
    for &(a, b) in &specs.ranges {
        if a > total_lines {
            misses.push(format!("L{a}-{b} (file has {total_lines} lines)"));
        }
    }
    for (u, slot) in &uuid_line {
        if slot.is_none() {
            misses.push(format!("uuid {u}"));
        }
    }
    if !misses.is_empty() {
        bail!("no such record(s): {}", misses.join(", "));
    }

    // Context-flood guard: keep the FIRST `cap` lines, report the drop on stderr (stdout
    // stays a pure jsonl stream) with the exact continuation command.
    let cap = effective_cap(max_count);
    let mut dropped = 0usize;
    let mut continuation: Option<(usize, usize)> = None;
    if keep.len() > cap {
        dropped = keep.len() - cap;
        let omitted: Vec<usize> = keep.keys().skip(cap).copied().collect();
        if let (Some(&a), Some(&b)) = (omitted.first(), omitted.last()) {
            continuation = Some((a, b));
        }
        keep = keep.into_iter().take(cap).collect();
    }

    let mut out = std::io::stdout().lock();
    for line in keep.values() {
        out.write_all(line)?;
        out.write_all(b"\n")?;
    }
    drop(out);
    if dropped > 0 {
        let sid = crate::subagent::session_id_from_path(file);
        let cont = match continuation {
            Some((a, b)) => format!(" · continue: csift show @{sid} --line {a}..{b} --raw"),
            None => String::new(),
        };
        eprintln!(
            "csift: note: +{dropped} more line(s) beyond the {cap}-unit cap{cont}, or pass \
             --max-count 0 (uncapped)"
        );
    }
    Ok(())
}

/// Rendered mode: fetch through search's per-record pipeline (pure matcher - every
/// addressed record emits, FULL), then render text or the header/record/summary JSON.
pub(crate) fn run_rendered(
    file: &std::path::Path,
    specs: &LineSpecs,
    uuids: &BTreeSet<String>,
    turn_range: Option<crate::text::RangeSpec>,
    format: OutputFormat,
    max_count: Option<usize>,
) -> Result<()> {
    let (mut exchanges, skipped, turn_count) =
        fetch_records(file, specs.all(), uuids.clone(), turn_range)?;

    // Turn address-miss: an EXPLICIT `--turn N` / `--turn A..B` is an ADDRESS (law 1) -
    // resolving to zero records is a hard error naming the transcript's turn domain,
    // exactly like a `--line` miss. Open/from-end forms clamp (tail-peek robustness).
    if let Some(spec) = &turn_range {
        if turn_spec_is_explicit(spec) && exchanges.is_empty() {
            return Err(turn_miss_error(spec, turn_count));
        }
    }

    // Address-miss accounting (explicit lines + uuids must resolve; ranges must yield ≥1).
    let mut hit_lines: BTreeSet<usize> = BTreeSet::new();
    let mut hit_uuids: BTreeSet<String> = BTreeSet::new();
    for ex in &exchanges {
        for h in &ex.hits {
            hit_lines.insert(h.line);
            if let Some(u) = h.uuid.as_deref() {
                hit_uuids.insert(u.to_string());
            }
        }
    }
    let mut misses: Vec<String> = specs
        .explicit
        .iter()
        .filter(|n| !hit_lines.contains(n))
        .map(|n| format!("L{n}"))
        .collect();
    for &(a, b) in &specs.ranges {
        if !hit_lines.iter().any(|n| (a..=b).contains(n)) {
            misses.push(format!("L{a}-{b}"));
        }
    }
    for u in uuids {
        if !hit_uuids.contains(u.as_str()) {
            misses.push(format!("uuid {u}"));
        }
    }
    if !misses.is_empty() {
        bail!(
            "no such record(s): {} — an explicit address renders message lines \
             (role:user/role:assistant, superseded drafts included), attachment lines, and \
             the promoted non-record lines (a queue-operation with text, turn_duration, \
             away_summary, stop_hook_summary, file-history-snapshot/-delta); session-state \
             cache lines (last-prompt, mode, ai-title, …), a content-less queue dequeue, \
             unpromoted system subtypes and torn lines are inspectable with `--raw`",
            misses.join(", ")
        );
    }

    let session_id = crate::subagent::session_id_from_path(file);
    let is_subagent = crate::subagent::is_subagent_path(file);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(file).unwrap_or_else(|| session_id.clone());

    // Line-addressed RANGES may cover non-record lines (metadata/attachment - never
    // renderable, silently excluded by the pipeline). Count them so "fetched 1 unit"
    // from a 12-line range is self-explaining; explicit single-line misses already
    // errored above.
    let addressed = specs.all();
    let non_record_lines = if addressed.is_empty() {
        0
    } else {
        addressed.iter().filter(|n| !hit_lines.contains(n)).count()
    };

    // Context-flood guard (law 4's last hole): an open range (`--line ..` / `--turn ..`)
    // used to dump the WHOLE transcript uncapped. Keep the FIRST `cap` record units and
    // report the drop with the exact continuation command; `--max-count 0` lifts the cap.
    let cap = effective_cap(max_count);
    let total_units: usize = exchanges.iter().map(|e| e.hits.len()).sum();
    let mut dropped = 0usize;
    let mut remainder_cmd: Option<String> = None;
    if total_units > cap {
        dropped = total_units - cap;
        // The remainder's exact line-domain, computed BEFORE truncation (sidecar hits
        // have no physical line and cannot ride a --line continuation).
        let omitted_lines: Vec<usize> = exchanges
            .iter()
            .flat_map(|e| e.hits.iter())
            .skip(cap)
            .filter(|h| !h.from_sidecar && h.line > 0)
            .map(|h| h.line)
            .collect();
        if let (Some(&a), Some(&b)) = (omitted_lines.first(), omitted_lines.last()) {
            remainder_cmd = Some(format!("csift show @{session_id} --line {a}..{b}"));
        }
        let mut budget = cap;
        exchanges.retain_mut(|ex| {
            if budget == 0 {
                return false;
            }
            if ex.hits.len() <= budget {
                budget -= ex.hits.len();
            } else {
                ex.hits.truncate(budget);
                budget = 0;
            }
            true
        });
    }

    match format {
        OutputFormat::Text => {
            render_text(
                &exchanges,
                &session_id,
                is_subagent,
                &parent_session_id,
                skipped,
                dropped,
                cap,
                remainder_cmd.as_deref(),
                non_record_lines,
            );
        }
        OutputFormat::Json => render_json(
            &exchanges,
            file,
            &session_id,
            is_subagent,
            &parent_session_id,
            skipped,
            dropped,
            remainder_cmd.as_deref(),
            non_record_lines,
        )?,
    }
    Ok(())
}
