//! Per-transcript scan: candidates, extraction entry, turn-attributed events.

use super::*;

/// Scan ONE transcript for ALL manifest targets it mentions: AC-gate, parse + turn-group once,
/// then extract each present target. Returns `(target_index, ScanResult)` for every target with
/// at least one event in this transcript.
pub(crate) fn scan_one_file_multi(
    path: &Path,
    targets: &[String],
    ac: &aho_corasick::AhoCorasick,
    always: &[usize],
) -> Result<Vec<(usize, ScanResult)>> {
    let session_id = crate::subagent::session_id_from_path(path);
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(Vec::new());
    };
    let bytes: &[u8] = &mmap;
    if always.is_empty() && !ac.is_match(bytes) {
        return Ok(Vec::new());
    }
    // Which manifest basenames appear (overlapping, so a basename that is a substring of
    // another match region is still detected) → the targets worth extracting from this file.
    let mut present: Vec<usize> = ac
        .find_overlapping_iter(bytes)
        .map(|m| m.pattern().as_usize())
        .collect();
    present.extend_from_slice(always); // gate-exempt targets are always extracted
    present.sort_unstable();
    present.dedup();

    let (records, skipped) =
        crate::parse::parse_candidates_parallel(bytes, line_is_recover_candidate);
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices_deduped(&recs, |r| *r);

    let mut out = Vec::new();
    for ti in present {
        let events = extract_with_turns(&records, &turns, Some(&targets[ti]));
        if !events.is_empty() {
            out.push((
                ti,
                ScanResult {
                    session_id: session_id.clone(),
                    is_subagent,
                    parent_session_id: parent_session_id.clone(),
                    events,
                    skipped_lines: skipped,
                },
            ));
        }
    }
    Ok(out)
}

/// Reconstruct a target's FINAL content (or its `--at`/window snapshot) as RAW bytes - the
/// restorable file, not the line-numbered diff view. Cross-session writes are merged per
/// top-level group; when unrelated sessions each hold a version, the FRESHEST (latest-write)
/// candidate wins. Returns `(content, known_lines, total_lines)`, or `None` when nothing is
/// recoverable. A partial reconstruction (`known < total`) joins the known lines in order.
pub(crate) fn reconstruct_best(
    scans: Vec<ScanResult>,
    when: &str,
) -> Result<Option<(String, usize, usize)>> {
    let merged = merge_groups_for_reconstruction(scans);
    let mut best: Option<(String, usize, usize, Option<String>)> = None;
    for s in &merged {
        if s.events.is_empty() {
            continue;
        }
        let cutoff = resolve_cutoff(when, &s.events)?;
        let rep = replay(&s.events, cutoff);
        let known = rep.final_buffer.known_lines();
        if known.is_empty() {
            continue;
        }
        let total = rep
            .final_buffer
            .seen_total_lines
            .unwrap_or_else(|| known.last().map(|(n, _)| *n).unwrap_or(0));
        let latest_ts = s
            .events
            .iter()
            .filter_map(|e| e.timestamp_utc.clone())
            .max();
        let mut content = known
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if !content.is_empty() {
            content.push('\n');
        }
        let fresher = match &best {
            None => true,
            Some((_, bk, _, bts)) => (&latest_ts, known.len()) > (bts, *bk),
        };
        if fresher {
            best = Some((content, known.len(), total, latest_ts));
        }
    }
    Ok(best.map(|(c, k, t, _)| (c, k, t)))
}

/// Write `recovery-report.tsv` under `out_dir` and print the one-line summary.
pub(crate) fn write_batch_report(out_dir: &Path, outcomes: &[BatchOutcome]) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let mut body = String::from("status\tknown_lines\ttotal_lines\ttarget\twritten_to\n");
    let (mut complete, mut partial, mut none, mut skipped) = (0usize, 0usize, 0usize, 0usize);
    for o in outcomes {
        match o.status {
            "complete" => complete += 1,
            "partial" => partial += 1,
            "no-history" => none += 1,
            "skipped-exists" => skipped += 1,
            _ => {}
        }
        body.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            o.status,
            o.known,
            o.total,
            o.target,
            o.written
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ));
    }
    let report = out_dir.join("recovery-report.tsv");
    std::fs::write(&report, &body).with_context(|| format!("cannot write {}", report.display()))?;
    println!(
        "recovered {} file(s): {complete} complete · {partial} partial · {none} no-history · \
         {skipped} skipped (already present)",
        outcomes.len()
    );
    println!("report: {}", report.display());
    Ok(())
}

