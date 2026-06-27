//! `search` subcommand — regex over transcripts, returning complete round-trip
//! exchanges.
//!
//! Behavior (SPEC.md §6.2, §6.4):
//! - Pattern is ripgrep-like, default smart-case (`-i` forces insensitive,
//!   `--multiline` lets `.` cross newlines). Empty pattern == pure filter.
//! - Filters: `--category/-t` (repeatable), `--turn-range` XOR (`--since`/`--until`),
//!   a positional `[PATH]...` target (cwd / encoded dir / `@<uuid>` / `*.jsonl`, repeatable,
//!   multi-target).
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

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use regex::bytes::Regex as BytesRegex;

use crate::cli::{Category, OutputFormat, SearchArgs};
use crate::model::{
    group_turn_indices_deduped, is_auq_answer_text, normalize_line, tool_result_content_text,
    Block, PlanIndex, Record,
};
use crate::parse::mmap_bytes;
use crate::path::{self, Caller, SubagentScope};
use crate::subagent::{is_subagent_path, session_id_from_path};
use crate::time_window::TimeWindow;
use crate::timez::{format_local_compact, local_iso};

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
    /// 1-based PHYSICAL line number of the source record in its session jsonl — the stable
    /// address `csift search --line N` re-fetches. Backfilled by the turn collector (make_hit
    /// leaves it 0); 0 means "not located" (never happens for a real scanned hit).
    pub line: usize,
    /// The source record's `uuid` (jsonl's own globally-unique id), when present — the
    /// alternative `csift search --uuid U` address. `None` for records that carry no uuid.
    pub uuid: Option<String>,
    /// Stable image ids the SOURCE RECORD carries (`#N` session handle, else `L<line>i<n>`) —
    /// the `[N image(s): …]` suffix, so a `search` hit on an image-bearing message exposes the
    /// SAME extractable id as `turns`/`image` (feed it to `csift image <session> --id <ID>`).
    /// Backfilled onto the record's first hit only (avoids repeating it per matched block).
    pub image_ids: Vec<String>,
    /// True when this hit came from a hook-backfilled ELICITATION SIDECAR record (§3.10) — an
    /// unresolved-pending AskUserQuestion/ExitPlanMode/MCP that is MISSING from the native
    /// transcript. Such a hit has NO physical `line` (it is not a real jsonl line), so it
    /// renders `(elicitation sidecar)` in place of `Lnnnn` and carries `source:"elicitation-
    /// sidecar"` in JSON. Backfilled with the address.
    pub from_sidecar: bool,
}

/// A complete reconstructed request/response exchange (round-trip) containing the
/// hit(s).
#[derive(Debug, Clone)]
pub struct Exchange {
    /// The transcript's own id: a top-level session uuid, OR a bare SUBAGENT hex when the
    /// hit came from a subagent transcript. A subagent hex is NOT a re-feedable `@<uuid>`
    /// target — use `parent_session_id` to re-feed (`csift turns @<parent>`). `is_subagent`
    /// discriminates.
    pub session_id: String,
    /// True when this exchange came from a subagent transcript (so `session_id` is a
    /// non-re-feedable bare hex). When true, `parent_session_id` carries the re-feedable uuid.
    pub is_subagent: bool,
    /// The re-feedable PARENT session uuid (the owning top-level session). Equal to
    /// `session_id` for a top-level hit; the subagent's parent uuid for a subagent hit.
    pub parent_session_id: String,
    /// 0-based turn index (turns delimited by genuine-user messages).
    pub turn_index: usize,
    /// Turn-opening (genuine-user) record timestamp — this exchange's position in the
    /// COMBINED chronological timeline (top-level + subagent exchanges interleaved by
    /// absolute time). ISO-8601 UTC sorts lexicographically == chronologically. `None`
    /// when the opening record carries no timestamp (rare); such exchanges sort LAST,
    /// deterministically. Surfaced as `ts_utc`/`ts_local` on the JSON envelope and in
    /// the text header so the chronological position is visible per result.
    pub started_utc: Option<String>,
    pub hits: Vec<Hit>,
    /// Sibling records of this turn that did NOT themselves match — populated only under
    /// `--siblings`, so a matched user question can surface WITH the agent's reply. Each is
    /// rendered head-anchored (no match span) and filtered to the effective sibling
    /// categories; a record that produced a hit is never repeated here. Empty otherwise.
    pub siblings: Vec<Hit>,
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
    /// SCOPE-span counts of the RESOLVED transcript set (top-level + subagent files), from
    /// `resolve_session_files` — so the fan-out is announced even when a spanned subagent
    /// yields no hits. Drives the shared SCOPE banner / JSON header (suppressed when sub==0).
    pub scope_top: usize,
    pub scope_sub: usize,
    /// EXPLICITLY requested addresses (`--line N` singletons / `--uuid U`) that resolved to no
    /// record — surfaced as an `unresolved:` line so an addressing batch is gap-aware. Empty
    /// for a normal (non-addressing) search. Each entry is a render-ready token (`L999` / a uuid).
    pub unresolved: Vec<String>,
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
    /// Test-only since production hits go through [`Matcher::locate`] (which also
    /// yields the span for excerpt-centering); kept as a clean bool API for tests.
    #[cfg(test)]
    fn is_match(&self, text: &str) -> bool {
        match &self.regex {
            None => true,
            Some(re) => re.is_match(text.as_bytes()),
        }
    }

