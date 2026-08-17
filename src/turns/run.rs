//! run_verbatim: scope resolution, per-file scan, candidate prefilter.

use super::*;

/// Entry point for `csift verbatim`.
pub fn run_verbatim(args: &VerbatimArgs) -> Result<()> {
    // `--turn` and `--since`/`--until` INTERSECT (AND) - the one windowing rule every
    // command shares (the former mutual-exclusion bail was a leftover; search/recover/stats
    // already intersected).
    if !(args.round_trip_fraction > 0.0 && args.round_trip_fraction < 1.0) {
        bail!(
            "--round-trip-fraction must be in the open interval (0.0, 1.0), got {}",
            args.round_trip_fraction
        );
    }
    if args.slices.is_none() && args.budget == 0 {
        bail!("--budget must be > 0");
    }
    if let Some(n) = args.slices {
        if n == 0 {
            bail!("--slices must be > 0 (it pins the fleet to N chunks)");
        }
        if args.slice.is_none() {
            bail!("--slices N sets the fleet size; pass --slice i to pick which chunk to emit");
        }
    }
    // ── Validate --slice / --window (chunked-output mode) ──
    if let Some(slice) = args.slice {
        if slice == 0 {
            bail!("--slice is 1-based: the first chunk is --slice 1");
        }
        if args.window == 0 {
            bail!("--window must be > 0");
        }
        if args.out.is_some() {
            bail!(
                "--slice and --out are mutually exclusive: --slice writes the selected chunk \
                 to stdout, --out writes the whole document to a file"
            );
        }
        if matches!(args.format, OutputFormat::Json) {
            bail!(
                "--slice requires the text format (the chunked-injection use case is verbatim \
                 text); drop --format json"
            );
        }
    }

    // Normalize the budget to characters. `--slices N` pins the FLEET size, so the budget is
    // derived as N windows (the slice COUNT is the hard constraint - a fixed set of registered
    // hooks must never need to grow); otherwise it is the requested char/token amount.
    let budget_chars = if let Some(n) = args.slices {
        n.saturating_mul(args.window)
    } else {
        // `--budget` is CHARS, always (the former `--budget-unit tokens` mode - which
        // silently reinterpreted the unchanged default as 4× the output - is gone;
        // ≈4 chars/token is the documented rule of thumb for sizing a token budget).
        args.budget
    };

    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // `verbatim` is single-conversation recovery and `--budget` applies PER session, so a bare
    // `csift verbatim` (0 targets ⇒ ALL projects everywhere else) would realize budget × every
    // session of every project - an output flood that is never what the caller wants. A
    // target is REQUIRED here (the `show` precedent: name what you mean).
    if args.paths.is_empty() && args.sessions_from.is_none() {
        bail!(
            "verbatim reconstructs ONE conversation's recent turns — name a target: `@<uuid>` / \
             `@main` / `@<agent-id>` / a project path / `--sessions-from <FILE|->`. (A bare \
             `csift verbatim` would realize --budget chars × EVERY session of EVERY project.)"
        );
    }

    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;

    // Parallel scan across files (default rayon pool = CPU count).
    let per_file: Vec<ScanResult> = session_files
        .par_iter()
        .map(|p| scan_one_file(p))
        .collect::<Result<Vec<_>>>()?;

    let mut skipped_lines = 0usize;
    let mut sessions: Vec<ScanResult> = Vec::new();
    for sr in per_file {
        skipped_lines += sr.skipped_lines;
        sessions.push(sr);
    }
    sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    // Apply the window to the turns of each session (a turn admitted by turn-index /
    // its user/assistant timestamp). The summary dedup set is computed from the FULL
    // (un-windowed) newest summary so a window never silently un-dedups.
    for sr in &mut sessions {
        // Resolve `--turn` open/from-end forms against THIS session's own turn count.
        let bounds = turn_range.map(|spec| {
            let tc = sr
                .turns
                .iter()
                .map(|t| t.turn_index)
                .max()
                .map_or(0, |m| m + 1);
            spec.resolve(tc, false)
        });
        sr.turns.retain(|t| {
            let ts = t
                .user
                .as_ref()
                .and_then(|u| u.ts_utc.as_deref())
                .or_else(|| t.assistant_eot().and_then(|a| a.ts_utc.as_deref()));
            window_admits(t.turn_index, ts, bounds, &time_window)
        });
    }

    // Misuse self-diagnosis: `verbatim` exists to restore what a compaction CLIPPED. A
    // session with ZERO compaction summaries has nothing clipped - the caller almost
    // certainly wants `show --turn` (full records, no budget/truncation). stderr only
    // (stdout stays the reconstruction); suppressed in --slice mode (the hook path is
    // deliberate and must stay quiet).
    if args.slice.is_none() {
        for sr in &sessions {
            if sr.summaries.is_empty() {
                eprintln!(
                    "csift: note: @{} has no compaction — nothing was clipped; for plain \
                     reading use `csift show @{} --turn <N|A..B|-k..>` (full records, no \
                     budget)",
                    sr.session_id, sr.session_id
                );
            }
        }
    }

    // Resolve the richness configuration (master mode + thresholds, profile applied
    // first then explicit flags). Defaults to EotOnly → today's single-EOT behavior.
    let cfg = args.richness_cfg();

    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| {
            plan_session(
                sr,
                budget_chars,
                args.round_trip_fraction,
                args.max_compactions,
                &cfg,
            )
        })
        .collect();

    let ctx = RenderCtx {
        budget_chars,
        rt_fraction: args.round_trip_fraction,
        skipped_lines,
        cfg,
    };

    match args.format {
        OutputFormat::Text => render_text(
            &ctx,
            &sessions,
            &plans,
            args.out.as_deref(),
            args.slice,
            args.window,
            args.slices,
        )?,
        OutputFormat::Json => render_json(&ctx, &sessions, &plans, args.out.as_deref())?,
    }
    Ok(())
}