/// True when an event at `turn_index` / `ts` is admitted by the active window.
/// A timestamp-less item never falls inside a BOUNDED time window (same rule as `files`).
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

/// Scan one session file: mmap → forward line-numbered scan → extract `--file` events.
/// The forward `scan_lines_bytes` path is mandatory (NOT head/tail): it visits every line
/// including blanks, so the local counter == the true jsonl line.
pub(crate) fn scan_one_file(path: &Path, target_file: Option<&str>) -> Result<ScanResult> {
    // Bare-hex canonical id for a subagent transcript (strip the `agent-` filename
    // prefix) so a recovered subagent row's `session_id` matches the `agents` topology id
    // - id-form unification (a top-level session uuid is unaffected: no `agent-` prefix).
    let session_id = crate::subagent::session_id_from_path(path);
    // Id-domain discriminator (the r5 shape, now on recover): a subagent transcript's
    // `session_id` is a non-re-feedable bare hex; carry `is_subagent` + the re-feedable
    // parent uuid (the dir before `subagents/`) onto every emitted recover record.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(ScanResult {
            session_id,
            is_subagent,
            parent_session_id,
            events: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // ── File-level prefilter (the dominant cost of an UNSCOPED recover). A transcript that
    //    never mentions the target's BASENAME holds no events for it: every Read/Edit/Write/Bash
    //    op and result `extract` replays carries the path literal somewhere in that transcript,
    //    and `extract` matches by full path OR basename-suffix - so the basename is the correct
    //    SUPERSET gate. One SIMD `memmem` over the mmap lets us skip PARSING the file entirely,
    //    turning an unscoped recover from a whole-corpus JSON parse into a parse of only the few
    //    transcripts that touched the file. (No target ⇒ no gate; behaviour unchanged.)
    if let Some(t) = target_file {
        // Both separators: a Windows path's basename must carry no `\` - a needle
        // with a backslash can never match the raw line (JSON escapes it to `\\`).
        let base = basename_of(t);
        if raw_needle_safe(base) && memmem::find(bytes, base.as_bytes()).is_none() {
            return Ok(ScanResult {
                session_id,
                is_subagent,
                parent_session_id,
                events: Vec::new(),
                skipped_lines: 0,
            });
        }
    }

    // Parse all recover-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool),
    // preserving each record's exact 1-based line number (counts EVERY visited line, 1:1 with
    // jsonl) - a single giant transcript is no longer scanned on one core.
    let (records, skipped) =
        crate::parse::parse_candidates_parallel(bytes, line_is_recover_candidate);

    let events = extract(&records, target_file);
    Ok(ScanResult {
        session_id,
        is_subagent,
        parent_session_id,
        events,
        skipped_lines: skipped,
    })
}