    /// Locate the FIRST match, so the excerpt can be CENTERED on it instead of
    /// always showing the message head. Returns:
    /// - `None` — no match (the record is not a hit);
    /// - `Some(None)` — matches with no specific span (the pure filter matches every
    ///   record, so there is no offset to center on → excerpt shows the head);
    /// - `Some(Some((start, end)))` — matches at this BYTE range.
    fn locate(&self, text: &str) -> Option<Option<(usize, usize)>> {
        match &self.regex {
            None => Some(None),
            Some(re) => re.find(text.as_bytes()).map(|m| Some((m.start(), m.end()))),
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
/// Record-address selectors (`--line` / `--uuid`) parsed into membership sets — the "fetch
/// THESE records" filter that turns `search` into the in-permission message-getter. Active when
/// either set is non-empty; a record is addressed when its physical line OR uuid is in range.
struct AddressSet {
    lines: BTreeSet<usize>,
    uuids: BTreeSet<String>,
}

impl AddressSet {
    fn is_active(&self) -> bool {
        !self.lines.is_empty() || !self.uuids.is_empty()
    }

    fn addresses(&self, kept: &Kept) -> bool {
        (!self.lines.is_empty() && self.lines.contains(&kept.line_no))
            || (!self.uuids.is_empty()
                && kept
                    .rec
                    .uuid
                    .as_deref()
                    .is_some_and(|u| self.uuids.contains(u)))
    }
}

/// An optional single subagent hex (parsed from a `--line <hex>:<spec>` prefix) plus the
/// ordered `(line, from_range)` addresses.
type ParsedLineSpecs = (Option<String>, Vec<(usize, bool)>);

/// Parse `--line` tokens (already comma-split by clap) into an OPTIONAL single subagent hex
/// prefix + ordered `(line, from_range)` addresses. A token carrying a `:` pins a SUBAGENT
/// transcript: the part before the colon MUST be a bare subagent hex (as `csift agents` prints),
/// the part after is the usual `N` / `A-B` spec. Every hex-bearing token must name the SAME hex
/// (lines address ONE transcript) — a second, different hex is a hard error. A bare token
/// (no colon) is a top-level line spec. `N` → one EXPLICIT line; `A-B` → an ascending inclusive
/// RANGE (range members are non-explicit, so a miss inside a range is silent, not `unresolved`).
/// Duplicates collapse to their first occurrence.
fn parse_line_specs(tokens: &[String]) -> Result<ParsedLineSpecs> {
    let mut out: Vec<(usize, bool)> = Vec::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut hex: Option<String> = None;
    for tok in tokens {
        let raw = tok.trim();
        if raw.is_empty() {
            continue;
        }
        // A `<id>:<spec>` token pins one subagent transcript; the id must be a subagent id (a
        // bare hex OR a name-embedded teammate id) and all id-bearing tokens must agree on the
        // SAME id. The split is on the FIRST `:` — safe because a subagent id never contains one.
        let t = if let Some((prefix, rest)) = raw.split_once(':') {
            let prefix = prefix.trim();
            if !crate::path::is_subagent_id(prefix) {
                bail!(
                    "--line: '{raw}' — the part before ':' must be a subagent id from \
                     `csift agents` (a bare hex, or a name-embedded teammate id like \
                     `aVSRepro-68a2a1661c9390c1`)"
                );
            }
            match &hex {
                Some(prev) if prev != prefix => {
                    bail!("--line: all addressed lines must be in ONE transcript")
                }
                _ => hex = Some(prefix.to_string()),
            }
            rest.trim()
        } else {
            raw
        };
        if t.is_empty() {
            continue;
        }
        if let Some((a, b)) = t.split_once('-') {
            let a: usize = a
                .trim()
                .parse()
                .map_err(|_| anyhow!("--line: '{t}' is not a valid range (want A-B, 1-based)"))?;
            let b: usize = b
                .trim()
                .parse()
                .map_err(|_| anyhow!("--line: '{t}' is not a valid range (want A-B, 1-based)"))?;
            if a == 0 || b == 0 {
                bail!("--line: lines are 1-based; '{t}' includes line 0");
            }
            if a > b {
                bail!("--line: range '{t}' is descending — write it ascending (A-B with A ≤ B)");
            }
            for n in a..=b {
                if seen.insert(n) {
                    out.push((n, true));
                }
            }
        } else {
            let n: usize = t
                .parse()
                .map_err(|_| anyhow!("--line: '{t}' is not a line number or A-B range"))?;
            if n == 0 {
                bail!("--line: lines are 1-based; line 0 does not exist");
            }
            if seen.insert(n) {
                out.push((n, false));
            }
        }
    }
    if out.is_empty() {
        bail!("--line: no line numbers given");
    }
    Ok((hex, out))
}

/// Resolve the scope to exactly ONE transcript for `--line` addressing (lines are per-file).
/// A `Some(hex)` (parsed from a `--line <hex>:<spec>` prefix) pins that SUBAGENT transcript;
/// `None` pins the top-level one. Fail-CLOSED: an unmatched hex or an ambiguous/empty scope is a
/// pointed error, never a silent widen to the whole corpus.
fn resolve_single_transcript(args: &SearchArgs, subagent_hex: Option<&str>) -> Result<PathBuf> {
    let scope = if subagent_hex.is_some() {
        SubagentScope::WithSubagents
    } else {
        SubagentScope::TopLevelOnly
    };
    let files = path::resolve_session_files(&args.targets(), scope, Caller::Other)?;
    let target: Vec<PathBuf> = if let Some(hex) = subagent_hex {
        files
            .into_iter()
            .filter(|p| is_subagent_path(p) && session_id_from_path(p) == hex)
            .collect()
    } else {
        files.into_iter().filter(|p| !is_subagent_path(p)).collect()
    };
    match target.as_slice() {
        [one] => Ok(one.clone()),
        [] => {
            if let Some(hex) = subagent_hex {
                bail!(
                    "--line: no subagent transcript `{hex}` found in scope — pass its parent \
                     `@<uuid>` and check the hex with `csift agents`"
                )
            }
            bail!(
                "--line: the scope resolves to no single transcript — add `@<uuid>` \
                 (lines of WHICH session?)"
            )
        }
        many => bail!(
            "--line is ambiguous: the scope resolves to {} transcripts. Narrow it with \
             `@<uuid> --no-subagents` (or address a subagent via `--line <hex>:<spec>`) so the \
             line numbers name one file.",
            many.len()
        ),
    }
}

pub fn run_search(args: &SearchArgs) -> Result<()> {
    // ── Validate flag combinations up front (SPEC §6.2 validation) ──
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }

    // Unlike files/turns/list/agents/recover (whose first positional is the PATH/`@<uuid>`
    // target), search's FIRST positional is PATTERN — so a bare uuid here is a LITERAL pattern,
    // searched verbatim across scope. To scope to a session, pass it as an `@<uuid>` POSITIONAL
    // (a PATH target), exactly like every sibling (`csift search PATTERN @<uuid>`).
    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // A truly unbounded search (empty pattern + no filters) will emit a lot. Warn,
    // but do not refuse — SPEC §6.2 explicitly allows it. An `@<uuid>` / `@<hex>` / `*.jsonl`
    // POSITIONAL pins a single session (via resolve_session_files), so it counts as a session
    // filter here too — otherwise the warning would falsely claim "no session filter" on a run
    // that is in fact scoped to one session.
    let has_session_filter = args
        .targets()
        .iter()
        .filter_map(|p| p.to_str())
        .any(path::pins_single_session);
    // ── Address selectors (`--line` / `--uuid`): "fetch THESE records" (rendered full) ──
    // `--line` may carry a `<hex>:<spec>` subagent prefix → an optional single subagent hex.
    let (line_subagent_hex, line_specs) = if args.line.is_empty() {
        (None, Vec::new())
    } else {
        parse_line_specs(&args.line)?
    };
    let address = AddressSet {
        lines: line_specs.iter().map(|&(n, _)| n).collect(),
        uuids: args
            .uuid
            .iter()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .collect(),
    };

    let matcher = build_matcher(args)?;
    if matcher.is_pure_filter()
        && args.categories.is_empty()
        && turn_range.is_none()
        && time_window.is_unbounded()
        && !has_session_filter
        && !address.is_active()
    {
        eprintln!(
            "csift: warning: empty pattern with no category/time/turn/session filter \
             matches every exchange in scope — this may emit a lot."
        );
    }

    // ── Resolve targets → session files. `--line` addressing is PER-FILE and pins one
    //    transcript (the top-level one by default, or a SUBAGENT when a `--line <hex>:<spec>`
    //    prefix names one) ⇒ the single-transcript resolver (which fail-CLOSES: an unmatched hex
    //    errors, never widens scope to the whole corpus). Everything else uses the shared
    //    (optionally subagent-spanning) resolver. ──
    let session_files = if !args.line.is_empty() {
        vec![resolve_single_transcript(
            args,
            line_subagent_hex.as_deref(),
        )?]
    } else {
        path::resolve_session_files(
            &args.targets(),
            args.want_subagents().into(),
            path::Caller::Other,
        )?
    };
    let address_opt = address.is_active().then_some(&address);

    // `--siblings <SPEC>`: parse the repeatable caps ONCE here (a malformed spec is a hard
    // error, surfaced before any scan). `None` ⇒ siblings off. Parsed up front so the per-file
    // parallel scan just borrows the result.
    let sibling_caps = parse_sibling_specs(&args.siblings)?;

    // ── Parallel scan across files; collect order-stable, then merge ──
    let per_file: Vec<FileResult> = session_files
        .par_iter()
        .map(|p| {
            search_one_file(
                p,
                args,
                &matcher,
                turn_range.as_ref(),
                &time_window,
                address_opt,
                sibling_caps.as_ref(),
            )
        })
        .collect::<Result<Vec<_>>>()?;

    // SCOPE span of the resolved set (every transcript, incl. hit-free subagents).
    let scope_sub = session_files
        .iter()
        .filter(|p| crate::subagent::is_subagent_path(p))
        .count();
    let scope_top = session_files.len() - scope_sub;

    // ── Combined STABLE chronological timeline (SPEC §6.2) ──
    // Flatten every file's exchanges into ONE timeline, then stable-sort by the
    // turn-opening timestamp so subagent exchanges INTERLEAVE with top-level ones by
    // absolute time (both clocks are the same machine's UTC). ISO-8601 sorts as text;
    // timestamp-less exchanges sort LAST (mirrors `files --timeline`). The pre-sort
    // order — sorted file order, then turn order — is deterministic, and a stable sort
    // keeps it as the tie-break, so the timeline is fully reproducible. The GLOBAL
    // --max-count cap is applied AFTER the sort (keeping the EARLIEST N), never
    // silently — the dropped remainder is reported in the footer.
    let mut outcome = SearchOutcome {
        scope_top,
        scope_sub,
        ..SearchOutcome::default()
    };
    let mut all: Vec<Exchange> = Vec::new();
    for fr in per_file {
        outcome.skipped_lines += fr.skipped_lines;
        all.extend(fr.exchanges);
    }
    all.sort_by(|a, b| {
        timestamp_sort_key(a.started_utc.as_deref())
            .cmp(&timestamp_sort_key(b.started_utc.as_deref()))
    });

    if let Some(cap) = args.max_count {
        if all.len() > cap {
            outcome.dropped_by_cap = all.len() - cap;
            all.truncate(cap);
        }
    }
    outcome.exchanges = all;

    // ── Addressing gap-awareness: which EXPLICITLY-requested addresses produced no record? ──
    if address.is_active() {
        let mut hit_lines: BTreeSet<usize> = BTreeSet::new();
        let mut hit_uuids: BTreeSet<&str> = BTreeSet::new();
        for ex in &outcome.exchanges {
            for h in ex.hits.iter().chain(ex.siblings.iter()) {
                hit_lines.insert(h.line);
                if let Some(u) = h.uuid.as_deref() {
                    hit_uuids.insert(u);
                }
            }
        }
        // Explicit `--line N` singletons (range members are clamped, not reported) …
        for &(n, from_range) in &line_specs {
            if !from_range && !hit_lines.contains(&n) {
                outcome.unresolved.push(format!("L{n}"));
            }
        }
        // … and every requested `--uuid`.
        for u in &address.uuids {
            if !hit_uuids.contains(u.as_str()) {
                outcome.unresolved.push(format!("uuid {u}"));
            }
        }
    }

    // `--count-only`: emit only the TRUE total of matching exchanges (add back any capped by
    // `--max-count`), the ripgrep `-c` idiom — no per-exchange output.
    if args.count_only {
        let total = outcome.exchanges.len() + outcome.dropped_by_cap;
        match args.format {
            OutputFormat::Text => println!("{total}"),
            OutputFormat::Json => println!("{{\"matched\":{total}}}"),
        }
        return Ok(());
    }

    match args.format {
        OutputFormat::Text => render_text(&outcome, args),
        OutputFormat::Json => render_json(&outcome)?,
    }
    Ok(())
}

/// True when ANY emitted hit/sibling came from the elicitation sidecar (§3.10) — drives the
/// `with elicitation sidecar` note so a consumer knows the output includes hook-backfilled
/// records, not raw native jsonl.
fn merged_any_sidecar(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| h.from_sidecar)
    })
}