/// True when a turn at `turn_index` / `ts` is admitted by the active window. A
/// timestamp-less turn never falls inside a BOUNDED time window (same rule as recover).
pub(crate) fn window_admits(
    turn_index: usize,
    ts: Option<&str>,
    turn_range: Option<(usize, usize)>,
    time_window: &TimeWindow,
) -> bool {
    if let Some((lo, hi)) = turn_range {
        if turn_index < lo || turn_index > hi {
            return false;
        }
    }
    time_window.contains(ts)
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-file scan
// ─────────────────────────────────────────────────────────────────────────────

/// Scan one session file: mmap → forward line-numbered scan → build the per-turn
/// `TurnSlice`s + the per-summary dedup sets. The forward `scan_lines_bytes` path is
/// mandatory (NOT head/tail): it visits every line including blanks, so the local
/// counter == the true jsonl line (the recover discipline, reused verbatim).
pub(crate) fn scan_one_file(path: &Path) -> Result<ScanResult> {
    // Canonical bare-hex id (subagent `agent-` prefix stripped) - the SAME derivation
    // every other surface uses, so a `turns` subagent unit's `session_id` is joinable to
    // `files`/`search`/`recover`/`agents` (id-form unification; a top-level uuid is
    // unaffected). See [`crate::subagent::session_id_from_path`].
    let session_id = crate::subagent::session_id_from_path(path);
    // Id-domain discriminator (the r5 shape, now on turns JSON): a subagent transcript's
    // `session_id` is a non-re-feedable bare hex; carry `is_subagent` + the re-feedable
    // parent uuid (the dir before `subagents/`). A top-level file is its own parent.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(ScanResult {
            session_id,
            is_subagent,
            parent_session_id,
            turns: Vec::new(),
            summaries: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // Parse all turn-candidate lines IN PARALLEL (newline-aligned chunks on the rayon
    // pool). `scan_lines_parallel` visits every line - blanks included - with its exact
    // 1-based number, so the `(line_no, Record)` stream and the malformed count are
    // byte-for-byte identical to the serial `scan_lines_bytes` pass this replaces; the
    // win is that a single giant transcript (the default `turns @main` case is ONE file)
    // is no longer bottlenecked on one core.
    let (records, mut skipped) = crate::parse::scan_lines_parallel(bytes, |line, line_no| {
        if !line_is_turn_candidate(line) {
            // R10: obviously-corrupt non-candidates are COUNTED (the malformed law).
            return crate::parse::non_candidate_verdict(line);
        }
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => crate::parse::LineVerdict::Keep((line_no, rec)),
            Ok(None) => crate::parse::LineVerdict::Ignore, // blank - counted in numbering
            Err(_) => crate::parse::LineVerdict::Skip,     // malformed - counted
        }
    });

    // ── Transparent elicitation-sidecar merge (§3.10) ──
    // A TOP-LEVEL session's unresolved-pending elicitations (AskUserQuestion/ExitPlanMode/MCP)
    // are MISSING from the native transcript (whole-turn buffered / in-memory). Append them as
    // native-shaped records with line_no 0 (no physical line - `from_sidecar`); `build` turns
    // each into its own pending turn unit so the reconstruction includes it. Subagent
    // transcripts have no sidecar (it is keyed by the top-level session). Near-free when
    // nothing is pending.
    let mut sidecar: Vec<Record> = Vec::new();
    if !is_subagent {
        let (pending, pending_skipped) = crate::elicitation::unresolved_pending(path)?;
        skipped += pending_skipped;
        sidecar = pending;
    }

    let (turns, summaries) = build(&records, &sidecar);
    Ok(ScanResult {
        session_id,
        is_subagent,
        parent_session_id,
        turns,
        summaries,
        skipped_lines: skipped,
    })
}

/// Pre-JSON byte prefilter - a SUPERSET of recover's `line_is_recover_candidate`,
/// broadened so a pure-text assistant turn (no Edit/Write/Read/Bash) is never missed.
/// Coarse by design; the structural parse decides what each line really is.
pub(crate) fn line_is_turn_candidate(line: &[u8]) -> bool {
    // R13: role markers matched serialization-tolerantly (whitespace around the
    // colon is the same record); the remaining needles are key-only / value
    // substrings, which survive reserialization by construction. Finders built ONCE
    // (per-line hot path - the stateless form rebuilt its searcher every call).
    static TYPE_ASSISTANT: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(br#""type":"assistant""#));
    static IS_COMPACT_SUMMARY: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"isCompactSummary"));
    static TOOL_USE: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"tool_use"));
    crate::parse::line_has_role_marker(line)
        || TYPE_ASSISTANT.find(line).is_some() // redundant belt for the role hook
        || IS_COMPACT_SUMMARY.find(line).is_some() // summaries: seeds + boundaries
        || TOOL_USE.find(line).is_some() // for the [N tool calls] count
}

// ─────────────────────────────────────────────────────────────────────────────
// Build: (line_no, Record) → TurnSlice + SummaryInfo
// ─────────────────────────────────────────────────────────────────────────────
