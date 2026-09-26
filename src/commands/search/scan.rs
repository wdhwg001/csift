//! Per-file scan: fetch_records (show's engine), search_one_file, the per-file pipeline.
//!
//! A PAIRED ROOT: the pipeline lives here and the CANDIDATE GATES - which lines a query
//! admits, and the needle each one costs - live in [`gates`].

mod gates;

pub(crate) use gates::*;

use super::*;

/// `csift show`'s fetch engine: the ADDRESSED records of exactly ONE transcript, rendered
/// FULL through the same per-record pipeline `search` uses (classify, plan pointers, tool
/// pairing, elicitation-sidecar merge) with the pure matcher, so every addressed record
/// emits regardless of any pattern. Returns the addressed exchanges + the malformed count.
pub(crate) fn fetch_records(
    path: &Path,
    lines: BTreeSet<usize>,
    uuids: BTreeSet<String>,
    turn_range: Option<crate::text::RangeSpec>,
    cross: CrossingBudget,
) -> Result<(Vec<Exchange>, usize, usize, crate::model::CrossingReport)> {
    let args = SearchArgs::default();
    let matcher = Matcher::pure();
    // A line/uuid ADDRESS restricts to named records; a `--turn` range (address empty) selects
    // every record of the named turns - the SAME per-file grouping `search` numbers turns by, so
    // `show --turn N` is byte-identical to the turn `search` cites as `<tok>·tN`.
    let address = AddressSet { lines, uuids };
    let use_address = !(address.lines.is_empty() && address.uuids.is_empty());
    let time_window = TimeWindow::from_args(None, None)?;
    let mut spawn_map: HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>> = HashMap::new();
    spawn_map
        .entry(discovery_root_for(path))
        .or_insert_with_key(|root| build_spawn_lookup(root).map(Arc::new));
    let fr = search_one_file(
        path,
        &args,
        &matcher,
        turn_range,
        &time_window,
        use_address.then_some(&address),
        false,
        &spawn_map,
        // ONE transcript - nothing else fills the pool, so the inner fan-out is free.
        true,
        // `show` fetches the records an address NAMES; the reverse provenance query is a
        // scan over a scope and lives on `search` alone.
        None,
        // `show` projects the engine's edges in its JSON rows and reaches this with a
        // DEFAULT `SearchArgs`, so the answer is stated here rather than read off a format
        // that is not show's. An address fetches at most a capped handful of records.
        true,
        cross,
    )?;
    // The crossing report rides back with the records: `show` MOVES with the budget - an
    // addressed record's `survival` changes under it - so it discloses what it crossed on
    // the same terms every other survival-printing surface does.
    Ok((
        fr.exchanges,
        fr.skipped_lines,
        fr.turn_count,
        fr.chain.crossing,
    ))
}