/// Count of DISTINCT sessions among these exchanges (by transcript `session_id`, in
/// first-seen order). One cheap always-on number — surfaced in every search footer.
fn distinct_session_count(exchanges: &[Exchange]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for ex in exchanges {
        if !seen.contains(&ex.session_id.as_str()) {
            seen.push(&ex.session_id);
        }
    }
    seen.len()
}

/// Sort key that places timestamp-less exchanges LAST (after all timestamped ones) and
/// orders timestamped ones chronologically (ISO-8601 UTC sorts as text). Same shape as
/// `files::timestamp_sort_key`, so `search` and `files --timeline` order identically.
fn timestamp_sort_key(ts: Option<&str>) -> (bool, &str) {
    match ts {
        Some(t) => (false, t),
        None => (true, ""),
    }
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
    /// 1-based PHYSICAL line number of this record in its source jsonl (from the scanner) —
    /// a stable address (jsonl is append-only), surfaced per hit so `csift search --line N` (and
    /// raw `sed -n 'Np'`) can re-fetch the exact record. `0` for a merged elicitation-sidecar
    /// record (it has no physical transcript line — see `from_sidecar`).
    line_no: usize,
    /// True when this record was merged from the elicitation SIDECAR (§3.10), not scanned from
    /// the native jsonl. Such a record has no physical `line_no` (0); its hits render
    /// `(elicitation sidecar)` instead of `Lnnnn`.
    from_sidecar: bool,
}

/// Scan a single session file: prefilter → parse → delimit turns → match → stitch.
#[allow(clippy::too_many_arguments)]
fn search_one_file(
    path: &Path,
    args: &SearchArgs,
    matcher: &Matcher,
    turn_range: Option<&(usize, usize)>,
    time_window: &TimeWindow,
    address: Option<&AddressSet>,
    sibling_caps: Option<&SiblingCaps>,
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
    // Parse all transcript-candidate lines IN PARALLEL (newline-aligned chunks on the rayon pool)
    // so a single giant transcript is not scanned on one core. The stage-2 keyword prefilter
    // (`can_hit`) is computed per line inside the parallel scan, where the raw bytes are in hand.
    let (mut records, mut skipped) = crate::parse::scan_lines_parallel(bytes, |line, line_no| {
        if !line_is_transcript_candidate(line) {
            return crate::parse::LineVerdict::Ignore;
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
                can_hit: true, // no physical line to prefilter — let the matcher decide.
                line_no: 0,
                from_sidecar: true,
            });
        }
    }

    let exchanges = reconstruct_and_match(
        path,
        &records,
        args,
        matcher,
        turn_range,
        time_window,
        address,
        sibling_caps,
    );

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
#[allow(clippy::too_many_arguments)]
fn reconstruct_and_match(
    path: &Path,
    records: &[Kept],
    args: &SearchArgs,
    matcher: &Matcher,
    turn_range: Option<&(usize, usize)>,
    time_window: &TimeWindow,
    address: Option<&AddressSet>,
    sibling_caps: Option<&SiblingCaps>,
) -> Vec<Exchange> {
    // Canonical bare-hex id (subagent `agent-` prefix stripped) — the SAME derivation
    // every other surface uses, so a `search` subagent hit's `session_id` is joinable to
    // `files`/`turns`/`recover`/`agents` (id-form unification; a top-level uuid is
    // unaffected). See [`crate::subagent::session_id_from_path`].
    let session_id = crate::subagent::session_id_from_path(path);
    // A subagent transcript's `session_id` is a non-re-feedable bare hex; its re-feedable
    // owner is the parent uuid (the dir before `subagents/`). For a top-level file there is
    // no parent, so the parent IS the session id.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    // Group records into turns via the shared §6.4 delimiter (model::group_turn_indices
    // is the single source of truth, used identically by `files`). The outer index is
    // the 0-based turn index; map each index group back to its `Kept` borrows.
    let index_turns = group_turn_indices_deduped(records, |k| &k.rec);
    // ExitPlanMode plan pointers for this session (§4.2.4) — a rejection-with-message
    // hit surfaces a `[plan: <path>]` pointer. Cheap; empty in a no-plan session.
    let plan_index = PlanIndex::from_records(records.iter().map(|k| &k.rec));

    let want_categories = &args.categories;
    // `tool_use_id → tool name` across the whole file, so a `tool-response` (a bare
    // `tool_result` carrying only the id) can name the tool it answers (e.g. `tool-response Edit`).
    let tool_names = build_tool_name_index(records);
    // `--full` lifts the excerpt cap so a found message renders end-to-end (no `… (+N)`).
    // Addressing (`--line`/`--uuid`) means "fetch THIS record" → always full, no excerpt cap.
    let excerpt_max = if args.full || address.is_some() {
        usize::MAX
    } else {
        EXCERPT_MAX
    };
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

        // Collect the hits in this turn that satisfy category + time + regex, plus the
        // turn-record indices that produced them (so siblings can exclude matched records).
        let (hits, hit_idxs) = collect_turn_hits(
            &turn,
            want_categories,
            matcher,
            time_window,
            args.resolve_persisted,
            excerpt_max,
            &plan_index,
            &tool_names,
            address,
        );
        if hits.is_empty() {
            continue;
        }

        // `--siblings <SPEC>`: render the turn's NON-matched records (the rest of the
        // back-and-forth) so a matched user question surfaces with the agent's reply, capped
        // per the parsed SPEC.
        let siblings = match sibling_caps {
            Some(caps) => collect_turn_siblings(
                &turn,
                caps,
                &hit_idxs,
                args.resolve_persisted,
                excerpt_max,
                &plan_index,
                &tool_names,
            ),
            None => Vec::new(),
        };

        let record_uuids = turn
            .records
            .iter()
            .filter_map(|k| k.rec.uuid.clone())
            .collect();

        // Chronological key for the combined timeline: the turn-opening (genuine-user)
        // record's timestamp, falling back to the earliest hit's timestamp when the
        // opener carries none. ISO-8601 UTC sorts lexicographically == chronologically.
        let started_utc = turn
            .records
            .first()
            .and_then(|k| k.rec.timestamp.clone())
            .or_else(|| hits.iter().find_map(|h| h.timestamp_utc.clone()));

        out.push(Exchange {
            session_id: session_id.clone(),
            is_subagent,
            parent_session_id: parent_session_id.clone(),
            turn_index: turn.index,
            started_utc,
            hits,
            siblings,
            record_uuids,
        });
    }

    out
}