/// Pre-JSON byte prefilter - a SUPERSET of `files`' (we need Reads, tool_result bodies,
/// integrity errors, attachments, history snapshots - not just mutations).
/// Coarse by design; the structural parse decides what each line really is.
pub(crate) fn line_is_recover_candidate(line: &[u8]) -> bool {
    // R13: the genuine-user hook is serialization-tolerant (user-only, like files').
    // Finders built ONCE (per-line hot path - the stateless form rebuilt its searcher
    // every call).
    static NEEDLES: std::sync::LazyLock<[memmem::Finder<'static>; 10]> =
        std::sync::LazyLock::new(|| {
            [
                memmem::Finder::new(b"toolUseResult"),
                memmem::Finder::new(b"Edit"),
                memmem::Finder::new(b"Write"),
                memmem::Finder::new(b"Read"),
                memmem::Finder::new(b"Bash"),
                memmem::Finder::new(b"filePath"),
                memmem::Finder::new(b"file_path"),
                memmem::Finder::new(b"file-history-snapshot"),
                memmem::Finder::new(b"edited_text_file"),
                memmem::Finder::new(b"tool_use_error"),
            ]
        });
    crate::parse::line_has_user_role_marker(line) || NEEDLES.iter().any(|f| f.find(line).is_some())
}

// ─────────────────────────────────────────────────────────────────────────────
// Extraction: (line_no, Record) → FileEvent
// ─────────────────────────────────────────────────────────────────────────────

/// Extract `--file` events from a session's line-numbered records.
///
/// Intent↔result is joined by `tool_use_id` WITHIN a turn (never by adjacency - an
/// integrity error can precede its own tool_use line). We build, per turn, two maps:
/// `tool_use_id → file_path` (from the originating Read/Edit/Write tool_use) so an
/// integrity-error carrier with no inline path can be attributed to `--file`.
pub(crate) fn extract(records: &[(usize, Record)], target_file: Option<&str>) -> Vec<FileEvent> {
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    // Turn delimiting keys on the shared boundary predicate (§6.4) so file-event
    // attribution lines up with `turns`/`search`: an answered AskUserQuestion / a
    // tool-use rejection-with-message opens a turn, an interrupt / local-command-stdout
    // does not.
    let turns = group_turn_indices_deduped(&recs, |r| *r);
    extract_with_turns(records, &turns, target_file)
}

/// Extract `--file` events given PRE-COMPUTED turn groups. Batch reconstruction groups each
/// transcript ONCE and calls this per target, so a transcript mentioning many manifest files
/// is grouped a single time rather than once per file.
pub(crate) fn extract_with_turns(
    records: &[(usize, Record)],
    turns: &[Vec<usize>],
    target_file: Option<&str>,
) -> Vec<FileEvent> {
    let mut events: Vec<FileEvent> = Vec::new();

    for (turn_index, idxs) in turns.iter().enumerate() {
        // tool_use_id → file_path for THIS turn's Read/Edit/Write/MultiEdit tool_uses.
        let mut id_to_path: BTreeMap<String, String> = BTreeMap::new();
        // tool_use_ids whose result carrier carries the structured `toolUseResult` echo -
        // i.e. the ops `extract_from_tool_use_result` can reconstruct from. SUBAGENT and
        // workflow-agent transcripts OMIT `toolUseResult` (the tool_result is just a
        // `"File created successfully…"` string), so those ids are absent here and the
        // input-side fallback below supplies their content (§ subagent recover).
        let mut ids_with_result: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // tool_use_ids whose RESULT was an error (`is_error:true`) - e.g. a failed Edit
        // ("String to replace not found in file", "File has not been read yet"). The op did
        // NOT mutate the file, so its tool_use INPUT must never be replayed as if it landed.
        // A failed Edit also has NO `toolUseResult` echo, so its id is absent from
        // `ids_with_result` - without this set the input-side fallback below would apply the
        // ghost edit. Captured per-turn; the result block sits in the same turn as its call.
        let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &i in idxs {
            let rec = &records[i].1;
            collect_tool_use_paths(rec.blocks(), &mut id_to_path);
            let has_structured_result = rec.tool_use_result.is_some();
            if let Some(blocks) = rec.blocks() {
                for b in blocks {
                    if let Block::ToolResult {
                        tool_use_id: Some(id),
                        is_error,
                        ..
                    } = b
                    {
                        if has_structured_result {
                            ids_with_result.insert(id.clone());
                        }
                        if *is_error == Some(true) {
                            failed_ids.insert(id.clone());
                        }
                    }
                }
            }
        }

        for &i in idxs {
            let (line_no, rec) = (&records[i].0, &records[i].1);
            extract_from_record(
                *line_no,
                turn_index,
                rec,
                target_file,
                &id_to_path,
                &mut events,
            );
            // Carrier-less ops (subagent/workflow): reconstruct content from the tool_use
            // INPUT. Gated on `ids_with_result` so it NEVER double-emits in a top-level
            // session (whose carriers always carry `toolUseResult`) - main-session
            // reconstruction stays byte-identical - and on `failed_ids` so a failed Edit's
            // input is never applied as a phantom mutation.
            extract_input_fallback(
                *line_no,
                turn_index,
                rec,
                target_file,
                &ids_with_result,
                &failed_ids,
                &mut events,
            );
        }
    }

    events
}
