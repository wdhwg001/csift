//! `search` subcommand — regex over transcripts, returning complete round-trip
//! exchanges.
//!
//! Behavior (SPEC.md §6.2, §6.4):
//! - Pattern is ripgrep-like, default smart-case (`-i` forces insensitive,
//!   `--multiline` lets `.` cross newlines). Empty pattern == pure filter.
//! - Filters: `--category/-t` (repeatable), `--turn-range` XOR (`--since`/`--until`),
//!   `--session`, `--path` (repeatable, multi-target).
//! - A **turn** is delimited by GENUINE user messages; a `tool_result`-carrier, an
//!   `isMeta` pseudo-turn, and a compaction summary never start a turn.
//! - On a hit, the COMPLETE round-trip (Exchange) is returned: a matched `tool_use`
//!   WITH its `tool_result`; a matched user turn WITH the agent response; etc. The
//!   exchange is the whole turn (opening genuine-user + every record chained under
//!   it until the next genuine-user), so every form of completeness in §6.4 holds.
//! - `--max-count` caps results but NEVER silently — the dropped count is reported.
//! - rayon parallelizes across files; lazy parse keeps it fast on 200 MB+ inputs.
//!
//! ## Scan strategy
//!
//! `list` can head/tail-read, but `search` must see the whole session to delimit
//! turns and stitch exchanges, so it mmaps the file once and does a single forward
//! [`crate::parse::scan_lines_bytes`] pass with the two-stage byte prefilter (§7d):
//! the category prefilter gates the `serde_json` parse (dropping the ~54%
//! attachment/noise lines pre-JSON); the keyword prefilter marks `can_hit` so the
//! match phase skips regex work on records that provably lack the literal. Turn
//! reconstruction then runs over the retained transcript records.

use std::path::Path;

