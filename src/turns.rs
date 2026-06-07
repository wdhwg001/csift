//! `turns` subcommand — turn-fidelity reconstruction.
//!
//! A Claude Code COMPACTION SUMMARY preserves task STATE (the 9-section synthesis:
//! intent, file ledger, errors+fixes, plan, next step) in high fidelity, but provably
//! LOSES turn fidelity — its "All user messages" section clips ~22 real prose turns to
//! ~17 `...`-truncated bullets, and the assistant side collapses to a SINGLE verbatim
//! quote (the last pre-compaction message). `turns` SUPPLEMENTS (never replaces) the
//! summary: it re-emits the clipped user phrasings + discarded assistant end-of-turn
//! replies, in ORIGINAL ORDER, each line carrying the jsonl LINE NUMBER so a consumer
//! can `Read` the raw transcript at the cited line.
//!
//! ## Reuse, never re-parse
//!
//! This module sits squarely on the `recover` extraction layer: the same
//! mmap → forward line-numbered [`crate::parse::scan_lines_bytes`] scan (so the local
//! `line_no` counter is a 1:1 map to the jsonl), the same
//! [`crate::model::group_turn_indices`] turn delimiter, the same `Record` helpers
//! ([`Record::is_genuine_user`] / [`Record::genuine_user_text`] /
//! [`Record::agent_text`] / [`Record::blocks`] / `is_compact_summary`), the same
//! [`crate::path::resolve_session_files`] / [`crate::time_window::TimeWindow`] /
//! [`crate::timez`] rendering. The `Record`/`Block` model needs no change.
//!
//! ## Selection vs render order
//!
//! Selection walks BACKWARD from EOF (recency-first) so the budget is spent on what a
//! resumed agent most needs; the emitted document is sorted ASCENDING so it reads as a
//! forward transcript. The backward walk is TRANSPARENT to `isCompactSummary` records
//! (a summary is a turn member, never a delimiter — `src/model.rs`), so it reaches back
//! across multiple compaction boundaries by default.
//!
//! ## Never fabricate, never silently drop
//!
//! An over-cap unit is MIDDLE-truncated (head+tail kept) with an explicit
//! `… [+K chars, L lines elided] …` marker that carries the exact elided counts; its
//! `Lnnnnn` points at the full record. Dedup against the live summary is
//! DEMOTE-AND-FLAG, never delete.

use std::path::Path;

use anyhow::{bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;

use crate::cli::{BudgetUnit, OutputFormat, TurnsArgs};
use crate::model::{group_turn_indices, normalize_line, Block, Content, Record};
use crate::parse::{mmap_bytes, scan_lines_bytes};
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Characters-per-token heuristic for `--budget-unit tokens`. CC compaction summaries
/// measure ~17K chars ≈ ~3.5–4.5K tokens, i.e. ~4 chars/token (documented estimate;
/// `turns` never tokenizes — it converts a token budget to a char budget by this ratio).
pub const TOKEN_CHARS: f64 = 4.0;

/// Pre-ellipsis full-keep ceiling for a USER unit (chars). Sized from the measured
/// user-message length distribution (median ~410, p90 ~2,574): a 600 cap keeps the
/// median turn whole and forces ellipsis only on the long tail.
const USER_CAP: usize = 600;

/// Pre-ellipsis full-keep ceiling for an ASSISTANT end-of-turn unit (chars). Larger
/// than [`USER_CAP`] because assistant EOT prose is 1.45–2.16× longer with more
/// newlines (measured) — so its head fraction is larger too (see [`ASST_HEAD_FRAC`]).
const ASST_CAP: usize = 900;

/// Head fraction for an ASSISTANT unit's middle-truncation: EOT prose front-loads
/// context and back-loads the decision, so keep ≈⅔ head / ⅓ tail.
const ASST_HEAD_FRAC: f64 = 0.66;

/// Head fraction for a USER unit: the ask is front-loaded, slightly less tail needed.
const USER_HEAD_FRAC: f64 = 0.60;

/// Fixed per-unit header-line cost charged in the budget (the `▽ Lnnnnn USER (ts)\n`
/// line). A conservative flat estimate — the real header is timestamp-dependent, but a
/// fixed charge keeps the budget model deterministic and is always ≥ the bare marker.
const HEADER_COST: usize = 24;

/// Normalized-prefix length used for the summary-dedup fingerprint (§6.2): a unit whose
/// first `DEDUP_PREFIX` normalized chars match a summary bullet/quote is flagged
/// `also_in_summary` and demoted. Strict (long) prefix ⇒ a false positive is unlikely.
const DEDUP_PREFIX: usize = 80;

// ─────────────────────────────────────────────────────────────────────────────
// Role + intermediate model
// ─────────────────────────────────────────────────────────────────────────────

/// One side of a turn the reconstruction can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// The pre-ellipsis full-keep ceiling for this role.
    fn cap(self) -> usize {
        match self {
            Role::User => USER_CAP,
            Role::Assistant => ASST_CAP,
        }
    }

    /// The head fraction for this role's middle-truncation.
    fn head_frac(self) -> f64 {
        match self {
            Role::User => USER_HEAD_FRAC,
            Role::Assistant => ASST_HEAD_FRAC,
        }
    }
}

