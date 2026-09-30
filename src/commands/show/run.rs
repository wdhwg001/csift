//! run_show: raw + rendered fetch drives.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use super::*;

/// Entry point for `csift show`.
pub fn run_show(args: &ShowArgs, cross: CrossingBudget) -> Result<()> {
    // Reject the (hidden, no-op) span pair with the pointed rule instead of letting
    // `allow_hyphen_values` feed it to the TARGET parser as a mistyped-flag guess.
    if let Some(msg) = args.span_flag_error() {
        bail!(msg);
    }
    if args.raw && args.format == OutputFormat::Json {
        bail!("--raw is mutually exclusive with --format json (raw IS the file's own JSON)");
    }
    let parsed = parse_line_specs(&args.line)?;
    // In the order given, each once: the continuation of a capped fetch names the dropped
    // uuids in the reader's order.
    let mut uuids: Vec<String> = Vec::new();
    for u in args.uuid.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
        if !uuids.iter().any(|seen| seen == u) {
            uuids.push(u.to_string());
        }
    }
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
        return run_branch_points(&file, args.format, cross);
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
    // A range that names no line of the file is the same miss in both fetch modes.
    let empty = specs.empty_range_misses();
    if !empty.is_empty() {
        bail!("no such record(s): {}", empty.join(", "));
    }

    if args.raw {
        return run_raw(&file, &specs, &uuids, turn_spec, args.max_count, cross);
    }
    run_rendered(
        &file,
        &specs,
        &uuids,
        turn_spec,
        args.format,
        args.max_count,
        cross,
    )
}