use anyhow::{bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use regex::bytes::Regex as BytesRegex;

use crate::cli::{Category, OutputFormat, SearchArgs};
use crate::model::{
    group_turn_indices, is_auq_answer_text, normalize_line, tool_result_content_text, Block, Record,
};
use crate::parse::{mmap_bytes, scan_lines_bytes};
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Max characters of a matched excerpt shown inline before truncation. Truncation
/// is ALWAYS explicit (`… (+N chars)`) — never silent (SPEC §0, §8.1).
///
/// Deliberately LONGER than `list`'s 200-char cap (`session::EXCERPT_MAX`): a search
/// hit wants enough of the matched exchange to be useful in context, whereas `list`
/// is a dense at-a-glance identity index. The difference is intentional.
const EXCERPT_MAX: usize = 400;

/// A single category-tagged hit inside an exchange.
#[derive(Debug, Clone)]
pub struct Hit {
    pub category: Category,
    /// The matched text excerpt (whitespace-normalized, explicitly truncated).
    pub excerpt: String,
    pub timestamp_utc: Option<String>,
    /// Tool name when the hit is a `tool`/`tool-response` block, for the header.
    pub tool_name: Option<String>,
}

/// A complete reconstructed request/response exchange (round-trip) containing the
/// hit(s).
#[derive(Debug, Clone)]
pub struct Exchange {
    pub session_id: String,
    /// 0-based turn index (turns delimited by genuine-user messages).
    pub turn_index: usize,
    pub hits: Vec<Hit>,
    /// Uuids of every record stitched into this exchange (for traceability).
    pub record_uuids: Vec<String>,
}

/// Outcome of a search run, including the no-silent-truncation accounting.
#[derive(Debug, Clone, Default)]
pub struct SearchOutcome {
    pub exchanges: Vec<Exchange>,
    /// How many matching exchanges were dropped by `--max-count` (0 if none).
    pub dropped_by_cap: usize,
    /// Total malformed lines skipped while scanning (surfaced, never hidden).
    pub skipped_lines: usize,
}

/// A compiled pattern + flags, plus the optional literal prefilter needle.
#[derive(Debug)]
pub struct Matcher {
    /// `None` ⇒ empty pattern (pure filter: every category-eligible record matches).
    regex: Option<BytesRegex>,
    /// A required literal substring extracted from the regex, for the cheap
    /// `memmem` line prefilter (§7d stage 2). `None` ⇒ no anchorable literal.
    prefilter: Option<memmem::Finder<'static>>,
}

impl Matcher {
    /// True when the pattern is empty (pure filter — matches any text).
    fn is_pure_filter(&self) -> bool {
        self.regex.is_none()
    }

    /// True if `text` matches the pattern (always true for the pure filter).
    fn is_match(&self, text: &str) -> bool {
        match &self.regex {
            None => true,
            Some(re) => re.is_match(text.as_bytes()),
        }
    }

    /// Cheap raw-line prefilter (§7d stage 2): if a required literal exists and the
    /// line lacks it, the line cannot match — drop it pre-JSON. With no literal (or
    /// pure filter) we cannot prove absence, so the line passes to the parse stage.
    fn line_may_match(&self, line: &[u8]) -> bool {
        match &self.prefilter {
            Some(finder) => finder.find(line).is_some(),
            None => true,
        }
    }
}

/// Compile the user pattern honoring smart-case / `-i` / `--multiline`.
///
/// Smart-case: case-insensitive iff the pattern has NO uppercase letter; `-i`
/// forces insensitive regardless (and wins on conflict). `--multiline` sets
/// `.dot_matches_new_line(true)` + multi-line mode. An empty pattern compiles to
/// the pure-filter matcher (no regex, no prefilter).
pub fn build_matcher(args: &SearchArgs) -> Result<Matcher> {
    if args.pattern.is_empty() {
        return Ok(Matcher {
            regex: None,
            prefilter: None,
        });
    }

    let has_uppercase = args.pattern.chars().any(|c| c.is_uppercase());
    let case_insensitive = args.ignore_case || !has_uppercase;

    let regex = BytesRegex::new(&apply_builder(
        &args.pattern,
        case_insensitive,
        args.multiline,
    )?)
    .with_context(|| format!("invalid regex pattern: {:?}", args.pattern))?;

    // Extract a required literal for the cheap line prefilter. We only do this for
    // a case-SENSITIVE pattern: a case-insensitive needle can't be matched by a
    // single byte-exact `memmem` without casefolding the haystack (too costly), so
    // we skip the literal prefilter there and rely on the regex (still pre-JSON, on
    // raw bytes — far cheaper than parsing). For case-sensitive patterns, a leading
    // required literal (when the regex starts with plain bytes) is a big win.
    let prefilter = if case_insensitive {
        None
    } else {
        required_literal(&args.pattern).map(|lit| memmem::Finder::new(&lit).into_owned())
    };

    Ok(Matcher {
        regex: Some(regex),
        prefilter,
    })
}

/// Build the final regex source string with the requested flags applied via an
/// inline flag group `(?ims)`, so we keep `regex::bytes` (needed for raw-byte
/// matching) while honoring case/multiline.
fn apply_builder(pattern: &str, case_insensitive: bool, multiline: bool) -> Result<String> {
    let mut flags = String::new();
    if case_insensitive {
        flags.push('i');
    }
    if multiline {
        // `s` = dot matches newline; `m` = `^`/`$` match line boundaries.
        flags.push('s');
        flags.push('m');
    }
    // Validate the bare pattern early for a clean error before we wrap it.
    regex::bytes::Regex::new(pattern)
        .with_context(|| format!("invalid regex pattern: {pattern:?}"))?;
    if flags.is_empty() {
        Ok(pattern.to_string())
    } else {
        Ok(format!("(?{flags}){pattern}"))
    }
}

/// Extract a longest plain-literal run that MUST appear in any match, for the
/// `memmem` prefilter. Conservative: returns a literal only when the whole pattern
/// is plain (no regex metacharacters), so we never drop a line that could match.
/// (A richer HIR analysis is possible but this captures the common keyword case —
/// `csift search "carry"` — with zero false negatives.)
///
/// **JSON-escape safety (load-bearing — SPEC §0 "no silent truncation").** The
/// prefilter runs the literal against the RAW JSON line bytes, where string content
/// is JSON-encoded: `"` is stored as `\"`, `\` as `\\`, and every control char
/// (`< 0x20`) plus DEL (`0x7f`) as a `\uXXXX`/`\n`/`\t`/… escape. A literal
/// containing any such char therefore can NOT appear verbatim in the raw line — a
/// `memmem` for it would falsely report "absent" and silently drop a line whose
/// DECODED text actually matches (e.g. searching `Say"Xello`). So we refuse to emit
/// a literal prefilter whenever the pattern contains a JSON-escaped character; the
/// match then falls back to running the regex on the raw bytes (still pre-JSON, just
/// without the cheap literal short-circuit). Non-ASCII (`>= 0x80`) is emitted
/// verbatim as UTF-8 by serde_json (multi-byte searches confirm this), so it stays
/// prefilter-eligible.
fn required_literal(pattern: &str) -> Option<Vec<u8>> {
    const META: &[char] = &[
        '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '^', '$', '\\',
    ];
    if pattern.is_empty() || pattern.chars().any(|c| META.contains(&c)) {
        return None;
    }
    // A char that JSON escapes inside a string does not survive verbatim in the raw
    // line bytes the prefilter scans — emitting a literal for it causes false
    // negatives. `\` is already excluded via META above; guard `"`, control chars,
    // and DEL here.
    if pattern.chars().any(json_escapes_in_string) {
        return None;
    }
    Some(pattern.as_bytes().to_vec())
}

/// True when `c` is escaped inside a JSON string literal (so it never appears
/// verbatim in the raw line bytes): `"`, any C0 control char (`< 0x20`), or DEL.
/// (`\` is handled separately as a regex metacharacter.)
fn json_escapes_in_string(c: char) -> bool {
    c == '"' || (c as u32) < 0x20 || c == '\u{7f}'
}

/// Entry point for `csift search`.
pub fn run_search(args: &SearchArgs) -> Result<()> {
    // ── Validate flag combinations up front (SPEC §6.2 validation) ──
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // A truly unbounded search (empty pattern + no filters) will emit a lot. Warn,
    // but do not refuse — SPEC §6.2 explicitly allows it.
    let matcher = build_matcher(args)?;
    if matcher.is_pure_filter()
        && args.categories.is_empty()
        && turn_range.is_none()
        && time_window.is_unbounded()
        && args.session.is_none()
    {
        eprintln!(
            "csift: warning: empty pattern with no category/time/turn/session filter \
             matches every exchange in scope — this may emit a lot."
        );
    }

    // ── Resolve targets → session files (optionally spanning subagents) ──
    // Shared resolver (path::resolve_session_files), used identically by agents/files.
    let session_files =
        path::resolve_session_files(&args.paths, args.session.as_deref(), args.want_subagents())?;

    // ── Parallel scan across files; collect order-stable, then merge ──
    let per_file: Vec<FileResult> = session_files
        .par_iter()
        .map(|p| search_one_file(p, args, &matcher, turn_range.as_ref(), &time_window))
        .collect::<Result<Vec<_>>>()?;

    // Deterministic merge: by (path order already sorted) → flatten exchanges in
    // file order, applying the GLOBAL --max-count cap across the whole corpus.
    let mut outcome = SearchOutcome::default();
    for fr in per_file {
        outcome.skipped_lines += fr.skipped_lines;
        for ex in fr.exchanges {
            match args.max_count {
                Some(cap) if outcome.exchanges.len() >= cap => outcome.dropped_by_cap += 1,
                _ => outcome.exchanges.push(ex),
            }
        }
    }

    match args.format {
        OutputFormat::Text => render_text(&outcome, args),
        OutputFormat::Json => render_json(&outcome)?,
    }
    Ok(())
}

/// Per-file scan result before the global cap is applied.
struct FileResult {
    exchanges: Vec<Exchange>,
    skipped_lines: usize,
}

/// A retained record. `can_hit` is the §7d keyword-prefilter verdict on the raw
/// line: when `false`, the line provably lacks the required literal, so it can
/// never be a regex hit and we skip the (more expensive) per-block regex matching
/// on it — but it is STILL retained so it can appear as a sibling record in a
/// matched turn's complete round-trip (SPEC §6.4). When the matcher has no
/// anchorable literal (case-insensitive or regex-with-metachars) every record is
/// `can_hit`.
struct Kept {
    rec: Record,
    can_hit: bool,
}

/// Scan a single session file: prefilter → parse → delimit turns → match → stitch.
fn search_one_file(
    path: &Path,
    args: &SearchArgs,
    matcher: &Matcher,
    turn_range: Option<&(usize, usize)>,
    time_window: &TimeWindow,
) -> Result<FileResult> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(FileResult {
            exchanges: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // Retain every TRANSCRIPT record in file order (genuine users delimit turns;
    // the rest are turn members). Two-stage prefilter (§7d):
    //   1. CATEGORY prefilter — drop pure-noise lines (attachment/system/metadata)
    //      pre-JSON. This is the dominant cost win (attachment alone is 54% of
    //      records). Broad-by-design (a role substring) so no genuine turn is lost.
    //   2. KEYWORD prefilter — a per-line `memmem` of the regex's required literal.
    //      It does NOT gate parsing (a non-matching record may still be a sibling in
    //      a matched turn's round-trip); instead it records `can_hit`, letting the
    //      match phase skip regex work on records that provably can't match.
    let mut records: Vec<Kept> = Vec::new();
    let mut skipped = 0usize;

    scan_lines_bytes(bytes, |line| {
        if !line_is_transcript_candidate(line) {
            return;
        }
        let can_hit = matcher.line_may_match(line);
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => records.push(Kept { rec, can_hit }),
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    })?;

    let exchanges = reconstruct_and_match(path, &records, args, matcher, turn_range, time_window);

    Ok(FileResult {
        exchanges,
        skipped_lines: skipped,
    })
}

/// §7d stage-1 category prefilter on raw bytes: keep a line only if it could be a
/// transcript message (user/assistant role marker) — drops `attachment`, `system`,
/// `file-history-snapshot`, `queue-operation`, and metadata noise pre-JSON. Kept
/// deliberately permissive (substring, not structural) so no genuine turn is lost.
fn line_is_transcript_candidate(line: &[u8]) -> bool {
    // Every user/assistant record carries `"role":"user"`/`"role":"assistant"`.
    // (Genuine-user string content, tool carriers, assistant blocks all do.)
    memmem::find(line, br#""role":"user""#).is_some()
        || memmem::find(line, br#""role":"assistant""#).is_some()
}

/// Walk retained records in file order, delimit turns by genuine-user records, and
/// for each turn decide whether it matches the filters + regex; emit a complete
/// Exchange per matching turn.
fn reconstruct_and_match(
    path: &Path,
    records: &[Kept],
    args: &SearchArgs,
    matcher: &Matcher,
    turn_range: Option<&(usize, usize)>,
    time_window: &TimeWindow,
) -> Vec<Exchange> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_default();

    // Group records into turns via the shared §6.4 delimiter (model::group_turn_indices
    // is the single source of truth, used identically by `files`). The outer index is
    // the 0-based turn index; map each index group back to its `Kept` borrows.
    let index_turns = group_turn_indices(records, |k| k.rec.is_genuine_user());

    let want_categories = &args.categories;
    let mut out = Vec::new();

    for (turn_index, idxs) in index_turns.iter().enumerate() {
        // Turn-range filter (inclusive, 0-based on genuine-user order).
        if let Some(&(lo, hi)) = turn_range {
            if turn_index < lo || turn_index > hi {
                continue;
            }
        }

        let turn = Turn {
            index: turn_index,
            records: idxs.iter().map(|&i| &records[i]).collect(),
        };

        // Collect the hits in this turn that satisfy category + time + regex.
        let hits = collect_turn_hits(
            &turn,
            want_categories,
            matcher,
            time_window,
            args.resolve_persisted,
        );
        if hits.is_empty() {
            continue;
        }

        let record_uuids = turn
            .records
            .iter()
            .filter_map(|k| k.rec.uuid.clone())
            .collect();

        out.push(Exchange {
            session_id: session_id.clone(),
            turn_index: turn.index,
            hits,
            record_uuids,
        });
    }

    out
}

/// One reconstructed turn (the opening genuine-user record + every record chained
/// under it, in file order).
struct Turn<'a> {
    index: usize,
    records: Vec<&'a Kept>,
}

/// Gather the category-eligible, time-windowed, regex-matching hits inside a turn.
fn collect_turn_hits(
    turn: &Turn<'_>,
    want: &[Category],
    matcher: &Matcher,
    time_window: &TimeWindow,
    resolve_persisted: bool,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    for kept in &turn.records {
        // §7d keyword prefilter: if the raw line provably lacks the required
        // literal, this record can't be a hit — skip the regex work. (It still
        // stays a member of this turn for the complete round-trip; we just don't
        // emit a hit for it.)
        if !kept.can_hit {
            continue;
        }
        let rec = &kept.rec;
        // Time window applies per-record (records with no timestamp never match a
        // bounded window, per SPEC §6.2).
        if !time_window.contains(rec.timestamp.as_deref()) {
            continue;
        }
        collect_record_hits(rec, want, matcher, resolve_persisted, &mut hits);
    }
    hits
}

/// Emit hits for every category-eligible piece of `rec` that matches the regex.
fn collect_record_hits(
    rec: &Record,
    want: &[Category],
    matcher: &Matcher,
    resolve_persisted: bool,
    hits: &mut Vec<Hit>,
) {
    let ts = rec.timestamp.clone();

    // ── user category: genuine-user text OR an AUQ answer (§4.1, §5) ──
    if category_active(want, Category::User) {
        if let Some(text) = rec.genuine_user_text() {
            if matcher.is_match(&text) {
                hits.push(make_hit(Category::User, &text, ts.clone(), None));
            }
        } else if rec.is_auq_answer() {
            // Surface the AUQ answer string under `user` (§4.1 exception).
            if let Some(text) = auq_answer_text(rec) {
                if matcher.is_match(&text) {
                    hits.push(make_hit(Category::User, &text, ts.clone(), None));
                }
            }
        }
    }

    // ── block-bearing categories: thinking / tool / tool-response / agent ──
    if let Some(blocks) = rec.blocks() {
        for block in blocks {
            match block {
                Block::Thinking { thinking, .. } if category_active(want, Category::Thinking) => {
                    if matcher.is_match(thinking) {
                        hits.push(make_hit(Category::Thinking, thinking, ts.clone(), None));
                    }
                }
                Block::Text { text } if category_active(want, Category::Agent) => {
                    // Only assistant `text` blocks are the agent message; a user
                    // `text` block is genuine-user (handled above).
                    if rec.is_type("assistant") && matcher.is_match(text) {
                        hits.push(make_hit(Category::Agent, text, ts.clone(), None));
                    }
                }
                Block::ToolUse { name, input, .. } if category_active(want, Category::Tool) => {
                    let rendered = render_tool_use(name.as_deref(), input.as_ref());
                    if matcher.is_match(&rendered) {
                        hits.push(make_hit(
                            Category::Tool,
                            &rendered,
                            ts.clone(),
                            name.clone(),
                        ));
                    }
                }
                Block::ToolResult {
                    content: Some(c), ..
                } if category_active(want, Category::ToolResponse) => {
                    let mut text = tool_result_content_text(c);
                    // §5 de-dup: an AUQ answer IS a tool_result, so it is eligible for
                    // BOTH `user` (the §4.1 exception, emitted above) and
                    // `tool-response`. "Do not double-count within a single emitted
                    // exchange" — so when the `user` category is ALSO active we skip
                    // the tool-response copy (the answer already surfaced as `user`).
                    // A `-t tool-response` filter that does NOT name `user` still
                    // surfaces it, so the answer is never lost, just not duplicated.
                    if is_auq_answer_text(&text) && category_active(want, Category::User) {
                        continue;
                    }
                    // §4.6: when asked, replace the inline persisted-output pointer
                    // with the real file content (matching runs against the resolved
                    // text so a regex can hit the full output).
                    if resolve_persisted {
                        if let Some(path) = rec.persisted_output_path() {
                            text = resolve_persisted_text(&path, &text);
                        }
                    }
                    if matcher.is_match(&text) {
                        hits.push(make_hit(Category::ToolResponse, &text, ts.clone(), None));
                    }
                }
                _ => {}
            }
        }
    }
}

/// True when `cat` is requested (or no category filter was given ⇒ all eligible).
fn category_active(want: &[Category], cat: Category) -> bool {
    want.is_empty() || want.contains(&cat)
}

/// Render a `tool_use` block to searchable text: `name {json-input}`. The name is
/// matched first so `csift search AskUserQuestion -t tool` works; the input JSON is
/// included so a regex can match arguments too.
fn render_tool_use(name: Option<&str>, input: Option<&serde_json::Value>) -> String {
    let mut s = String::new();
    if let Some(n) = name {
        s.push_str(n);
    }
    if let Some(v) = input {
        s.push(' ');
        s.push_str(&v.to_string());
    }
    s
}

/// Resolve a `<persisted-output>` pointer (§4.6) to the referenced file's content.
/// On a read failure the inline text is kept and an explicit note appended — a
/// missing persisted file is reported, never fatal (SPEC §4.6).
fn resolve_persisted_text(path: &str, inline: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => format!("{inline}\n[csift: could not resolve persisted output {path}: {e}]"),
    }
}

/// The synthesized AUQ-answer string from a carrier's `tool_result` (§4.4). Matches
/// any known AUQ-answer marker (both shipped phrasings, see `model::AUQ_ANSWER_MARKERS`).
fn auq_answer_text(rec: &Record) -> Option<String> {
    let blocks = rec.blocks()?;
    for b in blocks {
        if let Block::ToolResult {
            content: Some(c), ..
        } = b
        {
            let t = tool_result_content_text(c);
            if is_auq_answer_text(&t) {
                return Some(t);
            }
        }
    }
    None
}

/// Build a hit with a normalized, explicitly-truncated excerpt.
fn make_hit(category: Category, text: &str, ts: Option<String>, tool_name: Option<String>) -> Hit {
    Hit {
        category,
        excerpt: truncate_excerpt(&normalize_line(text)),
        timestamp_utc: ts,
        tool_name,
    }
}

/// Truncate to [`EXCERPT_MAX`] chars with an explicit `… (+N chars)` marker
/// (CHARACTER-counted so multi-byte UTF-8 truncates cleanly). Never silent
/// (SPEC §0, §8.1).
fn truncate_excerpt(s: &str) -> String {
    let total = s.chars().count();
    if total <= EXCERPT_MAX {
        return s.to_string();
    }
    let kept: String = s.chars().take(EXCERPT_MAX).collect();
    format!("{kept}… (+{} chars)", total - EXCERPT_MAX)
}

/// Parse a `--turn-range START..END` string into an inclusive `(lo, hi)` pair.
/// Both bounds are 0-based, inclusive; `END < START` is an error.
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

// ── Rendering ──
//
// Timestamp formatting (system-local + raw UTC) lives in `crate::timez`, shared
// with `list` so the local-timezone choice is defined once.

fn category_label(c: Category) -> &'static str {
    match c {
        Category::Thinking => "thinking",
        Category::User => "user",
        Category::Tool => "tool",
        Category::ToolResponse => "tool-response",
        Category::Agent => "agent",
    }
}