/// Parsed `--siblings <SPEC>` caps. `per_cat` holds the explicit `<cat>:N` caps (cap of up to
/// N siblings of THAT category); `bare` is the bare-`N` cap (cap of up to N siblings across
/// every category that has NO typed cap — "the rest"). Sibling rendering is ON iff at least one
/// spec token was given (so an empty `--siblings` vec → no `SiblingCaps`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SiblingCaps {
    per_cat: Vec<(Category, usize)>,
    bare: Option<usize>,
}

impl SiblingCaps {
    /// The cap for a specific category: its typed `<cat>:N` cap if one was given, else the
    /// bare-`N` fallback (which caps "the rest"). `None` ⇒ this category is not shown at all.
    fn cap_for(&self, cat: Category) -> Option<usize> {
        self.per_cat
            .iter()
            .find(|(c, _)| *c == cat)
            .map(|(_, n)| *n)
            .or(self.bare)
    }

    /// True when ONLY a bare-`N` was given (no typed caps), so the `N` is a TOTAL cap across all
    /// categories rather than a per-category one.
    fn bare_is_total(&self) -> bool {
        self.per_cat.is_empty()
    }
}

/// Map a `--siblings` SPEC category token to its `Category` (the same value set as `-t`:
/// thinking|user|tool|tool-response|agent).
fn parse_sibling_category(token: &str) -> Option<Category> {
    match token {
        "thinking" => Some(Category::Thinking),
        "user" => Some(Category::User),
        "tool" => Some(Category::Tool),
        "tool-response" => Some(Category::ToolResponse),
        "agent" => Some(Category::Agent),
        _ => None,
    }
}

/// Parse the repeatable / comma-joined `--siblings <SPEC>` tokens into [`SiblingCaps`]. A bare
/// `N` (positive integer) caps the total siblings (the categories with no typed cap); a
/// `<category>:N` caps THAT category. `N` must be ≥1. An empty token list ⇒ `None` (siblings
/// off). A malformed token (unknown category, non-numeric / zero cap) is a hard error.
fn parse_sibling_specs(tokens: &[String]) -> Result<Option<SiblingCaps>> {
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut caps = SiblingCaps::default();
    for tok in tokens {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        if let Some((cat_tok, n_tok)) = t.split_once(':') {
            let cat = parse_sibling_category(cat_tok.trim()).ok_or_else(|| {
                anyhow!(
                    "--siblings: '{t}' — unknown category '{}' (want \
                     thinking|user|tool|tool-response|agent or a bare N)",
                    cat_tok.trim()
                )
            })?;
            let n: usize = n_tok.trim().parse().map_err(|_| {
                anyhow!("--siblings: '{t}' — the cap after ':' must be a positive integer")
            })?;
            if n == 0 {
                bail!("--siblings: '{t}' — the cap must be ≥1 (0 means 'do not show', so omit it)");
            }
            if let Some(slot) = caps.per_cat.iter_mut().find(|(c, _)| *c == cat) {
                slot.1 = n; // last write wins for a repeated category
            } else {
                caps.per_cat.push((cat, n));
            }
        } else {
            let n: usize = t.parse().map_err(|_| {
                anyhow!(
                    "--siblings: '{t}' — want a bare N or a <category>:N \
                     (category ∈ thinking|user|tool|tool-response|agent)"
                )
            })?;
            if n == 0 {
                bail!("--siblings: '{t}' — the cap must be ≥1 (0 means 'do not show', so omit it)");
            }
            caps.bare = Some(n);
        }
    }
    if caps.per_cat.is_empty() && caps.bare.is_none() {
        return Ok(None);
    }
    Ok(Some(caps))
}

/// One reconstructed turn (the opening genuine-user record + every record chained
/// under it, in file order).
struct Turn<'a> {
    index: usize,
    records: Vec<&'a Kept>,
}

/// Gather the category-eligible, time-windowed, regex-matching hits inside a turn, plus the
/// indices (into `turn.records`) of the records that produced at least one hit — so
/// `--siblings` can exclude an already-matched record from the sibling rendering.
/// Build the `tool_use_id → tool name` index for a file's records: every `tool_use` block's
/// `{id, name}`. A later `tool_result` (which carries only the `tool_use_id`) looks its tool up
/// here so a `tool-response` row can say WHICH tool it answers. First write wins (ids are unique).
fn build_tool_name_index(records: &[Kept]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for k in records {
        if let Some(blocks) = k.rec.blocks() {
            for b in blocks {
                if let Block::ToolUse {
                    id: Some(id),
                    name: Some(name),
                    ..
                } = b
                {
                    map.entry(id.clone()).or_insert_with(|| name.clone());
                }
            }
        }
    }
    map
}

// Internal pipeline function: the arg list grew as `tool_names` (tool-response naming) and
// `address` (--line/--uuid selector) were threaded through the per-turn scan. Bundling into a
// struct would only relocate the same fields without simplifying the data flow.
#[allow(clippy::too_many_arguments)]
fn collect_turn_hits(
    turn: &Turn<'_>,
    want: &[Category],
    matcher: &Matcher,
    time_window: &TimeWindow,
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    address: Option<&AddressSet>,
) -> (Vec<Hit>, Vec<usize>) {
    let mut hits = Vec::new();
    let mut hit_idxs = Vec::new();
    for (i, kept) in turn.records.iter().enumerate() {
        // Addressing (`--line`/`--uuid`): only the ADDRESSED records are eligible to hit — the
        // selector that turns `search` into the message-getter. (Applied before the keyword
        // prefilter so an addressed record is fetched regardless of the pattern literal.)
        if let Some(addr) = address {
            if !addr.addresses(kept) {
                continue;
            }
        }
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
        let before = hits.len();
        collect_record_hits(
            rec,
            want,
            matcher,
            resolve_persisted,
            excerpt_max,
            plan_index,
            tool_names,
            &mut hits,
        );
        // Backfill the source record's address onto every hit this record produced.
        backfill_address(&mut hits[before..], kept);
        if hits.len() > before {
            hit_idxs.push(i);
        }
    }
    (hits, hit_idxs)
}

/// Stamp the source record's line number + uuid onto each hit just appended for it — the
/// `csift search --line/--uuid` address. Done by the turn collector (not `make_hit`) because the line number
/// lives on the `Kept`, not the `Record`. Also attaches the record's image ids to its FIRST
/// hit (so an image-bearing message exposes the extractable `#N`/`L<line>i<n>` id once, not
/// repeated per matched block).
fn backfill_address(hits: &mut [Hit], kept: &Kept) {
    for h in hits.iter_mut() {
        h.line = kept.line_no;
        h.uuid = kept.rec.uuid.clone();
        h.from_sidecar = kept.from_sidecar;
    }
    if let Some(first) = hits.first_mut() {
        first.image_ids = crate::image::image_ids_for_record(&kept.rec, kept.line_no);
    }
}

