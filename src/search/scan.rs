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
            chain: ChainCounts::default(),
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
    // v0.10.0 promoted non-record lines: admitted ONLY under a selector that reaches the
    // leaf (`scans_gated` - a bare no-`-t` scan never parses them) or an address (`show
    // --line`/`--uuid` renders an addressed line flag-free). `scans_gated` is the ONE
    // predicate the zero-match diagnosis shares, so the note can never claim a line
    // shape was unscanned when this gate parsed it.
    let reach = |c: Class| args.scans_gated(c) || address.is_some();
    // v0.11.0 csift channel: a delivery is a real MESSAGE addressed at this lane, not
    // machinery, so it is admitted by a DEFAULT scan - the one attachment keep that is not
    // flag-gated. The gate is the LABEL selector alone (the D7 law): no `-t`, or any
    // selector reaching `agent.communication.channel`, keeps it; `-t user` / `-t harness`
    // short-circuit before the memmem and pay nothing (and never surface a delivery under
    // the hook leaf, which keeps its own flag). An addressed fetch is already admitted by
    // the two attachment gates above.
    let needs_channel = args.label_filter().selected(Class::CommChannel.path());
    let gates = CandidateGates {
        compact_boundary: needs_compact_boundary,
        hook_context: needs_hook_context,
        attachments: needs_attachments,
        channel: needs_channel,
        queued: reach(Class::UserQueued),
        turn_duration: reach(Class::MetaTurnDuration),
        away_summary: reach(Class::MetaAwaySummary),
        stop_hooks: reach(Class::MetaStopHooks),
        snapshot: reach(Class::MetaSnapshot),
        // v0.11.1: `scans_gated` ALSO admits this leaf under a BARE `harness` role
        // selector. It holds the one gated shape the model actually receives - a
        // `system`/`local_command` record, which the request assembler re-mints as a
        // user message - so a bare `-t harness` would otherwise be blind to the only
        // on-disk evidence of a slash command whose dialog wrote nothing else. The keep
        // is the same key-only `"subtype"` memmem, still `&&`-gated; the per-record
        // override (`Record::delivery_override`) then drops every non-delivered system
        // record this admission parsed. Every other gated leaf is unchanged.
        system: reach(Class::MetaSystem),
    };

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
                if !line_is_transcript_candidate(line, &gates) {
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
                    chain: ChainCounts::default(),
                });
            }
            let (pending, pending_skipped) = crate::elicitation::unresolved_pending(path)?;
            if pending.is_empty() {
                return Ok(FileResult {
                    exchanges: Vec::new(),
                    skipped_lines: gate_skipped + pending_skipped,
                    turn_count: 0,
                    chain: ChainCounts::default(),
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
        if !line_is_transcript_candidate(line, &gates) {
            // The SURVIVAL AXIS needs the DAG, and the DAG threads through the
            // `attachment` / `system` lines this prefilter drops - a prompt submitted
            // after a SessionStart hook is parented to that hook's attachment record. So
            // the non-candidate arm lifts the five structural fields (never the payload)
            // into a spine row. It is not a searchable record: `spine` keeps it out of
            // every emission pass.
            if let Some(rec) = crate::parse::spine_record(line) {
                return crate::parse::LineVerdict::Keep(Kept {
                    rec,
                    can_hit: false,
                    line_no,
                    from_sidecar: false,
                    spine: true,
                });
            }
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
                spine: false,
            }),
            Ok(None) => crate::parse::LineVerdict::Ignore,
            Err(_) => crate::parse::LineVerdict::Skip,
        }
    });

    // A DEFAULT-ON needle admits attachment lines this scan cannot search. The channel
    // needle is the v0.11.0 one - it keeps any line carrying the envelope literal, so a hook
    // context that quotes the header or a file-edit payload whose snippet does arrives here
    // too - and the D7 `compact_boundary` needle is a bare value substring, so on a flagless
    // scan it admits any payload that merely mentions the word. Both attachment leaves keep
    // their own flag (the gated-leaves law): such a record surfaces on a default scan only as
    // a delivery (its first content string opens with the header), never as bare
    // `harness.meta.hook` or `harness.meta.attachment`.
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
        for k in &mut records {
            if k.spine {
                continue;
            }
            let gated_attachment = k.rec.hook_additional_context_text().is_some()
                || k.rec.attachment_payload_text().is_some();
            if gated_attachment && k.rec.csift_channel_text().is_none() {
                k.rec = crate::parse::spine_from_record(&k.rec);
                k.can_hit = false;
                k.spine = true;
            }
        }
    }

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
                spine: false,
            });
        }
    }

    // A `/fork` child's line 1 is a `fork-context-ref` record: the transcript is a CLONE
    // of its parent's, so its first turn-opener is the parent's human message, not a
    // spawn-prompt seed (v0.10.2). The value substring is the R13-safe needle.
    let head_end = memchr::memchr(b'\n', bytes)
        .unwrap_or(bytes.len())
        .min(4096);
    let head_is_fork = memchr::memmem::find(&bytes[..head_end], b"fork-context-ref").is_some();
    let (mut exchanges, turn_count, chain_counts) = reconstruct_and_match(
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
        head_is_fork,
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
    })
}