/// Glyph for the side of the exchange a hit sits on (◂ user, ▸ agent-side).
fn category_glyph(c: Category) -> char {
    match c {
        Category::User => '◂',
        _ => '▸',
    }
}

fn render_text(outcome: &SearchOutcome, args: &SearchArgs) {
    if outcome.exchanges.is_empty() {
        println!("no matching exchanges");
        if outcome.skipped_lines > 0 {
            println!("({} malformed line(s) skipped)", outcome.skipped_lines);
        }
        return;
    }

    for (i, ex) in outcome.exchanges.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!(
            "═══ SESSION {} · TURN {} ═══",
            short_id(&ex.session_id),
            ex.turn_index
        );
        for hit in &ex.hits {
            let glyph = category_glyph(hit.category);
            let label = category_label(hit.category);
            let name = hit
                .tool_name
                .as_deref()
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            println!(
                "{glyph} {label}{name}  {}",
                format_timestamp(hit.timestamp_utc.as_deref())
            );
            println!("   {}", hit.excerpt);
        }
    }

    // Footer with match + drop accounting (no silent truncation).
    let cat = if args.categories.is_empty() {
        "all".to_string()
    } else {
        args.categories
            .iter()
            .map(|c| category_label(*c))
            .collect::<Vec<_>>()
            .join(",")
    };
    println!();
    let plural = if outcome.exchanges.len() == 1 {
        "exchange"
    } else {
        "exchanges"
    };
    print!(
        "matched {} {plural} (category={cat})  ·  {} dropped",
        outcome.exchanges.len(),
        outcome.dropped_by_cap
    );
    if outcome.dropped_by_cap > 0 {
        print!(" by --max-count");
    }
    println!();
    if outcome.skipped_lines > 0 {
        println!("({} malformed line(s) skipped)", outcome.skipped_lines);
    }
}

