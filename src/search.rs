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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use regex::bytes::Regex as BytesRegex;

use crate::cli::{
    label_selected, selector_is_segment_prefix, selector_is_valid, OutputFormat, SearchArgs,
};
use crate::model::{
    group_turn_indices_deduped, normalize_line, tool_result_content_text, Block, Class,
    ClassifyCtx, Content, PlanIndex, Record, SpawnLookup,
};
use crate::parse::mmap_bytes;
use crate::path::{self};
use crate::subagent::{discover_subagents, is_subagent_path};
use crate::time_window::TimeWindow;
use crate::timez::{format_local_compact, local_iso};

/// Max characters of a matched excerpt shown inline before truncation. Truncation
/// is ALWAYS explicit (`… (+N chars)`) — never silent (SPEC §0, §8.1).
///
/// Deliberately LONGER than `list`'s 200-char cap (`session::EXCERPT_MAX`): a search
/// hit wants enough of the matched exchange to be useful in context, whereas `list`
/// is a dense at-a-glance identity index. The difference is intentional.
const EXCERPT_MAX: usize = 400;

/// Render stand-in for a `redacted_thinking` block (GOLD §2 / oracle B3): the block carries
/// only an opaque/encrypted `data` payload (no readable text), so it surfaces this placeholder
/// while still classifying `agent.thinking` — so `-t agent.thinking` finds it without dumping
/// the opaque blob.
const REDACTED_THINKING_PLACEHOLDER: &str = "[redacted thinking]";

/// The `agent.tool.use ▹ agent.tool.result` pairing state of a tool hit (GOLD §7), joined by
/// `tool_use_id` across the transcript. Drives the render (`▹` / `(no result — pending)` /
/// `(use not in scope)`). `None` on a non-tool hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pairing {
    /// The use↔result partner is in scope (renders `agent.tool.use ▹ agent.tool.result`).
    Paired,
    /// An `agent.tool.use` whose `tool_result` is not in scope (frozen / elicitation / unreturned).
    PendingNoResult,
    /// An `agent.tool.result` whose `tool_use` is not in scope (compacted / sliced away).
    OrphanResult,
}

/// A single label-tagged hit inside an exchange.
#[derive(Debug, Clone)]
pub struct Hit {
    /// The matched LEAF [`Class`] — its [`Class::path`] is the rendered/JSON `label` (GOLD §6).
    pub class: Class,
    /// The full label path SET this record carries ([`Record::classify`]), for JSON `labels`.
    pub labels: Vec<&'static str>,
    /// The matched text excerpt (whitespace-normalized, explicitly truncated).
    pub excerpt: String,
    pub timestamp_utc: Option<String>,
    /// Tool name when the hit is a tool-use/tool-result block, for the header.
    pub tool_name: Option<String>,
    /// `from ⇨ to` comm direction ([`Record::direction`]) when the hit is `agent.communication.*`
    /// (GOLD §4); `None` otherwise. Rendered as `<from> ⇨ <to>`, JSON `from`/`to`.
    pub direction: Option<(String, String)>,
    /// The block's `tool_use_id` (the use's `id` / the result's `tool_use_id`) for the GOLD §7
    /// `▹` pairing join; `None` on a non-tool hit.
    pub tool_use_id: Option<String>,
    /// The resolved [`Pairing`] of a tool hit (filled by the per-file pairing pass); `None` on a
    /// non-tool hit or before the pass runs.
    pub pair: Option<Pairing>,
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
    /// True when this hit's `excerpt` was CLIPPED to fit the default cap (its match-centered
    /// window dropped surrounding content) — i.e. the reader is seeing a fragment, not the
    /// whole record. ALWAYS false under `--no-truncate` and in `--line`/`--uuid` fetch
    /// mode (both lift the cap to `usize::MAX`), so it doubles as the "default truncation was in
    /// effect AND bit" signal that drives the trailing reader-caution note (`render_text`) and
    /// the JSON summary's `excerpts_truncated` flag.
    pub truncated: bool,
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
}

/// A compiled pattern + flags, plus the optional literal prefilter needle.
#[derive(Debug)]
pub struct Matcher {
    /// `None` ⇒ empty pattern (pure filter: every category-eligible record matches).
    regex: Option<BytesRegex>,
    /// A required-literal prefilter derived from the pattern, run against RAW line/file
    /// bytes (§7d stage 2 + the §7f whole-file gate). `None` ⇒ no anchorable literal.
    prefilter: Option<Prefilter>,
    /// SYNTHESIZED-text markers (built only when `prefilter` is `Some`): a record whose
    /// raw line carries one of these can render matchable text that is NOT a verbatim
    /// substring of its raw bytes (an automation label's fabricated kind slug / status,
    /// the AUQ Q+options+answer scaffold, a rejection's `[plan: …]` pointer resolved from
    /// ANOTHER record, the compact-boundary `trigger=…` excerpt, `--resolve-persisted`
    /// external file content). The literal prefilter can only prove absence for
    /// VERBATIM-derived text, so a marker-bearing line/file always passes to the parse +
    /// regex stage. This also FIXES a latent pre-existing gap: the old case-sensitive
    /// `memmem` prefilter silently skipped regex work on exactly these records.
    ///
    /// One `memmem::Finder` per needle — deliberately NOT one Aho-Corasick automaton:
    /// the `"answers"` needle starts with a quote, and in JSON lines a quote is one of
    /// the DENSEST bytes, so AC's start-byte prefilter degenerated into a verification
    /// attempt at nearly every string boundary (`try_find_fwd` became the #1 profile
    /// entry). `memmem` picks a rare byte INSIDE each needle as its SIMD skip anchor,
    /// so per-needle scans stay at memory speed regardless of the leading byte.
    ///
    /// Two tiers, split by whether a marker-bearing line's synthesized text can be
    /// re-rendered SELF-CONTAINED for the §7f stage-2 verification:
    /// - `synth_verifiable` — notification / AUQ-answer / compact-boundary markers: the
    ///   line's every synthesized text derives from the line alone, so the gate can
    ///   parse JUST the marker lines, render via the SHARED engines
    ///   (`record_text_sections` / `auq_exchange` / `record_raw_text`) and regex-check
    ///   them — a big marker-heavy main session no longer forces a whole-file parse.
    /// - `synth_conservative` — a rejection's `[plan: …]` pointer resolves through
    ///   ANOTHER record (`PlanIndex`) and `--resolve-persisted` content lives in an
    ///   external file, so those lines force the full scan (rare).
    synth_verifiable: Vec<memmem::Finder<'static>>,
    synth_conservative: Vec<memmem::Finder<'static>>,
}