/// Render the SIBLING records of a turn — those that produced NO hit — as head-anchored
/// The turn's NON-matched records as sibling hits, restricted + CAPPED per the parsed
/// `--siblings <SPEC>`. Reuses [`collect_record_hits`] with a PURE-FILTER matcher (matches
/// every record, so each category-eligible block of a sibling surfaces with a head excerpt). A
/// record that matched (its index is in `hit_idxs`) is never repeated. The per-record time
/// window is intentionally NOT re-applied: the turn already qualified, and the siblings are
/// context for that qualifying turn. Caps: a `<cat>:N` spec keeps the first N siblings of that
/// category; a bare `N` keeps the first N across the categories that have no typed cap ("the
/// rest"), and when ONLY a bare `N` was given it is a single TOTAL cap across all categories.
fn collect_turn_siblings(
    turn: &Turn<'_>,
    caps: &SiblingCaps,
    hit_idxs: &[usize],
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
) -> Vec<Hit> {
    // Categories with ANY cap (typed, or covered by a bare-N fallback) are sibling-eligible.
    const ALL: [Category; 5] = [
        Category::Thinking,
        Category::User,
        Category::Tool,
        Category::ToolResponse,
        Category::Agent,
    ];
    let eligible: Vec<Category> = ALL
        .iter()
        .copied()
        .filter(|&c| caps.cap_for(c).is_some())
        .collect();
    if eligible.is_empty() {
        return Vec::new();
    }
    let pure = Matcher {
        regex: None,
        prefilter: None,
    };
    let mut sibs = Vec::new();
    for (i, kept) in turn.records.iter().enumerate() {
        if hit_idxs.contains(&i) {
            continue;
        }
        let before = sibs.len();
        collect_record_hits(
            &kept.rec,
            &eligible,
            &pure,
            resolve_persisted,
            excerpt_max,
            plan_index,
            tool_names,
            &mut sibs,
        );
        backfill_address(&mut sibs[before..], kept);
    }
    // Apply the caps in document order, keeping each category's first N (a bare-only spec is a
    // single TOTAL cap; otherwise the bare-N caps the categories lacking a typed cap, pooled).
    let bare_total = caps.bare_is_total();
    let mut total_kept = 0usize;
    let mut per_cat_kept: Vec<(Category, usize)> = Vec::new();
    let mut bare_pool_kept = 0usize;
    sibs.retain(|hit| {
        if bare_total {
            let cap = caps.bare.unwrap_or(0);
            if total_kept < cap {
                total_kept += 1;
                return true;
            }
            return false;
        }
        match caps
            .per_cat
            .iter()
            .find(|(c, _)| *c == hit.category)
            .map(|(_, n)| *n)
        {
            Some(cap) => {
                let kept = per_cat_kept
                    .iter_mut()
                    .find(|(c, _)| *c == hit.category)
                    .map(|(_, n)| n);
                match kept {
                    Some(n) if *n < cap => {
                        *n += 1;
                        true
                    }
                    Some(_) => false,
                    None => {
                        per_cat_kept.push((hit.category, 1));
                        cap >= 1
                    }
                }
            }
            None => {
                // No typed cap → governed by the bare-N "rest" pool (if any).
                match caps.bare {
                    Some(cap) if bare_pool_kept < cap => {
                        bare_pool_kept += 1;
                        true
                    }
                    _ => false,
                }
            }
        }
    });
    sibs
}