fn render_json(outcome: &SearchOutcome) -> Result<()> {
    use serde_json::json;
    for ex in &outcome.exchanges {
        let hits: Vec<_> = ex
            .hits
            .iter()
            .map(|h| {
                json!({
                    "category": category_label(h.category),
                    "excerpt": h.excerpt,
                    "ts_utc": h.timestamp_utc,
                    "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
                    "tool_name": h.tool_name,
                })
            })
            .collect();
        let obj = json!({
            "session_id": ex.session_id,
            "turn_index": ex.turn_index,
            "hits": hits,
            "record_uuids": ex.record_uuids,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    // Trailing summary object (SPEC §8.2).
    let summary = json!({
        "matched": outcome.exchanges.len(),
        "dropped_by_cap": outcome.dropped_by_cap,
        "skipped_lines": outcome.skipped_lines,
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

/// Shorten a uuid to its first segment for compact headers (full id stays in JSON).
fn short_id(id: &str) -> String {
    match id.split_once('-') {
        Some((head, _)) => format!("{head}…"),
        None => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::OutputFormat;

    fn args(pattern: &str) -> SearchArgs {
        SearchArgs {
            pattern: pattern.to_string(),
            paths: Vec::new(),
            session: None,
            categories: Vec::new(),
            ignore_case: false,
            multiline: false,
            turn_range: None,
            since: None,
            until: None,
            max_count: None,
            resolve_persisted: false,
            include_subagents: true,
            no_subagents: false,
            format: OutputFormat::Text,
        }
    }

    fn rec(line: &str) -> Record {
        serde_json::from_str(line).expect("valid record")
    }

    #[test]
    fn smart_case_lowercase_is_insensitive() {
        let m = build_matcher(&args("carry")).unwrap();
        assert!(m.is_match("the CARRY logic"));
        assert!(m.is_match("the carry logic"));
    }

    #[test]
    fn smart_case_uppercase_is_sensitive() {
        let m = build_matcher(&args("Carry")).unwrap();
        assert!(m.is_match("the Carry logic"));
        assert!(!m.is_match("the carry logic"));
    }

    #[test]
    fn ignore_case_overrides_smart_case() {
        let mut a = args("Carry");
        a.ignore_case = true;
        let m = build_matcher(&a).unwrap();
        assert!(m.is_match("the carry logic"));
    }

    #[test]
    fn empty_pattern_is_pure_filter_matches_all() {
        let m = build_matcher(&args("")).unwrap();
        assert!(m.is_pure_filter());
        assert!(m.is_match("literally anything"));
    }

    #[test]
    fn multiline_dot_crosses_newline() {
        let mut a = args("foo.*bar");
        a.multiline = true;
        let m = build_matcher(&a).unwrap();
        assert!(m.is_match("foo\nmiddle\nbar"));
        let m2 = build_matcher(&args("foo.*bar")).unwrap();
        assert!(!m2.is_match("foo\nbar"));
    }

    #[test]
    fn required_literal_only_for_plain_patterns() {
        assert_eq!(required_literal("carry"), Some(b"carry".to_vec()));
        assert!(required_literal("ca.ry").is_none());
        assert!(required_literal("a|b").is_none());
    }

    #[test]
    fn required_literal_rejects_json_escaped_chars() {
        // DEFECT 2: a pattern char that JSON escapes inside a string ('"', control
        // chars, DEL) does not appear verbatim in the raw JSON line, so a memmem
        // prefilter for it would silently drop a line whose decoded text matches.
        // Such patterns must NOT get a literal prefilter (fall back to regex).
        assert!(
            required_literal("Say\"Xello").is_none(),
            "a quote-containing literal must not be prefiltered"
        );
        assert!(required_literal("a\tb").is_none(), "tab is JSON-escaped");
        assert!(
            required_literal("a\nb").is_none(),
            "newline is JSON-escaped"
        );
        // Non-ASCII multi-byte UTF-8 is emitted verbatim by serde_json → still
        // prefilter-eligible (no JSON escaping). Use a locale-neutral fixture
        // (accented Latin + an emoji, both multi-byte) to prove the bytes pass
        // through unchanged.
        assert_eq!(required_literal("café🛠"), Some("café🛠".as_bytes().to_vec()));
    }

    #[test]
    fn quote_pattern_no_silent_drop_case_sensitive() {
        // DEFECT 2 end-to-end: a record whose DECODED text is `Say"Xello there`. The
        // raw line stores the quote escaped as \". A case-sensitive search for
        // `Say"Xello` must STILL match (no literal prefilter → regex runs on decoded
        // text), not silently drop the hit (SPEC §0). Build the matcher with an
        // uppercase letter so smart-case stays case-SENSITIVE (the buggy path).
        let m = build_matcher(&args("Say\"Xello")).unwrap();
        assert!(
            m.prefilter.is_none(),
            "no byte prefilter for a quote-containing literal"
        );
        // The decoded text matches the regex.
        assert!(m.is_match("Say\"Xello there"));
        // And the raw-line gate must NOT drop the carrier (can't prove absence).
        let raw = br#"{"type":"user","message":{"role":"user","content":"Say\"Xello there"}}"#;
        assert!(
            m.line_may_match(raw),
            "without a literal prefilter the line passes to the regex stage"
        );
    }

    #[test]
    fn prefilter_drops_lines_without_literal() {
        let m = build_matcher(&args("carry")).unwrap();
        // smart-case lowercased → case-insensitive → no byte prefilter (see build).
        assert!(m.prefilter.is_none());
        // A case-sensitive plain literal DOES get a prefilter.
        let m2 = build_matcher(&args("Carry")).unwrap();
        assert!(m2.prefilter.is_some());
        assert!(m2.line_may_match(b"...the Carry logic..."));
        assert!(!m2.line_may_match(b"...nothing relevant..."));
    }

    #[test]
    fn turn_range_parsing() {
        assert_eq!(parse_turn_range("10..20").unwrap(), (10, 20));
        assert_eq!(parse_turn_range("0..0").unwrap(), (0, 0));
        assert!(parse_turn_range("20..10").is_err());
        assert!(parse_turn_range("notarange").is_err());
        assert!(parse_turn_range("a..b").is_err());
    }

    #[test]
    fn resolve_persisted_text_reads_file_or_notes_failure() {
        // Success: the resolved text is the file content (not the inline pointer).
        let dir = std::env::temp_dir();
        let p = dir.join(format!("csift-persist-test-{}.txt", std::process::id()));
        std::fs::write(&p, "THE REAL PERSISTED BODY with a deep token zzqqxx").unwrap();
        let resolved = resolve_persisted_text(&p.to_string_lossy(), "<persisted-output> pointer");
        assert!(resolved.contains("zzqqxx"), "got: {resolved}");
        assert!(
            !resolved.contains("pointer"),
            "inline pointer should be replaced"
        );
        std::fs::remove_file(&p).ok();

        // Failure: a missing file keeps the inline text + an explicit note (never fatal).
        let missing = resolve_persisted_text("/no/such/csift/file.txt", "inline preview text");
        assert!(missing.contains("inline preview text"));
        assert!(missing.contains("could not resolve persisted output"));
    }

    #[test]
    fn resolve_persisted_end_to_end_matches_deep_token() {
        // Build a carrier whose inline tool_result is a <persisted-output> pointer to
        // a temp file; a token that lives ONLY in the file (not inline) must match
        // ONLY when --resolve-persisted is set. This is the discriminating test.
        let dir = std::env::temp_dir();
        let p = dir.join(format!("csift-e2e-persist-{}.txt", std::process::id()));
        std::fs::write(&p, "deep file body containing the token wibblewobble here").unwrap();
        let line = format!(
            r#"{{"type":"user","uuid":"u0","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (1 KB). Full output saved to: {}\n\nPreview (first 2KB):\n(no token here)\n</persisted-output>"}}]}}}}"#,
            p.to_string_lossy()
        );
        let r: Record = serde_json::from_str(&line).expect("valid record");

        // WITHOUT resolution: the deep token is not in the inline content → no hit.
        let m = build_matcher(&args("wibblewobble")).unwrap();
        let mut no_resolve = Vec::new();
        collect_record_hits(&r, &[Category::ToolResponse], &m, false, &mut no_resolve);
        assert!(no_resolve.is_empty(), "deep token must NOT match inline");

        // WITH resolution: the file is read, the token is found → exactly one hit.
        let mut with_resolve = Vec::new();
        collect_record_hits(&r, &[Category::ToolResponse], &m, true, &mut with_resolve);
        assert_eq!(with_resolve.len(), 1, "deep token matches after resolution");
        assert_eq!(with_resolve[0].category, Category::ToolResponse);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn collect_hits_thinking_category() {
        let r = rec(
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"the carry holds a partial line"},{"type":"text","text":"done"}]}}"#,
        );
        let m = build_matcher(&args("carry")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::Thinking], &m, false, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].category, Category::Thinking);
        assert!(hits[0].excerpt.contains("carry"));
    }

    #[test]
    fn collect_hits_agent_text_only_from_assistant() {
        let r = rec(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"the answer is foo"}]}}"#,
        );
        let m = build_matcher(&args("foo")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::Agent], &m, false, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].category, Category::Agent);
    }

    #[test]
    fn collect_hits_tool_use_matches_name_and_input() {
        let r = rec(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        );
        let m = build_matcher(&args("AskUserQuestion")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::Tool], &m, false, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool_name.as_deref(), Some("AskUserQuestion"));
    }

    #[test]
    fn collect_hits_auq_answer_under_user() {
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Q\"=\"chosen\". You can now continue."}]}}"#,
        );
        let m = build_matcher(&args("chosen")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::User], &m, false, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].category, Category::User);
    }

    #[test]
    fn tool_result_carrier_not_a_user_hit_when_plain() {
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"plain output here"}]}}"#,
        );
        let m = build_matcher(&args("output")).unwrap();
        let mut hits = Vec::new();
        // User category must NOT surface a plain tool_result carrier.
        collect_record_hits(&r, &[Category::User], &m, false, &mut hits);
        assert_eq!(hits.len(), 0);
        // But the tool-response category does.
        let mut hits2 = Vec::new();
        collect_record_hits(&r, &[Category::ToolResponse], &m, false, &mut hits2);
        assert_eq!(hits2.len(), 1);
        assert_eq!(hits2[0].category, Category::ToolResponse);
    }

    // ── End-to-end exchange reconstruction over a synthetic multi-turn fixture ──
    //
    // Records as they appear in a real jsonl (file order). Two genuine-user turns;
    // each expands into the assistant chain + a tool round-trip. Interleaved noise:
    // an isMeta pseudo-turn and a tool_result carrier — NEITHER may start a turn.

    /// The synthetic session, one jsonl line per element, in file order.
    fn fixture() -> Vec<&'static str> {
        vec![
            // ── turn 0 ──
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"why is the tail-read carry needed?"}}"#,
            r#"{"type":"assistant","uuid":"a0t","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"the carry holds an incomplete line straddling a chunk boundary"}]}}"#,
            r#"{"type":"assistant","uuid":"a0u","parentUuid":"a0t","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Read","input":{"file":"parse.rs"}}]}}"#,
            r#"{"type":"user","uuid":"c0","parentUuid":"a0u","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call0","content":"the carry is the partial line at the low edge"}]}}"#,
            r#"{"type":"assistant","uuid":"a0f","parentUuid":"c0","timestamp":"2026-06-07T05:00:40.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The carry is the partial line at the low-offset edge of each chunk."}]}}"#,
            // An isMeta pseudo-turn — looks human, must NOT open a turn.
            r#"{"type":"user","uuid":"meta","isMeta":true,"parentUuid":"a0f","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"Continue from where you left off."}}"#,
            // ── turn 1 ──
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"now explain the panic path"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"There is no panic on a malformed line — it is skipped and counted."}]}}"#,
        ]
    }

    fn kept_fixture(lines: &[&str], matcher: &Matcher) -> Vec<Kept> {
        lines
            .iter()
            .map(|l| {
                let raw = l.as_bytes();
                Kept {
                    rec: serde_json::from_slice(raw).expect("valid fixture record"),
                    can_hit: matcher.line_may_match(raw),
                }
            })
            .collect()
    }

    fn search(lines: &[&str], a: &SearchArgs) -> Vec<Exchange> {
        let matcher = build_matcher(a).unwrap();
        let kept = kept_fixture(lines, &matcher);
        let tr = a
            .turn_range
            .as_deref()
            .map(parse_turn_range)
            .transpose()
            .unwrap();
        let tw = TimeWindow::from_args(a.since.as_deref(), a.until.as_deref()).unwrap();
        reconstruct_and_match(
            std::path::Path::new("/x/0a1b2c3d-0000-0000-0000-000000000000.jsonl"),
            &kept,
            a,
            &matcher,
            tr.as_ref(),
            &tw,
        )
    }

    #[test]
    fn turn_delimiting_two_genuine_users_only() {
        // A regex that matches in both turns; isMeta + carrier must not add turns.
        let ex = search(&fixture(), &args("the"));
        let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
        assert_eq!(
            indices,
            vec![0, 1],
            "exactly two turns, 0-based, no meta turn"
        );
    }

    #[test]
    fn exchange_returns_full_round_trip() {
        // Match only the thinking block; the emitted exchange's record_uuids must
        // include the WHOLE turn 0 chain (user, thinking, tool_use, carrier, agent),
        // proving the complete round-trip is stitched, not just the matched record.
        let mut a = args("straddling");
        a.categories = vec![Category::Thinking];
        let ex = search(&fixture(), &a);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].turn_index, 0);
        assert_eq!(ex[0].hits.len(), 1);
        assert_eq!(ex[0].hits[0].category, Category::Thinking);
        // Full turn membership (the carry's complete round-trip).
        let uuids = &ex[0].record_uuids;
        for expected in ["u0", "a0t", "a0u", "c0", "a0f"] {
            assert!(uuids.contains(&expected.to_string()), "missing {expected}");
        }
        // The isMeta record belongs to NEITHER turn's body (it sits after turn 0's
        // agent but before turn 1's user; our grouping appends it to turn 0 as a
        // member — but it is not a turn delimiter). It must be present as a member
        // of turn 0 (a sibling record), never as its own turn.
        assert!(uuids.contains(&"meta".to_string()));
    }

    #[test]
    fn category_filter_restricts_hits() {
        // `-t agent` over a pattern present in both an agent text and a thinking
        // block must only surface the agent hit.
        let mut a = args("carry");
        a.categories = vec![Category::Agent];
        let ex = search(&fixture(), &a);
        // "carry" appears in turn 0's thinking AND agent text; with -t agent only
        // the agent hit is emitted (one exchange, one agent hit).
        assert_eq!(ex.len(), 1);
        assert!(ex[0].hits.iter().all(|h| h.category == Category::Agent));
    }

    #[test]
    fn turn_range_filter_selects_turn() {
        let mut a = args("");
        a.turn_range = Some("1..1".to_string());
        let ex = search(&fixture(), &a);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].turn_index, 1);
    }

    #[test]
    fn time_window_filters_by_timestamp() {
        // turn 1's records are at 06:00; a since=06:00 window drops turn 0 entirely
        // (its records are at 05:00) and the isMeta member at 05:01.
        let mut a = args("");
        a.since = Some("2026-06-07T06:00:00Z".to_string());
        let ex = search(&fixture(), &a);
        let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
        assert_eq!(indices, vec![1], "only the 06:00 turn survives the window");
    }

    #[test]
    fn empty_pattern_pure_filter_emits_all_turns() {
        let ex = search(&fixture(), &args(""));
        assert_eq!(ex.len(), 2, "empty pattern matches every turn");
    }

    #[test]
    fn auq_answer_surfaces_under_user_category_end_to_end() {
        let lines = vec![
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the bold option\". You can now continue."}]}}"#,
        ];
        let mut a = args("bold option");
        a.categories = vec![Category::User];
        let ex = search(&lines, &a);
        // The AUQ answer is surfaced under `user` even though it rides on a carrier,
        // and it did NOT start a new turn (still turn 0).
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].turn_index, 0);
        assert!(ex[0]
            .hits
            .iter()
            .any(|h| h.category == Category::User && h.excerpt.contains("bold option")));
    }

    #[test]
    fn auq_alternate_phrasing_surfaces_under_user_end_to_end() {
        // DEFECT 1: the dominant real-data phrasing ("Your questions have been
        // answered: …") must surface under `user`, exactly like the other phrasing.
        // (Previously the single hardcoded marker missed it entirely.)
        let lines = vec![
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"which?\"=\"the teal option\". You can now continue with these answers in mind."}]}}"#,
        ];
        let mut a = args("teal option");
        a.categories = vec![Category::User];
        let ex = search(&lines, &a);
        assert_eq!(
            ex.len(),
            1,
            "alternate AUQ phrasing must surface a user hit"
        );
        assert_eq!(ex[0].turn_index, 0);
        assert!(ex[0]
            .hits
            .iter()
            .any(|h| h.category == Category::User && h.excerpt.contains("teal option")));
    }

    #[test]
    fn auq_answer_not_double_counted_no_category_filter() {
        // DEFECT 3: with NO -t filter (all categories active) an AUQ answer must be
        // surfaced ONCE (under `user`), NOT twice (also `tool-response`). SPEC §5.
        let lines = vec![
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"zzqq choice\". You can now continue."}]}}"#,
        ];
        // No category filter → all five eligible.
        let ex = search(&lines, &args("zzqq"));
        assert_eq!(ex.len(), 1);
        let cats: Vec<Category> = ex[0].hits.iter().map(|h| h.category).collect();
        assert_eq!(
            cats,
            vec![Category::User],
            "AUQ answer must appear once under `user`, not also tool-response"
        );
    }

    // ── Branch-completeness for the pure helpers ──

    #[test]
    fn short_id_with_and_without_dash() {
        assert_eq!(short_id("0a1b2c3d-4e5f-4a6b"), "0a1b2c3d…");
        // A uuid-less id with no dash → returned verbatim (the `None` arm).
        assert_eq!(short_id("nodashes"), "nodashes");
    }

    #[test]
    fn category_label_and_glyph_all_variants() {
        for (c, label, glyph) in [
            (Category::Thinking, "thinking", '▸'),
            (Category::User, "user", '◂'),
            (Category::Tool, "tool", '▸'),
            (Category::ToolResponse, "tool-response", '▸'),
            (Category::Agent, "agent", '▸'),
        ] {
            assert_eq!(category_label(c), label);
            assert_eq!(category_glyph(c), glyph);
        }
    }

    #[test]
    fn render_tool_use_name_only_input_only_both_neither() {
        assert_eq!(render_tool_use(Some("Bash"), None), "Bash");
        let v = serde_json::json!({"k":"v"});
        // input only (no name) → leading space then the json.
        assert_eq!(render_tool_use(None, Some(&v)), " {\"k\":\"v\"}");
        // both
        assert_eq!(
            render_tool_use(Some("Read"), Some(&v)),
            "Read {\"k\":\"v\"}"
        );
        // neither → empty
        assert_eq!(render_tool_use(None, None), "");
    }

    #[test]
    fn auq_answer_text_none_when_no_blocks_or_no_marker() {
        // A record with string content → no blocks → None (the `blocks()?` arm).
        let r = rec(r#"{"type":"user","message":{"role":"user","content":"plain string"}}"#);
        assert!(auq_answer_text(&r).is_none());
        // A carrier whose tool_result is NOT an AUQ answer → None (loop falls through).
        let r2 = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"just normal output"}]}}"#,
        );
        assert!(auq_answer_text(&r2).is_none());
    }

    #[test]
    fn auq_answer_text_skips_non_tool_result_blocks() {
        // The helper's loop must skip a non-ToolResult block (the `if let
        // Block::ToolResult` FALSE arm) and still find the AUQ answer in a later one.
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}},{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"q\"=\"the picked one\"."}]}}"#,
        );
        assert_eq!(
            auq_answer_text(&r).as_deref(),
            Some("User has answered your questions: \"q\"=\"the picked one\".")
        );
    }

    #[test]
    fn auq_answer_under_user_present_but_pattern_does_not_match() {
        // is_auq_answer is true and auq_answer_text returns Some, but the regex does
        // NOT match the answer → the `matcher.is_match(&text)` FALSE arm: no hit.
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"q\"=\"alpha\"."}]}}"#,
        );
        // Pattern present in NEITHER a genuine-user text (there is none) NOR the answer.
        let m = build_matcher(&args("zzzznomatch")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::User], &m, false, &mut hits);
        assert!(
            hits.is_empty(),
            "non-matching AUQ answer yields no user hit"
        );
    }

    #[test]
    fn truncate_excerpt_long_and_short() {
        assert_eq!(truncate_excerpt("short"), "short");
        let s = "y".repeat(EXCERPT_MAX + 3);
        let out = truncate_excerpt(&s);
        assert!(out.ends_with("… (+3 chars)"), "got: {out}");
    }

    #[test]
    fn collect_record_hits_can_hit_false_is_skipped_via_collect_turn_hits() {
        // A record marked `can_hit:false` is skipped before any regex work in
        // collect_turn_hits (the `if !kept.can_hit { continue }` arm).
        let m = build_matcher(&args("Carry")).unwrap(); // case-sensitive → has prefilter
                                                        // A line lacking the literal → can_hit=false.
        let raw = br#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"nothing relevant"}]}}"#;
        let kept = Kept {
            rec: serde_json::from_slice(raw).unwrap(),
            can_hit: m.line_may_match(raw),
        };
        assert!(!kept.can_hit);
        let turn = Turn {
            index: 0,
            records: vec![&kept],
        };
        let tw = TimeWindow::default();
        let hits = collect_turn_hits(&turn, &[], &m, &tw, false);
        assert!(hits.is_empty(), "a can_hit=false record yields no hits");
    }

    #[test]
    fn collect_turn_hits_excludes_record_outside_time_window() {
        // A record whose timestamp is outside a bounded window is skipped (the
        // `!time_window.contains(...)` arm), even when it would otherwise match.
        let m = build_matcher(&args("carry")).unwrap();
        let raw = br#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the carry"}]}}"#;
        let kept = Kept {
            rec: serde_json::from_slice(raw).unwrap(),
            can_hit: m.line_may_match(raw),
        };
        let turn = Turn {
            index: 0,
            records: vec![&kept],
        };
        // Window starting AFTER the record's timestamp → excluded.
        let tw = TimeWindow::from_args(Some("2026-06-07T06:00:00Z"), None).unwrap();
        assert!(collect_turn_hits(&turn, &[], &m, &tw, false).is_empty());
        // An unbounded window admits it.
        let tw2 = TimeWindow::default();
        assert!(!collect_turn_hits(&turn, &[], &m, &tw2, false).is_empty());
    }

    #[test]
    fn reconstruct_synthetic_lead_records_merge_into_first_real_turn() {
        // A file whose FIRST records are NOT genuine users (leading tool noise) must
        // fold into turn 0 once the first genuine user appears, and turns re-index
        // 0-based on genuine users (the synthetic_lead re-index branch).
        let lines = vec![
            // leading non-user noise (a tool_result carrier) — synthetic lead.
            r#"{"type":"user","uuid":"lead","timestamp":"2026-06-07T04:59:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"orphan carry note"}]}}"#,
            // first genuine user (turn 0).
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"real first about carry"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"answer carry"}]}}"#,
            // second genuine user (turn 1).
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second carry"}}"#,
        ];
        let ex = search(&lines, &args("carry"));
        let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
        assert_eq!(indices, vec![0, 1], "synthetic lead folds into turn 0");
        // The orphan lead record is a MEMBER of turn 0's round-trip.
        assert!(ex[0].record_uuids.contains(&"lead".to_string()));
    }

    #[test]
    fn reconstruct_only_synthetic_lead_no_genuine_user() {
        // A file with ONLY non-genuine records (no genuine user ever) → a single
        // standalone turn 0 holding the orphans (the `else` seed-turn-0 arm, and the
        // `turns.len() > 1` false guard so no re-index).
        let lines = vec![
            r#"{"type":"user","uuid":"o0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"orphan carry one"}]}}"#,
            r#"{"type":"user","uuid":"o1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"y","content":"orphan carry two"}]}}"#,
        ];
        // Search tool-response category so the orphan carriers can produce a hit.
        let mut a = args("carry");
        a.categories = vec![Category::ToolResponse];
        let ex = search(&lines, &a);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].turn_index, 0);
    }

    #[test]
    fn parse_turn_range_equal_bounds_ok() {
        // hi == lo is valid (single turn); only hi < lo errors.
        assert_eq!(parse_turn_range("5..5").unwrap(), (5, 5));
    }

    #[test]
    fn turn_range_excludes_below_lo_and_above_hi() {
        // The two-turn fixture: a range `0..0` keeps turn 0 and excludes turn 1 via
        // the `turn.index > hi` arm (complementing the `< lo` arm other tests cover).
        let mut a = args("");
        a.turn_range = Some("0..0".to_string());
        let ex = search(&fixture(), &a);
        let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
        assert_eq!(
            indices,
            vec![0],
            "only turn 0; turn 1 excluded by the > hi arm"
        );
    }

    #[test]
    fn collect_record_hits_resolve_persisted_with_no_pointer_keeps_inline() {
        // resolve_persisted=true but the tool_result has NO persisted pointer → the
        // `persisted_output_path()` None arm: the inline text is matched as-is.
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"plain inline output with token zzinline"}]}}"#,
        );
        let m = build_matcher(&args("zzinline")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::ToolResponse], &m, true, &mut hits);
        assert_eq!(
            hits.len(),
            1,
            "inline text still matches when there is no pointer"
        );
        assert_eq!(hits[0].category, Category::ToolResponse);
    }

    #[test]
    fn agent_text_block_only_from_assistant_not_user_text_block() {
        // A USER record with a text block must NOT surface under `agent` (the
        // `rec.is_type("assistant")` false arm of the agent-text branch).
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"user text foo"}]}}"#,
        );
        let m = build_matcher(&args("foo")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(&r, &[Category::Agent], &m, false, &mut hits);
        assert!(hits.is_empty(), "a user text block is not an agent hit");
    }

    #[test]
    fn auq_answer_still_surfaces_under_tool_response_alone() {
        // The de-dup must NOT hide the AUQ answer from a `-t tool-response` filter
        // that does not also name `user` — it is genuinely a tool_result.
        let lines = vec![
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"zzqq choice\". You can now continue."}]}}"#,
        ];
        let mut a = args("zzqq");
        a.categories = vec![Category::ToolResponse];
        let ex = search(&lines, &a);
        assert_eq!(ex.len(), 1);
        assert!(ex[0]
            .hits
            .iter()
            .all(|h| h.category == Category::ToolResponse));
        assert_eq!(ex[0].hits.len(), 1, "exactly one tool-response hit");
    }
}