/// Raw-byte prefilter for a pattern that IS a plain literal (no regex metachars, no
/// JSON-escaped chars — see [`required_literal`]). Both variants are CONSERVATIVE:
/// they can only prove a haystack CANNOT match (false positives fine, false negatives
/// impossible), so gating on them never drops a genuine hit.
#[derive(Debug)]
enum Prefilter {
    /// Case-sensitive literal: SIMD `memmem` substring search.
    Literal(memmem::Finder<'static>),
    /// Smart-case / `-i` insensitive literal: a `(?i)`-wrapped escaped literal as a
    /// bytes regex — `memmem` has no caseless mode, but the regex engine compiles a
    /// caseless literal to an accelerated (Teddy-class) multi-substring scan, so the
    /// dominant lowercase-smart-case search gets the SAME prefilter power the
    /// case-sensitive path always had.
    CaselessLiteral(BytesRegex),
}

impl Prefilter {
    /// True when `haystack` (a raw jsonl line OR a whole mmapped file) could contain a
    /// match. The JSON-escape safety argument is [`required_literal`]'s: the literal
    /// contains no char that serde/JS JSON-encodes, so it survives verbatim (module
    /// case) in the raw bytes whenever the DECODED text matches.
    fn may_match(&self, haystack: &[u8]) -> bool {
        match self {
            Prefilter::Literal(finder) => finder.find(haystack).is_some(),
            Prefilter::CaselessLiteral(re) => re.is_match(haystack),
        }
    }
}

impl Matcher {
    /// The PURE-FILTER matcher (no regex, no prefilter): every text matches. Used for
    /// `--siblings` rendering and as `csift show`'s fetch matcher (an addressed record
    /// always emits).
    pub(crate) fn pure() -> Matcher {
        Matcher {
            regex: None,
            prefilter: None,
            synth_verifiable: Vec::new(),
            synth_conservative: Vec::new(),
        }
    }

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
            Some(pf) => pf.may_match(line) || self.synth_may_match(line),
            None => true,
        }
    }

    /// The literal-prefilter check ALONE (no synth-marker OR) — the §7f pre-scan needs
    /// the two signals separately (a literal hit forces the full scan; a marker hit
    /// routes to its tier). `false` is only possible when a prefilter is anchored.
    fn line_prefilter_hits(&self, line: &[u8]) -> bool {
        match &self.prefilter {
            Some(pf) => pf.may_match(line),
            None => true,
        }
    }

    /// True when a prefilter is anchored — the precondition for the §7f whole-file gate
    /// (without one nothing is provably a miss, so the gate pre-scan would be waste).
    fn has_prefilter(&self) -> bool {
        self.prefilter.is_some()
    }

    /// Whole-slice version of [`Matcher::line_may_match`] — test-only: production gates
    /// per LINE inside the parallel pre-scan (a serial whole-mmap pass would bottleneck
    /// the single-giant-file case); tests use this to pin the miss/hit semantics.
    #[cfg(test)]
    fn file_may_match(&self, bytes: &[u8]) -> bool {
        match &self.prefilter {
            Some(pf) => pf.may_match(bytes) || self.synth_may_match(bytes),
            None => true,
        }
    }

    /// True when the haystack carries ANY synthesized-text marker (either tier).
    fn synth_may_match(&self, haystack: &[u8]) -> bool {
        self.synth_verifiable
            .iter()
            .chain(self.synth_conservative.iter())
            .any(|f| f.find(haystack).is_some())
    }

    /// True when the haystack carries a CONSERVATIVE marker (must full-scan).
    fn synth_conservative_hits(&self, haystack: &[u8]) -> bool {
        self.synth_conservative
            .iter()
            .any(|f| f.find(haystack).is_some())
    }

    /// True when the haystack carries a VERIFIABLE marker (stage-2 re-render + check).
    fn synth_verifiable_hits(&self, haystack: &[u8]) -> bool {
        self.synth_verifiable
            .iter()
            .any(|f| f.find(haystack).is_some())
    }

    /// §7f stage-2: could this parsed marker-line record's SYNTHESIZED texts match?
    /// Renders through the same shared engines the hit collector uses (no drift):
    /// notification section labels + normalized `<result>` bodies
    /// (`record_text_sections` — direction/owner do not affect the TEXT, so the neutral
    /// ctx is exact), the answered-AUQ reconstruction (`auq_exchange`), and the
    /// compact-boundary content + metadata excerpt (`record_raw_text`). Any VERBATIM
    /// text these return is already covered by the literal scan, so a miss here plus a
    /// literal miss proves the record cannot hit.
    fn synth_texts_match(&self, rec: &Record) -> bool {
        let ctx = crate::model::ClassifyCtx::top_level();
        if rec
            .record_text_sections(&ctx)
            .iter()
            .any(|sec| self.locate(&sec.text).is_some())
        {
            return true;
        }
        if let Some(t) = rec.auq_exchange() {
            if self.locate(&t).is_some() {
                return true;
            }
        }
        if let Some(t) = record_raw_text(rec) {
            if self.locate(&t).is_some() {
                return true;
            }
        }
        false
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
            synth_verifiable: Vec::new(),
            synth_conservative: Vec::new(),
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

    // Extract a required literal for the cheap raw-byte prefilter (line + whole-file).
    // Case-sensitive → byte-exact `memmem`. Case-insensitive (the smart-case DEFAULT
    // for a lowercase pattern) → a `(?i)`-wrapped ESCAPED literal compiled as its own
    // bytes regex: `memmem` has no caseless mode, but the regex engine lowers a
    // caseless literal to an accelerated multi-substring scan, so the dominant
    // lowercase search is no longer forced to parse every candidate line.
    let prefilter = match required_literal(&args.pattern) {
        None => None,
        Some(_) if case_insensitive => {
            // `lit` == the whole pattern (no metachars by construction); escape anyway
            // so this stays correct if `required_literal` ever loosens.
            let src = format!("(?i){}", regex::escape(&args.pattern));
            let re = BytesRegex::new(&src)
                .with_context(|| format!("invalid caseless prefilter for {:?}", args.pattern))?;
            Some(Prefilter::CaselessLiteral(re))
        }
        Some(lit) => Some(Prefilter::Literal(memmem::Finder::new(&lit).into_owned())),
    };

    // The synthesized-text escape hatch is only needed when a prefilter can prune.
    let (synth_verifiable, synth_conservative) = if prefilter.is_some() {
        synth_marker_finders(args)
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(Matcher {
        regex: Some(regex),
        prefilter,
        synth_verifiable,
        synth_conservative,
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
    // WHITESPACE-safety (the render-normalization mirror of the JSON-escape rule):
    // several render paths rewrite whitespace before matching — `normalize_line`
    // collapses runs to a single space (genuine-user text via `flatten_content_text`,
    // peer bodies, notification reports) and multi-part texts are joined with `' '` /
    // `'\n'` seams. A literal CONTAINING whitespace can therefore match rendered text
    // (`hello world`) whose raw bytes hold `hello\nworld` — a `memmem` for it would
    // falsely prove absence. A whitespace-FREE literal always sits inside one
    // unrewritten non-whitespace run, which survives verbatim in the raw bytes, so
    // only those stay prefilter-eligible. (This also closes a latent gap the old
    // case-sensitive prefilter had for space-carrying patterns.)
    if pattern.chars().any(char::is_whitespace) {
        return None;
    }
    Some(pattern.as_bytes().to_vec())
}

/// Build the SYNTHESIZED-text marker finders for [`Matcher::synth`] (one SIMD
/// `memmem` scan per needle, per line — see the field doc for why not Aho-Corasick).
///
/// Rationale: the literal prefilter proves absence only for matchable text that is a
/// VERBATIM substring of the record's raw line bytes. A small, closed set of render
/// paths synthesizes text from other sources; each is detectable by a raw marker its
/// carrier record ALWAYS contains:
/// - `<task-notification>` — `automation_label` fabricates the kind slug
///   (`subagent`/`background-command`/…), a `completed` status fallback, and `[…]`
///   scaffolding; the G1 inbox view normalizes the `<result>` body.
/// - `"answers"` + the two synthesized answer markers — the ANSWER carrier's
///   `auq_exchange` render fabricates the `[AskUserQuestion · N question(s)]` scaffold,
///   `Q1/A1` labels and option lists that appear verbatim nowhere in the raw line.
///   (The QUESTION-side `tool_use` needs no needle: its matchable text is
///   `render_tool_use` = the verbatim `name` + the re-serialized `input`, and the name
///   bytes sit in the raw line — a bare `AskUserQuestion` needle would disable the gate
///   for the ~29% of files whose injected context merely MENTIONS the tool.)
/// - `To tell you how to proceed` — the rejection reconstruction appends a
///   `[plan: <path>]` pointer whose path lives on a DIFFERENT record.
/// - `compact_boundary` (only when the `-t` selection can reach
///   `harness.compaction.boundary` — otherwise the boundary line is not even a scan
///   candidate, so its synthesized excerpt is unreachable) — `trigger=…`/`preTokens=…`
///   key=value text is fabricated from `compactMetadata`.
/// - Under `--resolve-persisted`: `persistedOutputPath` / `Full output saved to:` —
///   the matched text is EXTERNAL file content, absent from the transcript bytes by
///   definition.
///
/// False positives only cost speed (the line/file falls back to the full parse +
/// regex pipeline); false negatives are what the set is built to make impossible.
fn synth_marker_finders(
    args: &SearchArgs,
) -> (Vec<memmem::Finder<'static>>, Vec<memmem::Finder<'static>>) {
    // VERIFIABLE (stage-2 re-renderable from the line alone; see `Matcher::synth_*`).
    let mut verifiable: Vec<&[u8]> = vec![
        b"<task-notification>",
        br#""answers""#,
        b"User has answered your questions",
        b"Your questions have been answered",
    ];
    if label_selected(&args.categories, Class::CompactionBoundary.path()) {
        verifiable.push(b"compact_boundary");
    }
    // CONSERVATIVE (needs cross-record / external data — force the full scan).
    let mut conservative: Vec<&[u8]> = vec![b"To tell you how to proceed"];
    if args.resolve_persisted {
        conservative.push(b"persistedOutputPath");
        conservative.push(b"Full output saved to:");
    }
    let mk = |ns: Vec<&[u8]>| {
        ns.into_iter()
            .map(|n| memmem::Finder::new(n).into_owned())
            .collect()
    };
    (mk(verifiable), mk(conservative))
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
pub(crate) struct AddressSet {
    pub(crate) lines: BTreeSet<usize>,
    pub(crate) uuids: BTreeSet<String>,
}

impl AddressSet {
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
    let matcher = build_matcher(args)?;
    if matcher.is_pure_filter()
        && args.categories.is_empty()
        && turn_range.is_none()
        && time_window.is_unbounded()
        && !has_session_filter
    {
        eprintln!(
            "csift: warning: empty pattern with no category/time/turn/session filter \
             matches every exchange in scope — this may emit a lot."
        );
    }

    // ── Resolve targets → session files via the shared (optionally subagent-spanning)
    //    resolver. (Record FETCHING by line/uuid is `csift show`'s job, not search's.) ──
    let session_files = path::resolve_session_files(
        &args.targets(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;

    // `--siblings <SPEC>`: parse the repeatable caps ONCE here (a malformed spec is a hard
    // error, surfaced before any scan). `None` ⇒ siblings off. Parsed up front so the per-file
    // parallel scan just borrows the result.
    let sibling_caps = parse_sibling_specs(&args.siblings)?;

    // ── Spawn-lookup hoist (GOLD §3): build each DISTINCT discovery-root's DiscoveredSpawns
    //    ONCE, then share it across the whole par_iter. A subagent's discovery-root is its parent
    //    top-level `.jsonl` (the SAME for all its siblings), so building the lookup per file made
    //    `search .` O(subagent_count²) — 3290 files each re-running `discover_subagents` over the
    //    parent's 3290-entry `subagents/` tree (~66s on a 1.4 GB corpus). Distinct roots number
    //    only a handful (one per top-level session in scope), so `discover_subagents` now runs
    //    ~7× total instead of once per file. The lookup values are IDENTICAL — only WHEN/how
    //    often they are built changes — so output is byte-for-byte unchanged. Sequential build is
    //    fine: distinct-root count is tiny. `None` ⇒ that root has no resolvable spawns. ──
    let mut spawn_map: HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>> = HashMap::new();
    for p in &session_files {
        spawn_map
            .entry(discovery_root_for(p))
            .or_insert_with_key(|root| build_spawn_lookup(root).map(Arc::new));
    }

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
                None,
                sibling_caps.as_ref(),
                &spawn_map,
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
pub(crate) fn merged_any_sidecar(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| h.from_sidecar)
    })
}

/// True when ANY emitted hit/sibling excerpt was CLIPPED to the default cap — drives the
/// trailing reader-caution note (text) / the `excerpts_truncated` JSON flag. Always false under
/// `--no-truncate` and in `--line`/`--uuid` fetch mode (the cap is lifted to
/// `usize::MAX`, so no hit can be truncated), so a single check both detects truncation AND
/// auto-suppresses the note exactly when the reader already asked for whole records.
fn any_truncated_excerpt(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| h.truncated)
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

/// `csift show`'s fetch engine: the ADDRESSED records of exactly ONE transcript, rendered
/// FULL through the same per-record pipeline `search` uses (classify, plan pointers, tool
/// pairing, elicitation-sidecar merge) with the pure matcher, so every addressed record
/// emits regardless of any pattern. Returns the addressed exchanges + the malformed count.
pub(crate) fn fetch_records(
    path: &Path,
    lines: BTreeSet<usize>,
    uuids: BTreeSet<String>,
) -> Result<(Vec<Exchange>, usize)> {
    let args = SearchArgs::default();
    let matcher = Matcher::pure();
    let address = AddressSet { lines, uuids };
    let time_window = TimeWindow::from_args(None, None)?;
    let mut spawn_map: HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>> = HashMap::new();
    spawn_map
        .entry(discovery_root_for(path))
        .or_insert_with_key(|root| build_spawn_lookup(root).map(Arc::new));
    let fr = search_one_file(
        path,
        &args,
        &matcher,
        None,
        &time_window,
        Some(&address),
        None,
        &spawn_map,
    )?;
    Ok((fr.exchanges, fr.skipped_lines))
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
    spawn_map: &HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>>,
) -> Result<FileResult> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(FileResult {
            exchanges: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    // The D7 `compact_boundary` prefilter-widening is GATED on the active `-t` selector —
    // computed up front because BOTH the whole-file gate below and the candidate scan key on it.
    let needs_compact_boundary = label_selected(&args.categories, Class::CompactionBoundary.path());

    // ── §7f whole-file gate ──
    // When the pattern anchors a raw-byte prefilter (a plain literal, either case mode) and
    // this is NOT an addressing fetch (`--line`/`--uuid` emit records regardless of the
    // pattern), a cheap PARALLEL pre-scan can prove that no candidate line matches: no
    // per-line literal occurrence AND no synthesized-text marker (see [`Matcher::synth`]).
    // Every emitted exchange requires >=1 regex hit (`hits.is_empty() -> continue`), so such
    // a file provably yields nothing — skip building records for it entirely. Mechanics:
    // - the pre-scan runs on the SAME newline-aligned rayon chunking as the full scan (never
    //   a serial whole-mmap pass — that would bottleneck the single-giant-file case);
    // - a relaxed AtomicBool short-circuits it the moment ANY line may match: the remaining
    //   lines skim (one load + return), the partial malformed count is discarded, and the
    //   full scan below recounts exactly — a file WITH matches pays only the skim;
    // - the malformed-line count is a TESTED contract (no silent skip): a gated file's
    //   candidate lines were each syntax-validated (`validate_line_syntax` — no Record
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
                if !line_is_transcript_candidate(line, needs_compact_boundary) {
                    return crate::parse::LineVerdict::Ignore;
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
        // (it was deliberately NOT validated in the pre-scan — no double count).
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
                });
            }
            let (pending, pending_skipped) = crate::elicitation::unresolved_pending(path)?;
            if pending.is_empty() {
                return Ok(FileResult {
                    exchanges: Vec::new(),
                    skipped_lines: gate_skipped + pending_skipped,
                });
            }
        }
    }

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
    // The D7 `compact_boundary` prefilter-widening is GATED on the active `-t` selector: only look
    // for the rare `type:"system"` boundary line when a selector can actually reach
    // `harness.compaction.boundary` (or no `-t` = match-all). A `-t user` / `-t agent.*` search can
    // never match a boundary, so it pays ZERO for the extra check — the hard `-t` filter PRUNES the
    // byte-scan instead of taxing it (computed once above the whole-file gate, captured here).
    let (mut records, mut skipped) = crate::parse::scan_lines_parallel(bytes, |line, line_no| {
        if !line_is_transcript_candidate(line, needs_compact_boundary) {
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
        spawn_map,
    );

    Ok(FileResult {
        exchanges,
        skipped_lines: skipped,
    })
}

/// §7d stage-1 category prefilter on raw bytes: keep a line only if it could be a
/// transcript message (user/assistant role marker) — drops `attachment`,
/// `file-history-snapshot`, `queue-operation`, and metadata noise pre-JSON. Kept
/// deliberately permissive (substring, not structural) so no genuine turn is lost.
fn line_is_transcript_candidate(line: &[u8], needs_compact_boundary: bool) -> bool {
    // Every user/assistant record carries `"role":"user"`/`"role":"assistant"`.
    // (Genuine-user string content, tool carriers, assistant blocks all do.)
    memmem::find(line, br#""role":"user""#).is_some()
        || memmem::find(line, br#""role":"assistant""#).is_some()
        // D7: ALSO keep the rare `compact_boundary` metrics record (a `type:"system"` record with no
        // role marker) so `search -t harness.compaction.boundary` can enumerate compaction points +
        // inspect their `compactMetadata` — but ONLY when an active `-t` selector can reach that label
        // (`needs_compact_boundary`, derived once via `label_selected`). For every other query the
        // `&&` short-circuits BEFORE the memmem, so a non-boundary search pays ZERO. When it IS run,
        // the `||` chain still reaches this memmem only on lines that already failed both role checks,
        // and boundary records are rare — so the §7 perf contract holds either way.
        || (needs_compact_boundary && memmem::find(line, b"compact_boundary").is_some())
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
    spawn_map: &HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>>,
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

    let selectors = &args.categories;
    // `tool_use_id → tool name` across the whole file, so a `tool-response` (a bare
    // `tool_result` carrying only the id) can name the tool it answers (e.g. `tool-response Edit`).
    let tool_names = build_tool_name_index(records);
    // The `▹` pairing id sets (GOLD §7): every `tool_use` id + every `tool_result` `tool_use_id`
    // in this transcript, joined GLOBALLY (not by contiguity) so a use↔result pair resolves across
    // records / parallel calls. A use with no result-id ⇒ pending; a result with no use-id ⇒ orphan.
    let (use_ids, result_ids) = tool_pair_ids(records);
    // Cross-record classify context (GOLD §6): owner identity, subagent-ness, parent id, the first
    // turn-opener line (the subagent spawn-prompt seed), and a spawn lookup. The lookup is HOISTED
    // (GOLD §3): `run_search` built one `DiscoveredSpawns` per DISTINCT discovery-root up front, so
    // here we just BORROW this file's root's entry from the shared map — never re-run the (formerly
    // O(N²) per-file) `discover_subagents` dir+meta scan.
    let spawn_lookup = spawn_map
        .get(&discovery_root_for(path))
        .and_then(|o| o.as_deref());
    let first_opener_line = records
        .iter()
        .find(|k| k.rec.opens_turn())
        .map(|k| k.line_no);
    let env = ClassifyEnv {
        owner_id: &session_id,
        is_subagent,
        parent_id: &parent_session_id,
        first_opener_line,
        spawn: spawn_lookup.map(|s| s as &dyn SpawnLookup),
    };
    // `--no-truncate` lifts the excerpt cap so a found message renders end-to-end (no `… (+N)`).
    // Addressing (`--line`/`--uuid`) means "fetch THIS record" → always full, no excerpt cap.
    let excerpt_max = if args.no_truncate || address.is_some() {
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
        let (mut hits, hit_idxs) = collect_turn_hits(
            &turn,
            selectors,
            matcher,
            time_window,
            args.resolve_persisted,
            excerpt_max,
            &plan_index,
            &tool_names,
            address,
            &env,
        );
        if hits.is_empty() {
            continue;
        }

        // `--siblings <SPEC>`: render the turn's NON-matched records (the rest of the
        // back-and-forth) so a matched user question surfaces with the agent's reply, capped
        // per the parsed SPEC.
        let mut siblings = match sibling_caps {
            Some(caps) => collect_turn_siblings(
                &turn,
                caps,
                &hit_idxs,
                args.resolve_persisted,
                excerpt_max,
                &plan_index,
                &tool_names,
                &env,
            ),
            None => Vec::new(),
        };

        // Resolve the `▹` tool-pairing state of every tool hit/sibling against the file-level id
        // sets (GOLD §7) now that the hits are collected.
        for h in hits.iter_mut().chain(siblings.iter_mut()) {
            set_pairing(h, &use_ids, &result_ids);
        }

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

/// Parsed `--siblings <SPEC>` caps. `per_sel` holds the explicit `<selector>:N` caps (cap of up
/// to N siblings whose label is UNDER that dotted selector); `bare` is the bare-`N` cap (cap of
/// up to N siblings across every label with NO typed cap — "the rest"). Sibling rendering is ON
/// iff at least one spec token was given (so an empty `--siblings` vec → no `SiblingCaps`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SiblingCaps {
    per_sel: Vec<(String, usize)>,
    bare: Option<usize>,
}

impl SiblingCaps {
    /// The cap governing a hit's leaf [`Class`]: the FIRST typed `<selector>:N` whose selector is
    /// a segment-prefix of the class path (so `agent.tool:2` governs both use+result), else the
    /// bare-`N` fallback ("the rest"). `None` ⇒ this label is not shown at all. (Test-only: the
    /// production retain pools by [`Self::matched_selector`]; this is the per-class convenience the
    /// `parse_sibling_specs` unit tests assert.)
    #[cfg(test)]
    fn cap_for(&self, class: Class) -> Option<usize> {
        self.matched_selector(class).map(|(_, n)| n).or(self.bare)
    }

    /// The first typed `(selector, cap)` whose selector is a segment-prefix of `class`'s path —
    /// the cap-pooling KEY (so two leaves under one selector share its counter). `None` ⇒ no
    /// typed cap governs this class (it falls to the bare-`N` pool).
    fn matched_selector(&self, class: Class) -> Option<(&str, usize)> {
        self.per_sel
            .iter()
            .find(|(sel, _)| selector_is_segment_prefix(sel, class.path()))
            .map(|(sel, n)| (sel.as_str(), *n))
    }

    /// True when ONLY a bare-`N` was given (no typed caps), so the `N` is a TOTAL cap across all
    /// labels rather than a per-selector one.
    fn bare_is_total(&self) -> bool {
        self.per_sel.is_empty()
    }
}

/// Validate a `--siblings` SPEC selector token (the same dotted value set as `-t`): returns the
/// trimmed selector when valid, else `None` (the caller frames the error).
fn parse_sibling_category(token: &str) -> Option<String> {
    let t = token.trim();
    selector_is_valid(t).then(|| t.to_string())
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
            let sel = parse_sibling_category(cat_tok.trim()).ok_or_else(|| {
                anyhow!(
                    "--siblings: '{t}' — unknown selector '{}' (want a dotted role.class.sub path, \
                     the same set as -t, or a bare N)",
                    cat_tok.trim()
                )
            })?;
            let n: usize = n_tok.trim().parse().map_err(|_| {
                anyhow!("--siblings: '{t}' — the cap after ':' must be a positive integer")
            })?;
            if n == 0 {
                bail!("--siblings: '{t}' — the cap must be ≥1 (0 means 'do not show', so omit it)");
            }
            if let Some(slot) = caps.per_sel.iter_mut().find(|(s, _)| *s == sel) {
                slot.1 = n; // last write wins for a repeated selector
            } else {
                caps.per_sel.push((sel, n));
            }
        } else {
            let n: usize = t.parse().map_err(|_| {
                anyhow!(
                    "--siblings: '{t}' — want a bare N or a <selector>:N \
                     (selector = a dotted role.class.sub path, the same set as -t)"
                )
            })?;
            if n == 0 {
                bail!("--siblings: '{t}' — the cap must be ≥1 (0 means 'do not show', so omit it)");
            }
            caps.bare = Some(n);
        }
    }
    if caps.per_sel.is_empty() && caps.bare.is_none() {
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

/// A [`SpawnLookup`] for one session, built from its discovered subagents (a cheap
/// `discover_subagents` dir+meta scan — NOT a transcript re-read). Maps the spawn `tool_use_id`
/// → the spawned child's agent id (the id-join) and the spawn NAME → child (the teammate
/// name-join, GOLD §4). Powers comm direction (`self ⇨ child`) + subagent-return detection in
/// [`Record::classify`]/[`Record::direction`]. Absent ⇒ those degrade to the raw name / `?`.
#[derive(Debug, Default)]
struct DiscoveredSpawns {
    by_tool_use_id: HashMap<String, String>,
    by_name: HashMap<String, String>,
}

impl SpawnLookup for DiscoveredSpawns {
    fn child_for_spawn_tool_use_id(&self, tool_use_id: &str) -> Option<String> {
        self.by_tool_use_id.get(tool_use_id).cloned()
    }
    fn child_for_spawn_name(&self, name: &str) -> Option<String> {
        self.by_name.get(name).cloned()
    }
}

/// The TOP-LEVEL parent session `.jsonl` for a subagent transcript path
/// `<ENCODED>/<uuid>/subagents/…/agent-<hex>.jsonl` → `<ENCODED>/<uuid>.jsonl`. `None` when `path`
/// is not under a `subagents/` dir. The parent's sidecar holds the FLAT set of ALL subagents under
/// it, so a lookup built from it resolves an in-subagent spawn / Task-return (GOLD §4).
fn parent_session_jsonl(path: &Path) -> Option<PathBuf> {
    for anc in path.ancestors() {
        if anc.file_name().and_then(|n| n.to_str()) == Some("subagents") {
            // The `<uuid>/` dir sits directly above `subagents/`; the parent session file is its
            // `.jsonl` sibling (a uuid carries no `.`, so `with_extension` only appends).
            return anc.parent().map(|d| d.with_extension("jsonl"));
        }
    }
    None
}

/// The DISCOVERY-ROOT for a session file — the transcript whose sidecar holds the FLAT set of
/// subagents that `classify()`/`direction()` must resolve. For a SUBAGENT transcript that is its
/// PARENT top-level `.jsonl` (ALL of a session's subagents share ONE root); for a TOP-LEVEL file
/// it is the file itself. Because the spawn lookup is IDENTICAL for every file sharing a root,
/// `run_search` builds it ONCE per distinct root and shares it — the O(N²)→O(N) hoist (GOLD §3).
/// (The `parent_session_jsonl` fallback is unreachable: `is_subagent_path` true ⇒ a `subagents/`
/// ancestor exists ⇒ `parent_session_jsonl` returns `Some`; the `unwrap_or_else` only satisfies
/// the type and, even if hit, `discover_subagents` on a subagent path yields no spawns ⇒ `None`.)
fn discovery_root_for(path: &Path) -> PathBuf {
    if is_subagent_path(path) {
        parent_session_jsonl(path).unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

/// Build the [`DiscoveredSpawns`] lookup powering comm direction (`self ⇨ child`) + subagent-return
/// detection, from an already-resolved DISCOVERY-ROOT (see [`discovery_root_for`]). A failed /
/// empty discovery yields `None` (the engine degrades gracefully). Cheap: dir-listing + small
/// `meta.json` reads, bounded by the subagent count — never a transcript content scan. Called ONCE
/// per distinct root (not per file) — the GOLD §3 hoist.
fn build_spawn_lookup(discovery_root: &Path) -> Option<DiscoveredSpawns> {
    let subs = discover_subagents(discovery_root).ok()?;
    if subs.is_empty() {
        return None;
    }
    let mut out = DiscoveredSpawns::default();
    for s in subs {
        if let Some(tuid) = s.spawn_tool_use_id {
            out.by_tool_use_id.entry(tuid).or_insert(s.agent_id.clone());
        }
        if let Some(name) = s.name {
            out.by_name.entry(name).or_insert(s.agent_id.clone());
        }
    }
    if out.by_tool_use_id.is_empty() && out.by_name.is_empty() {
        return None;
    }
    Some(out)
}

/// The per-file cross-record context [`Record::classify`]/[`Record::direction`] need (GOLD §6):
/// the transcript-owner identity, whether it is a subagent transcript, the parent id (a subagent
/// opener's FROM), the FIRST turn-opener line (the spawn-prompt seed), and the spawn lookup.
/// [`Self::ctx_for`] mints the per-record [`ClassifyCtx`] (only `is_transcript_opener` varies).
struct ClassifyEnv<'a> {
    owner_id: &'a str,
    is_subagent: bool,
    parent_id: &'a str,
    /// The physical line of the first record that `opens_turn()` — the subagent spawn-prompt seed
    /// (flips it from `user.message` to `agent.communication.inbox`). `None` ⇒ no opener.
    first_opener_line: Option<usize>,
    spawn: Option<&'a dyn SpawnLookup>,
}

impl ClassifyEnv<'_> {
    fn ctx_for(&self, kept: &Kept) -> ClassifyCtx<'_> {
        ClassifyCtx {
            owner_id: Some(self.owner_id),
            owner_name: None,
            is_subagent: self.is_subagent,
            parent_id: Some(self.parent_id),
            // Only the subagent transcript's first opener (a real native line) is the seed.
            is_transcript_opener: self.is_subagent
                && kept.line_no != 0
                && Some(kept.line_no) == self.first_opener_line,
            spawn: self.spawn,
        }
    }
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

/// The `▹` pairing id sets for a file (GOLD §7): every `tool_use` block's `id` and every
/// `tool_result` block's `tool_use_id`. Joined GLOBALLY (membership, not contiguity) so a use
/// pairs with its result across records / parallel calls.
fn tool_pair_ids(records: &[Kept]) -> (HashSet<String>, HashSet<String>) {
    let mut uses = HashSet::new();
    let mut results = HashSet::new();
    for k in records {
        if let Some(blocks) = k.rec.blocks() {
            for b in blocks {
                match b {
                    Block::ToolUse { id: Some(id), .. } => {
                        uses.insert(id.clone());
                    }
                    Block::ToolResult {
                        tool_use_id: Some(id),
                        ..
                    } => {
                        results.insert(id.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    (uses, results)
}

/// Resolve a tool hit's [`Pairing`] against the file-level id sets (GOLD §7): an
/// `agent.tool.use` is paired iff its result-id is present (else pending — frozen / elicitation /
/// unreturned); an `agent.tool.result` is paired iff its use-id is present (else orphan —
/// compacted / sliced away). A non-tool hit (or one with no id) is left `None`.
fn set_pairing(h: &mut Hit, use_ids: &HashSet<String>, result_ids: &HashSet<String>) {
    let Some(id) = h.tool_use_id.as_deref() else {
        return;
    };
    h.pair = match h.class {
        Class::AgentToolUse => Some(if result_ids.contains(id) {
            Pairing::Paired
        } else {
            Pairing::PendingNoResult
        }),
        Class::AgentToolResult => Some(if use_ids.contains(id) {
            Pairing::Paired
        } else {
            Pairing::OrphanResult
        }),
        _ => None,
    };
}

// Internal pipeline function: the arg list grew as `tool_names` (tool-response naming) and
// `address` (--line/--uuid selector) were threaded through the per-turn scan. Bundling into a
// struct would only relocate the same fields without simplifying the data flow.
#[allow(clippy::too_many_arguments)]
fn collect_turn_hits(
    turn: &Turn<'_>,
    selectors: &[String],
    matcher: &Matcher,
    time_window: &TimeWindow,
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    address: Option<&AddressSet>,
    env: &ClassifyEnv<'_>,
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
            selectors,
            matcher,
            resolve_persisted,
            excerpt_max,
            plan_index,
            tool_names,
            &env.ctx_for(kept),
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

/// The turn's NON-matched records as sibling hits, restricted + CAPPED per the parsed
/// `--siblings <SPEC>`. Reuses [`collect_record_hits`] with a PURE-FILTER matcher (matches
/// every record, so each label-eligible unit of a sibling surfaces with a head excerpt). A
/// record that matched (its index is in `hit_idxs`) is never repeated. The per-record time
/// window is intentionally NOT re-applied: the turn already qualified, and the siblings are
/// context for that qualifying turn. Caps: a `<selector>:N` spec keeps the first N siblings under
/// that selector; a bare `N` keeps the first N across the labels with no typed cap ("the rest"),
/// and when ONLY a bare `N` was given it is a single TOTAL cap across all labels.
#[allow(clippy::too_many_arguments)]
fn collect_turn_siblings(
    turn: &Turn<'_>,
    caps: &SiblingCaps,
    hit_idxs: &[usize],
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    env: &ClassifyEnv<'_>,
) -> Vec<Hit> {
    // Eligible selectors fed to the collector: when a bare-`N` is present it covers "the rest", so
    // EVERY label is eligible (empty selectors ⇒ all); otherwise only the typed selectors are.
    let eligible: Vec<String> = if caps.bare.is_some() {
        Vec::new()
    } else {
        caps.per_sel.iter().map(|(s, _)| s.clone()).collect()
    };
    if eligible.is_empty() && caps.bare.is_none() {
        return Vec::new();
    }
    let pure = Matcher::pure();
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
            &env.ctx_for(kept),
            &mut sibs,
        );
        backfill_address(&mut sibs[before..], kept);
    }
    // Apply the caps in document order, keeping each selector's first N (a bare-only spec is a
    // single TOTAL cap; otherwise the bare-N caps the labels lacking a typed cap, pooled). The
    // typed-cap counter pools by the MATCHED selector string (so `agent.tool:2` caps use+result
    // together).
    let bare_total = caps.bare_is_total();
    let mut total_kept = 0usize;
    let mut per_sel_kept: Vec<(String, usize)> = Vec::new();
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
        match caps.matched_selector(hit.class) {
            Some((sel, cap)) => {
                let kept = per_sel_kept
                    .iter_mut()
                    .find(|(s, _)| s == sel)
                    .map(|(_, n)| n);
                match kept {
                    Some(n) if *n < cap => {
                        *n += 1;
                        true
                    }
                    Some(_) => false,
                    None => {
                        per_sel_kept.push((sel.to_string(), 1));
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

/// Emit hits for every label-eligible UNIT of `rec` that matches the regex (the P2 cutover —
/// GOLD §6). The record is classified ONCE via [`Record::classify`]; each emission UNIT (the
/// record-level user/comm/harness text, the user-facing tool_result dual, or a block) picks the
/// RICHEST selected [`Class`] among its candidate labels (GOLD §3 Q4 dedup) and emits ONE hit.
/// Comm units carry the `from ⇨ to` direction ([`Record::direction`]); tool units carry the
/// `tool_use_id` for the later `▹` pairing pass. A record carrying NO label (metadata / an
/// excluded isMeta pseudo-turn) yields nothing.
// Internal pipeline function; `tool_names` (tool-response naming) + `ctx` (cross-record classify
// context) are threaded through. Same rationale as `collect_turn_hits` for not bundling into a
// struct.
#[allow(clippy::too_many_arguments)]
fn collect_record_hits(
    rec: &Record,
    selectors: &[String],
    matcher: &Matcher,
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    ctx: &ClassifyCtx,
    hits: &mut Vec<Hit>,
) {
    let labels = rec.classify(ctx);
    if labels.is_empty() {
        return; // unmodeled / excluded record — carries no role.class.sub label
    }
    let ts = rec.timestamp.clone();
    let label_paths: Vec<&'static str> = labels.iter().map(|c| c.path()).collect();
    let sel = |c: Class| label_selected(selectors, c.path());
    let has = |c: Class| labels.contains(&c);
    // Direction is per-record (the first comm direction); computed only when a comm label is
    // present (it parses peer sections / scans blocks), and attached to comm hits only. The
    // owner's own id renders as `self` (GOLD §3/§4: `self ⇨ to`, `from ⇨ self`).
    let direction = if labels.iter().copied().any(is_comm_class) {
        alias_self(rec.direction(ctx), ctx.owner_id)
    } else {
        None
    };

    // One emission: locate the match, build the match-centered excerpt, carry class/labels/
    // direction/tool_use_id. `pair` is filled later by the per-file pairing pass.
    let mut emit = |class: Class,
                    text: &str,
                    tool_name: Option<String>,
                    dir: Option<(String, String)>,
                    tuid: Option<String>| {
        if let Some(span) = matcher.locate(text) {
            let (excerpt, truncated) = match_excerpt(text, span, excerpt_max);
            hits.push(Hit {
                class,
                labels: label_paths.clone(),
                excerpt,
                timestamp_utc: ts.clone(),
                tool_name,
                direction: dir,
                tool_use_id: tuid,
                pair: None,
                line: 0,
                uuid: None,
                image_ids: Vec::new(),
                from_sidecar: false,
                truncated,
            });
        }
    };

    // ── 1. Record-level TEXT unit(s). A BATCHED record (≥1 `<task-notification>` / inbound-peer
    //    section) renders ONE hit PER section (GOLD §3 G4/G5), each with its own label + direction
    //    — so a notification-with-`<result>` ALSO surfaces its `agent.communication.inbox`
    //    (child ⇨ self, G1), and several mixed-kind sections no longer collapse to one. Any other
    //    record-text class (user.message, harness markers, compaction, a subagent-opener inbox)
    //    renders ONE richest-label hit. The §1 fix (teammate → inbox) + the `<task-notification>`
    //    → harness.notification reparent flow straight from `classify`. ──
    let sections = rec.record_text_sections(ctx);
    if sections.is_empty() {
        if let Some((class, text)) = record_text_emission(rec, &labels, selectors, plan_index) {
            let dir = if is_comm_class(class) {
                direction.clone()
            } else {
                None
            };
            emit(class, &text, None, dir, None);
        }
    } else {
        for crate::model::RecordTextSection {
            class,
            text,
            direction: dir,
        } in sections
        {
            if !label_selected(selectors, class.path()) {
                continue;
            }
            let dir = if is_comm_class(class) {
                alias_self(dir, ctx.owner_id)
            } else {
                None
            };
            emit(class, &text, None, dir, None);
        }
    }

    // ── 2. Record-level user-facing tool_result DUAL (AUQ answer / typed rejection) ──
    // These are RECORD-level facts, so emit ONCE (not per tool_result block); GOLD §3 Q4: the
    // user-facing view is RICHEST, superseding the agent.tool.result copy (the block loop then
    // skips it). `reconstructed_user_text` yields the clean Q+options+answer / rejection (+[plan:])
    // unit. When neither user-facing label is SELECTED, `user_dual` is None and the block loop
    // surfaces the plain agent.tool.result instead (so `-t agent.tool.result` still finds it).
    let user_dual = if has(Class::UserAnswer) && sel(Class::UserAnswer) {
        Some(Class::UserAnswer)
    } else if has(Class::UserRejection) && sel(Class::UserRejection) {
        Some(Class::UserRejection)
    } else {
        None
    };
    if let Some(class) = user_dual {
        if let Some(text) = rec.reconstructed_user_text(Some(plan_index)) {
            emit(class, &text, None, None, None);
        }
    }

    // ── 3. §3.10 MCP elicitation marker with NO tool_use block → agent.tool.use (content string).
    // The AUQ/ExitPlanMode markers DO carry a tool_use block and surface via the block loop, so
    // this arm is GUARDED to a no-tool_use marker to avoid a double emit (keep the guard). ──
    if has(Class::AgentToolUse)
        && sel(Class::AgentToolUse)
        && rec.is_elicitation_marker()
        && rec
            .blocks()
            .is_none_or(|bs| !bs.iter().any(|b| matches!(b, Block::ToolUse { .. })))
    {
        if let Some(text) = rec.content.as_ref().and_then(serde_json::Value::as_str) {
            emit(
                Class::AgentToolUse,
                text,
                rec.csift_kind.clone(),
                None,
                None,
            );
        }
    }

    // ── 4. Block-bearing units: thinking / agent text / tool_use (+comm) / tool_result (+comm). ──
    if let Some(blocks) = rec.blocks() {
        for block in blocks {
            match block {
                Block::Thinking { thinking, .. }
                    if has(Class::AgentThinking) && sel(Class::AgentThinking) =>
                {
                    emit(Class::AgentThinking, thinking, None, None, None);
                }
                Block::RedactedThinking { .. }
                    if has(Class::AgentThinking) && sel(Class::AgentThinking) =>
                {
                    // Opaque/encrypted reasoning — no readable text; surface a placeholder so
                    // `-t agent.thinking` still finds the block (GOLD §2 / oracle B3).
                    emit(
                        Class::AgentThinking,
                        REDACTED_THINKING_PLACEHOLDER,
                        None,
                        None,
                        None,
                    );
                }
                Block::Text { text }
                    if rec.is_type("assistant")
                        && has(Class::AgentMessage)
                        && sel(Class::AgentMessage) =>
                {
                    // Only assistant `text` blocks are the agent message; a user `text` block is
                    // a record-text unit (handled above), never agent.message.
                    emit(Class::AgentMessage, text, None, None, None);
                }
                Block::ToolUse { id, name, input } => {
                    // Richest-selected for this tool_use: comm (sent/signal) > agent.tool.use.
                    let comm = tool_use_comm_class(name.as_deref(), input.as_ref());
                    let class = match comm {
                        Some(cc) if has(cc) && sel(cc) => Some(cc),
                        _ if has(Class::AgentToolUse) && sel(Class::AgentToolUse) => {
                            Some(Class::AgentToolUse)
                        }
                        _ => None,
                    };
                    if let Some(class) = class {
                        let rendered = render_tool_use(name.as_deref(), input.as_ref());
                        let dir = if is_comm_class(class) {
                            direction.clone()
                        } else {
                            None
                        };
                        emit(class, &rendered, name.clone(), dir, id.clone());
                    }
                }
                Block::ToolResult {
                    content: Some(c),
                    tool_use_id,
                    ..
                } => {
                    // The user-facing dual was SELECTED + emitted as the richest view (§3 Q4) → skip
                    // the agent.tool.result duplicate. (When the dual is present but NOT selected —
                    // e.g. `-t agent.tool.result` alone — `user_dual` is None, so the plain result
                    // still surfaces and the answer is never lost.)
                    if user_dual.is_some() {
                        continue;
                    }
                    // Richest-selected: agent.communication.inbox (subagent return) > tool.result.
                    let class = if has(Class::CommInbox) && sel(Class::CommInbox) {
                        Class::CommInbox
                    } else if has(Class::AgentToolResult) && sel(Class::AgentToolResult) {
                        Class::AgentToolResult
                    } else {
                        continue;
                    };
                    let mut text = tool_result_content_text(c);
                    // §4.6: when asked, replace the inline persisted-output pointer with the real
                    // file content (matching runs against the resolved text).
                    if resolve_persisted {
                        if let Some(path) = rec.persisted_output_path() {
                            text = resolve_persisted_text(&path, &text);
                        }
                    }
                    let name = tool_use_id
                        .as_deref()
                        .and_then(|id| tool_names.get(id).cloned());
                    let dir = if class == Class::CommInbox {
                        direction.clone()
                    } else {
                        None
                    };
                    emit(class, &text, name, dir, tool_use_id.clone());
                }
                _ => {}
            }
        }
    }
}

/// True for the three `agent.communication.*` leaves (render `from ⇨ to`, GOLD §4).
fn is_comm_class(c: Class) -> bool {
    matches!(c, Class::CommInbox | Class::CommSent | Class::CommSignal)
}

/// Render the transcript owner's own id as the literal `self` on either side of a comm direction
/// (GOLD §3/§4 notation: `self ⇨ to`, `from ⇨ self`) — a verbose session uuid / bare agent hex on
/// the self side becomes `self`, while a peer id/name on the OTHER side is kept verbatim (a peer
/// never equals the owner). No-op when `owner_id` is `None`.
fn alias_self(dir: Option<(String, String)>, owner_id: Option<&str>) -> Option<(String, String)> {
    let Some(owner) = owner_id else {
        return dir;
    };
    dir.map(|(from, to)| {
        let sub = |s: String| if s == owner { "self".to_string() } else { s };
        (sub(from), sub(to))
    })
}

/// True for a RECORD-LEVEL text class — one classified from a record's string / text-block
/// content (NOT a per-block agent class, and NOT the tool_result duals `user.answer`/
/// `user.rejection`, which are handled in the ToolResult arm). Drives [`record_text_emission`].
fn is_record_text_class(c: Class) -> bool {
    matches!(
        c,
        Class::UserMessage
            | Class::CommInbox
            | Class::CommSignal
            | Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask
            | Class::CompactionSummary
            | Class::CompactionBoundary
            | Class::CommandInvocation
            | Class::CommandStdout
            | Class::InterruptUser
            | Class::InterruptTool
            | Class::ScheduleWakeup
            | Class::ScheduleContinuation
            | Class::MetaHook
            | Class::MetaLoop
    )
}

/// The richest SELECTED record-level text class + its display text (GOLD §3/§6). Iterates the
/// record's labels in `classify`'s richest-first order, taking the first record-text class that
/// is selected, then resolving its text source: a `<task-notification>` → `automation_label`; a
/// genuine/AUQ/rejection/teammate-prose/subagent-opener → `reconstructed_user_text`; any other
/// harness marker / compaction summary / teammate-signal → the raw string. `None` ⇒ no record-
/// text class is selected (or it has no text).
fn record_text_emission(
    rec: &Record,
    labels: &[Class],
    selectors: &[String],
    plan_index: &PlanIndex,
) -> Option<(Class, String)> {
    for &c in labels {
        if !is_record_text_class(c) || !label_selected(selectors, c.path()) {
            continue;
        }
        let text = match c {
            Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask => rec.automation_label(),
            Class::UserMessage | Class::CommInbox => rec.reconstructed_user_text(Some(plan_index)),
            // A teammate signal rides on the raw string; `reconstructed_user_text` returns it for a
            // teammate record (it flattens the content), with the raw text as the fallback.
            Class::CommSignal => rec
                .reconstructed_user_text(Some(plan_index))
                .or_else(|| record_raw_text(rec)),
            _ => record_raw_text(rec),
        };
        if let Some(text) = text {
            return Some((c, text));
        }
    }
    None
}

/// The raw textual body of a record for harness-marker matching: the bare string, or the text
/// blocks joined with `\n` (mirrors the engine's `raw_message_text`). For a MESSAGE-LESS record (a
/// `type:"system"` record — e.g. the `compact_boundary` metrics record) it falls back (D7) to the
/// top-level `content` plus a readable `compactMetadata` excerpt, so the boundary is BOTH matchable
/// and rendered. `None` when there is no text anywhere.
fn record_raw_text(rec: &Record) -> Option<String> {
    let Some(msg) = rec.message.as_ref() else {
        // No `message` blocks → a system record. D7: the boundary's content + compactMetadata.
        return system_record_text(rec);
    };
    match msg.content.as_ref()? {
        Content::Text(s) => Some(s.clone()),
        Content::Blocks(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
    }
}

/// D7: the searchable + renderable text of a MESSAGE-LESS system record — in practice the
/// `compact_boundary` metrics record (the only message-less system record `classify` labels).
/// Combines the top-level `content` string (`"Conversation compacted …"`) with a readable
/// `compactMetadata` excerpt so `-t harness.compaction.boundary` can both MATCH the boundary and SEE
/// what each compaction clipped. `None` when neither is present (no fabricated text).
fn system_record_text(rec: &Record) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(serde_json::Value::String(s)) = rec.content.as_ref() {
        let s = s.trim();
        if !s.is_empty() {
            parts.push(s.to_string());
        }
    }
    if let Some(excerpt) = rec
        .compact_metadata
        .as_ref()
        .and_then(compact_metadata_excerpt)
    {
        parts.push(excerpt);
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Render a `compact_boundary` record's `compactMetadata` object as a one-line readable excerpt —
/// `[compaction boundary: trigger=auto preTokens=1000 postTokens=200 durationMs=50]` (only the
/// present fields, stable order, scalars unquoted). `None` when it is not an object or carries none
/// of the known fields.
fn compact_metadata_excerpt(meta: &serde_json::Value) -> Option<String> {
    let obj = meta.as_object()?;
    let mut fields: Vec<String> = Vec::new();
    for key in ["trigger", "preTokens", "postTokens", "durationMs"] {
        if let Some(v) = obj.get(key) {
            let rendered = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            fields.push(format!("{key}={rendered}"));
        }
    }
    (!fields.is_empty()).then(|| format!("[compaction boundary: {}]", fields.join(" ")))
}

/// The communication [`Class`] a `tool_use` block carries (GOLD §3): a `SendMessage` →
/// `…sent`/`…signal`; a `Task`/`Agent`/`Workflow` spawn → `…sent`. `None` for any other tool.
/// REPLICATES the engine's per-record decision per-BLOCK (so a record with mixed comm/non-comm
/// tool_use blocks labels each correctly) — kept faithful to model.rs `classify_assistant`.
fn tool_use_comm_class(name: Option<&str>, input: Option<&serde_json::Value>) -> Option<Class> {
    match name? {
        "SendMessage" => Some(if send_message_is_signal(input) {
            Class::CommSignal
        } else {
            Class::CommSent
        }),
        n if is_spawn_tool_name(n) => Some(Class::CommSent),
        _ => None,
    }
}

/// Replica of the engine's spawn-tool set (model.rs `is_spawn_tool_name`).
fn is_spawn_tool_name(name: &str) -> bool {
    matches!(name, "Task" | "Agent" | "Workflow")
}

/// Replica of the engine's `send_message_is_signal` (model.rs): a `SendMessage` whose top-level
/// (or nested `message`) `type` is present and is NOT `message`/`direct` is a control SIGNAL.
fn send_message_is_signal(input: Option<&serde_json::Value>) -> bool {
    let Some(input) = input else {
        return false;
    };
    let type_at = |v: &serde_json::Value| {
        v.get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    if let Some(t) = type_at(input) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    if let Some(t) = input.get("message").and_then(type_at) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    false
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
            if crate::model::is_auq_answer_text(&t) {
                return Some(t);
            }
        }
    }
    None
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
/// Test-only: production excerpting goes through [`match_excerpt`], which carries the
/// caller's (possibly `--no-truncate`) budget; this fixed-budget wrapper backs the unit tests.
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
///
/// Returns `(excerpt, truncated)` — `truncated` is true iff content was CLIPPED to fit `max`
/// (the head form when the normalized text exceeds `max`, or any match-centered window). Under
/// `--no-truncate`'s `usize::MAX` budget nothing is ever clipped, so `truncated` is always false there.
fn match_excerpt(text: &str, span: Option<(usize, usize)>, max: usize) -> (String, bool) {
    let total = text.chars().count();
    // Pure filter, or the whole message already fits (incl. `--no-truncate`'s `usize::MAX`): keep
    // the head-anchored form, capped at `max` (uncapped under `--no-truncate`). Truncated iff the
    // normalized body still overruns `max`.
    let head_form = |text: &str| -> (String, bool) {
        let norm = normalize_line(text);
        let truncated = norm.chars().count() > max;
        (crate::text::truncate_excerpt(&norm, max), truncated)
    };
    let start_byte = match span {
        Some((s, _)) if total > max => s,
        _ => return head_form(text),
    };
    // Char index of the match start; a non-char-boundary byte offset (possible with a
    // raw-byte regex) falls back to the head rather than panicking.
    let Some(prefix) = text.get(..start_byte) else {
        return head_form(text);
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
    // The window form is only reached when `total > max`, so a `max`-char window necessarily
    // dropped surrounding content — this is always a truncated fragment.
    (out, true)
}

/// Parse a `--turn-range START..END` into an inclusive 0-based `(lo, hi)` (shared parser).
fn parse_turn_range(s: &str) -> Result<(usize, usize)> {
    crate::text::parse_range(s, "--turn-range", false)
}

// ── Rendering ──
//
// Timestamp formatting (system-local + raw UTC) lives in `crate::timez`, shared
// with `list` so the local-timezone choice is defined once.

/// Glyph for the ROLE a hit sits on (GOLD §6): `◂` user, `▸` agent, `⚙` harness machinery.
/// (`⚙`/gear is the chosen distinct harness marker — visually separate from the two
/// conversational sides without colliding with the `⇨`/`▹` comm/pairing markers.)
pub(crate) fn role_glyph(class: Class) -> char {
    match class.role() {
        crate::model::Role::User => '◂',
        crate::model::Role::Agent => '▸',
        crate::model::Role::Harness => '⚙',
    }
}

/// The rendered label for a hit: the dotted [`Class::path`], DECORATED with the GOLD §4/§7
/// markers — a `▹` for a paired/pending/orphan tool hit, an `<from> ⇨ <to>` for a comm hit.
fn render_label(h: &Hit) -> String {
    // Tool pairing (▹) takes the dedicated two-sided form (GOLD §7).
    match (h.class, h.pair) {
        (Class::AgentToolUse | Class::AgentToolResult, Some(Pairing::Paired)) => {
            return "agent.tool.use ▹ agent.tool.result".to_string();
        }
        (Class::AgentToolUse, Some(Pairing::PendingNoResult)) => {
            return "agent.tool.use (no result — pending)".to_string();
        }
        (Class::AgentToolResult, Some(Pairing::OrphanResult)) => {
            return "agent.tool.result (use not in scope)".to_string();
        }
        _ => {}
    }
    // Comm direction (⇨): append `from ⇨ to` to the label path (GOLD §4).
    if let Some((from, to)) = &h.direction {
        return format!("{}  {from} ⇨ {to}", h.class.path());
    }
    h.class.path().to_string()
}

fn render_text(outcome: &SearchOutcome, args: &SearchArgs) {
    // SCOPE banner FIRST (before the empty check) so a bare `csift search '' <uuid>` fan-out
    // announces it spanned N subagents up front — same disclosure as list/files/turns.
    crate::text::emit_scope_banner(outcome.scope_top, outcome.scope_sub);
    if outcome.exchanges.is_empty() {
        println!("no matching exchanges");
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
            print_record_line(role_glyph(hit.class), hit);
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
        args.categories.join(",")
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
    if outcome.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(outcome.skipped_lines));
    }
    // ── Reader-caution (LAST, only when the default cap actually CLIPPED ≥1 excerpt) ──
    // The excerpts above are match-centered FRAGMENTS, not summaries — a consumer that trusts the
    // first sentences of a clipped fragment can badly misread the record's full intent. Tell it
    // exactly how to get the whole text. Auto-suppressed under --no-truncate / --line / --uuid (those
    // lift the cap, so nothing is truncated → `any_truncated_excerpt` is false).
    if any_truncated_excerpt(&outcome.exchanges) {
        emit_truncation_caution();
    }
}

/// The trailing reader-caution printed when ≥1 excerpt was truncated: what the excerpts ARE
/// (clipped fragments, not summaries), why that matters (a fragment can misrepresent the whole),
/// and the exact flags to read the full text. Kept as its own fn so the wording lives in one
/// place (text only — JSON callers read the `excerpts_truncated` summary flag instead).
fn emit_truncation_caution() {
    println!();
    println!(
        "note: matches above are TRUNCATED, match-centered FRAGMENTS — not summaries. A fragment \
         can read very differently from the record's full intent, so do NOT draw conclusions from \
         it alone."
    );
    println!("  whole records: re-run with --no-truncate");
    println!(
        "  one record in full: csift show <@session|@agent-id> --line <N> (the L<n> shown on \
         a row) or --uuid <U>"
    );
}

/// One hit/sibling line: `<marker> <label>[ <tool>]  L<line>  <excerpt>` (excerpt inline; its
/// newlines are already collapsed to single spaces). `marker` is the role glyph for a match or a
/// dim `·` for a `--siblings` context record; `<label>` is the dotted path with the GOLD §4/§7
/// `⇨`/`▹` decorations ([`render_label`]).
pub(crate) fn print_record_line(marker: char, h: &Hit) {
    let label = render_label(h);
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

/// The JSON rendering of a tool hit's `▹` pairing state (shared with `show`).
pub(crate) fn pairing_json(p: Option<Pairing>) -> serde_json::Value {
    match p {
        Some(Pairing::Paired) => serde_json::json!("paired"),
        Some(Pairing::PendingNoResult) => serde_json::json!("pending"),
        Some(Pairing::OrphanResult) => serde_json::json!("orphan"),
        None => serde_json::Value::Null,
    }
}

/// Render one `Hit` (a match OR a `--siblings` context record) to its JSON object — the
/// shared per-hit shape used by both the `hits` and `siblings` envelope arrays.
fn hit_json(h: &Hit) -> serde_json::Value {
    // Comm direction (GOLD §4): `from`/`to` only for an `agent.communication.*` hit, else null.
    let (from, to) = match &h.direction {
        Some((f, t)) => (serde_json::json!(f), serde_json::json!(t)),
        None => (serde_json::Value::Null, serde_json::Value::Null),
    };
    // Tool pairing (GOLD §7): the `▹` join state of an agent.tool.use/result hit, else null.
    let pairing = pairing_json(h.pair);
    serde_json::json!({
        // The matched dotted leaf path (`label`) + the record's FULL label set (`labels`).
        "label": h.class.path(),
        "labels": h.labels,
        "excerpt": h.excerpt,
        "ts_utc": h.timestamp_utc,
        "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
        "tool_name": h.tool_name,
        // Comm direction (`agent.communication.*`); null on a non-comm hit.
        "from": from,
        "to": to,
        // Tool-pairing (§7): the use↔result join state + the joining `tool_use_id`; null on a
        // non-tool hit.
        "pairing": pairing,
        "tool_use_id": h.tool_use_id,
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
    // alongside `matched` — the same cheap always-on total the text footer carries.
    let summary = json!({
        "matched": outcome.exchanges.len(),
        "sessions": distinct_session_count(&outcome.exchanges),
        "dropped_by_cap": outcome.dropped_by_cap,
        "skipped_lines": outcome.skipped_lines,
        // True when ≥1 emitted record was merged from the elicitation sidecar (§3.10) — the
        // machine echo of the `with elicitation sidecar` text note.
        "with_elicitation_sidecar": merged_any_sidecar(&outcome.exchanges),
        // True when ≥1 emitted excerpt was CLIPPED to the default cap — the machine echo of the
        // trailing reader-caution. A consumer seeing this should re-fetch the record in full
        // (per-hit `excerpt` is a match-centered fragment, not the whole text) via `--no-truncate`, or a
        // single record via `--line`/`--uuid`. Always false under those (the cap is lifted).
        "excerpts_truncated": any_truncated_excerpt(&outcome.exchanges),
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
            no_truncate: false,
            resolve_persisted: false,
            no_subagents: false,
            format: OutputFormat::Text,
        }
    }

    fn rec(line: &str) -> Record {
        serde_json::from_str(line).expect("valid record")
    }

    /// A neutral top-level [`ClassifyEnv`] for the `collect_turn_hits` unit tests (no subagent,
    /// no spawn lookup) — the per-record ctx degrades to [`ClassifyCtx::top_level`]'s behavior.
    fn test_env() -> ClassifyEnv<'static> {
        ClassifyEnv {
            owner_id: "0a1b2c3d-0000-0000-0000-000000000000",
            is_subagent: false,
            parent_id: "0a1b2c3d-0000-0000-0000-000000000000",
            first_opener_line: None,
            spawn: None,
        }
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
        // Smart-case lowercased → case-insensitive → the CASELESS literal prefilter:
        // still a raw-byte gate, but folding case (any-case occurrences pass).
        let m = build_matcher(&args("carry")).unwrap();
        assert!(matches!(m.prefilter, Some(Prefilter::CaselessLiteral(_))));
        assert!(m.line_may_match(b"...the CARRY logic..."));
        assert!(m.line_may_match(b"...the carry logic..."));
        assert!(!m.line_may_match(b"...nothing relevant..."));
        assert!(!m.file_may_match(b"a whole file without the needle"));
        assert!(m.file_may_match(b"prefix bytes then Carry appears"));
        // A case-sensitive plain literal gets the byte-exact memmem prefilter.
        let m2 = build_matcher(&args("Carry")).unwrap();
        assert!(matches!(m2.prefilter, Some(Prefilter::Literal(_))));
        assert!(m2.line_may_match(b"...the Carry logic..."));
        assert!(!m2.line_may_match(b"...the CARRY logic..."));
        assert!(!m2.line_may_match(b"...nothing relevant..."));
    }

    #[test]
    fn prefilter_whitespace_literal_is_ineligible() {
        // `normalize_line` collapses whitespace in several render paths (genuine-user
        // text, peer bodies, notification reports), so a rendered "hello world" can be
        // raw "hello\nworld" — a space-carrying literal must NOT anchor a byte
        // prefilter in EITHER case mode.
        let m = build_matcher(&args("hello world")).unwrap();
        assert!(m.prefilter.is_none());
        let m2 = build_matcher(&args("Hello World")).unwrap();
        assert!(m2.prefilter.is_none(), "case-sensitive too");
        // No prefilter ⇒ nothing is provably a miss.
        assert!(m.line_may_match(b"unrelated bytes"));
        assert!(m.file_may_match(b"unrelated bytes"));
    }

    #[test]
    fn synth_marker_keeps_line_and_file_matchable_without_literal() {
        // A `<task-notification>` record renders a FABRICATED kind slug ("subagent") +
        // status that appear nowhere in its raw bytes; the marker must keep the
        // line/file in the match pipeline even though the literal prefilter misses.
        let m = build_matcher(&args("subagent")).unwrap();
        assert!(m.prefilter.is_some());
        let line = br#"{"type":"user","message":{"role":"user","content":"<task-notification><task-id>t1</task-id><summary>Agent \"probe\" completed</summary></task-notification>"}}"#;
        assert!(m.line_may_match(line));
        assert!(m.file_may_match(line));
        // The rejection reconstruction appends a `[plan: …]` pointer resolved from a
        // DIFFERENT record — its marker keeps the carrier matchable too.
        let rej = br#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"To tell you how to proceed, the user said:\ngo"}]}}"#;
        let m_plan = build_matcher(&args("plan")).unwrap();
        assert!(m_plan.line_may_match(rej));
        // A line with neither literal nor marker is still provably a miss.
        assert!(!m.line_may_match(b"{\"type\":\"user\"} nothing relevant"));
        assert!(!m.file_may_match(b"a whole file with nothing relevant"));
    }

    #[test]
    fn resolve_persisted_flag_adds_pointer_markers() {
        let mut a = args("zzguarded");
        a.resolve_persisted = true;
        let m = build_matcher(&a).unwrap();
        // A persisted-output pointer line can match EXTERNAL file content, so under
        // `--resolve-persisted` it must stay matchable despite lacking the literal.
        let line = br#"{"toolUseResult":{"persistedOutputPath":"/tmp/x.txt"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"Full output saved to: /tmp/x.txt"}]}}"#;
        assert!(m.line_may_match(line));
        // Without the flag the same line is provably a miss.
        let m2 = build_matcher(&args("zzguarded")).unwrap();
        assert!(!m2.line_may_match(line));
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
            &["agent.tool.result".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut no_resolve,
        );
        assert!(no_resolve.is_empty(), "deep token must NOT match inline");

        // WITH resolution: the file is read, the token is found → exactly one hit.
        let mut with_resolve = Vec::new();
        collect_record_hits(
            &r,
            &["agent.tool.result".to_string()],
            &m,
            true,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut with_resolve,
        );
        assert_eq!(with_resolve.len(), 1, "deep token matches after resolution");
        assert_eq!(with_resolve[0].class, Class::AgentToolResult);

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
            &["agent.thinking".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].class, Class::AgentThinking);
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
            &["agent.message".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].class, Class::AgentMessage);
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
            &["agent.tool.use".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
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
            &["agent.tool.use".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(
            hits.len(),
            1,
            "MCP system marker must produce exactly one hit"
        );
        assert_eq!(hits[0].class, Class::AgentToolUse);
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
            &["agent.tool.use".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
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
            &["user".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].class, Class::UserAnswer);
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
            &["user".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(hits.len(), 0);
        // But the tool-response category does.
        let mut hits2 = Vec::new();
        collect_record_hits(
            &r,
            &["agent.tool.result".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits2,
        );
        assert_eq!(hits2.len(), 1);
        assert_eq!(hits2[0].class, Class::AgentToolResult);
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
        // The fixture path is a non-existent top-level transcript, so its discovery-root resolves
        // no subagents — an empty spawn map (lookup miss ⇒ `None`) reproduces exactly what the
        // former per-file `build_spawn_lookup` returned here.
        let spawn_map: HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>> = HashMap::new();
        reconstruct_and_match(
            std::path::Path::new("/x/0a1b2c3d-0000-0000-0000-000000000000.jsonl"),
            &kept,
            a,
            &matcher,
            tr.as_ref(),
            &tw,
            None,
            sibling_caps.as_ref(),
            &spawn_map,
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
        a.categories = vec!["agent.message".to_string()];
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
        a.categories = vec!["agent.thinking".to_string()];
        let ex = search(&fixture(), &a);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].turn_index, 0);
        assert_eq!(ex[0].hits.len(), 1);
        assert_eq!(ex[0].hits[0].class, Class::AgentThinking);
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
        a.categories = vec!["agent.message".to_string()];
        let ex = search(&fixture(), &a);
        // "carry" appears in turn 0's thinking AND agent text; with -t agent only
        // the agent hit is emitted (one exchange, one agent hit).
        assert_eq!(ex.len(), 1);
        assert!(ex[0].hits.iter().all(|h| h.class == Class::AgentMessage));
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
        a.categories = vec!["user".to_string()];
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
            .any(|h| h.class == Class::UserAnswer && h.excerpt.contains("bold option")));
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
        a.categories = vec!["user".to_string()];
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
            .any(|h| h.class == Class::UserAnswer && h.excerpt.contains("teal option")));
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
        // No selector filter → every label eligible.
        let ex = search(&lines, &args("zzqq"));
        assert_eq!(ex.len(), 1);
        let cats: Vec<Class> = ex[0].hits.iter().map(|h| h.class).collect();
        assert_eq!(
            cats,
            vec![Class::UserAnswer],
            "AUQ answer must appear ONCE as the richest view `user.answer`, not also agent.tool.result"
        );
    }

    // ── Branch-completeness for the pure render helpers ──

    #[test]
    fn class_path_and_role_glyph_cover_every_leaf() {
        // Every Class::ALL leaf round-trips through path() (the rendered/JSON label) and maps to a
        // role glyph (◂ user, ▸ agent, ⚙ harness) — the cutover replacement for the old flat
        // category_label/glyph table.
        for &c in Class::ALL {
            assert!(!c.path().is_empty());
            let g = role_glyph(c);
            assert!(matches!(g, '◂' | '▸' | '⚙'), "{} -> {g}", c.path());
        }
        assert_eq!(role_glyph(Class::UserMessage), '◂');
        assert_eq!(role_glyph(Class::AgentMessage), '▸');
        assert_eq!(role_glyph(Class::CommInbox), '▸'); // comm is agent-side
        assert_eq!(role_glyph(Class::NotificationWorkflow), '⚙');
    }

    #[test]
    fn render_label_decorates_pairing_and_direction() {
        // ▹ pairing: a paired tool.use renders the two-sided form; a pending use / orphan result
        // render their notes. ⇨ direction: a comm hit appends `from ⇨ to`.
        let paired = Hit {
            class: Class::AgentToolUse,
            labels: vec!["agent.tool.use"],
            excerpt: String::new(),
            timestamp_utc: None,
            tool_name: None,
            direction: None,
            tool_use_id: Some("t1".into()),
            pair: Some(Pairing::Paired),
            line: 0,
            uuid: None,
            image_ids: Vec::new(),
            from_sidecar: false,
            truncated: false,
        };
        assert_eq!(render_label(&paired), "agent.tool.use ▹ agent.tool.result");
        let pending = Hit {
            pair: Some(Pairing::PendingNoResult),
            ..paired.clone()
        };
        assert_eq!(
            render_label(&pending),
            "agent.tool.use (no result — pending)"
        );
        let orphan = Hit {
            class: Class::AgentToolResult,
            pair: Some(Pairing::OrphanResult),
            ..paired.clone()
        };
        assert_eq!(
            render_label(&orphan),
            "agent.tool.result (use not in scope)"
        );
        let comm = Hit {
            class: Class::CommInbox,
            direction: Some(("VSMultiRegion".into(), "self".into())),
            tool_use_id: None,
            pair: None,
            ..paired.clone()
        };
        assert_eq!(
            render_label(&comm),
            "agent.communication.inbox  VSMultiRegion ⇨ self"
        );
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
            &["user".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
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
        let (ex, truncated) = match_excerpt(&text, Some(span), EXCERPT_MAX);
        assert!(ex.contains(needle), "excerpt must show the match: {ex}");
        assert!(
            ex.starts_with('…'),
            "content precedes the window → leading …: {ex}"
        );
        assert!(
            ex.contains("chars)"),
            "content follows → trailing count: {ex}"
        );
        assert!(truncated, "a clipped match-centered window is truncated");
    }

    #[test]
    fn match_excerpt_short_message_is_shown_whole() {
        let text = "a short hit here";
        let m = build_matcher(&args("hit")).unwrap();
        let span = m.locate(text).unwrap();
        let (ex, truncated) = match_excerpt(text, span, EXCERPT_MAX);
        assert_eq!(ex, "a short hit here");
        assert!(!truncated, "a message that fits the cap is not truncated");
    }

    #[test]
    fn match_excerpt_early_match_keeps_the_head() {
        let text = format!("needle {}", "z".repeat(EXCERPT_MAX));
        let m = build_matcher(&args("needle")).unwrap();
        let span = m.locate(&text).unwrap();
        let (ex, truncated) = match_excerpt(&text, span, EXCERPT_MAX);
        assert!(!ex.starts_with('…'), "match at char 0 → no leading …: {ex}");
        assert!(ex.starts_with("needle"), "got: {ex}");
        assert!(truncated, "the tail past the window was dropped");
    }

    #[test]
    fn match_excerpt_pure_filter_falls_back_to_head() {
        let text = "X".repeat(EXCERPT_MAX + 50);
        let m = build_matcher(&args("")).unwrap(); // empty pattern = pure filter
        let span = m.locate(&text).expect("pure filter matches");
        assert_eq!(span, None, "pure filter has no locatable span");
        let (ex, truncated) = match_excerpt(&text, span, EXCERPT_MAX);
        assert!(!ex.starts_with('…'), "head form has no leading …");
        assert!(ex.ends_with("… (+50 chars)"), "got: {ex}");
        assert!(truncated, "the head form clipped 50 chars");
    }

    #[test]
    fn match_excerpt_full_budget_emits_whole_message() {
        // `--no-truncate` passes `usize::MAX` as the budget: a message longer than EXCERPT_MAX is
        // emitted whole, with NO truncation marker — whereas the default budget truncates.
        let n = EXCERPT_MAX + 200;
        let text = "🤖".repeat(n);
        let (capped, capped_truncated) = match_excerpt(&text, None, EXCERPT_MAX);
        assert!(
            capped.contains("… (+"),
            "default budget truncates: {capped}"
        );
        assert!(capped_truncated, "default budget reports truncation");
        let (full, full_truncated) = match_excerpt(&text, None, usize::MAX);
        assert!(
            !full.contains("… (+"),
            "full budget has no truncation marker"
        );
        assert!(
            !full_truncated,
            "--no-truncate's usize::MAX budget never truncates — the signal the caution note keys on"
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
        // A bare `N` → a single TOTAL cap across all labels, no per-selector entries.
        let caps = parse_sibling_specs(&specs(&["3"])).unwrap().unwrap();
        assert_eq!(caps.bare, Some(3));
        assert!(caps.per_sel.is_empty());
        assert!(caps.bare_is_total());
        // Every label resolves to the bare-N cap.
        assert_eq!(caps.cap_for(Class::AgentMessage), Some(3));
        assert_eq!(caps.cap_for(Class::AgentToolUse), Some(3));
    }

    #[test]
    fn parse_sibling_specs_cat_n_caps_that_category_only() {
        // A `<selector>:N` → caps labels under THAT selector; others are not shown.
        let caps = parse_sibling_specs(&specs(&["agent.message:1"]))
            .unwrap()
            .unwrap();
        assert_eq!(caps.per_sel, vec![("agent.message".to_string(), 1)]);
        assert_eq!(caps.bare, None);
        assert_eq!(caps.cap_for(Class::AgentMessage), Some(1));
        assert_eq!(caps.cap_for(Class::AgentToolUse), None); // not shown
        assert!(!caps.bare_is_total());
        // A ROLE/intermediate selector pools its whole subtree: `agent.tool:2` governs use+result.
        let pooled = parse_sibling_specs(&specs(&["agent.tool:2"]))
            .unwrap()
            .unwrap();
        assert_eq!(pooled.cap_for(Class::AgentToolUse), Some(2));
        assert_eq!(pooled.cap_for(Class::AgentToolResult), Some(2));
    }

    #[test]
    fn parse_sibling_specs_mixed_typed_and_bare() {
        // `agent.tool.use:1`, `agent.thinking:2`, bare `3` → typed caps govern their labels; the
        // bare caps the rest (the labels with no typed cap).
        let caps = parse_sibling_specs(&specs(&["agent.tool.use:1", "agent.thinking:2", "3"]))
            .unwrap()
            .unwrap();
        assert_eq!(caps.cap_for(Class::AgentToolUse), Some(1));
        assert_eq!(caps.cap_for(Class::AgentThinking), Some(2));
        assert_eq!(caps.cap_for(Class::AgentMessage), Some(3)); // "the rest" via bare-N
        assert_eq!(caps.cap_for(Class::UserMessage), Some(3));
        assert!(!caps.bare_is_total());
    }

    #[test]
    fn parse_sibling_specs_invalid_tokens_error() {
        // Unknown selector, non-numeric bare, bad typed cap, a zero cap, AND the old flat values
        // (`tool`, `thinking`) all error (0 back-compat).
        assert!(parse_sibling_specs(&specs(&["foo"])).is_err());
        assert!(parse_sibling_specs(&specs(&["bad:2"])).is_err());
        assert!(parse_sibling_specs(&specs(&["tool:x"])).is_err()); // old flat selector
        assert!(parse_sibling_specs(&specs(&["agent.tool.use:x"])).is_err());
        assert!(parse_sibling_specs(&specs(&["agent.tool.use:0"])).is_err());
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
            &test_env(),
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
            None,
            &test_env()
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
            None,
            &test_env()
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
        a.categories = vec!["agent.tool.result".to_string()];
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
            &["agent.tool.result".to_string()],
            &m,
            true,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(
            hits.len(),
            1,
            "inline text still matches when there is no pointer"
        );
        assert_eq!(hits[0].class, Class::AgentToolResult);
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
            &["agent.message".to_string()],
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
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
        a.categories = vec!["agent.tool.result".to_string()];
        let ex = search(&lines, &a);
        assert_eq!(ex.len(), 1);
        assert!(ex[0].hits.iter().all(|h| h.class == Class::AgentToolResult));
        assert_eq!(ex[0].hits.len(), 1, "exactly one tool-response hit");
    }
}