/// One side of a turn (user opener OR assistant end-of-turn), with the full verbatim
/// text plus the data needed to render + budget it.
#[derive(Debug, Clone)]
struct TurnUnit {
    line_no: usize,
    role: Role,
    /// `chars().count()` of the normalized one-line full text.
    full_chars: usize,
    /// Normalized one-line text (rendered verbatim or middle-truncated later).
    text: String,
    /// Newline count of the ORIGINAL (pre-normalization) text — drives the
    /// `L lines elided` note (omitted when 0, i.e. a single-line message).
    orig_newlines: usize,
    ts_utc: Option<String>,
    /// True once dedup flags this unit as already present in the newest summary.
    also_in_summary: bool,
}

/// One reconstructable turn: the user opener, the tool-call count, the assistant EOT.
#[derive(Debug, Clone)]
struct TurnSlice {
    /// 0-based genuine-user turn index (from `group_turn_indices`).
    turn_index: usize,
    user: Option<TurnUnit>,
    /// `tool_use` block count across the turn → the `[N tool calls]` marker.
    tool_calls: usize,
    assistant_eot: Option<TurnUnit>,
    /// How many compaction boundaries sit between this turn and EOF (drives the
    /// boundary banners + dedup scope).
    compactions_before: usize,
}

impl TurnSlice {
    /// A round-trip-complete turn has BOTH a user opener and an assistant EOT.
    fn is_round_trip(&self) -> bool {
        self.user.is_some() && self.assistant_eot.is_some()
    }
}

/// The data a summary record contributes: its jsonl line + the dedup fingerprints of
/// the verbatim turns it already holds (§6 user bullets + §9 assistant quote).
#[derive(Debug, Clone)]
struct SummaryInfo {
    line_no: usize,
    /// Normalized-prefix fingerprints of everything the summary quotes verbatim.
    fingerprints: Vec<String>,
    /// Char length of the summary body (for the JSON boundary record).
    body_chars: usize,
}

/// A per-session scan result before global merge.
#[derive(Debug)]
struct ScanResult {
    session_id: String,
    turns: Vec<TurnSlice>,
    /// Summary records in file order (oldest → newest), each with its line + dedup set.
    summaries: Vec<SummaryInfo>,
    skipped_lines: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Entry point for `csift turns`.
pub fn run_turns(args: &TurnsArgs) -> Result<()> {
    // ── Validate window mutual-exclusion (same rule + wording as recover/files) ──
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }
    if !(args.round_trip_fraction > 0.0 && args.round_trip_fraction < 1.0) {
        bail!(
            "--round-trip-fraction must be in the open interval (0.0, 1.0), got {}",
            args.round_trip_fraction
        );
    }
    if args.budget == 0 {
        bail!("--budget must be > 0");
    }

    // Normalize the budget to characters.
    let budget_chars = match args.budget_unit {
        BudgetUnit::Chars => args.budget,
        // round-half-up; saturate so a giant token budget never overflows usize.
        BudgetUnit::Tokens => ((args.budget as f64) * TOKEN_CHARS).round() as usize,
    };

    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    let session_files =
        path::resolve_session_files(&args.paths, args.session.as_deref(), args.want_subagents())?;

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
        sr.turns.retain(|t| {
            let ts = t
                .user
                .as_ref()
                .and_then(|u| u.ts_utc.as_deref())
                .or_else(|| t.assistant_eot.as_ref().and_then(|a| a.ts_utc.as_deref()));
            window_admits(t.turn_index, ts, turn_range, &time_window)
        });
    }

    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| {
            plan_session(
                sr,
                budget_chars,
                args.round_trip_fraction,
                args.max_compactions,
            )
        })
        .collect();

    let ctx = RenderCtx {
        budget_chars,
        rt_fraction: args.round_trip_fraction,
        skipped_lines,
    };

    match args.format {
        OutputFormat::Text => render_text(&ctx, &sessions, &plans, args.out.as_deref())?,
        OutputFormat::Json => render_json(&ctx, &sessions, &plans, args.out.as_deref())?,
    }
    Ok(())
}