/// Emit hits for every category-eligible piece of `rec` that matches the regex.
// Internal pipeline function; `tool_names` was threaded in so a `tool-response` hit can name the
// tool it answers. Same rationale as `collect_turn_hits` for not bundling into a struct.
#[allow(clippy::too_many_arguments)]
fn collect_record_hits(
    rec: &Record,
    want: &[Category],
    matcher: &Matcher,
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    hits: &mut Vec<Hit>,
) {
    let ts = rec.timestamp.clone();

    // ── user category: genuine-user text, an answered AskUserQuestion (Q+options+answer
    // as one unit), or a tool-use rejection-with-message (+plan pointer) (§4.1, §4.4,
    // §4.2.4, §5). `reconstructed_user_text` returns the clean answer prose from the
    // structured `toolUseResult.answers` (not the noisy synthesized string), and a
    // `[plan: <path>]` pointer for a rejection. ──
    if category_active(want, Category::User) {
        // An automation trigger (`<task-notification>`) opens a user turn but renders as the
        // parsed `[workflow <id> …] <summary>` ATTRIBUTION label, never the raw XML wrapper —
        // matched against the LABEL so a `-t user` search surfaces the clean attribution.
        let text = rec
            .automation_label()
            .or_else(|| rec.reconstructed_user_text(Some(plan_index)));
        if let Some(text) = text {
            if let Some(span) = matcher.locate(&text) {
                hits.push(make_hit(
                    Category::User,
                    &text,
                    span,
                    ts.clone(),
                    None,
                    excerpt_max,
                ));
            }
        }
    }

    // ── elicitation-sidecar non-tool_use marker (§3.10) ──
    // An MCP-elicitation pending marker is a `system` record carrying its prose in the
    // top-level `content` string (no blocks, not genuine-user), so BOTH the genuine-user path
    // above AND the block-bearing loop below miss it — leaving `search` unable to find a
    // session blocked on an MCP elicitation (`list`/`turns` surface it via `pending_text`, so
    // only `search` had the gap). Match that `content` so it surfaces like the
    // AskUserQuestion/ExitPlanMode tool_use markers do. GUARDED to a marker with NO `tool_use`
    // block, so AQ/ExitPlanMode (which DO carry a tool_use block and match via the
    // `Block::ToolUse` arm below, category Tool) never double-emit. Tagged `Tool` for
    // consistency — every merged elicitation surfaces under `-t tool`.
    if category_active(want, Category::Tool)
        && rec.is_elicitation_marker()
        && rec
            .blocks()
            .is_none_or(|bs| !bs.iter().any(|b| matches!(b, Block::ToolUse { .. })))
    {
        if let Some(text) = rec.content.as_ref().and_then(serde_json::Value::as_str) {
            if let Some(span) = matcher.locate(text) {
                hits.push(make_hit(
                    Category::Tool,
                    text,
                    span,
                    ts.clone(),
                    rec.csift_kind.clone(),
                    excerpt_max,
                ));
            }
        }
    }

    // ── block-bearing categories: thinking / tool / tool-response / agent ──
    if let Some(blocks) = rec.blocks() {
        for block in blocks {
            match block {
                Block::Thinking { thinking, .. } if category_active(want, Category::Thinking) => {
                    if let Some(span) = matcher.locate(thinking) {
                        hits.push(make_hit(
                            Category::Thinking,
                            thinking,
                            span,
                            ts.clone(),
                            None,
                            excerpt_max,
                        ));
                    }
                }
                Block::Text { text } if category_active(want, Category::Agent) => {
                    // Only assistant `text` blocks are the agent message; a user
                    // `text` block is genuine-user (handled above).
                    let span = if rec.is_type("assistant") {
                        matcher.locate(text)
                    } else {
                        None
                    };
                    if let Some(span) = span {
                        hits.push(make_hit(
                            Category::Agent,
                            text,
                            span,
                            ts.clone(),
                            None,
                            excerpt_max,
                        ));
                    }
                }
                Block::ToolUse { name, input, .. } if category_active(want, Category::Tool) => {
                    let rendered = render_tool_use(name.as_deref(), input.as_ref());
                    if let Some(span) = matcher.locate(&rendered) {
                        hits.push(make_hit(
                            Category::Tool,
                            &rendered,
                            span,
                            ts.clone(),
                            name.clone(),
                            excerpt_max,
                        ));
                    }
                }
                Block::ToolResult {
                    content: Some(c),
                    tool_use_id,
                    ..
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
                    if let Some(span) = matcher.locate(&text) {
                        // Name the tool this response answers (joined via `tool_use_id`).
                        let name = tool_use_id
                            .as_deref()
                            .and_then(|id| tool_names.get(id).cloned());
                        hits.push(make_hit(
                            Category::ToolResponse,
                            &text,
                            span,
                            ts.clone(),
                            name,
                            excerpt_max,
                        ));
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
/// Test-only: production now surfaces the AUQ answer via the model's reconstructed unit
/// ([`Record::reconstructed_user_text`] → [`Record::auq_exchange`]), which prefers the
/// clean structured `toolUseResult.answers`; this helper backs the legacy-shape tests.
#[cfg(test)]
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

/// Build a hit with a normalized excerpt CENTERED on the match (`span`), capped at
/// `excerpt_max` chars (`usize::MAX` under `--full` ⇒ the whole record, uncapped).
fn make_hit(
    category: Category,
    text: &str,
    span: Option<(usize, usize)>,
    ts: Option<String>,
    tool_name: Option<String>,
    excerpt_max: usize,
) -> Hit {
    Hit {
        category,
        excerpt: match_excerpt(text, span, excerpt_max),
        timestamp_utc: ts,
        tool_name,
        // line/uuid/image_ids are per-RECORD, not known here — the turn collector backfills
        // them onto the hits it appends (it holds the `Kept`, which carries line + record).
        line: 0,
        uuid: None,
        image_ids: Vec::new(),
        from_sidecar: false,
    }
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
/// Test-only: production excerpting goes through [`match_excerpt`], which carries the
/// caller's (possibly `--full`) budget; this fixed-budget wrapper backs the unit tests.
#[cfg(test)]
fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}

/// Build the inline excerpt, CENTERED on the match so a hit DEEP in a long message is
/// actually visible — not just the message head (the old behavior, which silently hid
/// any match past the first `max` chars and forced readers back to the raw jsonl).
///
/// `span` is the first match's BYTE range, or `None` for the pure filter (no specific
/// match → show the head). When the message fits in `max` chars it is shown whole.
/// Otherwise a `max`-char window is taken around the match (a quarter of the budget as
/// leading context), whitespace-normalized, with a leading `…` when content precedes
/// the window and the shared `… (+N chars)` marker when content follows — so clipping
/// on either side is explicit, never silent (SPEC §0).
fn match_excerpt(text: &str, span: Option<(usize, usize)>, max: usize) -> String {
    let total = text.chars().count();
    // Pure filter, or the whole message already fits (incl. `--full`'s `usize::MAX`): keep
    // the head-anchored form, capped at `max` (uncapped under `--full`).
    let start_byte = match span {
        Some((s, _)) if total > max => s,
        _ => return crate::text::truncate_excerpt(&normalize_line(text), max),
    };
    // Char index of the match start; a non-char-boundary byte offset (possible with a
    // raw-byte regex) falls back to the head rather than panicking.
    let Some(prefix) = text.get(..start_byte) else {
        return crate::text::truncate_excerpt(&normalize_line(text), max);
    };
    let match_char = prefix.chars().count();
    let win_start = match_char.saturating_sub(max / 4);
    let window: String = text.chars().skip(win_start).take(max).collect();
    let body = normalize_line(&window);
    let after = total.saturating_sub(win_start + max);
    let mut out = String::new();
    if win_start > 0 {
        out.push('…');
    }
    out.push_str(&body);
    if after > 0 {
        out.push_str(&format!("… (+{after} chars)"));
    }
    out
}

/// Parse a `--turn-range START..END` into an inclusive 0-based `(lo, hi)` (shared parser).
fn parse_turn_range(s: &str) -> Result<(usize, usize)> {
    crate::text::parse_range(s, "--turn-range", false)
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
    // SCOPE banner FIRST (before the empty check) so a bare `csift search '' <uuid>` fan-out
    // announces it spanned N subagents up front — same disclosure as list/files/turns.
    crate::text::emit_scope_banner(outcome.scope_top, outcome.scope_sub);
    if outcome.exchanges.is_empty() {
        println!("no matching exchanges");
        emit_unresolved(&outcome.unresolved);
        if outcome.skipped_lines > 0 {
            println!("({})", crate::text::malformed_note(outcome.skipped_lines));
        }
        return;
    }

    // ── Session-label table: each distinct session's FULL id is printed ONCE here (`s1 = …`),
    //    then every exchange references the cheap `s1` label. An LLM follows the reference for
    //    free, so the uuid never repeats per row (the dominant token cost of the old header). ──
    let mut label: HashMap<&str, String> = HashMap::new();
    let mut order: Vec<&Exchange> = Vec::new();
    for ex in &outcome.exchanges {
        if !label.contains_key(ex.session_id.as_str()) {
            label.insert(ex.session_id.as_str(), format!("s{}", label.len() + 1));
            order.push(ex);
        }
    }
    for ex in &order {
        let lab = &label[ex.session_id.as_str()];
        if ex.is_subagent {
            // The parent's own label if it is in scope, else its bare uuid.
            let parent = label
                .get(ex.parent_session_id.as_str())
                .map(String::as_str)
                .unwrap_or(ex.parent_session_id.as_str());
            println!("{lab} = {} (subagent · parent {parent})", ex.session_id);
        } else {
            println!("{lab} = {}", ex.session_id);
        }
    }

    for ex in &outcome.exchanges {
        println!();
        // `s1·t6` — the session label + 0-based turn index + the single compact local instant
        // (offset already pins it; no second UTC copy). Per-hit timestamps are omitted in text
        // (this turn time covers them); the JSON envelope still carries each hit's `ts_utc`.
        let lab = &label[ex.session_id.as_str()];
        println!(
            "{lab}·t{}  {}",
            ex.turn_index,
            format_local_compact(ex.started_utc.as_deref())
        );
        for hit in &ex.hits {
            print_record_line(category_glyph(hit.category), hit);
        }
        // `--siblings`: the turn's non-matched records, under a dim `·` context marker so
        // they read as surrounding back-and-forth, not as matches.
        for sib in &ex.siblings {
            print_record_line('·', sib);
        }
    }

    // ── Compact lowercase footer: match + distinct-session totals (both always present — each is
    //    one cheap number, isolated by `-c`/`-l` only for piping), drop accounting, unresolved. ──
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
    let n = outcome.exchanges.len();
    let ex_word = if n == 1 { "exchange" } else { "exchanges" };
    let n_sessions = distinct_session_count(&outcome.exchanges);
    let sess_word = if n_sessions == 1 {
        "session"
    } else {
        "sessions"
    };
    print!("matched {n} {ex_word} · {n_sessions} {sess_word} · category={cat}");
    if outcome.dropped_by_cap > 0 {
        print!(" · {} dropped by --max-count", outcome.dropped_by_cap);
    }
    println!();
    if merged_any_sidecar(&outcome.exchanges) {
        println!("with elicitation sidecar");
    }
    emit_unresolved(&outcome.unresolved);
    if outcome.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(outcome.skipped_lines));
    }
}

/// One hit/sibling line: `<marker> <label>[ <tool>]  L<line>  <excerpt>` (excerpt inline; its
/// newlines are already collapsed to single spaces). `marker` is the category glyph for a match
/// or a dim `·` for a `--siblings` context record.
fn print_record_line(marker: char, h: &Hit) {
    let label = category_label(h.category);
    let name = h
        .tool_name
        .as_deref()
        .map(|n| format!(" {n}"))
        .unwrap_or_default();
    let images = image_suffix(&h.image_ids);
    // A merged elicitation-sidecar hit has no physical jsonl line — render the provenance
    // locator instead of a fabricated `Lnnnn` (§3.10).
    let locator = if h.from_sidecar {
        "(elicitation sidecar)".to_string()
    } else {
        format!("L{}", h.line)
    };
    println!(
        "  {marker} {label}{name}  {locator}  {}{}",
        h.excerpt, images
    );
}

/// ` [N image(s): …]` suffix when the hit's record carries images — the SAME ids `turns` shows,
/// feedable straight to `csift image <session> --id <ID>`. Empty string when there are none.
fn image_suffix(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let noun = if ids.len() == 1 { "image" } else { "images" };
    format!("  [{} {}: {}]", ids.len(), noun, ids.join(", "))
}

/// Print the `unresolved:` line when an EXPLICIT address (`--line N` / `--uuid U`) matched no
/// record — so an addressing batch is gap-aware. No-op when nothing is unresolved.
fn emit_unresolved(unresolved: &[String]) {
    if !unresolved.is_empty() {
        println!("unresolved: {}", unresolved.join(", "));
    }
}

/// Render one `Hit` (a match OR a `--siblings` context record) to its JSON object — the
/// shared per-hit shape used by both the `hits` and `siblings` envelope arrays.
fn hit_json(h: &Hit) -> serde_json::Value {
    serde_json::json!({
        "category": category_label(h.category),
        "excerpt": h.excerpt,
        "ts_utc": h.timestamp_utc,
        "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
        "tool_name": h.tool_name,
        // The `csift search --line/--uuid` address: 1-based source line + the record uuid (when
        // present). A merged elicitation-sidecar hit has NO physical line, so `line` is null and
        // `source:"elicitation-sidecar"` marks the provenance (§3.10); a native hit omits `source`.
        "line": if h.from_sidecar { serde_json::Value::Null } else { serde_json::json!(h.line) },
        "uuid": h.uuid,
        "source": if h.from_sidecar { serde_json::json!("elicitation-sidecar") } else { serde_json::Value::Null },
        // Extractable image ids (`#N`/`L<line>i<n>`) the record carries; empty array when none.
        "image_ids": h.image_ids,
    })
}

fn render_json(outcome: &SearchOutcome) -> Result<()> {
    use serde_json::json;
    // Leading `{kind:"session_header", …}` scope record (same three span fields as turns),
    // emitted only when the scope spans ≥1 subagent — uniform JSON scope disclosure.
    if outcome.scope_sub > 0 {
        println!(
            "{}",
            serde_json::to_string(&crate::text::scope_header_json(
                outcome.scope_top,
                outcome.scope_sub
            ))?
        );
    }
    for ex in &outcome.exchanges {
        let hits: Vec<_> = ex.hits.iter().map(hit_json).collect();
        let mut obj = json!({
            "session_id": ex.session_id,
            // Discriminate the id-domain so a consumer can tell a re-feedable parent UUID
            // from a non-re-feedable subagent transcript hex: `is_subagent` + the always-
            // re-feedable `parent_session_id` (= session_id for a top-level hit).
            "is_subagent": ex.is_subagent,
            "parent_session_id": ex.parent_session_id,
            "turn_index": ex.turn_index,
            // Envelope-level chronological position = the turn-opening timestamp, the key
            // the combined timeline is sorted on. `ts_local` is the same instant in the
            // host TZ. Per-hit `ts_utc` (in `hits`) can diverge for a deep tool_use match.
            "ts_utc": ex.started_utc,
            "ts_local": ex.started_utc.as_deref().and_then(local_iso),
            "hits": hits,
            "record_uuids": ex.record_uuids,
        });
        // `--siblings`: attach the non-matched records of the turn (same per-hit shape).
        // Present only when there are siblings — absent ⇒ none (keeps the common envelope lean).
        if !ex.siblings.is_empty() {
            let sibs: Vec<_> = ex.siblings.iter().map(hit_json).collect();
            obj["siblings"] = json!(sibs);
        }
        println!("{}", serde_json::to_string(&obj)?);
    }
    // Trailing summary object (SPEC §8.2). `sessions` (distinct matching sessions) rides
    // alongside `matched` — the same cheap always-on total the text footer carries; `unresolved`
    // lists explicit addresses (`--line`/`--uuid`) that matched no record (empty in a normal run).
    let summary = json!({
        "matched": outcome.exchanges.len(),
        "sessions": distinct_session_count(&outcome.exchanges),
        "dropped_by_cap": outcome.dropped_by_cap,
        "skipped_lines": outcome.skipped_lines,
        "unresolved": outcome.unresolved,
        // True when ≥1 emitted record was merged from the elicitation sidecar (§3.10) — the
        // machine echo of the `with elicitation sidecar` text note.
        "with_elicitation_sidecar": merged_any_sidecar(&outcome.exchanges),
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::OutputFormat;

    fn args(pattern: &str) -> SearchArgs {
        SearchArgs {
            pattern: pattern.to_string(),
            paths: Vec::new(),
            categories: Vec::new(),
            ignore_case: false,
            multiline: false,
            turn_range: None,
            since: None,
            until: None,
            max_count: None,
            count_only: false,
            siblings: Vec::new(),
            full: false,
            line: Vec::new(),
            uuid: Vec::new(),
            resolve_persisted: false,
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
        collect_record_hits(
            &r,
            &[Category::ToolResponse],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut no_resolve,
        );
        assert!(no_resolve.is_empty(), "deep token must NOT match inline");

        // WITH resolution: the file is read, the token is found → exactly one hit.
        let mut with_resolve = Vec::new();
        collect_record_hits(
            &r,
            &[Category::ToolResponse],
            &m,
            true,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut with_resolve,
        );
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
        collect_record_hits(
            &r,
            &[Category::Thinking],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
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
        collect_record_hits(
            &r,
            &[Category::Agent],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
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
        collect_record_hits(
            &r,
            &[Category::Tool],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool_name.as_deref(), Some("AskUserQuestion"));
    }

    #[test]
    fn collect_hits_mcp_elicitation_system_marker() {
        // §3.10: an MCP-elicitation pending marker is a `system` record with NO tool_use
        // block — `search` must still find it via its top-level `content` string (the gap the
        // §3.10 arm closes), tagged `Tool` and named by `csiftKind`.
        let r = rec(
            r#"{"type":"system","subtype":"mcp_elicitation","timestamp":"2026-06-27T02:00:00.000Z","content":"MCP elicitation [gdrive] (url): authorize wibblewobble access","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"el-1","csiftMcpServer":"gdrive"}"#,
        );
        let m = build_matcher(&args("wibblewobble")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(
            &r,
            &[Category::Tool],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(
            hits.len(),
            1,
            "MCP system marker must produce exactly one hit"
        );
        assert_eq!(hits[0].category, Category::Tool);
        assert_eq!(hits[0].tool_name.as_deref(), Some("mcp-elicitation"));
        assert!(hits[0].excerpt.contains("wibblewobble"));
    }

    #[test]
    fn collect_hits_auq_marker_does_not_double_emit() {
        // §3.10: an AskUserQuestion pending marker DOES carry a tool_use block, so it matches
        // via the `Block::ToolUse` arm. The §3.10 non-tool_use arm is guarded to markers with
        // NO tool_use block, so this must yield EXACTLY ONE hit (not two).
        let r = rec(
            r#"{"type":"assistant","timestamp":"2026-06-27T01:00:00.000Z","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"k1","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"k1","name":"AskUserQuestion","input":{"questions":[{"question":"pick wibblewobble?"}]}}]}}"#,
        );
        let m = build_matcher(&args("wibblewobble")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(
            &r,
            &[Category::Tool],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(
            hits.len(),
            1,
            "AQ marker must not double-emit via the §3.10 arm"
        );
        assert_eq!(hits[0].tool_name.as_deref(), Some("AskUserQuestion"));
    }

    #[test]
    fn collect_hits_auq_answer_under_user() {
        let r = rec(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Q\"=\"chosen\". You can now continue."}]}}"#,
        );
        let m = build_matcher(&args("chosen")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(
            &r,
            &[Category::User],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
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
        collect_record_hits(
            &r,
            &[Category::User],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 0);
        // But the tool-response category does.
        let mut hits2 = Vec::new();
        collect_record_hits(
            &r,
            &[Category::ToolResponse],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits2,
        );
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
                    line_no: 1,
                    from_sidecar: false,
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
        let sibling_caps = parse_sibling_specs(&a.siblings).unwrap();
        reconstruct_and_match(
            std::path::Path::new("/x/0a1b2c3d-0000-0000-0000-000000000000.jsonl"),
            &kept,
            a,
            &matcher,
            tr.as_ref(),
            &tw,
            None,
            sibling_caps.as_ref(),
        )
    }

    #[test]
    fn started_utc_is_none_when_opener_and_hits_lack_timestamps() {
        // A genuine-user opener with NO `timestamp` and no later hit ts → started_utc is None
        // (the `.or_else(hits…)` fallback finds nothing). Such an exchange sorts LAST in the
        // combined timeline (the timestamp_sort_key None arm, asserted separately).
        let lines = vec![
            r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"no ts but matches carry"}}"#,
        ];
        let ex = search(&lines, &args("carry"));
        assert_eq!(ex.len(), 1);
        assert!(
            ex[0].started_utc.is_none(),
            "no opener ts, no hit ts → None"
        );
    }

    #[test]
    fn started_utc_falls_back_to_first_hit_when_opener_lacks_timestamp() {
        // The opener carries no ts, but a later agent record in the turn does → started_utc falls
        // back to that hit's timestamp rather than staying None.
        let lines = vec![
            r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"opener carry no ts"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:09.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply carry"}]}}"#,
        ];
        let mut a = args("carry");
        a.categories = vec![Category::Agent];
        let ex = search(&lines, &a);
        assert_eq!(ex.len(), 1);
        assert_eq!(
            ex[0].started_utc.as_deref(),
            Some("2026-06-07T05:00:09.000Z"),
            "fallback to the first hit's timestamp"
        );
    }

    #[test]
    fn timestamp_sort_key_orders_timestamped_first_none_last() {
        // ISO-8601 UTC strings sort chronologically as text; a None timestamp sorts LAST.
        let early = timestamp_sort_key(Some("2026-06-07T05:00:00.000Z"));
        let late = timestamp_sort_key(Some("2026-06-07T05:00:05.000Z"));
        let none = timestamp_sort_key(None);
        assert!(early < late, "earlier ts sorts first");
        assert!(late < none, "any timestamp sorts before None");
        assert!(early < none);
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
        // The AUQ answer is surfaced under `user` (it rides on a carrier) AND — the
        // sanctioned behavior change (§6.4) — it now OPENS a new turn (the answer is a
        // genuine user message that was previously missed as a boundary). So the hit
        // lands in turn 1 (the genuine "pick one" opener is turn 0).
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].turn_index, 1);
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
        // §6.4 behavior change: the answer opens its own turn (turn 1), after the
        // genuine "pick one" opener (turn 0).
        assert_eq!(ex[0].turn_index, 1);
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
        collect_record_hits(
            &r,
            &[Category::User],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
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
    fn match_excerpt_centers_on_a_deep_match() {
        // The needle sits ~800 chars in — far past EXCERPT_MAX. The OLD head-only
        // excerpt hid it entirely (the bug that forced raw-jsonl reads); centering
        // must surface it, with explicit clipping markers on both sides.
        // Synthetic multi-byte placeholder (neutral emoji) + neutral padding.
        let needle = "🤖🎉✅🚀🌟";
        let text = format!("{}{needle}{}", "🔵".repeat(800), "🟥".repeat(800));
        let m = build_matcher(&args(needle)).unwrap();
        let span = m.locate(&text).expect("matches").expect("has a span");
        let ex = match_excerpt(&text, Some(span), EXCERPT_MAX);
        assert!(ex.contains(needle), "excerpt must show the match: {ex}");
        assert!(
            ex.starts_with('…'),
            "content precedes the window → leading …: {ex}"
        );
        assert!(
            ex.contains("chars)"),
            "content follows → trailing count: {ex}"
        );
    }

    #[test]
    fn match_excerpt_short_message_is_shown_whole() {
        let text = "a short hit here";
        let m = build_matcher(&args("hit")).unwrap();
        let span = m.locate(text).unwrap();
        assert_eq!(match_excerpt(text, span, EXCERPT_MAX), "a short hit here");
    }

    #[test]
    fn match_excerpt_early_match_keeps_the_head() {
        let text = format!("needle {}", "z".repeat(EXCERPT_MAX));
        let m = build_matcher(&args("needle")).unwrap();
        let span = m.locate(&text).unwrap();
        let ex = match_excerpt(&text, span, EXCERPT_MAX);
        assert!(!ex.starts_with('…'), "match at char 0 → no leading …: {ex}");
        assert!(ex.starts_with("needle"), "got: {ex}");
    }

    #[test]
    fn match_excerpt_pure_filter_falls_back_to_head() {
        let text = "X".repeat(EXCERPT_MAX + 50);
        let m = build_matcher(&args("")).unwrap(); // empty pattern = pure filter
        let span = m.locate(&text).expect("pure filter matches");
        assert_eq!(span, None, "pure filter has no locatable span");
        let ex = match_excerpt(&text, span, EXCERPT_MAX);
        assert!(!ex.starts_with('…'), "head form has no leading …");
        assert!(ex.ends_with("… (+50 chars)"), "got: {ex}");
    }

    #[test]
    fn match_excerpt_full_budget_emits_whole_message() {
        // `--full` passes `usize::MAX` as the budget: a message longer than EXCERPT_MAX is
        // emitted whole, with NO truncation marker — whereas the default budget truncates.
        let n = EXCERPT_MAX + 200;
        let text = "🤖".repeat(n);
        let capped = match_excerpt(&text, None, EXCERPT_MAX);
        assert!(
            capped.contains("… (+"),
            "default budget truncates: {capped}"
        );
        let full = match_excerpt(&text, None, usize::MAX);
        assert!(
            !full.contains("… (+"),
            "full budget has no truncation marker"
        );
        assert_eq!(full.chars().count(), n, "full text length preserved");
    }

    fn specs(toks: &[&str]) -> Vec<String> {
        toks.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_sibling_specs_empty_is_off() {
        // No `--siblings` → None (sibling rendering off).
        assert_eq!(parse_sibling_specs(&[]).unwrap(), None);
        // A whitespace-only token is ignored and also yields None.
        assert_eq!(parse_sibling_specs(&specs(&["  "])).unwrap(), None);
    }

    #[test]
    fn parse_sibling_specs_bare_n_caps_total() {
        // A bare `N` → a single TOTAL cap across all categories, no per-cat entries.
        let caps = parse_sibling_specs(&specs(&["3"])).unwrap().unwrap();
        assert_eq!(caps.bare, Some(3));
        assert!(caps.per_cat.is_empty());
        assert!(caps.bare_is_total());
        // Every category resolves to the bare-N cap.
        assert_eq!(caps.cap_for(Category::Agent), Some(3));
        assert_eq!(caps.cap_for(Category::Tool), Some(3));
    }

    #[test]
    fn parse_sibling_specs_cat_n_caps_that_category_only() {
        // A `<cat>:N` → caps THAT category; others are not shown.
        let caps = parse_sibling_specs(&specs(&["agent:1"])).unwrap().unwrap();
        assert_eq!(caps.per_cat, vec![(Category::Agent, 1)]);
        assert_eq!(caps.bare, None);
        assert_eq!(caps.cap_for(Category::Agent), Some(1));
        assert_eq!(caps.cap_for(Category::Tool), None); // not shown
        assert!(!caps.bare_is_total());
    }

    #[test]
    fn parse_sibling_specs_mixed_typed_and_bare() {
        // `tool:1`, `thinking:2`, bare `3` → typed caps govern their categories; the bare
        // caps the rest (the categories with no typed cap).
        let caps = parse_sibling_specs(&specs(&["tool:1", "thinking:2", "3"]))
            .unwrap()
            .unwrap();
        assert_eq!(caps.cap_for(Category::Tool), Some(1));
        assert_eq!(caps.cap_for(Category::Thinking), Some(2));
        assert_eq!(caps.cap_for(Category::Agent), Some(3)); // "the rest" via bare-N
        assert_eq!(caps.cap_for(Category::User), Some(3));
        assert!(!caps.bare_is_total());
    }

    #[test]
    fn parse_sibling_specs_invalid_tokens_error() {
        // Unknown category, non-numeric bare, bad typed cap, and a zero cap all error.
        assert!(parse_sibling_specs(&specs(&["foo"])).is_err());
        assert!(parse_sibling_specs(&specs(&["bad:2"])).is_err());
        assert!(parse_sibling_specs(&specs(&["tool:x"])).is_err());
        assert!(parse_sibling_specs(&specs(&["tool:0"])).is_err());
        assert!(parse_sibling_specs(&specs(&["0"])).is_err());
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
            line_no: 1,
            from_sidecar: false,
        };
        assert!(!kept.can_hit);
        let turn = Turn {
            index: 0,
            records: vec![&kept],
        };
        let tw = TimeWindow::default();
        let (hits, hit_idxs) = collect_turn_hits(
            &turn,
            &[],
            &m,
            &tw,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            None,
        );
        assert!(hits.is_empty(), "a can_hit=false record yields no hits");
        assert!(hit_idxs.is_empty(), "no record produced a hit");
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
            line_no: 1,
            from_sidecar: false,
        };
        let turn = Turn {
            index: 0,
            records: vec![&kept],
        };
        // Window starting AFTER the record's timestamp → excluded.
        let tw = TimeWindow::from_args(Some("2026-06-07T06:00:00Z"), None).unwrap();
        assert!(collect_turn_hits(
            &turn,
            &[],
            &m,
            &tw,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            None
        )
        .0
        .is_empty());
        // An unbounded window admits it.
        let tw2 = TimeWindow::default();
        assert!(!collect_turn_hits(
            &turn,
            &[],
            &m,
            &tw2,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            None
        )
        .0
        .is_empty());
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
        collect_record_hits(
            &r,
            &[Category::ToolResponse],
            &m,
            true,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
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
        collect_record_hits(
            &r,
            &[Category::Agent],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &mut hits,
        );
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
