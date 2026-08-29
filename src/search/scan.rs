//! Per-file scan: fetch_records (show's engine), search_one_file, the candidate prefilter.

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
) -> Result<(Vec<Exchange>, usize, usize)> {
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
    )?;
    Ok((fr.exchanges, fr.skipped_lines, fr.turn_count))
}

/// Scan a single session file: prefilter → parse → delimit turns → match → stitch.
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
) -> Result<FileResult> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(FileResult {
            exchanges: Vec::new(),
            skipped_lines: 0,
            turn_count: 0,
            superseded_drafts: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // A GIANT transcript's per-turn match phase fans out even under a broad scan: such a
    // file is the straggler the rest of the pool ends up waiting on (only a handful of
    // files this size exist, so the nested fan-out adds no measurable steal churn - unlike
    // enabling it for every mid-size file, which did).
    const HUGE_FILE_BYTES: usize = 64 * 1024 * 1024;
    let inner_parallel = inner_parallel || bytes.len() >= HUGE_FILE_BYTES;

    // The D7 `compact_boundary` prefilter-widening is GATED on the active `-t` selector -
    // computed up front because BOTH the whole-file gate below and the candidate scan key on it.
    let needs_compact_boundary = args
        .label_filter()
        .selected(Class::CompactionBoundary.path());
    // The `--additional-context` widening mirrors the D7 gate: only when the flag is set AND
    // the selector can reach `harness.meta.hook` - OR when an ADDRESS names records directly
    // (`show --line`/`--uuid` must render an addressed attachment record flag-free).
    let needs_hook_context = (args.additional_context
        && args.label_filter().selected(Class::MetaHook.path()))
        || address.is_some();
    // The `--attachments` widening (a SUPERSET of `--additional-context`): keep EVERY
    // `type:"attachment"` line when the flag (or the `--count-by attachment` axis, which
    // implies it) is set AND the selector can reach either `harness.meta` leaf - or when
    // an ADDRESS names records directly (`show --line`/`--uuid` renders any addressed
    // attachment record flag-free).
    let needs_attachments = (args.scan_attachments()
        && (args.label_filter().selected(Class::MetaAttachment.path())
            || args.label_filter().selected(Class::MetaHook.path())))
        || address.is_some();

    // ── §7f whole-file gate ──
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
                if !line_is_transcript_candidate(
                    line,
                    needs_compact_boundary,
                    needs_hook_context,
                    needs_attachments,
                ) {
                    // R10: obviously-corrupt non-candidates are COUNTED (the malformed law).
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
                        if matcher.synth_texts_match(&rec) {
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
                    superseded_drafts: 0,
                });
            }
            let (pending, pending_skipped) = crate::elicitation::unresolved_pending(path)?;
            if pending.is_empty() {
                return Ok(FileResult {
                    exchanges: Vec::new(),
                    skipped_lines: gate_skipped + pending_skipped,
                    turn_count: 0,
                    superseded_drafts: 0,
                });
            }
        }
    }

    // Retain every TRANSCRIPT record in file order (genuine users delimit turns;
    // the rest are turn members). Two-stage prefilter (§7d):
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
    // The D7 `compact_boundary` prefilter-widening is GATED on the active `-t` selector: only look
    // for the rare `type:"system"` boundary line when a selector can actually reach
    // `harness.compaction.boundary` (or no `-t` = match-all). A `-t user` / `-t agent.*` search can
    // never match a boundary, so it pays ZERO for the extra check - the hard `-t` filter PRUNES the
    // byte-scan instead of taxing it (computed once above the whole-file gate, captured here).
    let (mut records, mut skipped) = crate::parse::scan_lines_parallel(bytes, |line, line_no| {
        if !line_is_transcript_candidate(
            line,
            needs_compact_boundary,
            needs_hook_context,
            needs_attachments,
        ) {
            // R10: obviously-corrupt non-candidates are COUNTED (the malformed law).
            return crate::parse::non_candidate_verdict(line);
        }
        let can_hit = matcher.line_may_match(line);
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => crate::parse::LineVerdict::Keep(Kept {
                rec,
                can_hit,
                line_no,
                from_sidecar: false,
            }),
            Ok(None) => crate::parse::LineVerdict::Ignore,
            Err(_) => crate::parse::LineVerdict::Skip,
        }
    });

    // ── Transparent elicitation-sidecar merge (§3.10) ──
    // A TOP-LEVEL session may have a hook-written `elicitations.jsonl` carrying the
    // unresolved-pending AskUserQuestion/ExitPlanMode/MCP records that are MISSING from the
    // native transcript (whole-turn buffered / in-memory). Merge them in as native-shaped
    // records so they classify + match normally; they have no physical line (line_no 0,
    // from_sidecar). Subagent transcripts have no sidecar (it is keyed by the top-level
    // session). The merge is near-free when nothing is pending (typically 0 records).
    if !crate::subagent::is_subagent_path(path) {
        let (pending, pending_skipped) = crate::elicitation::unresolved_pending(path)?;
        skipped += pending_skipped;
        for rec in pending {
            records.push(Kept {
                rec,
                can_hit: true, // no physical line to prefilter - let the matcher decide.
                line_no: 0,
                from_sidecar: true,
            });
        }
    }

    let (mut exchanges, turn_count, superseded_drafts) = reconstruct_and_match(
        path,
        &records,
        args,
        matcher,
        turn_range,
        time_window,
        address,
        want_siblings,
        spawn_map,
        inner_parallel,
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
        superseded_drafts,
    })
}