/// The `&&`-gated candidate keeps beyond the role marker (each one a SIMD memmem that
/// runs only when its gate is on, so a default scan pays ZERO for all of them).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CandidateGates {
    /// D7: the `compact_boundary` metrics record (selected label).
    pub(crate) compact_boundary: bool,
    /// `--additional-context` (or an address): hook-injected context attachments.
    pub(crate) hook_context: bool,
    /// `--attachments` / the attachment axis (or an address): every attachment line.
    pub(crate) attachments: bool,
    /// v0.11.0 (a selector reaching `agent.communication.channel`, which a bare scan
    /// does): a csift-channel delivery attachment. DEFAULT-ON, unlike every other
    /// attachment keep - a delivery is a message, not machinery.
    pub(crate) channel: bool,
    /// v0.10.0 (explicit selector or address): `queue-operation` lines.
    pub(crate) queued: bool,
    /// v0.10.0: `system`/`turn_duration` lines.
    pub(crate) turn_duration: bool,
    /// v0.10.0: `system`/`away_summary` lines.
    pub(crate) away_summary: bool,
    /// v0.10.0: `system`/`stop_hook_summary` lines.
    pub(crate) stop_hooks: bool,
    /// v0.10.0: `file-history-snapshot` + `file-history-delta` lines.
    pub(crate) snapshot: bool,
    /// v0.10.1: every OTHER `type:"system"` subtype (`harness.meta.system`).
    pub(crate) system: bool,
}

