//! `files` subcommand — which files/dirs a session modified, and when.
//!
//! Extracts file mutations from a session's transcript (spanning subagents by
//! default), attributes each to its genuine-user turn (the same §6.4 delimiter
//! `search` uses, via [`crate::model::group_turn_indices`]), then aggregates at the
//! requested detail level (summary / by-dir / by-file / timeline) into text or JSON.
//!
//! ## Extraction split (AUTHORITATIVE vs HEURISTIC)
//!
//! - **Authoritative** — `Write`/`Edit`/`MultiEdit` (`input.file_path`) +
//!   `NotebookEdit` (`input.notebook_path`). create-vs-edit is resolved by JOINING the
//!   structured tool_use to its paired tool_result carrier
//!   (`toolUseResult.type == "create"`) by `tool_use_id` within the turn (see
//!   [`crate::model::Record::carrier_create_paths`]).
//! - **Heuristic** — Bash file mutations, parsed lexically from `input.command` by
//!   [`crate::bash_mutations`] (Bash carries no path field in its result). These are
//!   ALWAYS labelled `(heuristic)` and their `is_create` is itself a heuristic guess.
//!
//! ## Performance shape (the 200 MB+ contract)
//!
//! Like `search`, `files` does a SINGLE forward pass per file (mmap, SIMD newline
//! scan, a pre-JSON mutation byte-prefilter), with full `serde_json` parse only on
//! candidate lines. It must NOT retain large blobs — it extracts a few small owned
//! strings per mutation ([`crate::model::FileMutation`]) and drops the record, never
//! holding `originalFile`/`content`/`structuredPatch` bodies from `toolUseResult`.
//!
//! Per-file fan-out uses the default `rayon` pool, which sizes to
//! `std::thread::available_parallelism()` (= CPU count) — the same pool `search` and
//! `agents` use. No explicit `available_parallelism()` call is added: rayon already
//! consults it implicitly, so an explicit call would be dead code (stated here so a
//! future reader does not "fix" it by adding one).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobMatcher};
use memchr::memmem;
use rayon::prelude::*;
use regex::Regex;

use crate::bash_mutations::parse_bash_mutations;
use crate::cli::{FilesArgs, FilesDetail, OutputFormat};
use crate::model::{group_turn_indices_deduped, FileMutation, FileOp, Record};
use crate::parse::mmap_bytes;
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// One extracted mutation tagged with the turn it belongs to + its owning session.
#[derive(Debug, Clone)]
struct TaggedMutation {
    /// The transcript's own id: a top-level session uuid, OR a bare SUBAGENT hex when the
    /// mutation came from a subagent transcript. A subagent hex is NOT a re-feedable `@<uuid>`
    /// target; use `parent_session_id` to re-feed. `is_subagent` discriminates the id-domain.
    session_id: String,
    /// True when this mutation came from a subagent transcript (so `session_id` is a bare
    /// hex, not a re-feedable uuid). Defaults false; set per-file in `scan_one_file`.
    is_subagent: bool,
    /// The re-feedable PARENT session uuid (the owning top-level session). Equals
    /// `session_id` for a top-level mutation. Defaults to `session_id`; set in `scan_one_file`.
    parent_session_id: String,
    turn_index: usize,
    /// The JSONL physical line number of the mutating record (1-based), so a `files` row joins
    /// back to the raw transcript exactly like `recover`/`search`/`turns` do.
    line_no: usize,
    mutation: FileMutation,
}

/// An Edit-before-Read boundary `files` detected: the file changed OUTSIDE the Read/Write/Edit
/// stream (a formatter, husky/pre-commit, git, an external editor) and the harness rejected an
/// Edit/Write with `File has been modified since read`, forcing a fresh Read. Attributed to its
/// file via the failed op's `tool_use_id` ↔ that op's `file_path` join, carrying the jsonl line.
#[derive(Debug, Clone)]
struct TaggedBoundary {
    session_id: String,
    is_subagent: bool,
    parent_session_id: String,
    path: String,
    line_no: usize,
    turn_index: usize,
    kind: &'static str,
    timestamp_utc: Option<String>,
}

/// Per-file scan result before global aggregation.
struct FileResult {
    mutations: Vec<TaggedMutation>,
    boundaries: Vec<TaggedBoundary>,
    skipped_lines: usize,
    /// This transcript's genuine-user turn count — so a `--turn-range` spec resolves its
    /// open/from-end forms (`N..`, `-3..`) against THIS file's turns, not a global count.
    turn_count: usize,
}

/// The compiled `--regex` / `--glob` path predicates. Both are OPTIONAL and ANDed: a path is
/// kept iff it satisfies EVERY supplied filter, tested against the FULL absolute path string.
/// Applied to mutations AND Edit-before-Read boundaries BEFORE the `--by` rollup, so all views
/// reflect the filtered set. With neither supplied, [`Self::keeps`] keeps everything.
struct PathFilter {
    /// `--regex <RE>`: keep iff the pattern matches ANYWHERE in the full path (used as-is).
    regex: Option<Regex>,
    /// `--glob <PAT>`: keep iff the glob matches the full path (`**` crosses `/`).
    glob: Option<GlobMatcher>,
}

impl PathFilter {
    /// Compile the optional `--regex` / `--glob` patterns. An invalid pattern is a HARD error
    /// (named in the message), surfaced before any scan so the failure is fast.
    fn from_args(regex: Option<&str>, glob: Option<&str>) -> Result<Self> {
        let regex = regex
            .map(|re| Regex::new(re).with_context(|| format!("invalid --regex pattern: {re}")))
            .transpose()?;
        let glob = glob
            .map(|pat| {
                Glob::new(pat)
                    .map(|g| g.compile_matcher())
                    .with_context(|| format!("invalid --glob pattern: {pat}"))
            })
            .transpose()?;
        Ok(Self { regex, glob })
    }

    /// Whether `path` survives every supplied filter (vacuously true when none was supplied).
    fn keeps(&self, path: &str) -> bool {
        if let Some(re) = &self.regex {
            if !re.is_match(path) {
                return false;
            }
        }
        if let Some(g) = &self.glob {
            if !g.is_match(path) {
                return false;
            }
        }
        true
    }
}

/// Entry point for `csift files`.
pub fn run_files(args: &FilesArgs) -> Result<()> {
    // `--turn-range` and `--since`/`--until` INTERSECT (AND) — the one windowing rule every
    // command shares (the former mutual-exclusion bail was a leftover; search/recover/stats
    // already intersected).
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // ── Compile the optional path filters BEFORE any scan, so an invalid --regex/--glob
    //    fails fast (a hard error) rather than after a full pass ──
    let path_filter = PathFilter::from_args(args.regex.as_deref(), args.glob.as_deref())?;

    // ── Resolve targets → session files (subagent span per --no-subagents; default spans
    //    subagents, matching every other default-on command) ──
    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        path::SubagentScope::from(args.want_subagents()),
        path::Caller::Files,
    )?;

    // SCOPE span of the resolved set (every transcript, incl. mutation-free subagents) so the
    // fan-out is announced from the true file set, not just the mutation-bearing subset.
    let scope_sub = session_files
        .iter()
        .filter(|p| crate::subagent::is_subagent_path(p))
        .count();
    let scope_top = session_files.len() - scope_sub;

    // ── Parallel scan across files (default rayon pool = CPU count) ──
    let per_file: Vec<FileResult> = session_files
        .par_iter()
        .map(|p| scan_one_file(p))
        .collect::<Result<Vec<_>>>()?;

    // ── Merge + filter by turn-range / time-window per mutation + boundary ──
    let mut mutations: Vec<TaggedMutation> = Vec::new();
    let mut boundaries: Vec<TaggedBoundary> = Vec::new();
    let mut skipped_lines = 0usize;
    for fr in per_file {
        skipped_lines += fr.skipped_lines;
        // Resolve the `--turn-range` spec against THIS file's turn count (0-based), so
        // open/from-end forms (`N..`, `-3..` = the last 3) window each transcript's own turns.
        let turn_bounds = turn_range.map(|spec| spec.resolve(fr.turn_count, false));
        for tm in fr.mutations {
            if let Some((lo, hi)) = turn_bounds {
                if tm.turn_index < lo || tm.turn_index > hi {
                    continue;
                }
            }
            // A mutation with no timestamp never falls inside a BOUNDED window
            // (same rule as search/agents); an unbounded window admits it.
            if !time_window.contains(tm.mutation.timestamp_utc.as_deref()) {
                continue;
            }
            // --regex / --glob: keep only paths satisfying EVERY supplied filter, over the
            // FULL path, BEFORE the rollup so all --by views reflect the filtered set.
            if !path_filter.keeps(&tm.mutation.path) {
                continue;
            }
            mutations.push(tm);
        }
        // Boundaries obey the SAME turn-range / time-window / path filters as mutations.
        for tb in fr.boundaries {
            if let Some((lo, hi)) = turn_bounds {
                if tb.turn_index < lo || tb.turn_index > hi {
                    continue;
                }
            }
            if !time_window.contains(tb.timestamp_utc.as_deref()) {
                continue;
            }
            if !path_filter.keeps(&tb.path) {
                continue;
            }
            boundaries.push(tb);
        }
    }
    boundaries.sort_by(|a, b| a.line_no.cmp(&b.line_no));

    let outcome = Outcome {
        detail: args.detail(),
        mutations,
        boundaries,
        skipped_lines,
        turn_range: args.turn_range.clone(),
        time_window_bounded: !time_window.is_unbounded(),
        scope_top,
        scope_sub,
    };

    match args.format {
        OutputFormat::Text => render_text(&outcome),
        OutputFormat::Json => render_json(&outcome)?,
    }
    Ok(())
}