/// `--raw`: emit the verbatim bytes of each addressed jsonl line, ascending, exactly
/// as stored (a blank or torn line included - that is the point).
pub(crate) fn run_raw(
    file: &std::path::Path,
    specs: &LineSpecs,
    uuids: &[String],
    turn_range: Option<crate::text::RangeSpec>,
    max_count: Option<usize>,
    cross: CrossingBudget,
) -> Result<()> {
    let Some(mmap) = mmap_bytes(file)? else {
        bail!("transcript {} is empty", file.display());
    };
    let bytes: &[u8] = &mmap;

    // uuid -> line resolution: parse only the lines whose raw bytes contain a wanted
    // uuid (cheap memmem prefilter), then confirm against the parsed record's own uuid.
    let mut uuid_line: std::collections::BTreeMap<&String, Option<usize>> =
        uuids.iter().map(|u| (u, None)).collect();
    let mut wanted_lines = specs.all();
    // `--turn --raw`: resolve the turn range to its records' physical lines (via the shared
    // grouping, so the turn numbering matches `search`), then emit those lines verbatim. The
    // turn of each line rides along, because a capped window continues as a window.
    let mut crossing = crate::model::CrossingReport::new(cross);
    let mut line_turn: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    let mut window_last = None;
    if let Some(spec) = turn_range {
        let (exchanges, _, turn_count, report) =
            fetch_records(file, BTreeSet::new(), BTreeSet::new(), Some(spec), cross)?;
        crossing = report;
        // An EXPLICIT turn (`N` / `A..B`) is an address - zero records = miss (law 1).
        if turn_spec_is_explicit(&spec) && exchanges.is_empty() {
            return Err(turn_miss_error(&spec, turn_count));
        }
        window_last = Some(spec.resolve(turn_count, false).1);
        for ex in &exchanges {
            for h in &ex.hits {
                if h.line > 0 {
                    wanted_lines.insert(h.line);
                    if let Some(t) = ex.turn_index {
                        line_turn.insert(h.line, t);
                    }
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

    // Address-miss = error: explicit lines beyond EOF and unresolved uuids. An empty range was
    // refused before this mode ran ([`LineSpecs::empty_range_misses`]); a range with a line in
    // the file emits that line's bytes whatever they are.
    let mut misses: Vec<String> = specs
        .explicit
        .iter()
        .filter(|&&n| n > total_lines)
        .map(|&n| miss_label(format!("L{n}"), n, total_lines))
        .collect();
    for (u, slot) in &uuid_line {
        if slot.is_none() {
            misses.push(format!("uuid {u}"));
        }
    }
    if !misses.is_empty() {
        bail!("no such record(s): {}", misses.join(", "));
    }

    // Context-flood guard: keep the FIRST `cap` lines, report the drop on stderr (stdout
    // stays a pure jsonl stream) with the continuation command, in the address domain the
    // reader used - the rule the rendered fetch follows, one line a unit.
    let cap = effective_cap(max_count);
    let domain = match window_last {
        Some(last) => Addressed::Turns { last },
        None => Addressed::Records { specs, uuids },
    };
    let capped = {
        let uuid_of: std::collections::BTreeMap<usize, &str> = uuid_line
            .iter()
            .filter_map(|(u, slot)| slot.map(|l| (l, u.as_str())))
            .collect();
        let units: Vec<Unit> = keep
            .keys()
            .map(|&line| Unit {
                line,
                uuid: uuid_of.get(&line).copied(),
                turn: line_turn.get(&line).copied(),
            })
            .collect();
        cap_units(&units, cap, &domain)
    };
    let dropped = capped.dropped;
    let mut flags = capped.keep.iter().copied();
    keep.retain(|_, _| flags.next().unwrap_or(true));

    let mut out = crate::text::Out::stdout();
    for line in keep.values() {
        out.write_all(line)?;
        out.write_all(b"\n")?;
    }
    // Flushed before the notes below: they ride stderr and the contract puts them after
    // the byte stream.
    out.flush()?;
    drop(out);
    // Both notes ride stderr after that flush, and the writer is gone by here, so neither
    // can jump a buffer's queue.
    #[allow(clippy::print_stderr)]
    {
        if specs.past_end > 0 {
            eprintln!(
                "csift: note: {}",
                past_end_note(specs.past_end, specs.total_lines)
            );
        }
        if dropped > 0 {
            let sid = crate::subagent::session_id_from_path(file);
            let cont = match &capped.continuation {
                Some(c) => format!(
                    " · continue: csift show @{sid} {} --raw{}",
                    c.args,
                    overlap_note(c.overlap, c.overlap_turn, "line")
                ),
                None => String::new(),
            };
            eprintln!(
                "csift: note: +{dropped} more line(s) beyond the {cap}-unit cap{cont}, or pass \
                 --max-count 0 (uncapped)"
            );
        }
        // A `--turn` window is resolved through the chain, so which lines these ARE moves with
        // the crossing budget, and it says what the walk crossed. A `--line`/`--uuid` address
        // names its lines outright: this fetch builds NO chain for it (a line walk, a uuid
        // prefilter), because the bytes it prints are the same under every budget, and the
        // report stays the empty one, which says nothing. Building the chain only to disclose
        // it would cost the escape hatch the rendered fetch's price: the chain-building
        // `--line 1 --format json` runs one and a half to three times as long as
        // `--line 1 --raw` on live transcripts of 124 to 721 MB (SPEC 4.11 carries the
        // reading). Stderr either way: stdout here is the file's own bytes and nothing else.
        for line in crossing.lines() {
            eprintln!("{line}");
        }
    }
    Ok(())
}

/// Rendered mode: fetch through search's per-record pipeline (pure matcher - every
/// addressed record emits, FULL), then render text or the header/record/summary JSON.
pub(crate) fn run_rendered(
    file: &std::path::Path,
    specs: &LineSpecs,
    uuids: &[String],
    turn_range: Option<crate::text::RangeSpec>,
    format: OutputFormat,
    max_count: Option<usize>,
    cross: CrossingBudget,
) -> Result<()> {
    let (mut exchanges, skipped, turn_count, crossing) = fetch_records(
        file,
        specs.all(),
        uuids.iter().cloned().collect(),
        turn_range,
        cross,
    )?;

    // Turn address-miss: an EXPLICIT `--turn N` / `--turn A..B` is an ADDRESS (law 1) -
    // resolving to zero records is a hard error naming the transcript's turn domain,
    // exactly like a `--line` miss. Open/from-end forms clamp (tail-peek robustness).
    if let Some(spec) = &turn_range {
        if turn_spec_is_explicit(spec) && exchanges.is_empty() {
            return Err(turn_miss_error(spec, turn_count));
        }
    }

    // Address-miss accounting (explicit lines + uuids must resolve; ranges must yield >=1).
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
        .map(|&n| miss_label(format!("L{n}"), n, specs.total_lines))
        .collect();
    for r in &specs.ranges {
        if !hit_lines.iter().any(|n| (r.lo..=r.hi).contains(n)) {
            misses.push(range_miss_label(r, specs.total_lines));
        }
    }
    for u in uuids {
        if !hit_uuids.contains(u.as_str()) {
            misses.push(format!("uuid {u}"));
        }
    }
    if !misses.is_empty() {
        bail!(
            "{}",
            crate::commands::search::address_miss(&misses.join(", "))
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
    let uncovered = Uncovered {
        non_record: addressed.iter().filter(|n| !hit_lines.contains(n)).count(),
        past_end: specs.past_end,
        last_line: specs.total_lines,
    };

    // Context-flood guard (law 4's last hole): an open range (`--line ..` / `--turn ..`)
    // addresses the WHOLE transcript, so uncapped it would dump the lot. Keep the FIRST
    // `cap` record units and report the drop with the continuation command, in the address
    // domain the reader used; `--max-count 0` lifts the cap.
    let cap = effective_cap(max_count);
    let domain = match turn_range {
        Some(spec) => Addressed::Turns {
            last: spec.resolve(turn_count, false).1,
        },
        None => Addressed::Records { specs, uuids },
    };
    let capped = {
        let units: Vec<Unit> = exchanges
            .iter()
            .flat_map(|e| {
                e.hits.iter().map(|h| Unit {
                    line: h.line,
                    uuid: h.uuid.as_deref(),
                    turn: e.turn_index,
                })
            })
            .collect();
        cap_units(&units, cap, &domain)
    };
    let mut flags = capped.keep.iter().copied();
    for ex in &mut exchanges {
        ex.hits.retain(|_| flags.next().unwrap_or(true));
    }
    exchanges.retain(|e| !e.hits.is_empty());
    let dropped = capped.dropped;
    let remainder = capped.continuation.map(|c| Remainder {
        cmd: format!("csift show @{session_id} {}", c.args),
        overlap: c.overlap,
        overlap_turn: c.overlap_turn,
    });

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
                remainder.as_ref(),
                uncovered,
                &crossing,
            )?;
        }
        OutputFormat::Json => render_json(
            &exchanges,
            file,
            &session_id,
            is_subagent,
            &parent_session_id,
            skipped,
            dropped,
            remainder.as_ref(),
            uncovered,
            &crossing,
        )?,
    }
    Ok(())
}