/// §7d stage-1 category prefilter on raw bytes: keep a line only if it could be a
/// transcript message (user/assistant role marker) - drops `attachment`,
/// `file-history-snapshot`, `queue-operation`, and metadata noise pre-JSON. Kept
/// deliberately permissive (substring, not structural) so no genuine turn is lost.
pub(crate) fn line_is_transcript_candidate(
    line: &[u8],
    needs_compact_boundary: bool,
    needs_hook_context: bool,
    needs_attachments: bool,
) -> bool {
    // Every user/assistant record carries a `"role":"user"`/`"role":"assistant"`
    // marker (genuine-user string content, tool carriers, assistant blocks all do).
    // R13: matched serialization-tolerantly - `"role": "user"` (reserialized JSON,
    // whitespace around the colon) is the same record and must not vanish silently.
    static COMPACT_BOUNDARY_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"compact_boundary"));
    // R13 needle law: a bare VALUE substring (the attachment payload's `type` value), never a
    // compact `"key":"value"` byte pair - a reserialized line keeps the value intact.
    static HOOK_CONTEXT_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"hook_additional_context"));
    // Quoted needle: `"attachment"` appears verbatim as the record's `"type"` VALUE (and as
    // its payload KEY); an in-content quote is escaped to `\"` in raw bytes, so prose that
    // merely mentions the word never false-keeps. Serialization-tolerant (the quoted value
    // survives a reserialize; R13).
    static ATTACHMENT_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"attachment\""));
    crate::parse::line_has_role_marker(line)
        // D7: ALSO keep the rare `compact_boundary` metrics record (a `type:"system"` record with no
        // role marker) so `search -t harness.compaction.boundary` can enumerate compaction points +
        // inspect their `compactMetadata` - but ONLY when an active `-t` selector can reach that label
        // (`needs_compact_boundary`, derived once via `label_selected`). For every other query the
        // `&&` short-circuits BEFORE the memmem, so a non-boundary search pays ZERO. When it IS run,
        // the `||` chain still reaches this memmem only on lines that already failed both role checks,
        // and boundary records are rare - so the §7 perf contract holds either way.
        || (needs_compact_boundary && COMPACT_BOUNDARY_FINDER.find(line).is_some())
        // Opt-in hook-injected additionalContext (`search --additional-context`, or an explicit
        // `show --line`/`--uuid` address - the refetch a search hit prints must resolve without
        // the flag). Same `&&`-gating law as the boundary: a default scan pays ZERO.
        || (needs_hook_context && HOOK_CONTEXT_FINDER.find(line).is_some())
        // Opt-in FULL attachment keep (`search --attachments` / `--count-by attachment`, or an
        // explicit address). Same `&&`-gating law: a default scan pays ZERO.
        || (needs_attachments && ATTACHMENT_FINDER.find(line).is_some())
}
