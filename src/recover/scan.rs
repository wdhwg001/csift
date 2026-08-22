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
    // Scope accounting is per TRANSCRIPT (target-independent): collect once, share.
    let opaque = collect_opaque_commands(&session_id, &records, &turns);

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
                    opaque: opaque.clone(),
                    merged_line_origin: std::collections::BTreeMap::new(),
                    skipped_lines: skipped,
                },
            ));
        }
    }
    Ok(out)
}

/// The winning candidate of [`reconstruct_best`], with the per-window accounting the
/// batch report discloses beside every recovered file.
pub(crate) struct BestReconstruction {
    pub(crate) content: String,
    pub(crate) known: usize,
    pub(crate) total: usize,
    /// Integrity boundaries in the winning group's replay.
    pub(crate) boundaries: usize,
    /// Parsed bash mutations OF THIS FILE (disclosed as boundaries above).
    pub(crate) bash_file: usize,
    /// Opaque commands in the winning group's window (class markers + PowerShell).
    pub(crate) bash_opaque: usize,
}

/// Reconstruct a target's FINAL content (or its `--at`/window snapshot) as RAW bytes - the
/// restorable file, not the line-numbered diff view. Cross-session writes are merged per
/// top-level group; when unrelated sessions each hold a version, the FRESHEST (latest-write)
/// candidate wins. `None` when nothing is recoverable. A partial reconstruction
/// (`known < total`) joins the known lines in order.
pub(crate) fn reconstruct_best(
    scans: Vec<ScanResult>,
    when: &str,
) -> Result<Option<BestReconstruction>> {
    let merged = merge_groups_for_reconstruction(scans);
    let mut best: Option<(BestReconstruction, Option<String>)> = None;
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
            Some((b, bts)) => (&latest_ts, known.len()) > (bts, b.known),
        };
        if fresher {
            best = Some((
                BestReconstruction {
                    content,
                    known: known.len(),
                    total,
                    boundaries: rep.boundaries.len(),
                    bash_file: rep.counts.bash,
                    bash_opaque: s.opaque.len(),
                },
                latest_ts,
            ));
        }
    }
    Ok(best.map(|(b, _)| b))
}