/// Scan one session file: mmap → prefilter → parse → delimit turns → extract + join.
fn scan_one_file(path: &Path) -> Result<FileResult> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(FileResult {
            mutations: Vec::new(),
            boundaries: Vec::new(),
            skipped_lines: 0,
            turn_count: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // The session id is the jsonl basename, but for a SUBAGENT transcript the on-disk
    // stem is `agent-<hex>` whereas the canonical id (the record `agentId`, and what the
    // `agents` topology prints) is the BARE hex. The shared helper strips the prefix so a
    // `files` subagent row's `session_id` is joinable to `agents` (id-form unification).
    let session_id = crate::subagent::session_id_from_path(path);

    // Retain transcript records in file order. The mutation prefilter gates the parse
    // on raw bytes (pre-JSON): a line is kept only if it could carry a mutation
    // (`Edit`/`Write`/`NotebookEdit`/`MultiEdit`/`Bash`/`filePath`) OR it is a genuine-
    // user delimiter (`"role":"user"`, needed so turns can still be delimited). Skipped
    // malformed lines are counted, never hidden.
    // Parse all files-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool) so a
    // single giant transcript is not scanned on one core. KEEP the parallel scan's exact jsonl
    // line numbers (aligned with `records` by index) — every `files` row + Edit-before-Read
    // boundary carries its `Lnnnn` so it joins back to the raw transcript like recover/search.
    let (recs, skipped) = crate::parse::parse_candidates_parallel(bytes, line_is_files_candidate);
    let line_nos: Vec<usize> = recs.iter().map(|(ln, _)| *ln).collect();
    let records: Vec<Record> = recs.into_iter().map(|(_, rec)| rec).collect();

    // A subagent transcript's `session_id` is a non-re-feedable bare hex; stamp the
    // id-domain discriminator + the re-feedable parent uuid (the dir before `subagents/`)
    // onto every mutation, so the timeline JSON can distinguish a parent UUID from a
    // subagent transcript hex. A top-level file is its own parent.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());
    let mut mutations = extract_mutations(&session_id, &records, &line_nos);
    let mut boundaries = extract_boundaries(&session_id, &records, &line_nos);
    if is_subagent {
        for tm in &mut mutations {
            tm.is_subagent = true;
            tm.parent_session_id = parent_session_id.clone();
        }
        for tb in &mut boundaries {
            tb.is_subagent = true;
            tb.parent_session_id = parent_session_id.clone();
        }
    }
    // Per-file turn count for resolving `--turn-range` open/from-end forms (same grouping
    // `extract_mutations`/`extract_boundaries` used to assign each `turn_index`).
    let turn_count = group_turn_indices_deduped(&records, |r| r).len();
    Ok(FileResult {
        mutations,
        boundaries,
        skipped_lines: skipped,
        turn_count,
    })
}

/// Pre-JSON byte prefilter: keep a line if it could carry a file mutation OR is a
/// genuine-user delimiter (so turns can still be delimited even when a turn opens with
/// no mutation in it). Broad-by-design (substring, not structural) so no mutation is
/// lost. Like `search`'s prefilter, this only gates the parse.
fn line_is_files_candidate(line: &[u8]) -> bool {
    memmem::find(line, br#""role":"user""#).is_some()
        || memmem::find(line, b"Edit").is_some()
        || memmem::find(line, b"Write").is_some()
        || memmem::find(line, b"Bash").is_some()
        || memmem::find(line, b"filePath").is_some()
        // Keep tool_result ERROR carriers — they carry the Edit-before-Read boundaries (and
        // drive `failed_ids`, so a cancelled/errored op is never miscounted as a real mutation),
        // and an error carrier may not otherwise match (its `"role":"user"` is its only hook).
        || memmem::find(line, b"is_error").is_some()
}

/// Extract the bare file mutations carried by a record slice — the SAME structured +
/// carrier-join + Bash-heuristic logic [`extract_mutations`] uses, but WITHOUT turn
/// tagging (no session id, no turn index). Reused by the subagent topology to compute a
/// node's files-changed over its own transcript ([`crate::subagent::build_topology`]),
/// so the two surfaces never diverge on what counts as a mutation. Carriers are joined
/// over the whole slice (a subagent transcript is one logical scope).
#[must_use]
pub fn mutations_in_records(records: &[Record]) -> Vec<FileMutation> {
    // Build the carrier join map once over the whole slice: tool_use_id → (filePath,
    // is_create). A subagent transcript is a single scope, so a global join is correct.
    let mut carriers: BTreeMap<String, (String, bool)> = BTreeMap::new();
    // tool_use_ids whose RESULT errored / was cancelled (`is_error:true`) — those ops never
    // landed, so they are not real mutations (mirrors `extract_mutations` + `recover::extract`).
    let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rec in records {
        for (id, file_path, is_create) in rec.carrier_create_paths() {
            carriers.insert(id, (file_path, is_create));
        }
        if let Some(blocks) = rec.blocks() {
            for b in blocks {
                if let crate::model::Block::ToolResult {
                    tool_use_id: Some(id),
                    is_error: Some(true),
                    ..
                } = b
                {
                    failed_ids.insert(id.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    for rec in records {
        // A record whose tool op errored / was cancelled mutated nothing — skip it.
        if tool_use_id_for(rec).is_some_and(|id| failed_ids.contains(&id)) {
            continue;
        }
        for mut m in rec.structured_tool_mutations() {
            if let Some(id) = tool_use_id_for(rec) {
                if let Some((carrier_path, is_create)) = carriers.get(&id) {
                    m.is_create = *is_create;
                    if m.path.is_empty() {
                        m.path = carrier_path.clone();
                    }
                }
            }
            if m.path.is_empty() {
                continue;
            }
            out.push(m);
        }
        if let Some(cmd) = rec.bash_command() {
            for bm in parse_bash_mutations(cmd) {
                out.push(FileMutation {
                    path: bm.path,
                    op: FileOp::BashMutation,
                    timestamp_utc: rec.timestamp.clone(),
                    is_create: bash_verb_is_create(bm.verb),
                });
            }
        }
    }
    out
}

/// Heuristic create-vs-touch guess for a Bash mutation verb. A verb that names a fresh
/// output target (`>` truncate, `mkdir`/`touch`/`tee`/`cp`/`mv`/`install`/`ln`/`rsync`
/// dest, a download to a path, a `dd`/`zip`/`tar`-create/flag-specified output) is treated
/// as a create; an append (`>>`, `tee-a`), `rm`, `sed -i`, `mv-from`, and `git` are NOT.
/// (`emit_tar` only emits on a `-c`/`--create` flag, so the `tar` verb is unconditionally a
/// create; `tee-a` is `tee --append`, the non-truncating sibling of `tee`, mirroring `>>`
/// vs `>`.) Lexical-only, so it is just a heuristic (its `FileOp::BashMutation`
/// is_heuristic() gates the label everywhere).
fn bash_verb_is_create(verb: &str) -> bool {
    matches!(
        verb,
        "mkdir"
            | "touch"
            | "tee"
            | ">"
            | "cp"
            | "mv"
            | "install"
            | "ln"
            | "rsync"
            | "curl"
            | "wget"
            | "dd"
            | "zip"
            | "tar"
            | "flag-output"
    )
}

/// Delimit turns over the parsed records, then for each turn extract structured + Bash
/// mutations and JOIN the structured ones to their carriers for accurate `is_create`.
fn extract_mutations(
    session_id: &str,
    records: &[Record],
    line_nos: &[usize],
) -> Vec<TaggedMutation> {
    let index_turns = group_turn_indices_deduped(records, |r| r);
    let mut out = Vec::new();

    for (turn_index, idxs) in index_turns.iter().enumerate() {
        // Build the carrier join map for this turn: tool_use_id → (filePath, is_create).
        let mut carriers: BTreeMap<String, (String, bool)> = BTreeMap::new();
        // tool_use_ids whose RESULT was an error (`is_error:true`) — a failed Edit/Write, or a
        // Write `Cancelled: parallel tool call … errored` when a sibling op in the same batch
        // failed. The op NEVER landed, so it must NOT be counted as a real mutation: a `files`
        // `write:1` on a cancelled Write contradicts `recover` (which correctly finds no
        // history) and is a forensic FALSE POSITIVE ("did this session write X?"). Same
        // `failed_ids` gate `recover::extract` already applies; computed per turn (the result
        // block sits in the same turn as its call).
        let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &i in idxs {
            for (id, file_path, is_create) in records[i].carrier_create_paths() {
                carriers.insert(id, (file_path, is_create));
            }
            if let Some(blocks) = records[i].blocks() {
                for b in blocks {
                    if let crate::model::Block::ToolResult {
                        tool_use_id: Some(id),
                        is_error: Some(true),
                        ..
                    } = b
                    {
                        failed_ids.insert(id.clone());
                    }
                }
            }
        }

        for &i in idxs {
            let rec = &records[i];

            // A record whose tool op errored / was cancelled never mutated anything — skip its
            // structured AND heuristic-Bash mutations (the op's INPUT is a phantom, not a write).
            if tool_use_id_for(rec).is_some_and(|id| failed_ids.contains(&id)) {
                continue;
            }

            // Structured (authoritative) mutations, enriched from the carrier join.
            for mut m in rec.structured_tool_mutations() {
                // The carrier join keys on the tool_use's own id; find it on this record.
                if let Some(id) = tool_use_id_for(rec) {
                    if let Some((carrier_path, is_create)) = carriers.get(&id) {
                        m.is_create = *is_create;
                        if m.path.is_empty() {
                            m.path = carrier_path.clone();
                        }
                    }
                }
                if m.path.is_empty() {
                    continue;
                }
                out.push(TaggedMutation {
                    session_id: session_id.to_string(),
                    // is_subagent / parent default here; `scan_one_file` stamps the real
                    // per-file values once (the path-derived discriminator lives there).
                    is_subagent: false,
                    parent_session_id: session_id.to_string(),
                    turn_index,
                    line_no: line_nos.get(i).copied().unwrap_or(0),
                    mutation: m,
                });
            }

            // Bash (heuristic) mutations.
            if let Some(cmd) = rec.bash_command() {
                for bm in parse_bash_mutations(cmd) {
                    out.push(TaggedMutation {
                        session_id: session_id.to_string(),
                        is_subagent: false,
                        parent_session_id: session_id.to_string(),
                        turn_index,
                        line_no: line_nos.get(i).copied().unwrap_or(0),
                        mutation: FileMutation {
                            path: bm.path,
                            op: FileOp::BashMutation,
                            timestamp_utc: rec.timestamp.clone(),
                            // Bash create-vs-overwrite is NOT knowable lexically — this is
                            // a heuristic flag; the op's is_heuristic() gates the label.
                            is_create: bash_verb_is_create(bm.verb),
                        },
                    });
                }
            }
        }
    }
    out
}

/// True when a `tool_result` body is the `File has been modified since read` harness error
/// (the file changed OUTSIDE the tool stream — prettier/linter/git/etc. — and a fresh Read is
/// demanded). Mirrors `recover::classify_integrity_error`'s `ModifiedSinceRead` arm; kept local
/// so `files` doesn't depend on `recover`'s internals.
fn is_modified_since_read(content: &serde_json::Value) -> bool {
    let text = crate::model::tool_result_content_text(content);
    text.contains("has been modified since read") || text.contains("File has been modified")
}

/// Extract the Edit-before-Read boundaries a session hit on each file: an Edit/Write rejected
/// with `File has been modified since read` (the file changed outside the Read/Write/Edit
/// stream). Attribution: the error `tool_result`'s `tool_use_id` matches the rejected op, whose
/// `file_path` lives on its tool_use record (even though the op never landed) — so a per-turn
/// `id → path` map (built from EVERY edit/write tool_use, failed or not) names the file. The
/// jsonl line is taken from `line_nos` (aligned with `records` by index).
fn extract_boundaries(
    session_id: &str,
    records: &[Record],
    line_nos: &[usize],
) -> Vec<TaggedBoundary> {
    let index_turns = group_turn_indices_deduped(records, |r| r);
    let mut out = Vec::new();
    for (turn_index, idxs) in index_turns.iter().enumerate() {
        // id → file_path for every Edit/Write tool_use in this turn (incl. failed ones — the
        // rejected edit's INPUT still carries its file_path).
        let mut tool_use_path: BTreeMap<String, String> = BTreeMap::new();
        for &i in idxs {
            if let Some(id) = tool_use_id_for(&records[i]) {
                if let Some(m) = records[i]
                    .structured_tool_mutations()
                    .into_iter()
                    .find(|m| !m.path.is_empty())
                {
                    tool_use_path.entry(id).or_insert(m.path);
                }
            }
        }
        for &i in idxs {
            let Some(blocks) = records[i].blocks() else {
                continue;
            };
            for b in blocks {
                if let crate::model::Block::ToolResult {
                    tool_use_id: Some(id),
                    is_error: Some(true),
                    content: Some(content),
                } = b
                {
                    if is_modified_since_read(content) {
                        if let Some(path) = tool_use_path.get(id) {
                            out.push(TaggedBoundary {
                                session_id: session_id.to_string(),
                                is_subagent: false,
                                parent_session_id: session_id.to_string(),
                                path: path.clone(),
                                line_no: line_nos.get(i).copied().unwrap_or(0),
                                turn_index,
                                kind: "modified_since_read",
                                timestamp_utc: records[i].timestamp.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// The first tool_use block's `id` on this record (the structured-mutation join key).
/// A record's structured mutations all come from its tool_use blocks; in real data a
/// single assistant record carries one file-mutating tool_use, so the first id is the
/// join key. Returns `None` when there is no tool_use id.
fn tool_use_id_for(rec: &Record) -> Option<String> {
    let blocks = rec.blocks()?;
    for block in blocks {
        if let crate::model::Block::ToolUse { id: Some(id), .. } = block {
            return Some(id.clone());
        }
    }
    None
}

/// The merged + filtered result, ready to render.
struct Outcome {
    detail: FilesDetail,
    mutations: Vec<TaggedMutation>,
    /// Edit-before-Read boundaries (file changed outside the tool stream), sorted by jsonl line.
    boundaries: Vec<TaggedBoundary>,
    skipped_lines: usize,
    /// The raw `--turn-range` token, kept verbatim for the footer (the range resolves
    /// per-file, so there is no single global `(lo, hi)` to display).
    turn_range: Option<String>,
    time_window_bounded: bool,
    /// SCOPE-span counts of the RESOLVED transcript set (top-level + subagent files),
    /// computed from `resolve_session_files` BEFORE the mutation scan — so a subagent
    /// transcript with zero mutations still counts toward the announced fan-out. Drives the
    /// shared SCOPE banner / JSON `session_header`, suppressed when `scope_sub == 0`.
    scope_top: usize,
    scope_sub: usize,
}

impl Outcome {
    /// Distinct file paths touched (across all mutations).
    fn distinct_files(&self) -> usize {
        let mut set = std::collections::BTreeSet::new();
        for m in &self.mutations {
            set.insert(m.mutation.path.as_str());
        }
        set.len()
    }
}

// ── Aggregation ──

/// Per-op counts for one bucket/dir/file, plus first/last touch + distinct files.
#[derive(Debug, Clone, Default)]
struct OpCounts {
    write: usize,
    edit: usize,
    notebook_edit: usize,
    multi_edit: usize,
    bash: usize,
    /// Distinct file paths contributing to this group (for dir/bucket rows).
    files: std::collections::BTreeSet<String>,
    first_ts: Option<String>,
    last_ts: Option<String>,
}

impl OpCounts {
    fn add(&mut self, m: &FileMutation) {
        match m.op {
            FileOp::Write => self.write += 1,
            FileOp::Edit => self.edit += 1,
            FileOp::NotebookEdit => self.notebook_edit += 1,
            FileOp::MultiEdit => self.multi_edit += 1,
            FileOp::BashMutation => self.bash += 1,
        }
        self.files.insert(m.path.clone());
        if let Some(ts) = &m.timestamp_utc {
            // Min/max as raw ISO8601 strings (ISO8601 sorts chronologically as text).
            if self.first_ts.as_deref().is_none_or(|f| ts.as_str() < f) {
                self.first_ts = Some(ts.clone());
            }
            if self.last_ts.as_deref().is_none_or(|l| ts.as_str() > l) {
                self.last_ts = Some(ts.clone());
            }
        }
    }

    fn total(&self) -> usize {
        self.write + self.edit + self.notebook_edit + self.multi_edit + self.bash
    }

    /// The op-count fragment as `"N write, N edit, …"`, omitting zero counts; Bash is
    /// suffixed `(heuristic)`. Empty groups render `"0"`.
    fn ops_label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.write > 0 {
            parts.push(format!("{} write", self.write));
        }
        if self.edit > 0 {
            parts.push(format!("{} edit", self.edit));
        }
        if self.notebook_edit > 0 {
            parts.push(format!("{} notebook-edit", self.notebook_edit));
        }
        if self.multi_edit > 0 {
            parts.push(format!("{} multi-edit", self.multi_edit));
        }
        if self.bash > 0 {
            parts.push(format!("{} bash (heuristic)", self.bash));
        }
        if parts.is_empty() {
            "0".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// How many leading path SEGMENTS the `--summary` rollup keeps. Depth 4 keeps an absolute
/// path's project-root level distinct (`/Users/testuser/Projects/widget_app_prototype`)
/// while COLLAPSING everything deeper into that one bucket — so `--summary` is a genuine
/// coarse rollup, strictly smaller than `--by-dir` (which keys on the full parent dir). A
/// shallower path (e.g. `/tmp/x`) keeps all the segments it has.
const SUMMARY_BUCKET_SEGMENTS: usize = 4;

/// The `--summary` rollup BUCKET key for a path: a COARSE top-level prefix (the first
/// [`SUMMARY_BUCKET_SEGMENTS`] path segments), NOT the full parent dir. This is what makes
/// `--summary` the smallest output and a real rollup — distinct from `--by-dir`, which keys
/// on the full parent. Examples (depth 4): `/Users/testuser/Projects/p/spec/gaps.md` and
/// `/Users/testuser/Projects/p/src/main.rs` BOTH bucket to `/Users/testuser/Projects/p`;
/// `/tmp/x.md` → `/tmp`. A `git:<sub>` pseudo-path keeps its own `git:` bucket (it is not a
/// real file path). A bare relative filename (no `/`) buckets under `./`.
fn bucket_key(path: &str) -> String {
    // The intentional `git:<sub>` coarse pseudo-path is its own bucket, never split as a dir
    // (all `git:add`/`git:commit`/… roll up under one `git:` row, out of the `./` sink).
    if path.starts_with("git:") {
        return "git:".to_string();
    }
    // Roll up the PARENT directory (never the basename) to at most SUMMARY_BUCKET_SEGMENTS
    // segments. A bare relative filename has no parent → the `./` bucket.
    let Some(parent) = parent_dir(path) else {
        return "./".to_string();
    };
    let absolute = parent.starts_with('/');
    let segs: Vec<&str> = parent.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        // Parent is `/` (a top-level file like `/foo`) → the root bucket.
        return "/".to_string();
    }
    let take = segs.len().min(SUMMARY_BUCKET_SEGMENTS);
    let prefix = segs[..take].join("/");
    if absolute {
        format!("/{prefix}")
    } else {
        prefix
    }
}

/// The parent directory of a path string (lexical only — never touches the
/// filesystem). Returns `None` for a bare filename with no `/`.
fn parent_dir(path: &str) -> Option<String> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    match trimmed.rfind('/') {
        Some(0) => Some("/".to_string()), // a top-level path like `/foo`
        Some(idx) => Some(trimmed[..idx].to_string()),
        None => None,
    }
}

/// Group mutations by a key function into a deterministic (BTreeMap-sorted) map.
fn group_by<F: Fn(&FileMutation) -> String>(
    mutations: &[TaggedMutation],
    key: F,
) -> BTreeMap<String, OpCounts> {
    let mut map: BTreeMap<String, OpCounts> = BTreeMap::new();
    for m in mutations {
        map.entry(key(&m.mutation)).or_default().add(&m.mutation);
    }
    map
}

// ── Rendering ──

fn render_text(outcome: &Outcome) {
    // SCOPE banner FIRST (before the empty check) so a fan-out that touched no files still
    // announces it spanned N subagents — the same up-front disclosure `list`/`turns` give.
    crate::text::emit_scope_banner(outcome.scope_top, outcome.scope_sub);
    if outcome.mutations.is_empty() && outcome.boundaries.is_empty() {
        println!("no file mutations found");
        print_footer(outcome);
        return;
    }

    if !outcome.mutations.is_empty() {
        match outcome.detail {
            FilesDetail::Summary => render_summary(outcome),
            FilesDetail::ByDir => render_by_dir(outcome),
            FilesDetail::ByFile => render_by_file(outcome),
            FilesDetail::Timeline => render_timeline(outcome),
        }
    }
    render_boundaries_section(outcome);
    print_footer(outcome);
}

/// The Edit-before-Read boundary section — orthogonal to the mutation rollup, shown in every
/// detail mode (and on its own when a session ONLY hit boundaries, no mutations). Each row
/// carries the file, the jsonl line, turn, time, and kind so it joins back to the transcript
/// and feeds `recover --file <path> --coverage` for the precise per-boundary breakdown.
fn render_boundaries_section(outcome: &Outcome) {
    if outcome.boundaries.is_empty() {
        return;
    }
    println!();
    println!(
        "── Edit-before-Read boundaries ({}) — file changed OUTSIDE the tool stream (formatter / \
         git / external edit); recover with care ──",
        outcome.boundaries.len()
    );
    for b in &outcome.boundaries {
        let sub = if b.is_subagent {
            format!(
                "  ·  subagent {} (parent {})",
                b.session_id, b.parent_session_id
            )
        } else {
            String::new()
        };
        println!(
            "  ⚠ {}  ·  L{}  ·  turn {}  ·  {}  ·  {}{sub}",
            b.path,
            b.line_no,
            b.turn_index,
            format_timestamp(b.timestamp_utc.as_deref()),
            b.kind
        );
    }
}

/// Group mutations under their session header, then call `body` per session with that
/// session's mutations. Sessions render in sorted id order for determinism. A SUBAGENT
/// group's header is branded `SUBAGENT <hex> · parent SESSION <uuid>` (mirroring `search`'s
/// header + `turns`' `(subagent transcript)` annotation) so a consumer never reads a bare
/// subagent hex as a re-feedable `@<uuid>` target. All mutations in one group share the
/// same id-domain (same transcript), so the first row's flags brand the whole header.
fn per_session<F: Fn(&str, &[&TaggedMutation])>(outcome: &Outcome, body: F) {
    let mut by_session: BTreeMap<&str, Vec<&TaggedMutation>> = BTreeMap::new();
    for m in &outcome.mutations {
        by_session.entry(m.session_id.as_str()).or_default().push(m);
    }
    let mut first = true;
    for (sid, ms) in by_session {
        if !first {
            println!();
        }
        first = false;
        match ms.first() {
            Some(m) if m.is_subagent => {
                println!("SUBAGENT {sid}  ·  parent SESSION {}", m.parent_session_id);
            }
            _ => println!("SESSION {sid}"),
        }
        body(sid, &ms);
    }
}

fn render_summary(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let buckets = group_by(&owned, |m| bucket_key(&m.path));
        for (bucket, counts) in &buckets {
            println!("  {bucket}: {}", counts.ops_label());
        }
    });
}

fn render_by_dir(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let dirs = group_by(&owned, |m| {
            parent_dir(&m.path).unwrap_or_else(|| "./".to_string())
        });
        for (dir, counts) in &dirs {
            println!("  {dir}");
            println!(
                "    {}  ·  {} file(s)",
                counts.ops_label(),
                counts.files.len()
            );
            println!(
                "    first  {}",
                format_timestamp(counts.first_ts.as_deref())
            );
            println!("    last   {}", format_timestamp(counts.last_ts.as_deref()));
        }
    });
}

fn render_by_file(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let files = group_by(&owned, |m| m.path.clone());
        for (file, counts) in &files {
            println!("  {file}");
            println!("    {}", counts.ops_label());
            println!(
                "    first  {}",
                format_timestamp(counts.first_ts.as_deref())
            );
            println!("    last   {}", format_timestamp(counts.last_ts.as_deref()));
        }
    });
}

fn render_timeline(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        // Sort chronologically by timestamp (None last), then by original file order.
        let mut owned: Vec<&TaggedMutation> = ms.to_vec();
        owned.sort_by(|a, b| {
            timestamp_sort_key(a.mutation.timestamp_utc.as_deref())
                .cmp(&timestamp_sort_key(b.mutation.timestamp_utc.as_deref()))
        });
        for m in owned {
            let heuristic = if m.mutation.op.is_heuristic() {
                " (heuristic)"
            } else {
                ""
            };
            println!(
                "  L{}  {}  turn {}  {}{}  {}",
                m.line_no,
                format_timestamp(m.mutation.timestamp_utc.as_deref()),
                m.turn_index,
                m.mutation.op.label(),
                heuristic,
                m.mutation.path
            );
        }
    });
}

/// Sort key that places timestamp-less mutations LAST (after all timestamped ones) and
/// orders timestamped ones chronologically (ISO8601 sorts as text).
fn timestamp_sort_key(ts: Option<&str>) -> (bool, String) {
    match ts {
        Some(t) => (false, t.to_string()),
        None => (true, String::new()),
    }
}

fn print_footer(outcome: &Outcome) {
    let level = match outcome.detail {
        FilesDetail::Summary => "summary",
        FilesDetail::ByDir => "by-dir",
        FilesDetail::ByFile => "by-file",
        FilesDetail::Timeline => "timeline",
    };
    let filter = filter_context(outcome);
    println!();
    println!(
        "{} distinct file(s)  ·  {} mutation(s)  ·  {} Edit-before-Read boundary(ies)  ·  detail={level}  ·  {filter}",
        outcome.distinct_files(),
        outcome.mutations.len(),
        outcome.boundaries.len()
    );
    println!("(Bash mutations are heuristic — parsed from the command string.)");
    if outcome.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(outcome.skipped_lines));
    }
}

/// A short description of the active turn/time filter for the footer.
fn filter_context(outcome: &Outcome) -> String {
    if let Some(s) = &outcome.turn_range {
        format!("turn-range={s}")
    } else if outcome.time_window_bounded {
        "time-window".to_string()
    } else {
        "all turns".to_string()
    }
}

fn render_json(outcome: &Outcome) -> Result<()> {
    use serde_json::json;
    // envelope v2: header (always) → kind-tagged rows → summary (always).
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "files",
            outcome.scope_top,
            outcome.scope_sub,
            json!({})
        ))?
    );
    match outcome.detail {
        FilesDetail::Summary => json_grouped(outcome, |m| bucket_key(&m.path), "bucket")?,
        FilesDetail::ByDir => json_grouped(
            outcome,
            |m| parent_dir(&m.path).unwrap_or_else(|| "./".to_string()),
            "dir",
        )?,
        FilesDetail::ByFile => json_grouped(outcome, |m| m.path.clone(), "file")?,
        FilesDetail::Timeline => {
            for m in &outcome.mutations {
                let obj = json!({
                    "kind": "mutation",
                    "session_id": m.session_id,
                    // Discriminate the id-domain: `is_subagent` + the always-re-feedable
                    // `parent_session_id` (= session_id for a top-level mutation) so a
                    // consumer can `csift verbatim <parent_session_id>` even on a subagent row.
                    "is_subagent": m.is_subagent,
                    "parent_session_id": m.parent_session_id,
                    "path": m.mutation.path,
                    // UNDERSCORE-delimited op token (json_key, NOT the hyphenated text label)
                    // so the timeline `op` spelling matches the grouped per-op COUNT keys
                    // (`notebook_edit`/`multi_edit`) — one on-wire spelling across both modes.
                    "op": m.mutation.op.json_key(),
                    "ts_utc": m.mutation.timestamp_utc,
                    "ts_local": m.mutation.timestamp_utc.as_deref().and_then(local_iso),
                    "turn_index": m.turn_index,
                    "line": m.line_no,
                    "is_create": m.mutation.is_create,
                    "heuristic": m.mutation.op.is_heuristic(),
                });
                println!("{}", serde_json::to_string(&obj)?);
            }
        }
    }

    // Edit-before-Read boundary objects (orthogonal to the mutation rollup; emitted in every
    // detail mode so the recipe can `jq` them out of any `files --format json` run). Each carries
    // the id-domain discriminators + the jsonl line, so it joins back to the transcript and feeds
    // `recover --file <path> --coverage` for the precise per-boundary breakdown.
    for b in &outcome.boundaries {
        let obj = json!({
            "kind": "boundary",
            "session_id": b.session_id,
            "is_subagent": b.is_subagent,
            "parent_session_id": b.parent_session_id,
            "path": b.path,
            "line": b.line_no,
            "turn_index": b.turn_index,
            // WHAT changed the file out of band (formatter/git/external-editor/…) —
            // named `cause` so `kind` stays the envelope discriminator exclusively.
            "cause": b.kind,
            "ts_utc": b.timestamp_utc,
            "ts_local": b.timestamp_utc.as_deref().and_then(local_iso),
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    // envelope v2 summary. `detail_level` values equal the `--by` flag values verbatim.
    let summary = crate::text::envelope_summary(json!({
        "distinct_files": outcome.distinct_files(),
        "total_mutations": outcome.mutations.len(),
        "edit_before_read_boundaries": outcome.boundaries.len(),
        "skipped_lines": outcome.skipped_lines,
        "detail_level": match outcome.detail {
            FilesDetail::Summary => "summary",
            FilesDetail::ByDir => "dir",
            FilesDetail::ByFile => "file",
            FilesDetail::Timeline => "timeline",
        },
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

/// Emit one JSON object per group (bucket / dir / file), keyed per session.
fn json_grouped<F: Fn(&FileMutation) -> String>(
    outcome: &Outcome,
    key: F,
    row_kind: &str,
) -> Result<()> {
    use serde_json::json;
    // session_id → key → counts (deterministic order via BTreeMap).
    let mut by_session: BTreeMap<&str, Vec<&TaggedMutation>> = BTreeMap::new();
    for m in &outcome.mutations {
        by_session.entry(m.session_id.as_str()).or_default().push(m);
    }
    for (sid, ms) in by_session {
        // All mutations in this group share the id-domain (same transcript); the discriminator
        // (`is_subagent` + the re-feedable `parent_session_id`) brands every grouped row, the
        // SAME r5 shape the --timeline arm carries — so a grouped subagent row is now
        // distinguishable + re-feedable, not a bare hex on `session_id` alone.
        let (is_subagent, parent_session_id) = ms
            .first()
            .map(|m| (m.is_subagent, m.parent_session_id.clone()))
            .unwrap_or((false, sid.to_string()));
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let groups = group_by(&owned, &key);
        for (k, counts) in &groups {
            let obj = json!({
                "kind": row_kind,
                "session_id": sid,
                "is_subagent": is_subagent,
                "parent_session_id": parent_session_id,
                // The grouping key is ALWAYS `path` (a bucket prefix / a dir / a file) —
                // one on-wire key across every `--by` mode, discriminated by `kind`.
                "path": k,
                "write": counts.write,
                "edit": counts.edit,
                "notebook_edit": counts.notebook_edit,
                "multi_edit": counts.multi_edit,
                "bash": counts.bash,
                "total": counts.total(),
                "distinct_files": counts.files.len(),
                "first_utc": counts.first_ts,
                "first_local": counts.first_ts.as_deref().and_then(local_iso),
                "last_utc": counts.last_ts,
                "last_local": counts.last_ts.as_deref().and_then(local_iso),
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
    }
    Ok(())
}

/// Parse a `--turn-range` token into a [`RangeSpec`] (the shared grammar), resolved per-file
/// against each transcript's own turn count (0-based).
fn parse_turn_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--turn-range", false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Record {
        serde_json::from_slice(line.as_bytes()).expect("valid fixture record")
    }

    #[test]
    fn path_filter_none_keeps_everything() {
        let f = PathFilter::from_args(None, None).unwrap();
        assert!(f.keeps("/anything/at/all.rs"));
        assert!(f.keeps(""));
    }

    #[test]
    fn path_filter_regex_matches_anywhere_in_full_path() {
        let f = PathFilter::from_args(Some(r"\.rs$"), None).unwrap();
        assert!(f.keeps("/Users/x/src/lib.rs"));
        assert!(!f.keeps("/Users/x/docs/readme.md"));
        // "anywhere" semantics: a mid-path match is enough.
        let mid = PathFilter::from_args(Some("src"), None).unwrap();
        assert!(mid.keeps("/Users/x/src/lib.rs"));
        assert!(!mid.keeps("/Users/x/docs/readme.md"));
    }

    #[test]
    fn path_filter_glob_crosses_slash_with_double_star() {
        let f = PathFilter::from_args(None, Some("**/src/**")).unwrap();
        assert!(f.keeps("/Users/x/src/lib.rs"));
        assert!(!f.keeps("/Users/x/docs/readme.md"));
        let md = PathFilter::from_args(None, Some("**/*.md")).unwrap();
        assert!(md.keeps("/Users/x/docs/readme.md"));
        assert!(!md.keeps("/Users/x/src/lib.rs"));
    }

    #[test]
    fn path_filter_regex_and_glob_are_anded() {
        let f = PathFilter::from_args(Some(r"\.rs$"), Some("**/src/**")).unwrap();
        assert!(f.keeps("/Users/x/src/lib.rs")); // both match
        assert!(!f.keeps("/Users/x/src/readme.md")); // glob yes, regex no
        assert!(!f.keeps("/Users/x/other/lib.rs")); // regex yes, glob no
    }

    #[test]
    fn path_filter_invalid_patterns_error() {
        assert!(PathFilter::from_args(Some("("), None).is_err());
        assert!(PathFilter::from_args(None, Some("[abc")).is_err());
    }

    #[test]
    fn mutations_in_records_carrier_join_backfill_and_bash() {
        // A structured Write + its create carrier (is_create true joined by tool_use_id),
        // and a Bash `touch` (the heuristic arm). Covers the carrier-join + bash branches
        // of mutations_in_records (the shared subagent-topology extractor).
        let recs = vec![
            rec(
                r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/new.md","content":"x"}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
            ),
            rec(
                r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"touch /tmp/bashed.txt"}}]}}"#,
            ),
        ];
        let muts = mutations_in_records(&recs);
        let new_md = muts
            .iter()
            .find(|m| m.path == "/tmp/new.md")
            .expect("Write surfaced");
        assert!(
            new_md.is_create,
            "the create carrier joined → is_create true"
        );
        let bashed = muts
            .iter()
            .find(|m| m.path == "/tmp/bashed.txt")
            .expect("Bash mutation surfaced");
        assert_eq!(bashed.op, FileOp::BashMutation);
        assert!(bashed.is_create, "touch is a create verb");
    }

    #[test]
    fn mutations_in_records_excludes_cancelled_and_errored_writes() {
        // A Write whose RESULT is `is_error:true` (a failed Edit, or a `Cancelled: parallel tool
        // call … errored` when a sibling op in the same batch failed) NEVER landed → it must not
        // be counted as a mutation (the `files`↔`recover` consistency fix; recover already
        // excludes it via the same failed-id gate). A successful Write is still counted.
        let recs = vec![
            // turn 0: a SUCCESSFUL Write (create carrier, no error).
            rec(
                r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"wok","name":"Write","input":{"file_path":"/tmp/good.md","content":"real"}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/tmp/good.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"wok","content":"ok"}]}}"#,
            ),
            // turn 1: a CANCELLED Write — its result is_error:true.
            rec(
                r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"go"}}"#,
            ),
            rec(
                r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"wbad","name":"Write","input":{"file_path":"/tmp/bad.md","content":"never landed"}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T06:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"wbad","is_error":true,"content":"<tool_use_error>Cancelled: parallel tool call Bash(...) errored</tool_use_error>"}]}}"#,
            ),
        ];
        let muts = mutations_in_records(&recs);
        assert!(
            muts.iter().any(|m| m.path == "/tmp/good.md"),
            "successful write is still counted"
        );
        assert!(
            !muts.iter().any(|m| m.path == "/tmp/bad.md"),
            "cancelled/errored write is NOT counted: {muts:?}"
        );
    }

    #[test]
    fn bash_verb_is_create_classification() {
        // Fresh-target verbs (incl. the new ln/install/rsync) are creates.
        for v in [
            "mkdir",
            "touch",
            "tee",
            ">",
            "cp",
            "mv",
            "install",
            "ln",
            "rsync",
            "curl",
            "wget",
            "dd",
            "zip",
            "tar",
            "flag-output",
        ] {
            assert!(bash_verb_is_create(v), "{v} should be a create");
        }
        // Append / delete / in-place / source / git are NOT creates (`tee-a` is the
        // non-truncating `tee --append`, the `>>` analogue of `>`).
        for v in [">>", "tee-a", "rm", "sed-i", "mv-from", "git", "unknown"] {
            assert!(!bash_verb_is_create(v), "{v} should NOT be a create");
        }
    }

    /// A synthetic multi-turn session: turn 0 Writes two /tmp docs + Edits a gaps doc;
    /// an isMeta pseudo-turn (never a turn delimiter); turn 1 runs a Bash `rm` and a
    /// MultiEdit. Each structured tool_use is paired with its carrier so `is_create`
    /// joins correctly.
    fn fixture() -> Vec<Record> {
        vec![
            // ── turn 0 ──
            rec(
                r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"set up the docs"}}"#,
            ),
            // Write /tmp/a.md (a create) + its carrier.
            rec(
                r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/a.md","content":"x"}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c0","toolUseResult":{"type":"create","filePath":"/tmp/a.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
            ),
            // Write /tmp/b.md (a create) + carrier.
            rec(
                r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w2","name":"Write","input":{"file_path":"/tmp/b.md","content":"y"}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c1","toolUseResult":{"type":"create","filePath":"/tmp/b.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w2","content":"ok"}]}}"#,
            ),
            // Edit /p/spec/gaps.md (an update, not a create) + carrier.
            rec(
                r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/spec/gaps.md","old_string":"a","new_string":"b"}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c2","toolUseResult":{"type":"update","filePath":"/p/spec/gaps.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#,
            ),
            // isMeta pseudo-turn — NOT a delimiter.
            rec(
                r#"{"type":"user","uuid":"meta","isMeta":true,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"Continue from where you left off."}}"#,
            ),
            // ── turn 1 ──
            rec(
                r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"clean up and refactor"}}"#,
            ),
            // Bash rm (heuristic).
            rec(
                r#"{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"rm /tmp/a.md"}}]}}"#,
            ),
            // MultiEdit /p/src/lib.rs (an update) + carrier.
            rec(
                r#"{"type":"assistant","uuid":"a4","timestamp":"2026-06-07T06:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"m1","name":"MultiEdit","input":{"file_path":"/p/src/lib.rs","edits":[]}}]}}"#,
            ),
            rec(
                r#"{"type":"user","uuid":"c3","toolUseResult":{"type":"update","filePath":"/p/src/lib.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"m1","content":"ok"}]}}"#,
            ),
        ]
    }

    fn extract(records: &[Record]) -> Vec<TaggedMutation> {
        let line_nos: Vec<usize> = (1..=records.len()).collect();
        extract_mutations("0a1b2c3d-sess", records, &line_nos)
    }

    #[test]
    fn turn_index_assigned_and_meta_not_a_delimiter() {
        let muts = extract(&fixture());
        // turn 0 carries the three structured edits; turn 1 the bash + multiedit.
        let turn0: Vec<&str> = muts
            .iter()
            .filter(|m| m.turn_index == 0)
            .map(|m| m.mutation.path.as_str())
            .collect();
        assert!(turn0.contains(&"/tmp/a.md"));
        assert!(turn0.contains(&"/tmp/b.md"));
        assert!(turn0.contains(&"/p/spec/gaps.md"));
        let turn1: Vec<&str> = muts
            .iter()
            .filter(|m| m.turn_index == 1)
            .map(|m| m.mutation.path.as_str())
            .collect();
        assert!(turn1.contains(&"/p/src/lib.rs"), "multiedit in turn 1");
        assert!(turn1.contains(&"/tmp/a.md"), "bash rm in turn 1");
        // No mutation is attributed to a turn index >= 2 (the isMeta did not open one).
        assert!(muts.iter().all(|m| m.turn_index <= 1));
    }

    #[test]
    fn create_join_marks_writes_as_create_edits_as_not() {
        let muts = extract(&fixture());
        let by_path = |p: &str| -> &TaggedMutation {
            muts.iter()
                .find(|m| m.mutation.path == p && !m.mutation.op.is_heuristic())
                .unwrap_or_else(|| panic!("missing {p}"))
        };
        // The two Writes joined their carrier type:"create" → is_create true.
        assert!(by_path("/tmp/a.md").mutation.is_create);
        assert!(by_path("/tmp/b.md").mutation.is_create);
        // The Edit / MultiEdit joined type:"update" → is_create false.
        assert!(!by_path("/p/spec/gaps.md").mutation.is_create);
        assert!(!by_path("/p/src/lib.rs").mutation.is_create);
    }

    #[test]
    fn bash_mutation_is_heuristic_op() {
        let muts = extract(&fixture());
        let bash = muts
            .iter()
            .find(|m| m.mutation.op == FileOp::BashMutation)
            .expect("a bash mutation");
        assert_eq!(bash.mutation.path, "/tmp/a.md");
        assert!(bash.mutation.op.is_heuristic());
        assert_eq!(bash.turn_index, 1);
    }

    #[test]
    fn summary_bucketing_collapses_by_parent_dir() {
        let muts = extract(&fixture());
        let buckets = group_by(&muts, |m| bucket_key(&m.path));
        // /tmp bucket: two writes + the bash rm.
        let tmp = buckets.get("/tmp").expect("/tmp bucket");
        assert_eq!(tmp.write, 2, "two /tmp writes");
        assert_eq!(tmp.bash, 1, "one bash rm under /tmp");
        // /p/spec bucket: one edit (the fixture's parent dirs are ≤4 segments deep, so the
        // coarse rollup keeps them — the collapse only fires on DEEPER paths, see below).
        let spec = buckets.get("/p/spec").expect("/p/spec bucket");
        assert_eq!(spec.edit, 1);
        // /p/src bucket: one multi-edit.
        let src = buckets.get("/p/src").expect("/p/src bucket");
        assert_eq!(src.multi_edit, 1);
    }

    #[test]
    fn summary_rollup_collapses_deep_paths_unlike_by_dir() {
        // The fix: `--summary` is a COARSE top-level rollup (≤SUMMARY_BUCKET_SEGMENTS
        // segments), NOT the full parent dir `--by-dir` keys on. Two deeply-nested files
        // sharing a top-level prefix collapse to ONE summary bucket but stay TWO by-dir rows.
        let deep_a = "/Users/testuser/Projects/demo_app/components/wireframe/tabs/Foo.tsx";
        let deep_b = "/Users/testuser/Projects/demo_app/spec/gaps.md";
        // Summary buckets BOTH under the first 4 segments.
        assert_eq!(bucket_key(deep_a), "/Users/testuser/Projects/demo_app");
        assert_eq!(bucket_key(deep_b), "/Users/testuser/Projects/demo_app");
        // by-dir keeps the full distinct parents.
        assert_eq!(
            parent_dir(deep_a).as_deref(),
            Some("/Users/testuser/Projects/demo_app/components/wireframe/tabs")
        );
        assert_eq!(
            parent_dir(deep_b).as_deref(),
            Some("/Users/testuser/Projects/demo_app/spec")
        );
        assert_ne!(
            parent_dir(deep_a),
            parent_dir(deep_b),
            "by-dir keeps them SEPARATE; summary collapses them — a real 4-level ladder"
        );
    }

    #[test]
    fn summary_git_pseudo_path_is_its_own_bucket_not_the_dot_sink() {
        // `git:<sub>` pseudo-paths roll up under ONE `git:` bucket (out of the `./` relative
        // sink), so a genuine-relative-file bucket is not polluted by git subcommands.
        assert_eq!(bucket_key("git:commit"), "git:");
        assert_eq!(bucket_key("git:add"), "git:");
        assert_eq!(bucket_key("git:stash"), "git:");
        // A real relative file still buckets under `./`.
        assert_eq!(bucket_key("relative.md"), "./");
    }

    #[test]
    fn bucket_key_edge_cases() {
        // A top-level file `/foo` → parent `/` → the root bucket.
        assert_eq!(bucket_key("/foo"), "/");
        // A shallow path keeps all the segments it has (fewer than the cap).
        assert_eq!(bucket_key("/tmp/x.md"), "/tmp");
        assert_eq!(bucket_key("/a/b/c.txt"), "/a/b");
        // A relative MULTI-segment path keeps its (relative) parent prefix.
        assert_eq!(bucket_key("src/wireframe/Foo.tsx"), "src/wireframe");
        // A deep path is capped to the first SUMMARY_BUCKET_SEGMENTS segments of the parent.
        assert_eq!(
            bucket_key("/a/b/c/d/e/f/g.rs"),
            "/a/b/c/d",
            "deep paths cap at the segment limit"
        );
    }

    #[test]
    fn by_file_first_last_timestamps() {
        let muts = extract(&fixture());
        let files = group_by(&muts, |m| m.path.clone());
        // /tmp/a.md is Written at 05:00:01 and rm'd (bash) at 06:00:01.
        let a = files.get("/tmp/a.md").expect("/tmp/a.md row");
        assert_eq!(a.first_ts.as_deref(), Some("2026-06-07T05:00:01.000Z"));
        assert_eq!(a.last_ts.as_deref(), Some("2026-06-07T06:00:01.000Z"));
        assert_eq!(a.write, 1);
        assert_eq!(a.bash, 1);
    }

    #[test]
    fn distinct_file_count() {
        let muts = extract(&fixture());
        let outcome = Outcome {
            detail: FilesDetail::Summary,
            mutations: muts,
            boundaries: Vec::new(),
            skipped_lines: 0,
            turn_range: None,
            time_window_bounded: false,
            scope_top: 1,
            scope_sub: 0,
        };
        // Distinct paths: /tmp/a.md, /tmp/b.md, /p/spec/gaps.md, /p/src/lib.rs = 4.
        assert_eq!(outcome.distinct_files(), 4);
    }

    #[test]
    fn parent_dir_and_bucket_key_rules() {
        assert_eq!(parent_dir("/tmp/x.md").as_deref(), Some("/tmp"));
        assert_eq!(parent_dir("/p/spec/gaps.md").as_deref(), Some("/p/spec"));
        // A top-level path → parent is "/".
        assert_eq!(parent_dir("/foo").as_deref(), Some("/"));
        // A bare relative filename has no parent.
        assert_eq!(parent_dir("relative.md"), None);
        // A trailing slash is stripped before taking the parent (a dir target).
        assert_eq!(parent_dir("/a/b/").as_deref(), Some("/a"));
        // bucket_key falls back to "./" for a bare relative filename.
        assert_eq!(bucket_key("relative.md"), "./");
        assert_eq!(bucket_key("/tmp/x.md"), "/tmp");
    }

    #[test]
    fn ops_label_omits_zeroes_and_flags_bash() {
        let mut c = OpCounts::default();
        assert_eq!(c.ops_label(), "0", "empty group → 0");
        c.add(&FileMutation {
            path: "/tmp/a".into(),
            op: FileOp::Write,
            timestamp_utc: None,
            is_create: true,
        });
        c.add(&FileMutation {
            path: "/tmp/b".into(),
            op: FileOp::BashMutation,
            timestamp_utc: None,
            is_create: false,
        });
        let label = c.ops_label();
        assert!(label.contains("1 write"), "got: {label}");
        assert!(label.contains("1 bash (heuristic)"), "got: {label}");
        assert!(!label.contains("edit"), "zero edits omitted: {label}");
    }

    #[test]
    fn op_counts_and_label_cover_notebook_and_multiedit() {
        // Exercise the NotebookEdit + MultiEdit arms of OpCounts::add and ops_label
        // (the fixture has no NotebookEdit, so cover it explicitly).
        let mut c = OpCounts::default();
        for op in [FileOp::NotebookEdit, FileOp::MultiEdit, FileOp::Edit] {
            c.add(&FileMutation {
                path: format!("/p/nb-{}", op.label()),
                op,
                timestamp_utc: Some("2026-06-07T05:00:00.000Z".into()),
                is_create: false,
            });
        }
        let label = c.ops_label();
        assert!(label.contains("1 notebook-edit"), "got: {label}");
        assert!(label.contains("1 multi-edit"), "got: {label}");
        assert!(label.contains("1 edit"), "got: {label}");
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn structured_notebook_edit_flows_through_extract() {
        // A NotebookEdit tool_use → a notebook-edit op in the extracted mutations.
        let records = vec![
            rec(
                r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit nb"}}"#,
            ),
            rec(
                r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"n1","name":"NotebookEdit","input":{"notebook_path":"/p/analysis.ipynb","new_source":"code"}}]}}"#,
            ),
        ];
        let muts = extract(&records);
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].mutation.op, FileOp::NotebookEdit);
        assert_eq!(muts[0].mutation.path, "/p/analysis.ipynb");
    }

    #[test]
    fn timeline_sort_places_timestampless_last() {
        // A mutation with no timestamp sorts AFTER timestamped ones.
        assert!(timestamp_sort_key(Some("2026-06-07T05:00:00Z")) < timestamp_sort_key(None));
        assert!(
            timestamp_sort_key(Some("2026-06-07T05:00:00Z"))
                < timestamp_sort_key(Some("2026-06-07T06:00:00Z"))
        );
    }

    #[test]
    fn turn_range_parsing() {
        assert_eq!(
            parse_turn_range("0..1").unwrap().resolve(100, false),
            (0, 1)
        );
        assert_eq!(
            parse_turn_range("5..5").unwrap().resolve(100, false),
            (5, 5)
        );
        assert!(parse_turn_range("3..1").is_err());
        assert!(parse_turn_range("notarange").is_err());
        assert!(parse_turn_range("a..b").is_err());
    }

    #[test]
    fn tool_use_id_for_finds_first_and_none() {
        let with = rec(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"x1","name":"Write","input":{"file_path":"/p/a"}}]}}"#,
        );
        assert_eq!(tool_use_id_for(&with).as_deref(), Some("x1"));
        // A record with no tool_use → None.
        let without = rec(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(tool_use_id_for(&without).is_none());
        // A tool_use with no id → None.
        let no_id = rec(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Write","input":{"file_path":"/p/a"}}]}}"#,
        );
        assert!(tool_use_id_for(&no_id).is_none());
    }

    #[test]
    fn line_prefilter_keeps_candidates_drops_noise() {
        assert!(line_is_files_candidate(
            br#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Edit"}]}}"#
        ));
        assert!(line_is_files_candidate(
            br#"{"type":"user","message":{"role":"user"}}"#
        ));
        assert!(line_is_files_candidate(
            br#"{"toolUseResult":{"filePath":"/p/x"}}"#
        ));
        // A pure-noise attachment line with none of the markers is dropped.
        assert!(!line_is_files_candidate(
            br#"{"type":"attachment","data":{"x":1}}"#
        ));
    }

    #[test]
    fn extract_handles_missing_carrier_defaults_is_create_false() {
        // A Write tool_use with NO paired carrier in the turn → is_create stays false
        // (honest "unknown / treat as edit"), and the path comes from the tool_use.
        let records = vec![
            rec(
                r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"q"}}"#,
            ),
            rec(
                r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/no-carrier.md","content":"x"}}]}}"#,
            ),
        ];
        let muts = extract(&records);
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].mutation.path, "/tmp/no-carrier.md");
        assert!(!muts[0].mutation.is_create, "no carrier → is_create false");
    }

    #[test]
    fn extract_empty_for_session_with_no_mutations() {
        let records = vec![
            rec(
                r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"just chatting"}}"#,
            ),
            rec(
                r#"{"type":"assistant","uuid":"a0","message":{"role":"assistant","content":[{"type":"text","text":"sure"}]}}"#,
            ),
        ];
        assert!(extract(&records).is_empty());
    }

    // ── Rendering branches (called directly so coverage does not depend on the
    //    integration-binary merge; output goes to test stdout, harmless). ──

    fn outcome(detail: FilesDetail, muts: Vec<TaggedMutation>) -> Outcome {
        // Derive the scope span from the mutations' distinct transcripts (test-only proxy for
        // the real `resolve_session_files` span) so a subagent fixture still drives the banner.
        // Compute the owned counts BEFORE moving `muts` into the struct.
        let mut subs = std::collections::BTreeSet::new();
        let mut tops = std::collections::BTreeSet::new();
        for m in &muts {
            if m.is_subagent {
                subs.insert(m.session_id.clone());
            } else {
                tops.insert(m.session_id.clone());
            }
        }
        let (scope_top, scope_sub) = (tops.len(), subs.len());
        Outcome {
            detail,
            mutations: muts,
            boundaries: Vec::new(),
            skipped_lines: 0,
            turn_range: None,
            time_window_bounded: false,
            scope_top,
            scope_sub,
        }
    }

    #[test]
    fn render_text_all_detail_levels_run() {
        let muts = extract(&fixture());
        for d in [
            FilesDetail::Summary,
            FilesDetail::ByDir,
            FilesDetail::ByFile,
            FilesDetail::Timeline,
        ] {
            render_text(&outcome(d, muts.clone()));
        }
    }

    #[test]
    fn render_text_empty_prints_none() {
        // The empty-mutations branch of render_text (→ "no file mutations found").
        render_text(&outcome(FilesDetail::Summary, Vec::new()));
    }

    #[test]
    fn render_json_all_detail_levels_run() {
        let muts = extract(&fixture());
        for d in [
            FilesDetail::Summary,
            FilesDetail::ByDir,
            FilesDetail::ByFile,
            FilesDetail::Timeline,
        ] {
            render_json(&outcome(d, muts.clone())).expect("json render");
        }
    }

    #[test]
    fn footer_filter_context_all_arms() {
        let muts = extract(&fixture());
        // turn-range arm.
        let mut o = outcome(FilesDetail::Summary, muts.clone());
        o.turn_range = Some("0..1".to_string());
        assert_eq!(filter_context(&o), "turn-range=0..1");
        // bounded time-window arm.
        let mut o2 = outcome(FilesDetail::Summary, muts.clone());
        o2.time_window_bounded = true;
        assert_eq!(filter_context(&o2), "time-window");
        // unbounded "all turns" arm.
        let o3 = outcome(FilesDetail::Summary, muts);
        assert_eq!(filter_context(&o3), "all turns");
    }

    #[test]
    fn footer_reports_skipped_lines() {
        // The skipped-lines footer branch (> 0) fires.
        let mut o = outcome(FilesDetail::Summary, extract(&fixture()));
        o.skipped_lines = 3;
        print_footer(&o); // exercises the `skipped_lines > 0` true arm
    }

    #[test]
    fn render_multi_session_separator() {
        // Two sessions → the per_session blank-line separator arm (`!first`) fires.
        let ln: Vec<usize> = (1..=fixture().len()).collect();
        let mut a = extract_mutations("aaaa-sess", &fixture(), &ln);
        let b = extract_mutations("bbbb-sess", &fixture(), &ln);
        a.extend(b);
        render_text(&outcome(FilesDetail::Summary, a));
    }

    #[test]
    fn timeline_renders_heuristic_and_non_heuristic() {
        // The timeline heuristic-label ternary both arms: a Write (no label) and a
        // Bash mutation (heuristic label) in the same session.
        render_timeline(&outcome(FilesDetail::Timeline, extract(&fixture())));
    }

    #[test]
    fn json_grouped_emits_per_group_with_timestamps() {
        // A by-dir JSON render where groups carry first/last timestamps (the
        // `first_local`/`last_local` Some arms via local_iso) + the distinct-file count.
        render_json(&outcome(FilesDetail::ByDir, extract(&fixture()))).expect("json");
    }

    // ── scan_one_file branch coverage (mmap-None, skipped-line counting) ──

    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_jsonl(lines: &[&str]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "csift-files-{}-{}.jsonl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn scan_one_file_empty_is_safe() {
        // A zero-byte file → mmap_bytes None → empty result (the early-return arm).
        let p = std::env::temp_dir().join(format!(
            "csift-files-empty-{}-{}.jsonl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::File::create(&p).unwrap();
        let fr = scan_one_file(&p).expect("scan empty");
        std::fs::remove_file(&p).ok();
        assert!(fr.mutations.is_empty());
        assert_eq!(fr.skipped_lines, 0);
    }

    #[test]
    fn scan_one_file_extracts_mutations_and_counts_skips() {
        // A populated file: a genuine user, a Write tool_use + carrier, plus a
        // malformed line that survives the prefilter (carries "Write") → counted.
        let p = tmp_jsonl(&[
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/scan.md","content":"x"}}]}}"#,
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/scan.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
            r#"{"name":"Write" this is broken json after the marker}"#,
        ]);
        let fr = scan_one_file(&p).expect("scan");
        std::fs::remove_file(&p).ok();
        assert_eq!(fr.mutations.len(), 1, "one Write mutation");
        assert_eq!(fr.mutations[0].mutation.path, "/tmp/scan.md");
        assert!(fr.mutations[0].mutation.is_create, "carrier create joined");
        assert_eq!(fr.skipped_lines, 1, "the malformed Write line is counted");
    }

    #[test]
    fn scan_one_file_skips_non_candidate_noise_lines() {
        // An attachment line with NONE of the mutation/role markers is dropped pre-JSON
        // (the `!line_is_files_candidate` true arm), leaving zero mutations + zero skips.
        let p = tmp_jsonl(&[
            r#"{"type":"attachment","data":{"x":1}}"#,
            r#"{"type":"file-history-snapshot","snapshot":{}}"#,
        ]);
        let fr = scan_one_file(&p).expect("scan");
        std::fs::remove_file(&p).ok();
        assert!(fr.mutations.is_empty());
        assert_eq!(
            fr.skipped_lines, 0,
            "noise lines are not malformed, just skipped"
        );
    }

    #[test]
    fn subagent_mutation_carries_is_subagent_and_refeedable_parent_in_grouped_views() {
        // A subagent transcript path stamps is_subagent=true + the re-feedable PARENT uuid
        // onto every mutation, so the grouped (text + JSON) views can brand the row instead
        // of leaking the bare hex as a `SESSION` / re-feedable `session_id`.
        let dir = std::env::temp_dir().join(format!(
            "csift-files-sub-{}-{}/aaaabbbb-cccc-dddd-eeee-ffff00001111/subagents/workflows/wf_q",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("agent-c0ffee1234567890.jsonl");
        {
            let mut f = std::fs::File::create(&p).unwrap();
            writeln!(f, r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#).unwrap();
            writeln!(f, r#"{{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"w1","name":"Write","input":{{"file_path":"/tmp/sub.md","content":"x"}}}}]}}}}"#).unwrap();
        }
        let fr = scan_one_file(&p).expect("scan subagent");
        std::fs::remove_file(&p).ok();
        assert_eq!(fr.mutations.len(), 1);
        let m = &fr.mutations[0];
        assert_eq!(
            m.session_id, "c0ffee1234567890",
            "bare hex (agent- stripped)"
        );
        assert!(
            m.is_subagent,
            "a subagents/ path tags the mutation subagent"
        );
        assert_eq!(
            m.parent_session_id, "aaaabbbb-cccc-dddd-eeee-ffff00001111",
            "parent is the re-feedable uuid dir before subagents/"
        );
        // Both grouped renders run without panic on a subagent-tagged outcome (covers the
        // branded SUBAGENT header arm + the json_grouped discriminator arm).
        let muts = vec![m.clone()];
        render_text(&outcome(FilesDetail::ByFile, muts.clone()));
        render_json(&outcome(FilesDetail::ByFile, muts)).expect("grouped json");
    }
}
