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

use anyhow::{bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;

use crate::bash_mutations::parse_bash_mutations;
use crate::cli::{FilesArgs, FilesDetail, OutputFormat};
use crate::model::{group_turn_indices, FileMutation, FileOp, Record};
use crate::parse::{mmap_bytes, scan_lines_bytes};
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// One extracted mutation tagged with the turn it belongs to + its owning session.
#[derive(Debug, Clone)]
struct TaggedMutation {
    session_id: String,
    turn_index: usize,
    mutation: FileMutation,
}

/// Per-file scan result before global aggregation.
struct FileResult {
    mutations: Vec<TaggedMutation>,
    skipped_lines: usize,
}

/// Entry point for `csift files`.
pub fn run_files(args: &FilesArgs) -> Result<()> {
    // ── Validate flag combinations (same rule + wording as `search`) ──
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // ── Resolve targets → session files (subagent span per --no-subagents /
    //    --subagents-only; default spans subagents) ──
    let session_files =
        path::resolve_session_files(&args.paths, args.session.as_deref(), args.scope())?;

    // ── Parallel scan across files (default rayon pool = CPU count) ──
    let per_file: Vec<FileResult> = session_files
        .par_iter()
        .map(|p| scan_one_file(p))
        .collect::<Result<Vec<_>>>()?;

    // ── Merge + filter by turn-range / time-window per mutation ──
    let mut mutations: Vec<TaggedMutation> = Vec::new();
    let mut skipped_lines = 0usize;
    for fr in per_file {
        skipped_lines += fr.skipped_lines;
        for tm in fr.mutations {
            if let Some((lo, hi)) = turn_range {
                if tm.turn_index < lo || tm.turn_index > hi {
                    continue;
                }
            }
            // A mutation with no timestamp never falls inside a BOUNDED window
            // (same rule as search/agents); an unbounded window admits it.
            if !time_window.contains(tm.mutation.timestamp_utc.as_deref()) {
                continue;
            }
            mutations.push(tm);
        }
    }

    let outcome = Outcome {
        detail: args.detail(),
        mutations,
        skipped_lines,
        turn_range,
        time_window_bounded: !time_window.is_unbounded(),
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
            skipped_lines: 0,
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
    let mut records: Vec<Record> = Vec::new();
    let mut skipped = 0usize;
    scan_lines_bytes(bytes, |line| {
        if !line_is_files_candidate(line) {
            return;
        }
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => records.push(rec),
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    })?;

    let mutations = extract_mutations(&session_id, &records);
    Ok(FileResult {
        mutations,
        skipped_lines: skipped,
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
    for rec in records {
        for (id, file_path, is_create) in rec.carrier_create_paths() {
            carriers.insert(id, (file_path, is_create));
        }
    }
    let mut out = Vec::new();
    for rec in records {
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
/// dest, a download to a path, a `dd`/`zip`/flag-specified output) is treated as a create;
/// an append (`>>`), `rm`, `sed -i`, `mv-from`, and `git` are NOT. Lexical-only, so it is
/// just a heuristic (its `FileOp::BashMutation` is_heuristic() gates the label everywhere).
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
            | "flag-output"
    )
}

/// Delimit turns over the parsed records, then for each turn extract structured + Bash
/// mutations and JOIN the structured ones to their carriers for accurate `is_create`.
fn extract_mutations(session_id: &str, records: &[Record]) -> Vec<TaggedMutation> {
    let index_turns = group_turn_indices(records, Record::opens_turn);
    let mut out = Vec::new();

    for (turn_index, idxs) in index_turns.iter().enumerate() {
        // Build the carrier join map for this turn: tool_use_id → (filePath, is_create).
        let mut carriers: BTreeMap<String, (String, bool)> = BTreeMap::new();
        for &i in idxs {
            for (id, file_path, is_create) in records[i].carrier_create_paths() {
                carriers.insert(id, (file_path, is_create));
            }
        }

        for &i in idxs {
            let rec = &records[i];

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
                    turn_index,
                    mutation: m,
                });
            }

            // Bash (heuristic) mutations.
            if let Some(cmd) = rec.bash_command() {
                for bm in parse_bash_mutations(cmd) {
                    out.push(TaggedMutation {
                        session_id: session_id.to_string(),
                        turn_index,
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
    skipped_lines: usize,
    turn_range: Option<(usize, usize)>,
    time_window_bounded: bool,
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

/// The top-level-dir BUCKET key for a path: its parent directory. `/tmp/x.md` →
/// `/tmp`; `/p/spec/gaps.md` → `/p/spec`. A path with no parent (a bare relative
/// filename, or `/`) buckets under `./` so relative Bash paths still group cleanly.
fn bucket_key(path: &str) -> String {
    parent_dir(path).unwrap_or_else(|| "./".to_string())
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
    if outcome.mutations.is_empty() {
        println!("no file mutations found");
        print_footer(outcome);
        return;
    }

    match outcome.detail {
        FilesDetail::Summary => render_summary(outcome),
        FilesDetail::ByDir => render_by_dir(outcome),
        FilesDetail::ByFile => render_by_file(outcome),
        FilesDetail::Timeline => render_timeline(outcome),
    }
    print_footer(outcome);
}

/// Group mutations under their session header, then call `body` per session with that
/// session's mutations. Sessions render in sorted id order for determinism.
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
        println!("SESSION {sid}");
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
                "  {}  turn {}  {}{}  {}",
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
        "{} distinct file(s)  ·  {} mutation(s)  ·  detail={level}  ·  {filter}",
        outcome.distinct_files(),
        outcome.mutations.len()
    );
    println!("(Bash mutations are heuristic — parsed from the command string.)");
    if outcome.skipped_lines > 0 {
        println!("({} malformed line(s) skipped)", outcome.skipped_lines);
    }
}

/// A short description of the active turn/time filter for the footer.
fn filter_context(outcome: &Outcome) -> String {
    if let Some((lo, hi)) = outcome.turn_range {
        format!("turn-range={lo}..{hi}")
    } else if outcome.time_window_bounded {
        "time-window".to_string()
    } else {
        "all turns".to_string()
    }
}

fn render_json(outcome: &Outcome) -> Result<()> {
    use serde_json::json;
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
                    "session_id": m.session_id,
                    "path": m.mutation.path,
                    "op": m.mutation.op.label(),
                    "ts_utc": m.mutation.timestamp_utc,
                    "ts_local": m.mutation.timestamp_utc.as_deref().and_then(local_iso),
                    "turn_index": m.turn_index,
                    "is_create": m.mutation.is_create,
                    "heuristic": m.mutation.op.is_heuristic(),
                });
                println!("{}", serde_json::to_string(&obj)?);
            }
        }
    }
    // Trailing summary object (mirrors search's trailing-summary convention).
    let summary = json!({
        "distinct_files": outcome.distinct_files(),
        "total_mutations": outcome.mutations.len(),
        "skipped_lines": outcome.skipped_lines,
        "detail_level": match outcome.detail {
            FilesDetail::Summary => "summary",
            FilesDetail::ByDir => "by-dir",
            FilesDetail::ByFile => "by-file",
            FilesDetail::Timeline => "timeline",
        },
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

/// Emit one JSON object per group (bucket / dir / file), keyed per session.
fn json_grouped<F: Fn(&FileMutation) -> String>(
    outcome: &Outcome,
    key: F,
    key_name: &str,
) -> Result<()> {
    use serde_json::json;
    // session_id → key → counts (deterministic order via BTreeMap).
    let mut by_session: BTreeMap<&str, Vec<&TaggedMutation>> = BTreeMap::new();
    for m in &outcome.mutations {
        by_session.entry(m.session_id.as_str()).or_default().push(m);
    }
    for (sid, ms) in by_session {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let groups = group_by(&owned, &key);
        for (k, counts) in &groups {
            let obj = json!({
                "session_id": sid,
                key_name: k,
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

/// Parse a `--turn-range START..END` string into an inclusive `(lo, hi)` pair. Both
/// bounds are 0-based, inclusive; `END < START` is an error. (Identical contract to
/// `search`'s parser; kept local so the two subcommands stay independent.)
fn parse_turn_range(s: &str) -> Result<(usize, usize)> {
    let (a, b) = s
        .split_once("..")
        .with_context(|| format!("--turn-range must be START..END, got {s:?}"))?;
    let lo: usize = a
        .trim()
        .parse()
        .with_context(|| format!("--turn-range start is not a non-negative integer: {a:?}"))?;
    let hi: usize = b
        .trim()
        .parse()
        .with_context(|| format!("--turn-range end is not a non-negative integer: {b:?}"))?;
    if hi < lo {
        bail!("--turn-range end ({hi}) is before start ({lo})");
    }
    Ok((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Record {
        serde_json::from_slice(line.as_bytes()).expect("valid fixture record")
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
            "flag-output",
        ] {
            assert!(bash_verb_is_create(v), "{v} should be a create");
        }
        // Append / delete / in-place / source / git are NOT creates.
        for v in [">>", "rm", "sed-i", "mv-from", "git", "unknown"] {
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
        extract_mutations("0a1b2c3d-sess", records)
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
        // /p/spec bucket: one edit.
        let spec = buckets.get("/p/spec").expect("/p/spec bucket");
        assert_eq!(spec.edit, 1);
        // /p/src bucket: one multi-edit.
        let src = buckets.get("/p/src").expect("/p/src bucket");
        assert_eq!(src.multi_edit, 1);
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
            skipped_lines: 0,
            turn_range: None,
            time_window_bounded: false,
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
        assert_eq!(parse_turn_range("0..1").unwrap(), (0, 1));
        assert_eq!(parse_turn_range("5..5").unwrap(), (5, 5));
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
        Outcome {
            detail,
            mutations: muts,
            skipped_lines: 0,
            turn_range: None,
            time_window_bounded: false,
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
        o.turn_range = Some((0, 1));
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
        let mut a = extract_mutations("aaaa-sess", &fixture());
        let b = extract_mutations("bbbb-sess", &fixture());
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
}