/// Write `recovery-report.tsv` under `out_dir` and print the one-line summary. The
/// three accounting columns disclose what each recovered file's window held beyond
/// the replay: `boundaries` (integrity boundaries), `bash_file` (parsed bash
/// mutations of the file), `bash_opaque` (mutating-class + PowerShell commands whose
/// file set is unknowable). A `complete` row with non-zero columns is complete FROM
/// THE TOOL STREAM, not verified against disk.
pub(crate) fn write_batch_report(out_dir: &Path, outcomes: &[BatchOutcome]) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let mut body = String::from(
        "status\tknown_lines\ttotal_lines\tboundaries\tbash_file\tbash_opaque\ttarget\twritten_to\n",
    );
    let (mut complete, mut partial, mut none, mut skipped) = (0usize, 0usize, 0usize, 0usize);
    let mut flagged = 0usize;
    for o in outcomes {
        match o.status {
            "complete" => complete += 1,
            "partial" => partial += 1,
            "no-history" => none += 1,
            "skipped-exists" => skipped += 1,
            _ => {}
        }
        if o.boundaries + o.bash_file + o.bash_opaque > 0 {
            flagged += 1;
        }
        body.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            o.status,
            o.known,
            o.total,
            o.boundaries,
            o.bash_file,
            o.bash_opaque,
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
    if flagged > 0 {
        println!(
            "{flagged} file(s) had integrity boundaries or unparsed mutating bash in their \
             window (boundaries/bash_file/bash_opaque columns): complete means complete from \
             the tool stream, not verified against disk"
        );
    }
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
            opaque: Vec::new(),
            merged_line_origin: std::collections::BTreeMap::new(),
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
                opaque: Vec::new(),
                merged_line_origin: std::collections::BTreeMap::new(),
                skipped_lines: 0,
            });
        }
    }

    // Parse all recover-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool),
    // preserving each record's exact 1-based line number (counts EVERY visited line, 1:1 with
    // jsonl) - a single giant transcript is no longer scanned on one core.
    let (records, skipped) =
        crate::parse::parse_candidates_parallel(bytes, line_is_recover_candidate);

    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices_deduped(&recs, |r| *r);
    let events = extract_with_turns(&records, &turns, target_file);
    let opaque = collect_opaque_commands(&session_id, &records, &turns);
    Ok(ScanResult {
        session_id,
        is_subagent,
        parent_session_id,
        events,
        opaque,
        merged_line_origin: std::collections::BTreeMap::new(),
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
    static NEEDLES: std::sync::LazyLock<[memmem::Finder<'static>; 11]> =
        std::sync::LazyLock::new(|| {
            [
                memmem::Finder::new(b"toolUseResult"),
                memmem::Finder::new(b"Edit"),
                memmem::Finder::new(b"Write"),
                memmem::Finder::new(b"Read"),
                memmem::Finder::new(b"Bash"),
                // The opaque accounting counts PowerShell tool calls; an assistant
                // record carrying one matches no other needle, so without this the
                // P count silently missed the assistant-side records.
                memmem::Finder::new(b"PowerShell"),
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

/// Collect the SCOPE-ACCOUNTING commands of one transcript: every mutating-CLASS
/// marker a Bash command yields (`fmt:cargo`, `interp:python`, `pkg:npm`,
/// `extract:tar`, `git:<sub>` - commands that mutate files they never name), plus
/// every `PowerShell` tool call (its command text is never lexically parsed, so any
/// file it touched is invisible; see AGENTS 3.9). Target-independent by nature: these
/// commands CANNOT be joined to a `--file`, which is exactly why they are counted and
/// disclosed per window instead of silently ignored.
pub(crate) fn collect_opaque_commands(
    session_id: &str,
    records: &[(usize, Record)],
    turns: &[Vec<usize>],
) -> Vec<OpaqueCommand> {
    let mut out: Vec<OpaqueCommand> = Vec::new();
    for (turn_index, idxs) in turns.iter().enumerate() {
        for &i in idxs {
            let (line_no, rec) = (records[i].0, &records[i].1);
            let Some(blocks) = rec.blocks() else { continue };
            for b in blocks {
                let Block::ToolUse {
                    name: Some(name),
                    input: Some(input),
                    ..
                } = b
                else {
                    continue;
                };
                let cmd = input.get("command").and_then(serde_json::Value::as_str);
                if name == "Bash" {
                    let Some(cmd) = cmd else { continue };
                    for bm in crate::bash_mutations::parse_bash_mutations(cmd) {
                        // `git:add`/`git:commit` mutate the index and refs, never
                        // tracked WORKTREE content, so they cannot rewrite the file
                        // being reconstructed; every other marker class can.
                        if crate::bash_mutations::is_class_marker(&bm.path)
                            && !matches!(bm.path.as_str(), "git:add" | "git:commit")
                        {
                            out.push(OpaqueCommand {
                                session_id: session_id.to_string(),
                                line_no,
                                turn_index,
                                timestamp_utc: rec.timestamp.clone(),
                                marker: bm.path,
                            });
                        }
                    }
                } else if name == "PowerShell" && cmd.is_some() {
                    out.push(OpaqueCommand {
                        session_id: session_id.to_string(),
                        line_no,
                        turn_index,
                        timestamp_utc: rec.timestamp.clone(),
                        marker: "powershell".to_string(),
                    });
                }
            }
        }
    }
    out
}

/// Extract `--file` events given PRE-COMPUTED turn groups. Batch reconstruction groups each
/// transcript ONCE and calls this per target, so a transcript mentioning many manifest files
/// is grouped a single time rather than once per file.
///
/// Turn delimiting keys on the shared boundary predicate (SPEC 6.4) so file-event
/// attribution lines up with `verbatim`/`search`. Intent<->result is joined by
/// `tool_use_id` WITHIN a turn (never by adjacency - an integrity error can precede its
/// own tool_use line): per turn, `tool_use_id -> file_path` maps let an integrity-error
/// carrier with no inline path be attributed to `--file`.
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