/// §7d stage-1 category prefilter on raw bytes: keep a line only if it could be a
/// transcript message (user/assistant role marker) - drops `attachment`,
/// `file-history-*`, `queue-operation`, and metadata noise pre-JSON unless the
/// matching [`CandidateGates`] flag admits them. Kept deliberately permissive
/// (substring, not structural) so no genuine turn is lost.
pub(crate) fn line_is_transcript_candidate(line: &[u8], gates: &CandidateGates) -> bool {
    let needs_compact_boundary = gates.compact_boundary;
    let needs_hook_context = gates.hook_context;
    let needs_attachments = gates.attachments;
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
    // v0.11.0 csift channel: the envelope needle is a bare VALUE substring of the injected
    // content - ASCII, no JSON-escaped character - so it survives a reserialize (R13) and a
    // decoded match implies a raw match. Shared with the classifier via one constant so the
    // byte scan and the leaf can never drift.
    static CHANNEL_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(Record::CSIFT_CHANNEL_NEEDLE.as_bytes()));
    // v0.10.0 promoted lines. Quoted-value / bare-value needles per the R13 law: the
    // `type` values `"queue-operation"` and `"file-history-` (a prefix covering both
    // `-snapshot` and `-delta`), and the bare `subtype` values for the three system
    // records (a value substring survives a reserialize; prose quoting the word lands
    // on a role line and is harmless - it already parses).
    static QUEUED_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"queue-operation\""));
    static TURN_DURATION_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"turn_duration"));
    static AWAY_SUMMARY_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"away_summary"));
    static STOP_HOOKS_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"stop_hook_summary"));
    static SNAPSHOT_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"file-history-"));
    // v0.10.1 catch-all system subtypes: the key-only needle `"subtype"` (every system
    // record carries it; a quoted KEY survives a reserialize, and the classify arm
    // decides which subtype it is - the already-modeled ones simply reclassify). Also the
    // second half of the D7 boundary conjunction below, which narrows the same property to
    // the one subtype it models.
    static SUBTYPE_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"subtype\""));
    crate::parse::line_has_role_marker(line)
        // D7: ALSO keep the rare `compact_boundary` metrics record (a `type:"system"` record with no
        // role marker) so `search -t harness.compaction.boundary` can enumerate compaction points +
        // inspect their `compactMetadata` - but ONLY when an active `-t` selector can reach that label
        // (`needs_compact_boundary`, derived once via `label_selected`). For every other query the
        // `&&` short-circuits BEFORE the memmem, so a non-boundary search pays ZERO. When it IS run,
        // the `||` chain still reaches this memmem only on lines that already failed both role checks,
        // and boundary records are rare - so the §7 perf contract holds either way.
        // The keep is a CONJUNCTION because the rare literal alone is not the discriminator:
        // over every top-level transcript on one corpus (76 files, 2661636749 bytes, lines split
        // on `\n` only) 252 of 252 true boundaries - a JSON object with top-level
        // `type=="system"` and `subtype=="compact_boundary"` - carry the key-only bytes
        // `"subtype"`, while 226 of 226 lines that carry the literal in a payload and would
        // otherwise be admitted by D7 alone (attachments and `queue-operation` lines, which no
        // other flagless keep reaches) carry it in none. The key needle is R13-safe by form (a
        // quoted KEY survives a reserialize) and the boundary finder stays FIRST, so the second
        // memmem runs only on the handful of lines carrying the rare literal.
        || (needs_compact_boundary
            && COMPACT_BOUNDARY_FINDER.find(line).is_some()
            && SUBTYPE_FINDER.find(line).is_some())
        // Opt-in hook-injected additionalContext (`search --additional-context`, or an explicit
        // `show --line`/`--uuid` address - the refetch a search hit prints must resolve without
        // the flag). Same `&&`-gating law as the boundary: a default scan pays ZERO.
        || (needs_hook_context && HOOK_CONTEXT_FINDER.find(line).is_some())
        // Opt-in FULL attachment keep (`search --attachments` / `--count-by attachment`, or an
        // explicit address). Same `&&`-gating law: a default scan pays ZERO.
        || (needs_attachments && ATTACHMENT_FINDER.find(line).is_some())
        // v0.11.0: a csift-channel delivery, kept under a DEFAULT scan (the label gate is on
        // whenever a selector can reach the leaf). Same `&&` shape as the keeps above, so it
        // still runs only on a line that failed both role checks, and a `-t` that cannot
        // reach the leaf short-circuits before the memmem.
        || (gates.channel && CHANNEL_FINDER.find(line).is_some())
        // v0.10.0 promoted lines, each behind its own explicit-selector gate.
        || (gates.queued && QUEUED_FINDER.find(line).is_some())
        || (gates.turn_duration && TURN_DURATION_FINDER.find(line).is_some())
        || (gates.away_summary && AWAY_SUMMARY_FINDER.find(line).is_some())
        || (gates.stop_hooks && STOP_HOOKS_FINDER.find(line).is_some())
        || (gates.snapshot && SNAPSHOT_FINDER.find(line).is_some())
        || (gates.system && SUBTYPE_FINDER.find(line).is_some())
}