/// Scan a single session file: prefilter -> parse -> delimit turns -> match -> stitch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn search_one_file(
    path: &Path,
    args: &SearchArgs,
    matcher: &Matcher,
    turn_range: Option<crate::text::RangeSpec>,
    time_window: &TimeWindow,
    address: Option<&AddressSet>,
    want_siblings: bool,
    spawn_map: &HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>>,
    inner_parallel: bool,
    for_addr: Option<&ForAddr>,
    engine_edges: bool,
    cross: CrossingBudget,
) -> Result<FileResult> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(FileResult {
            exchanges: Vec::new(),
            skipped_lines: 0,
            turn_count: 0,
            chain: ChainCounts::new(cross),
            names_for_addr: false,
        });
    };
    let bytes: &[u8] = &mmap;
    let names_for_addr = for_addr.is_some_and(|a| names_a_record(bytes, a));

    // A GIANT transcript's per-turn match phase fans out even under a broad scan: such a
    // file is the straggler the rest of the pool ends up waiting on (only a handful of
    // files this size exist, so the nested fan-out adds no measurable steal churn - unlike
    // enabling it for every mid-size file, which did).
    const HUGE_FILE_BYTES: usize = 64 * 1024 * 1024;
    let inner_parallel = inner_parallel || bytes.len() >= HUGE_FILE_BYTES;

    // The `compact_boundary` boundary keep is GATED on the active `-t` selector - computed up
    // front because BOTH the whole-file gate below and the candidate scan key on it.
    let needs_compact_boundary = args
        .label_filter()
        .selected(Class::CompactionBoundary.path());
    // The hook-context keep is SELECTOR-gated like every other gated leaf: an explicit
    // `-t` reaching `harness.hook.context`, or an ADDRESS, which must render an addressed
    // attachment record flag-free. It takes no flag of its own - the leaf's own name is
    // the way in.
    let needs_hook_context = args.scans_gated(Class::HookContext) || address.is_some();
    // The FULL attachment keep: every `type:"attachment"` line, admitted by `--attachments`
    // (or the `--count-by attachment` axis, which implies it) with a selector that can
    // reach some attachment-carried leaf, by an explicit selector naming one of those
    // leaves, or by an ADDRESS.
    let needs_attachments = args.scans_attachment_lines() || address.is_some();
    // Promoted non-record lines: admitted ONLY under a selector that reaches the
    // leaf (`scans_gated` - a bare no-`-t` scan never parses them) or an address (`show
    // --line`/`--uuid` renders an addressed line flag-free). `scans_gated` is the ONE
    // predicate the zero-match diagnosis shares, so the note can never claim a line
    // shape was unscanned when this gate parsed it.
    let reach = |c: Class| args.scans_gated(c) || address.is_some();
    // The csift channel: a delivery is a real MESSAGE addressed at this lane, not machinery,
    // so it is admitted by a DEFAULT scan - the one attachment keep that is not flag-gated.
    // The gate is the LABEL selector alone: no `-t`, or any selector reaching
    // `agent.communication.channel`, keeps it; `-t user` / `-t harness` short-circuit before
    // the memmem and pay nothing (and never surface a delivery under the hook leaf, which
    // keeps its own flag). An addressed fetch is already admitted by the two attachment gates
    // above.
    let needs_channel = args.label_filter().selected(Class::CommChannel.path());
    // `harness.schedule.fire`: the fired PROMPT is an ordinary role-bearing line the
    // scan already keeps, but the instant it fired lives on the `system`/`scheduled_task_fire`
    // sibling, which has no role marker. Same LABEL-only gate as the channel keep (a bare scan
    // is on, `-t user` / `-t agent.*` short-circuits) and a CONJUNCTION rather than
    // the bare literal - a payload can quote the word. The admitted lines are read for the
    // instant and then DEMOTED below, so a default scan gains no `harness.schedule.event` hit.
    let needs_schedule_fire = args.label_filter().selected(Class::ScheduleFire.path());
    let gates = CandidateGates {
        compact_boundary: needs_compact_boundary,
        hook_context: needs_hook_context,
        attachments: needs_attachments,
        channel: needs_channel,
        // ONE needle, TWO leaves: a `queue-operation` line is the human's queued text or
        // the harness's own queue row, and only its CONTENT says which - so either
        // selector has to admit the line.
        queued: reach(Class::UserQueued) || reach(Class::BookkeepingQueue),
        // The two leaves an attachment carries that an ORDINARY query names - the human's
        // mid-turn steer and the model's structured answer - each behind its own needle,
        // so neither turns the coarse attachment keep on.
        attachment_views: attachment_view_mask(args, address.is_some()),
        snapshot: reach(Class::BookkeepingFileHistory),
        // The 15 remaining non-transcript line types, one needle each behind
        // its own leaf's gate (the bitmask below). The whole group short-circuits on a
        // zero mask, so a default scan pays for none of them.
        bookkeeping: bookkeeping_mask(args, address.is_some()),
        // `scans_gated` ALSO admits this leaf under a BARE `harness` role
        // selector. It holds the one gated shape the model actually receives - a
        // `system`/`local_command` record, which the request assembler re-mints as a
        // user message - so a bare `-t harness` would otherwise be blind to the only
        // on-disk evidence of a slash command whose dialog wrote nothing else. The keep
        // is the same key-only `"subtype"` memmem, still `&&`-gated; the per-record
        // override (`Record::delivery_override`) then drops every non-delivered system
        // record this admission parsed. Every other gated leaf is unchanged.
        system: system_line_leaves().into_iter().any(reach),
        schedule_fire: needs_schedule_fire,
    };

    // -- section 7f whole-file gate --
    // When the pattern anchors a raw-byte prefilter (a plain literal, either case mode) and
    // this is NOT an addressing fetch (`--line`/`--uuid` emit records regardless of the
    // pattern), a cheap PARALLEL pre-scan can prove that no candidate line matches: no
    // per-line literal occurrence AND no synthesized-text marker (see [`Matcher::synth`]).
    // Every emitted exchange requires >=1 regex hit (`hits.is_empty() -> continue`), so such
    // a file provably yields nothing - skip building records for it entirely. Mechanics:
    // - the pre-scan runs on the SAME newline-aligned rayon chunking as the full scan (never
    //   a serial whole-mmap pass - that would bottleneck the single-giant-file case);
    // - a relaxed AtomicBool short-circuits it the moment ANY line may match: the remaining
    //   lines skim (one load + return), the partial malformed count is discarded, and the
    //   full scan below recounts exactly - a file WITH matches pays only the skim;
    // - the malformed-line count is a TESTED contract (no silent skip): a gated file's
    //   candidate lines were each syntax-validated (`validate_line_syntax` - no Record
    //   build, no allocation) before the verdict, so real corruption (torn writes) counts
    //   exactly as the full scan would;
    // - the elicitation-sidecar merges live OUTSIDE these bytes (a separate tiny file,
    //   top-level sessions only): when any are pending they could still match, so fall
    //   through to the normal scan (rare); their malformed count is reported either way.
    if address.is_none() && matcher.has_prefilter() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let force_full = AtomicBool::new(false);
        // Pre-scan verdict per candidate line: a literal or CONSERVATIVE-marker hit
        // forces the full scan (flag + short-circuit); a VERIFIABLE-marker line is
        // COLLECTED for stage-2 (parsed there, so its malformed accounting happens
        // there too); anything else is syntax-validated for the malformed count.
        let (marker_lines, mut gate_skipped): (Vec<Vec<u8>>, usize) =
            crate::parse::scan_lines_parallel(bytes, |line, _| {
                if force_full.load(Ordering::Relaxed) {
                    return crate::parse::LineVerdict::Ignore; // verdict already "full scan"
                }
                if !line_is_transcript_candidate(line, &gates) {
                    // The malformed-line rule: obviously-corrupt non-candidates are COUNTED.
                    return crate::parse::non_candidate_verdict(line);
                }
                if matcher.line_prefilter_hits(line) || matcher.synth_conservative_hits(line) {
                    force_full.store(true, Ordering::Relaxed);
                    return crate::parse::LineVerdict::Ignore;
                }
                if matcher.synth_verifiable_hits(line) {
                    return crate::parse::LineVerdict::Keep(line.to_vec());
                }
                match crate::parse::validate_line_syntax(line) {
                    Ok(()) => crate::parse::LineVerdict::Ignore,
                    Err(_) => crate::parse::LineVerdict::Skip,
                }
            });
        // Stage-2: re-render each collected marker line's SYNTHESIZED texts through the
        // shared engines and regex-check them. A malformed marker line is counted here
        // (it was deliberately NOT validated in the pre-scan - no double count).
        let mut synth_matched = force_full.load(Ordering::Relaxed);
        if !synth_matched {
            for raw in &marker_lines {
                match crate::parse::parse_line(raw) {
                    Ok(Some(rec)) => {
                        // LOSSY, exactly as the emission path decodes the same bytes
                        // (`gated_raw` below). A strict decode answers `None` on a line
                        // carrying one invalid byte, and `None` here means "re-render
                        // without the source line" - so the gate would judge a shorter
                        // text than the one the emission would produce and could prune a
                        // file whose only hit lies in the part it did not render. The two
                        // decodes have to agree, and the emission's is the one that
                        // decides what a reader sees.
                        let text = String::from_utf8_lossy(raw);
                        if matcher.synth_texts_match(&rec, Some(&text)) {
                            synth_matched = true;
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => gate_skipped += 1,
                }
            }
        }
        if !synth_matched {
            // No candidate line can hit => no exchange can emit; `gate_skipped` is the
            // exact malformed count (every candidate line was validated or parsed once).
            if crate::subagent::is_subagent_path(path) {
                return Ok(FileResult {
                    exchanges: Vec::new(),
                    skipped_lines: gate_skipped,
                    turn_count: 0,
                    chain: ChainCounts::new(cross),
                    names_for_addr,
                });
            }
            let (pending, pending_skipped) =
                crate::commands::elicitation::unresolved_pending(path)?;
            if pending.is_empty() {
                return Ok(FileResult {
                    exchanges: Vec::new(),
                    skipped_lines: gate_skipped + pending_skipped,
                    turn_count: 0,
                    chain: ChainCounts::new(cross),
                    names_for_addr,
                });
            }
        }
    }

    // Retain every TRANSCRIPT record in file order (genuine users delimit turns;
    // the rest are turn members). Two-stage prefilter (section 7d):
    //   1. CATEGORY prefilter - drop pure-noise lines (attachment/system/metadata)
    //      pre-JSON. This is the dominant cost win (attachment alone is 54% of
    //      records). Broad-by-design (a role substring) so no genuine turn is lost.
    //   2. KEYWORD prefilter - a per-line `memmem` of the regex's required literal.
    //      It does NOT gate parsing (a non-matching record may still be a sibling in
    //      a matched turn's round-trip); instead it records `can_hit`, letting the
    //      match phase skip regex work on records that provably can't match.
    // Parse all transcript-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool)
    // so a single giant transcript is not scanned on one core. The stage-2 keyword prefilter
    // (`can_hit`) is computed per line inside the parallel scan, where the raw bytes are in hand.
    // The `compact_boundary` boundary keep is GATED on the active `-t` selector: only look
    // for the rare `type:"system"` boundary line when a selector can actually reach
    // `harness.compaction.boundary` (or no `-t` = match-all). A `-t user` / `-t agent.*` search can
    // never match a boundary, so it pays ZERO for the extra check - the hard `-t` filter PRUNES the
    // byte-scan instead of taxing it (computed once above the whole-file gate, captured here).
    let (mut records, mut spine, mut skipped): (Vec<Kept>, Vec<crate::parse::SpineRow>, usize) =
        crate::parse::scan_lines_parallel_split(bytes, |line, line_no| {
            let ordinary = line_is_transcript_candidate(line, &gates);
            // The 1:1 LAW: an explicit `--line` address names a physical line, and csift
            // answers for it - a shape the prefilter drops and csift models no leaf for
            // still renders, as an unmodeled unit, rather than reading as "no such
            // record". Scoped to the NAMED lines, so a `show --line` on a giant transcript
            // parses those few extra lines and not the file. A `--uuid` address cannot
            // name a line before the scan, so it keeps the ordinary admission.
            if !ordinary && !address.is_some_and(|a| a.lines.contains(&line_no)) {
                // The SURVIVAL AXIS needs the DAG, and the DAG threads through the
                // `attachment` / `system` lines this prefilter drops - a prompt submitted
                // after a SessionStart hook is parented to that hook's attachment record.
                // So the non-candidate arm lifts the five structural fields (never the
                // payload) into a spine row, on its OWN output stream: it is not a
                // searchable record, and keeping the two kinds apart is what stops the
                // narrow row from being carried at a full record's width.
                if let Some(row) = crate::parse::spine_record(line_no, line) {
                    return crate::parse::SplitVerdict::Second(row);
                }
                // The malformed-line rule: obviously-corrupt non-candidates are COUNTED.
                return crate::parse::non_candidate_split(line);
            }
            let can_hit = matcher.line_may_match(line);
            match crate::parse::parse_line(line) {
                Ok(Some(rec)) => crate::parse::SplitVerdict::First(Kept {
                    rec,
                    can_hit,
                    line_no,
                    from_sidecar: false,
                    // A NON-ROLE line is one a gate admitted, and its content lives in
                    // fields the `Record` struct does not model - so the render and the
                    // unmodeled unit need the bytes. An ADDRESSED line keeps them too,
                    // whatever its role marker: the 1:1 law's key=value body is read off
                    // the raw line, and a text-less message record has nothing else to
                    // render. An ordinary conversation record on an ordinary scan
                    // allocates nothing.
                    gated_raw: (!crate::parse::line_has_role_marker(line)
                        || address.is_some_and(|a| a.lines.contains(&line_no)))
                    .then(|| String::from_utf8_lossy(line).into_owned()),
                }),
                Ok(None) => crate::parse::SplitVerdict::Ignore,
                Err(_) => crate::parse::SplitVerdict::Skip,
            }
        });

    // A DEFAULT-ON needle admits attachment lines this scan cannot search. The channel
    // needle keeps any line carrying the envelope literal, so a hook context that quotes
    // the header or a file-edit payload whose snippet does arrives here too - and the
    // `compact_boundary` needle is a bare value substring, so on a flagless
    // scan it admits any payload that merely mentions the word. Both attachment leaves keep
    // their own flag (the gated-leaves law): such a record surfaces on a default scan only as
    // a delivery (its first content string opens with the header), never as bare
    // `harness.hook.context` or one of the `harness.context.*` payload leaves. A scan that
    // named one of the per-VIEW leaves is not such a scan: it asked for those records by
    // name, so the demotion stands down for it exactly as it does for the two flags.
    //
    // DEMOTE it, never delete it. The SURVIVAL AXIS walks `parentUuid` THROUGH attachment
    // records - a prompt submitted after a SessionStart hook is parented to that hook's
    // attachment record - so removing a node breaks the chain AT it: the walk cannot resolve
    // that parent, its floor freezes at the break, and every record above reads `PreCut` (or
    // `Abandoned` off the main line). Reduced to its structural fields the record is exactly
    // the spine row the non-candidate arm would have produced for the same line, so it is
    // invisible to every emission pass and visible to the chain. An address or either
    // attachment flag admits it whole, as before.
    if !gates.hook_context && !gates.attachments && address.is_none() {
        let mut demoted: Vec<crate::parse::SpineRow> = Vec::new();
        records.retain(|k| {
            let gated_attachment = k.rec.hook_additional_context_text().is_some()
                || k.rec.attachment_payload_text().is_some();
            if !gated_attachment || k.rec.csift_channel_text().is_some() {
                return true;
            }
            // A record that IS one of the two attachment VIEWS is not machinery: it is the
            // person's own mid-turn steer or the model's own answer, and it stays whole
            // whenever its own needle admitted the line. That is per RECORD, not per scan -
            // the needle admits every `queued_command` line, and only the human-origin one
            // is a view, so the gated payloads around it are demoted exactly as before.
            if crate::commands::search::scan::gates::record_view_bit(&k.rec)
                .is_some_and(|bit| gates.attachment_views & bit != 0)
            {
                return true;
            }
            demoted.push(crate::parse::spine_from_record(k.line_no, &k.rec));
            false
        });
        // `retain` visits in order, so `demoted` is ascending by line and merges with the
        // scan's own spine stream in one linear pass.
        if !demoted.is_empty() {
            spine = merge_spine(std::mem::take(&mut spine), demoted);
        }
    }

    // A DEFAULT-ON needle is a bare byte substring, so it also admits a NON-TRANSCRIPT line
    // whose payload merely quotes it: a `queue-operation` rider carrying a
    // `<task-notification>` that mentions the channel envelope, a `last-prompt` line whose
    // prompt does. That line's own leaf is gated - `user.queued` and every
    // `harness.bookkeeping.*` leaf is admitted only by a selector reaching it, or an address -
    // so it survives only when THIS scan reaches ITS leaf, and otherwise leaves the record
    // set exactly as the non-candidate arm would have left it: per record, like the
    // attachment views above, because one queue line type carries both leaves. Only a line
    // whose type is none of the four transcript types can carry either family, which keeps
    // the check to a string compare on every other record.
    if address.is_none() {
        let mut demoted: Vec<crate::parse::SpineRow> = Vec::new();
        records.retain(|k| {
            let non_transcript = k
                .rec
                .r#type
                .as_deref()
                .is_some_and(|t| !matches!(t, "user" | "assistant" | "attachment" | "system"));
            let unreached = non_transcript
                && k.rec.promoted_class().is_some_and(|c| {
                    (c == Class::UserQueued || c.category() == "harness.bookkeeping") && !reach(c)
                });
            // A `last-prompt` line is a chain input (the recorded leaf) and goes back as the
            // spine row the byte walk lifts for it; every other bookkeeping type is no chain
            // node and the walk lifts nothing for it, so it simply leaves the record set.
            if unreached && crate::parse::SpineKind::from_type(k.rec.r#type.as_deref()).is_some() {
                demoted.push(crate::parse::spine_from_record(k.line_no, &k.rec));
            }
            !unreached
        });
        if !demoted.is_empty() {
            spine = merge_spine(std::mem::take(&mut spine), demoted);
        }
    }

    // Read the fired-prompt instants off the `system`/`scheduled_task_fire` lines the
    // gate above admitted, THEN put those lines back where a default scan had them. The join is
    // built first because the demote is what makes it unreachable afterwards.
    let schedule_fires = if gates.schedule_fire {
        crate::model::ScheduleFireIndex::from_records(records.iter().map(|k| &k.rec))
    } else {
        crate::model::ScheduleFireIndex::default()
    };
    // The fire line's own leaf is the GATED `harness.schedule.event`, so a scan that cannot reach
    // that leaf must not start emitting it just because this keep parsed the line. Demote, never
    // delete, for the same reason the attachment keep does: the chain walks `parentUuid` THROUGH
    // these records - a fired prompt's parent IS one - so removing a node would break the walk
    // at it. An address (`show`) turns `gates.system` on and keeps them whole, as before.
    if gates.schedule_fire && !gates.system {
        let mut demoted: Vec<crate::parse::SpineRow> = Vec::new();
        records.retain(|k| {
            if k.rec.is_scheduled_task_fire() {
                demoted.push(crate::parse::spine_from_record(k.line_no, &k.rec));
                return false;
            }
            true
        });
        if !demoted.is_empty() {
            spine = merge_spine(std::mem::take(&mut spine), demoted);
        }
    }

    // -- Transparent elicitation-sidecar merge (section 3.10) --
    // A TOP-LEVEL session may have a hook-written `elicitations.jsonl` carrying the
    // unresolved-pending AskUserQuestion/ExitPlanMode/MCP records that are MISSING from the
    // native transcript (whole-turn buffered / in-memory). Merge them in as native-shaped
    // records so they classify + match normally; they have no physical line (line_no 0,
    // from_sidecar). Subagent transcripts have no sidecar (it is keyed by the top-level
    // session). The merge is near-free when nothing is pending (typically 0 records).
    if !crate::subagent::is_subagent_path(path) {
        let (pending, pending_skipped) = crate::commands::elicitation::unresolved_pending(path)?;
        skipped += pending_skipped;
        for rec in pending {
            records.push(Kept {
                rec,
                can_hit: true, // no physical line to prefilter - let the matcher decide.
                line_no: 0,
                from_sidecar: true,
                gated_raw: None,
            });
        }
    }

    // A `/fork` child's line 1 is a `fork-context-ref` record: the transcript is a CLONE
    // of its parent's, so its first turn-opener is the parent's human message, not a
    // spawn-prompt seed. The value substring is the needle the needle law admits.
    let head_end = memchr::memchr(b'\n', bytes)
        .unwrap_or(bytes.len())
        .min(4096);
    let head_is_fork = memchr::memmem::find(&bytes[..head_end], b"fork-context-ref").is_some();
    let rows = merge_rows(&records, &spine);
    let (mut exchanges, turn_count, chain_counts) = reconstruct_and_match(
        path,
        &rows,
        args,
        matcher,
        turn_range,
        time_window,
        address,
        want_siblings,
        spawn_map,
        inner_parallel,
        head_is_fork,
        &schedule_fires,
        for_addr,
        engine_edges,
        cross,
    );

    // `--raw`: backfill each hit's VERBATIM source line from this file's mmap - one pass
    // over the wanted line numbers only; the render layer then emits bytes, never a
    // re-render (a re-serialization would not be verbatim).
    if args.raw {
        let wanted: std::collections::BTreeSet<usize> = exchanges
            .iter()
            .flat_map(|e| e.hits.iter())
            .filter(|h| !h.from_sidecar && h.line > 0)
            .map(|h| h.line)
            .collect();
        if !wanted.is_empty() {
            let mut raw_by_line: HashMap<usize, String> = HashMap::new();
            let mut ln = 0usize;
            let _ = crate::parse::scan_lines_bytes(bytes, |line| {
                ln += 1;
                if wanted.contains(&ln) {
                    raw_by_line.insert(ln, String::from_utf8_lossy(line).into_owned());
                }
            });
            for ex in &mut exchanges {
                for h in &mut ex.hits {
                    if let Some(r) = raw_by_line.get(&h.line) {
                        h.raw = Some(r.clone());
                    }
                }
            }
        }
    }

    Ok(FileResult {
        exchanges,
        skipped_lines: skipped,
        turn_count,
        chain: chain_counts,
        names_for_addr,
    })
}