/// True when a turn at `turn_index` / `ts` is admitted by the active window. A
/// timestamp-less turn never falls inside a BOUNDED time window (same rule as recover).
fn window_admits(
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
fn scan_one_file(path: &Path) -> Result<ScanResult> {
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_default();

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(ScanResult {
            session_id,
            turns: Vec::new(),
            summaries: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    let mut records: Vec<(usize, Record)> = Vec::new();
    let mut skipped = 0usize;
    let mut line_no = 0usize;
    scan_lines_bytes(bytes, |line| {
        line_no += 1;
        if !line_is_turn_candidate(line) {
            return;
        }
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => records.push((line_no, rec)),
            Ok(None) => {}          // blank — counted above
            Err(_) => skipped += 1, // malformed — counted above
        }
    })?;

    let (turns, summaries) = build(&records);
    Ok(ScanResult {
        session_id,
        turns,
        summaries,
        skipped_lines: skipped,
    })
}

/// Pre-JSON byte prefilter — a SUPERSET of recover's `line_is_recover_candidate`,
/// broadened so a pure-text assistant turn (no Edit/Write/Read/Bash) is never missed.
/// Coarse by design; the structural parse decides what each line really is.
fn line_is_turn_candidate(line: &[u8]) -> bool {
    memmem::find(line, br#""role":"user""#).is_some()
        || memmem::find(line, br#""role":"assistant""#).is_some()
        || memmem::find(line, br#""type":"assistant""#).is_some()
        || memmem::find(line, b"isCompactSummary").is_some() // summaries: seeds + boundaries
        || memmem::find(line, b"tool_use").is_some() // for the [N tool calls] count
}

// ─────────────────────────────────────────────────────────────────────────────
// Build: (line_no, Record) → TurnSlice + SummaryInfo
// ─────────────────────────────────────────────────────────────────────────────

/// Build the per-turn slices + summary dedup sets from a session's line-numbered
/// records. Turn segmentation reuses the single shared engine
/// [`group_turn_indices`]; a compaction summary is a turn MEMBER (it is excluded from
/// genuine-user), so the walk is transparent to it.
fn build(records: &[(usize, Record)]) -> (Vec<TurnSlice>, Vec<SummaryInfo>) {
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices(&recs, |r| r.is_genuine_user());

    // Summary line numbers in file order (for compactions_before + boundary banners).
    let mut summaries: Vec<SummaryInfo> = Vec::new();
    for (line_no, rec) in records {
        if rec.is_compact_summary.unwrap_or(false) {
            if let Some(body) = compact_summary_body(rec) {
                summaries.push(SummaryInfo {
                    line_no: *line_no,
                    fingerprints: summary_fingerprints(&body),
                    body_chars: body.chars().count(),
                });
            }
        }
    }
    // Newest summary line (max), for compactions_before accounting.
    let summary_lines: Vec<usize> = summaries.iter().map(|s| s.line_no).collect();

    let mut slices: Vec<TurnSlice> = Vec::with_capacity(turns.len());
    for (turn_index, idxs) in turns.iter().enumerate() {
        let mut user: Option<TurnUnit> = None;
        let mut assistant_eot: Option<TurnUnit> = None;
        let mut tool_calls = 0usize;

        for &i in idxs {
            let (line_no, rec) = (records[i].0, &records[i].1);

            // Tool-call count: every ToolUse block in the turn's records.
            if let Some(blocks) = rec.blocks() {
                tool_calls += blocks
                    .iter()
                    .filter(|b| matches!(b, Block::ToolUse { .. }))
                    .count();
            }

            // The turn opener (genuine human). `group_turn_indices` opens a turn on a
            // genuine-user, so the FIRST genuine-user in the turn's records is the
            // opener; keep the earliest.
            if user.is_none() && rec.is_genuine_user() {
                if let Some(text) = rec.genuine_user_text() {
                    user = Some(make_unit(line_no, Role::User, &text, rec));
                }
            }

            // Assistant end-of-turn: keep the LAST assistant text in the turn (the
            // final visible reply, what the summary's §9 would quote).
            if let Some(text) = rec.agent_text() {
                assistant_eot = Some(make_unit(line_no, Role::Assistant, &text, rec));
            }
        }

        // `compactions_before` is keyed on the turn's CONTENT lines (its user opener /
        // assistant EOT), NOT on member records like a trailing summary that joins the
        // turn — a summary that opens a NEW compacted region must sit AFTER this turn's
        // content, so count summaries strictly above the turn's latest content line.
        let content_line = user
            .as_ref()
            .map(|u| u.line_no)
            .into_iter()
            .chain(assistant_eot.as_ref().map(|a| a.line_no))
            .max()
            .unwrap_or(0);
        let compactions_before = summary_lines.iter().filter(|&&s| s > content_line).count();

        slices.push(TurnSlice {
            turn_index,
            user,
            tool_calls,
            assistant_eot,
            compactions_before,
        });
    }

    (slices, summaries)
}

/// Build a [`TurnUnit`] from a record's already-normalized one-line `text`. The
/// `orig_newlines` count is taken from the record's ORIGINAL (pre-normalization) text so
/// the `L lines elided` note is meaningful.
fn make_unit(line_no: usize, role: Role, text: &str, rec: &Record) -> TurnUnit {
    let orig_newlines = raw_body_newlines(rec);
    TurnUnit {
        line_no,
        role,
        full_chars: text.chars().count(),
        text: text.to_string(),
        orig_newlines,
        ts_utc: rec.timestamp.clone(),
        also_in_summary: false,
    }
}

/// Count newlines in a record's ORIGINAL message body (pre-normalization) — the basis
/// for the `L lines elided` note. A bare-string body is counted as-is; a block body is
/// the visible `text` blocks joined with `\n` (matching how they would print). Returns 0
/// when the body is unavailable (→ note omitted).
fn raw_body_newlines(rec: &Record) -> usize {
    let Some(msg) = rec.message.as_ref() else {
        return 0;
    };
    let Some(content) = msg.content.as_ref() else {
        return 0;
    };
    let raw = match content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    raw.matches('\n').count()
}

/// The body text of a compaction-summary record (a `type:"user"` `isCompactSummary`
/// record carrying string content). `genuine_user_text` filters summaries out, so we
/// read the `Content::Text` body directly. Returns `None` if it is not a string body.
fn compact_summary_body(rec: &Record) -> Option<String> {
    let content = rec.message.as_ref()?.content.as_ref()?;
    match content {
        Content::Text(s) => Some(s.clone()),
        // A summary always carries STRING content in real data; a block body would be
        // a genuine surprise — return None rather than guess.
        Content::Blocks(_) => None,
    }
}

/// Extract dedup fingerprints from a summary body: the §6 "All user messages" bullets
/// and the §9 verbatim last-assistant quote (the only verbatim turns a summary holds).
/// Each fingerprint is `normalize_line(text).to_lowercase()` truncated to the first
/// [`DEDUP_PREFIX`] chars. Conservative: when the structured sections are not found,
/// every `- ` bullet line in the body is fingerprinted (a superset — still strict per
/// line). Robust to summaries that omit the exact headers.
fn summary_fingerprints(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        // §6 bullets render as `- "<text>" …` (quoted) or `- <text>` (bare); §9 quote is
        // prose carrying a quoted run. Prefer the QUOTED inner (the verbatim turn text);
        // for an UNQUOTED bullet fall back to the whole bullet body. A bullet WITH quotes
        // but an empty inner contributes nothing (it would only fingerprint the quotes).
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let fp = match quoted_inner(rest) {
                Some(q) => fingerprint(&q),
                None => fingerprint(rest),
            };
            if !fp.is_empty() {
                out.push(fp);
            }
        } else if let Some(inner) = quoted_inner(trimmed) {
            let fp = fingerprint(&inner);
            if !fp.is_empty() {
                out.push(fp);
            }
        }
    }
    out
}

/// The text inside the FIRST pair of double-quotes on a line (the §9 quote / a quoted
/// §6 bullet body), or `None` if the line has no quoted run.
fn quoted_inner(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let rest = &s[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Normalized-prefix fingerprint of a candidate text (lowercased, whitespace-collapsed,
/// first [`DEDUP_PREFIX`] chars). Empty input → empty (never matches).
fn fingerprint(s: &str) -> String {
    let normalized = normalize_line(s).to_lowercase();
    normalized.chars().take(DEDUP_PREFIX).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Render cost model + ellipsis
// ─────────────────────────────────────────────────────────────────────────────

/// The rendered form of a unit's text body (verbatim if ≤ cap, else middle-truncated)
/// plus the metadata a JSON consumer needs.
#[derive(Debug, Clone)]
struct RenderedUnit {
    body: String,
    rendered_chars: usize,
    truncated: bool,
    elided_chars: usize,
    elided_lines: usize,
}

/// Middle-truncate a unit to its role cap, keeping head+tail, with an explicit elided
/// marker. A unit at or below the cap renders verbatim. The cut is on `char` boundaries
/// (never mid-codepoint). The `L lines elided` note is included only when the original
/// text spanned ≥1 newline.
fn render_unit_body(unit: &TurnUnit) -> RenderedUnit {
    let cap = unit.role.cap();
    let chars: Vec<char> = unit.text.chars().collect();
    let total = chars.len();
    if total <= cap {
        return RenderedUnit {
            body: unit.text.clone(),
            rendered_chars: total,
            truncated: false,
            elided_chars: 0,
            elided_lines: 0,
        };
    }
    let head_keep = ((cap as f64) * unit.role.head_frac()).round() as usize;
    let head_keep = head_keep.min(cap);
    let tail_keep = cap - head_keep;
    let head: String = chars[..head_keep].iter().collect();
    let tail: String = chars[total - tail_keep..].iter().collect();
    let elided_chars = total - cap;
    // Lines elided: original newline count, surfaced only for multi-line bodies. The
    // rendered one-line form has no newlines, so we report the original's count as the
    // magnitude the consumer should expect in the raw record.
    let elided_lines = unit.orig_newlines;
    let nl_note = if elided_lines > 0 {
        format!(", {elided_lines} lines elided")
    } else {
        String::new()
    };
    let body = format!("{head} … [+{elided_chars} chars{nl_note}] … {tail}");
    RenderedUnit {
        body,
        rendered_chars: cap,
        truncated: true,
        elided_chars,
        elided_lines,
    }
}

/// The budget cost of one unit: header + the rendered-body char count. The body ALREADY
/// includes the `… [+K …] …` elision scaffolding when truncated, so this is measured
/// against the SAME render used for output — summed cost == summed rendered body chars
/// (the budget test relies on it). No separate marker term (that would double-count).
fn unit_cost(unit: &TurnUnit) -> usize {
    let r = render_unit_body(unit);
    HEADER_COST + r.body.chars().count()
}

/// The `[N tool calls]` marker render cost (0 ⇒ omitted, no cost).
fn marker_cost(tool_calls: usize) -> usize {
    if tool_calls == 0 {
        0
    } else {
        // "  [N tool calls]\n"
        format!("  [{tool_calls} tool calls]\n").chars().count()
    }
}

/// Cost of a whole turn at the chosen selection granularity (`sides`): both sides +
/// the `[N tool calls]` marker when both are taken; a single side (no marker) otherwise.
/// This is the SAME accounting the renderer uses, so summed cost == summed rendered
/// chars (the budget test relies on it).
fn turn_cost(turn: &TurnSlice, sides: SelSides) -> usize {
    let mut c = 0;
    if matches!(sides, SelSides::Both | SelSides::UserOnly) {
        if let Some(u) = &turn.user {
            c += unit_cost(u);
        }
    }
    // The marker is only rendered BETWEEN the two sides, so it is charged only on a
    // both-sides selection (a single-side emit shows no marker).
    if matches!(sides, SelSides::Both) {
        c += marker_cost(turn.tool_calls);
    }
    if matches!(sides, SelSides::Both | SelSides::AssistantOnly) {
        if let Some(a) = &turn.assistant_eot {
            c += unit_cost(a);
        }
    }
    c
}

/// Which side(s) of a turn a selection takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelSides {
    Both,
    UserOnly,
    AssistantOnly,
}

// ─────────────────────────────────────────────────────────────────────────────
// Budget allocation (2-phase)
// ─────────────────────────────────────────────────────────────────────────────

/// A selection decision for one turn (which sides were chosen).
#[derive(Debug, Clone)]
struct Selected {
    turn_index: usize,
    sides: SelSides,
}

/// The per-session plan: the selected turns (already sorted ascending for render) +
/// accounting for the header.
#[derive(Debug)]
struct SessionPlan {
    selected: Vec<Selected>,
    /// The dedup-flagged + max-compaction-filtered turns the plan selected FROM. The
    /// renderer reads units (incl. the `also_in_summary` flag) from HERE — never from the
    /// un-flagged `ScanResult.turns` — so the dedup demote-flag reaches the output.
    turns: Vec<TurnSlice>,
    spanned_boundaries: usize,
    rendered_chars: usize,
    /// The newest summary line (if any) — for the dedup-note + banners.
    newest_summary_line: Option<usize>,
    dedup_demoted: usize,
}

/// Plan one session: dedup-flag turns against the newest summary, then run the 2-phase
/// recency-first budget allocation, then sort ascending for render.
fn plan_session(
    sr: &ScanResult,
    budget: usize,
    rt_fraction: f64,
    max_compactions: usize,
) -> SessionPlan {
    // ── Dedup-flag (mutate a working copy of the turns) ──
    let newest = sr.summaries.iter().max_by_key(|s| s.line_no);
    let newest_summary_line = newest.map(|s| s.line_no);
    let mut turns: Vec<TurnSlice> = sr.turns.clone();
    let mut dedup_demoted = 0usize;
    if let Some(summary) = newest {
        for t in &mut turns {
            // Dedup is keyed on the live (compactions_before == 0) region primarily —
            // turns predating an OLDER boundary are genuinely gone from context and are
            // pure restoration, never deduped.
            if t.compactions_before != 0 {
                continue;
            }
            for unit in [t.user.as_mut(), t.assistant_eot.as_mut()]
                .into_iter()
                .flatten()
            {
                if unit_matches_summary(unit, &summary.fingerprints) {
                    unit.also_in_summary = true;
                    dedup_demoted += 1;
                }
            }
        }
    }

    // ── Apply --max-compactions: drop turns beyond the cap (0 = unlimited) ──
    if max_compactions > 0 {
        turns.retain(|t| t.compactions_before <= max_compactions);
    }

    // Recency-first order = descending line_no of the turn's latest unit. Ties broken
    // by descending turn_index for determinism.
    let mut order: Vec<usize> = (0..turns.len()).collect();
    order.sort_by(|&a, &b| {
        turn_latest_line(&turns[b])
            .cmp(&turn_latest_line(&turns[a]))
            .then(turns[b].turn_index.cmp(&turns[a].turn_index))
    });

    let rt_budget = ((budget as f64) * rt_fraction).round() as usize;
    let mut spent = 0usize;
    // selection state per turn index into `turns`.
    let mut chosen: Vec<Option<SelSides>> = vec![None; turns.len()];
    // dedup-demoted turns are deferred to a second pass within each phase.

    // ── Phase 1: ROUND-TRIP GUARANTEE — spend rt_budget only on complete pairs ──
    let mut spent_rt = 0usize;
    // Non-dup complete turns first, then dup complete turns (demote, don't drop).
    for dedup_pass in [false, true] {
        for &ti in &order {
            if chosen[ti].is_some() {
                continue;
            }
            let t = &turns[ti];
            if !t.is_round_trip() {
                continue;
            }
            let is_dup = turn_is_dup(t);
            if is_dup != dedup_pass {
                continue;
            }
            let c = turn_cost(t, SelSides::Both);
            if spent_rt + c <= rt_budget {
                chosen[ti] = Some(SelSides::Both);
                spent_rt += c;
            } else if spent_rt == 0 && !dedup_pass {
                // The first (most-recent, non-dup) complete turn is larger than the whole
                // reservation: include it anyway clamped to rt_budget (the most-recent
                // exchange is load-bearing). It is already ellipsis-capped by the role
                // caps; we simply accept it and stop Phase 1 — it cannot exceed rt_budget
                // by much (caps bound each side), and the §4 budget test tolerates this.
                if c <= rt_budget {
                    chosen[ti] = Some(SelSides::Both);
                    spent_rt += c;
                } else {
                    // even a fully-capped single round-trip > rt_budget: take it, clamp
                    // the accounting to rt_budget so Phase 2 gets no negative pool.
                    chosen[ti] = Some(SelSides::Both);
                    spent_rt = rt_budget;
                }
                break;
            }
            // else: skip (leave for Phase 2 to maybe pick a cheaper single side).
        }
    }
    spent += spent_rt;

    // ── Phase 2: FILL — spend free_budget + any rt_budget left over ──
    let pool = budget.saturating_sub(spent);
    let mut spent_fill = 0usize;
    for dedup_pass in [false, true] {
        for &ti in &order {
            if chosen[ti].is_some() {
                continue;
            }
            let t = &turns[ti];
            let is_dup = turn_is_dup(t);
            if is_dup != dedup_pass {
                continue;
            }
            // Prefer a complete turn if it fits; else the user side first (scarcer,
            // higher-signal loss), then the assistant side.
            let candidates: &[SelSides] = if t.is_round_trip() {
                &[SelSides::Both, SelSides::UserOnly, SelSides::AssistantOnly]
            } else if t.user.is_some() {
                &[SelSides::UserOnly]
            } else if t.assistant_eot.is_some() {
                &[SelSides::AssistantOnly]
            } else {
                &[]
            };
            for &sides in candidates {
                let c = turn_cost(t, sides);
                if spent_fill + c <= pool {
                    chosen[ti] = Some(sides);
                    spent_fill += c;
                    break;
                }
            }
        }
    }
    spent += spent_fill;

    // ── Assemble selected set, ascending for render ──
    let mut selected: Vec<Selected> = Vec::new();
    for (ti, sel) in chosen.iter().enumerate() {
        if let Some(sides) = sel {
            selected.push(Selected {
                turn_index: turns[ti].turn_index,
                sides: *sides,
            });
        }
    }
    selected.sort_by_key(|s| s.turn_index);

    // Boundaries spanned = the GREATEST `compactions_before` among selected turns: the
    // oldest selected turn sits behind that many summaries, so the ascending render
    // crosses exactly that many compaction boundaries on the way to EOF.
    let spanned = spanned_boundary_count(&turns, &selected);

    SessionPlan {
        selected,
        turns,
        spanned_boundaries: spanned,
        rendered_chars: spent,
        newest_summary_line,
        dedup_demoted,
    }
}

/// The latest jsonl line a turn touches (for recency ordering): the max of its user /
/// assistant line numbers, 0 if neither.
fn turn_latest_line(t: &TurnSlice) -> usize {
    let u = t.user.as_ref().map(|x| x.line_no).unwrap_or(0);
    let a = t.assistant_eot.as_ref().map(|x| x.line_no).unwrap_or(0);
    u.max(a)
}

/// True when EITHER side of a turn is dedup-flagged.
fn turn_is_dup(t: &TurnSlice) -> bool {
    t.user.as_ref().is_some_and(|u| u.also_in_summary)
        || t.assistant_eot.as_ref().is_some_and(|a| a.also_in_summary)
}

/// True when a unit's fingerprint is a prefix-or-equal match of any summary fingerprint
/// (§6.2). The match is symmetric-prefix: a unit clipped to 80 chars matches a summary
/// bullet that begins with the same 80 chars, and vice-versa.
fn unit_matches_summary(unit: &TurnUnit, summary_fps: &[String]) -> bool {
    let unit_fp = fingerprint(&unit.text);
    if unit_fp.is_empty() {
        return false;
    }
    summary_fps.iter().any(|sfp| {
        !sfp.is_empty() && (unit_fp.starts_with(sfp.as_str()) || sfp.starts_with(unit_fp.as_str()))
    })
}

/// The number of compaction boundaries the selected turns span = the GREATEST
/// `compactions_before` among selected turns. The oldest selected turn sits behind that
/// many summaries, so the ascending render crosses exactly that many boundaries reaching
/// EOF. 0 when every selected turn is in the live (post-newest-summary) region.
fn spanned_boundary_count(turns: &[TurnSlice], selected: &[Selected]) -> usize {
    selected
        .iter()
        .filter_map(|s| turns.iter().find(|t| t.turn_index == s.turn_index))
        .map(|t| t.compactions_before)
        .max()
        .unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Window-range parsing (mirrors recover::parse_turn_range)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a `--turn-range START..END` into an inclusive, 0-based `(lo, hi)`. Same
/// wording + shape as `recover`'s parser (kept local so the shared signature is
/// untouched).
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

// ─────────────────────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the renderers need beyond the per-session scans + plans.
#[derive(Debug)]
struct RenderCtx {
    budget_chars: usize,
    rt_fraction: f64,
    skipped_lines: usize,
}

/// Look up the dedup-flagged `TurnSlice` for a selected turn index within the PLAN's
/// turns (NOT `ScanResult.turns`, which is un-flagged) so the renderer sees the
/// `also_in_summary` flag the plan set.
fn find_turn(plan: &SessionPlan, turn_index: usize) -> Option<&TurnSlice> {
    plan.turns.iter().find(|t| t.turn_index == turn_index)
}

fn render_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    out_path: Option<&Path>,
) -> Result<()> {
    let mut first = true;
    let mut any = false;
    let mut out_blob = String::new();

    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        any = true;
        if !first {
            println!();
        }
        first = false;

        let (n_user, n_asst) = count_sides(plan);
        println!("SESSION {}", sr.session_id);
        println!(
            "  budget {} chars · round-trip-fraction {:.2} · spanned {} compaction boundaries",
            ctx.budget_chars, ctx.rt_fraction, plan.spanned_boundaries
        );
        println!(
            "  selected {} user + {} assistant units across {} turns · {} / {} chars used",
            n_user,
            n_asst,
            plan.selected.len(),
            plan.rendered_chars,
            ctx.budget_chars
        );
        if let (Some(sline), true) = (plan.newest_summary_line, plan.dedup_demoted > 0) {
            println!(
                "  dedup: {} units also present in summary L{} (demoted, flagged)",
                plan.dedup_demoted, sline
            );
        }
        println!("  {}", "─".repeat(60));

        // Walk the ascending selected set, inserting boundary banners as
        // `compactions_before` decreases toward EOF.
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            maybe_boundary_banner(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |s| {
                    println!("{s}");
                    out_blob.push_str(&s);
                    out_blob.push('\n');
                },
            );
            render_turn_text(turn, sel.sides, &mut |s| {
                println!("{s}");
                out_blob.push_str(&s);
                out_blob.push('\n');
            });
        }
    }

    if !any {
        println!("no turns selected (empty session set or budget too small)");
    }
    if ctx.skipped_lines > 0 {
        println!();
        println!("(skipped {} malformed jsonl line(s))", ctx.skipped_lines);
    }
    if let Some(p) = out_path {
        std::fs::write(p, &out_blob)
            .with_context(|| format!("cannot write --out file {}", p.display()))?;
        println!();
        println!("(wrote full reconstruction to {})", p.display());
    }
    Ok(())
}

/// Emit a `══ compaction boundary ══` banner for every summary the ascending walk
/// crosses on the way to a turn with `compactions_before == current`. Crossings are
/// keyed on the summary RANK from newest (newest = rank 1): moving from a turn at
/// cb=`prev` to one at cb=`current` (`current < prev`) crosses every summary ranked
/// `(current, prev]`, each bannered once, in ascending line order. The FIRST turn
/// (`prev == None`) crosses NOTHING — there are no restored turns below it, so the
/// summaries older than it (which it predates) are not bannered.
fn maybe_boundary_banner(
    prev: &mut Option<usize>,
    current: usize,
    summaries: &[SummaryInfo],
    emit: &mut dyn FnMut(String),
) {
    for s in crossed_summaries(summaries, *prev, current) {
        emit(format!(
            "{0} compaction boundary · summary at L{1} · (turns below predate it) {0}",
            "══", s.line_no
        ));
    }
    *prev = Some(current);
}

/// The summaries crossed when the ascending cursor moves from a turn at cb=`from` to a
/// turn at cb=`to`. A summary's rank from newest is its 1-based position when sorted by
/// descending line number; it is crossed when `to < rank <= from`. The FIRST turn
/// (`from == None`) seeds the cursor at its OWN depth (crosses nothing) — a summary
/// older than every selected turn has no restored turn below it, so it is never
/// bannered. Total banners across a full walk therefore equal the GREATEST cb selected
/// (the spanned-boundary count). Returned in ascending line order so banners read
/// forward.
fn crossed_summaries(
    summaries: &[SummaryInfo],
    from: Option<usize>,
    to: usize,
) -> Vec<&SummaryInfo> {
    // The first turn seeds the cursor at its own depth → no crossing on arrival.
    let Some(from) = from else {
        return Vec::new();
    };
    if to >= from {
        return Vec::new();
    }
    // Rank by descending line number (newest = rank 1).
    let mut by_rank: Vec<&SummaryInfo> = summaries.iter().collect();
    by_rank.sort_by(|a, b| b.line_no.cmp(&a.line_no));
    let mut out: Vec<&SummaryInfo> = Vec::new();
    for (i, s) in by_rank.iter().enumerate() {
        let rank = i + 1; // newest = 1
        if rank > to && rank <= from {
            out.push(*s);
        }
    }
    out.sort_by_key(|s| s.line_no);
    out
}

/// Render one turn's selected side(s) to the text format.
/// Whether a selection shows the user side / assistant side.
fn shows_user(sides: SelSides) -> bool {
    matches!(sides, SelSides::Both | SelSides::UserOnly)
}
fn shows_assistant(sides: SelSides) -> bool {
    matches!(sides, SelSides::Both | SelSides::AssistantOnly)
}

/// The user side of a turn IF the selection shows it AND it exists. Centralizes the
/// `show_user && Some` logic so BOTH renderers share one (unit-testable) decision.
fn shown_user(turn: &TurnSlice, sides: SelSides) -> Option<&TurnUnit> {
    if shows_user(sides) {
        turn.user.as_ref()
    } else {
        None
    }
}
/// The assistant side of a turn IF the selection shows it AND it exists.
fn shown_assistant(turn: &TurnSlice, sides: SelSides) -> Option<&TurnUnit> {
    if shows_assistant(sides) {
        turn.assistant_eot.as_ref()
    } else {
        None
    }
}

fn render_turn_text(turn: &TurnSlice, sides: SelSides, emit: &mut dyn FnMut(String)) {
    if let Some(u) = shown_user(turn, sides) {
        emit_unit_text("▽", u, emit);
    }
    if matches!(sides, SelSides::Both) && turn.tool_calls > 0 {
        emit(format!("  [{} tool calls]", turn.tool_calls));
    }
    if let Some(a) = shown_assistant(turn, sides) {
        emit_unit_text("△", a, emit);
    }
}

/// Emit a unit's header line + rendered (possibly truncated) body.
fn emit_unit_text(glyph: &str, unit: &TurnUnit, emit: &mut dyn FnMut(String)) {
    let dup = if unit.also_in_summary {
        "   (also in summary)"
    } else {
        ""
    };
    emit(format!(
        "{glyph} L{}  {}  ({}){dup}",
        unit.line_no,
        unit.role.label().to_uppercase(),
        format_timestamp(unit.ts_utc.as_deref())
    ));
    let r = render_unit_body(unit);
    emit(r.body);
}

/// Count selected user + assistant units in a plan.
fn count_sides(plan: &SessionPlan) -> (usize, usize) {
    let mut u = 0;
    let mut a = 0;
    for s in &plan.selected {
        match s.sides {
            SelSides::Both => {
                u += 1;
                a += 1;
            }
            SelSides::UserOnly => u += 1,
            SelSides::AssistantOnly => a += 1,
        }
    }
    (u, a)
}

fn render_json(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    out_path: Option<&Path>,
) -> Result<()> {
    use serde_json::json;
    let mut out_blob = String::new();

    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            // Interleave boundary records the same way the text banners do.
            maybe_boundary_json(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |line_no, summary_chars| {
                    let obj = json!({
                        "kind": "compaction_boundary",
                        "line_no": line_no,
                        "summary_chars": summary_chars,
                    });
                    let s = serde_json::to_string(&obj).expect("serialize boundary");
                    println!("{s}");
                    out_blob.push_str(&s);
                    out_blob.push('\n');
                },
            );

            if let Some(u) = shown_user(turn, sel.sides) {
                emit_unit_json(sr, turn, u, &mut out_blob)?;
            }
            if let Some(a) = shown_assistant(turn, sel.sides) {
                emit_unit_json(sr, turn, a, &mut out_blob)?;
            }
        }
    }

    if ctx.skipped_lines > 0 {
        let obj = json!({"kind":"skipped_lines","count": ctx.skipped_lines});
        println!("{}", serde_json::to_string(&obj)?);
    }
    if let Some(p) = out_path {
        std::fs::write(p, &out_blob)
            .with_context(|| format!("cannot write --out file {}", p.display()))?;
    }
    Ok(())
}

/// Emit one unit as a JSON object. The `text` field is ALWAYS the full verbatim
/// message (json is for machines that do their own windowing); the truncation metadata
/// describes what the TEXT render would show.
fn emit_unit_json(
    sr: &ScanResult,
    turn: &TurnSlice,
    unit: &TurnUnit,
    out_blob: &mut String,
) -> Result<()> {
    use serde_json::json;
    let r = render_unit_body(unit);
    let obj = json!({
        "session_id": sr.session_id,
        "turn_index": turn.turn_index,
        "line_no": unit.line_no,
        "role": unit.role.label(),
        "ts_utc": unit.ts_utc,
        "ts_local": unit.ts_utc.as_deref().and_then(local_iso),
        "tool_calls": turn.tool_calls,
        "full_chars": unit.full_chars,
        "rendered_chars": r.rendered_chars,
        "truncated": r.truncated,
        "elided_chars": r.elided_chars,
        "elided_lines": r.elided_lines,
        "also_in_summary": unit.also_in_summary,
        "compactions_before": turn.compactions_before,
        "text": unit.text,
    });
    let s = serde_json::to_string(&obj)?;
    println!("{s}");
    out_blob.push_str(&s);
    out_blob.push('\n');
    Ok(())
}

/// JSON twin of [`maybe_boundary_banner`]: invoke `emit(line_no, summary_chars)` for
/// each crossed boundary, in ascending line order, exactly once each.
fn maybe_boundary_json(
    prev: &mut Option<usize>,
    current: usize,
    summaries: &[SummaryInfo],
    emit: &mut dyn FnMut(usize, usize),
) {
    for s in crossed_summaries(summaries, *prev, current) {
        emit(s.line_no, s.body_chars);
    }
    *prev = Some(current);
}

#[cfg(test)]
mod tests;
