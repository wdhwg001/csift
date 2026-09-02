//! Matcher: regex + raw-byte prefilter + synthesized-text markers (SPEC 7d/7f).

use super::*;

/// A compiled pattern + flags, plus the optional literal prefilter needle.
#[derive(Debug)]
pub struct Matcher {
    /// `None` ⇒ empty pattern (pure filter: every label-eligible record matches).
    pub(crate) regex: Option<BytesRegex>,
    /// A required-literal prefilter derived from the pattern, run against RAW line/file
    /// bytes (§7d stage 2 + the §7f whole-file gate). `None` ⇒ no anchorable literal.
    pub(crate) prefilter: Option<Prefilter>,
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
    /// One `memmem::Finder` per needle - deliberately NOT one Aho-Corasick automaton:
    /// the `"answers"` needle starts with a quote, and in JSON lines a quote is one of
    /// the DENSEST bytes, so AC's start-byte prefilter degenerated into a verification
    /// attempt at nearly every string boundary (`try_find_fwd` became the #1 profile
    /// entry). `memmem` picks a rare byte INSIDE each needle as its SIMD skip anchor,
    /// so per-needle scans stay at memory speed regardless of the leading byte.
    ///
    /// Two tiers, split by whether a marker-bearing line's synthesized text can be
    /// re-rendered SELF-CONTAINED for the §7f stage-2 verification:
    /// - `synth_verifiable` - notification / AUQ-answer / compact-boundary markers: the
    ///   line's every synthesized text derives from the line alone, so the gate can
    ///   parse JUST the marker lines, render via the SHARED engines
    ///   (`record_text_sections` / `auq_exchange` / `record_raw_text`) and regex-check
    ///   them - a big marker-heavy main session no longer forces a whole-file parse.
    /// - `synth_conservative` - a rejection's `[plan: …]` pointer resolves through
    ///   ANOTHER record (`PlanIndex`) and `--resolve-persisted` content lives in an
    ///   external file, so those lines force the full scan (rare).
    pub(crate) synth_verifiable: Vec<memmem::Finder<'static>>,
    pub(crate) synth_conservative: Vec<memmem::Finder<'static>>,
}

/// Raw-byte prefilter for a pattern that IS a plain literal (no regex metachars, no
/// JSON-escaped chars - see [`required_literal`]). Both variants are CONSERVATIVE:
/// they can only prove a haystack CANNOT match (false positives fine, false negatives
/// impossible), so gating on them never drops a genuine hit.
#[derive(Debug)]
pub(crate) enum Prefilter {
    /// Case-sensitive literal: SIMD `memmem` substring search.
    Literal(memmem::Finder<'static>),
    /// Smart-case / `-i` insensitive literal(s): a `(?i)`-wrapped escaped literal
    /// alternation as a bytes regex - `memmem` has no caseless mode, but the regex
    /// engine compiles a caseless literal alternation to an accelerated (Teddy-class)
    /// multi-substring scan, so the dominant lowercase-smart-case search gets the SAME
    /// prefilter power the case-sensitive path always had.
    CaselessLiteral(BytesRegex),
    /// Case-sensitive ANY-of needle set (a REQUIRED-literal extraction from a regex
    /// with metacharacters: every branch of the pattern demands one of these). One
    /// `memmem::Finder` per needle, same rationale as the synth markers (not AC).
    AnyLiteral(Vec<memmem::Finder<'static>>),
}

impl Prefilter {
    /// True when `haystack` (a raw jsonl line OR a whole mmapped file) could contain a
    /// match. The JSON-escape safety argument is [`required_literal`]'s: the literal
    /// contains no char that serde/JS JSON-encodes, so it survives verbatim (module
    /// case) in the raw bytes whenever the DECODED text matches.
    pub(crate) fn may_match(&self, haystack: &[u8]) -> bool {
        match self {
            Prefilter::Literal(finder) => finder.find(haystack).is_some(),
            Prefilter::CaselessLiteral(re) => re.is_match(haystack),
            Prefilter::AnyLiteral(finders) => finders.iter().any(|f| f.find(haystack).is_some()),
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

    /// True when the pattern is empty (pure filter - matches any text).
    pub(crate) fn is_pure_filter(&self) -> bool {
        self.regex.is_none()
    }

    /// True if `text` matches the pattern (always true for the pure filter).
    /// Test-only since production hits go through [`Matcher::locate`] (which also
    /// yields the span for excerpt-centering); kept as a clean bool API for tests.
    #[cfg(test)]
    pub(crate) fn is_match(&self, text: &str) -> bool {
        match &self.regex {
            None => true,
            Some(re) => re.is_match(text.as_bytes()),
        }
    }

    /// Locate the FIRST match, so the excerpt can be CENTERED on it instead of
    /// always showing the message head. Returns:
    /// - `None` - no match (the record is not a hit);
    /// - `Some(None)` - matches with no specific span (the pure filter matches every
    ///   record, so there is no offset to center on → excerpt shows the head);
    /// - `Some(Some((start, end)))` - matches at this BYTE range.
    pub(crate) fn locate(&self, text: &str) -> Option<Option<(usize, usize)>> {
        match &self.regex {
            None => Some(None),
            Some(re) => re.find(text.as_bytes()).map(|m| Some((m.start(), m.end()))),
        }
    }

    /// Cheap raw-line prefilter (§7d stage 2): if a required literal exists and the
    /// line lacks it, the line cannot match - drop it pre-JSON. With no literal (or
    /// pure filter) we cannot prove absence, so the line passes to the parse stage.
    pub(crate) fn line_may_match(&self, line: &[u8]) -> bool {
        match &self.prefilter {
            Some(pf) => pf.may_match(line) || self.synth_may_match(line),
            None => true,
        }
    }

    /// The literal-prefilter check ALONE (no synth-marker OR) - the §7f pre-scan needs
    /// the two signals separately (a literal hit forces the full scan; a marker hit
    /// routes to its tier). `false` is only possible when a prefilter is anchored.
    pub(crate) fn line_prefilter_hits(&self, line: &[u8]) -> bool {
        match &self.prefilter {
            Some(pf) => pf.may_match(line),
            None => true,
        }
    }

    /// True when a prefilter is anchored - the precondition for the §7f whole-file gate
    /// (without one nothing is provably a miss, so the gate pre-scan would be waste).
    pub(crate) fn has_prefilter(&self) -> bool {
        self.prefilter.is_some()
    }

    /// Whole-slice version of [`Matcher::line_may_match`] - test-only: production gates
    /// per LINE inside the parallel pre-scan (a serial whole-mmap pass would bottleneck
    /// the single-giant-file case); tests use this to pin the miss/hit semantics.
    #[cfg(test)]
    pub(crate) fn file_may_match(&self, bytes: &[u8]) -> bool {
        match &self.prefilter {
            Some(pf) => pf.may_match(bytes) || self.synth_may_match(bytes),
            None => true,
        }
    }

    /// True when the haystack carries ANY synthesized-text marker (either tier).
    pub(crate) fn synth_may_match(&self, haystack: &[u8]) -> bool {
        self.synth_verifiable
            .iter()
            .chain(self.synth_conservative.iter())
            .any(|f| f.find(haystack).is_some())
    }

    /// True when the haystack carries a CONSERVATIVE marker (must full-scan).
    pub(crate) fn synth_conservative_hits(&self, haystack: &[u8]) -> bool {
        self.synth_conservative
            .iter()
            .any(|f| f.find(haystack).is_some())
    }

    /// True when the haystack carries a VERIFIABLE marker (stage-2 re-render + check).
    pub(crate) fn synth_verifiable_hits(&self, haystack: &[u8]) -> bool {
        self.synth_verifiable
            .iter()
            .any(|f| f.find(haystack).is_some())
    }

    /// §7f stage-2: could this parsed marker-line record's SYNTHESIZED texts match?
    /// Renders through the same shared engines the hit collector uses (no drift):
    /// notification section labels + normalized `<result>` bodies
    /// (`record_text_sections` - direction/owner do not affect the TEXT, so the neutral
    /// ctx is exact), the answered-AUQ reconstruction (`auq_exchange`), and the
    /// compact-boundary content + metadata excerpt (`record_raw_text`). Any VERBATIM
    /// text these return is already covered by the literal scan, so a miss here plus a
    /// literal miss proves the record cannot hit.
    pub(crate) fn synth_texts_match(&self, rec: &Record) -> bool {
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
        // The agents-stopped notice renders through `automation_label` (a fabricated
        // `[subagent stopped]` head) without a `<task-notification>` section.
        if let Some(t) = rec.automation_label() {
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
    let prefilter = match required_needles(&args.pattern) {
        None => None,
        Some(needles) if case_insensitive => {
            // Each needle escaped; the alternation of escaped literals compiles to a
            // Teddy-class caseless multi-substring scan.
            let alts: Vec<String> = needles.iter().map(|n| regex::escape(n)).collect();
            let src = format!("(?i){}", alts.join("|"));
            let re = BytesRegex::new(&src)
                .with_context(|| format!("invalid caseless prefilter for {:?}", args.pattern))?;
            Some(Prefilter::CaselessLiteral(re))
        }
        Some(needles) if needles.len() == 1 => Some(Prefilter::Literal(
            memmem::Finder::new(needles[0].as_bytes()).into_owned(),
        )),
        Some(needles) => Some(Prefilter::AnyLiteral(
            needles
                .iter()
                .map(|n| memmem::Finder::new(n.as_bytes()).into_owned())
                .collect(),
        )),
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
pub(crate) fn apply_builder(
    pattern: &str,
    case_insensitive: bool,
    multiline: bool,
) -> Result<String> {
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
/// (A richer HIR analysis is possible but this captures the common keyword case -
/// `csift search "carry"` - with zero false negatives.)
///
/// **JSON-escape safety (load-bearing - SPEC §0 "no silent truncation").** The
/// prefilter runs the literal against the RAW JSON line bytes, where string content
/// is JSON-encoded: `"` is stored as `\"`, `\` as `\\`, and every control char
/// (`< 0x20`) plus DEL (`0x7f`) as a `\uXXXX`/`\n`/`\t`/… escape. A literal
/// containing any such char therefore can NOT appear verbatim in the raw line - a
/// `memmem` for it would falsely report "absent" and silently drop a line whose
/// DECODED text actually matches (e.g. searching `Say"Xello`). So we refuse to emit
/// a literal prefilter whenever the pattern contains a JSON-escaped character; the
/// match then falls back to running the regex on the raw bytes (still pre-JSON, just
/// without the cheap literal short-circuit). Non-ASCII (`>= 0x80`) is emitted
/// verbatim as UTF-8 by serde_json (multi-byte searches confirm this), so it stays
/// prefilter-eligible.
pub(crate) fn required_literal(pattern: &str) -> Option<Vec<u8>> {
    const META: &[char] = &[
        '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '^', '$', '\\',
    ];
    if pattern.is_empty() || pattern.chars().any(|c| META.contains(&c)) {
        return None;
    }
    // A char that JSON escapes inside a string does not survive verbatim in the raw
    // line bytes the prefilter scans - emitting a literal for it causes false
    // negatives. `\` is already excluded via META above; guard `"`, control chars,
    // and DEL here.
    if pattern.chars().any(json_escapes_in_string) {
        return None;
    }
    // WHITESPACE-safety (the render-normalization mirror of the JSON-escape rule):
    // several render paths rewrite whitespace before matching - `normalize_line`
    // collapses runs to a single space (genuine-user text via `flatten_content_text`,
    // peer bodies, notification reports) and multi-part texts are joined with `' '` /
    // `'\n'` seams. A literal CONTAINING whitespace can therefore match rendered text
    // (`hello world`) whose raw bytes hold `hello\nworld` - a `memmem` for it would
    // falsely prove absence. A whitespace-FREE literal always sits inside one
    // unrewritten non-whitespace run, which survives verbatim in the raw bytes, so
    // only those stay prefilter-eligible. (This also closes a latent gap the old
    // case-sensitive prefilter had for space-carrying patterns.)
    if pattern.chars().any(char::is_whitespace) {
        return None;
    }
    Some(pattern.as_bytes().to_vec())
}

/// A required-needle set for the prefilter: every string the pattern can match
/// contains AT LEAST ONE of these literals verbatim. The whole-pattern literal case
/// (no metachars) stays the fast path; a pattern WITH metacharacters goes through a
/// NECESSITY-ONLY walk of its parsed HIR ([`hir_needles`]). Each needle must pass the
/// SAME safety predicate the single-literal path enforces (whitespace-free,
/// JSON-escape-free - see [`required_literal`]'s safety argument, which transfers
/// per needle), so the historical "never extract a literal from a regex" rule's
/// RATIONALE is preserved while its blanket form is retired: the rule guarded against
/// unsafe or non-required extractions, and this walk emits neither.
pub(crate) fn required_needles(pattern: &str) -> Option<Vec<String>> {
    if let Some(lit) = required_literal(pattern) {
        return String::from_utf8(lit).ok().map(|s| vec![s]);
    }
    if pattern.is_empty() {
        return None;
    }
    // Parse WITHOUT case folding: extraction yields typed-case literals, and the
    // caller applies the engine's case mode to the prefilter (a caseless scan of a
    // typed needle is a superset gate under a caseless engine).
    let hir = regex_syntax::ParserBuilder::new()
        .utf8(false)
        .build()
        .parse(pattern)
        .ok()?;
    let needles = hir_needles(&hir)?;
    // Bound the gate: a huge ANY-of set scans slower than it saves.
    (needles.len() <= MAX_NEEDLES).then_some(needles)
}

/// Needle-set size cap and per-needle minimum length. A 1-2 byte needle hits nearly
/// every line (the gate would always pass - pure overhead), so short runs are
/// treated as no contribution.
const MAX_NEEDLES: usize = 8;
const MIN_NEEDLE_LEN: usize = 3;

/// NECESSITY-only literal extraction over a parsed pattern. Returns `Some(set)` iff
/// every match of this sub-pattern must contain one of `set` verbatim:
/// - a literal contributes its longest SAFE run (whitespace and JSON-escaped chars
///   split the run - a sub-run of a required literal is itself required);
/// - a concatenation must contain EVERY part, so the strongest single part's set is
///   chosen (longest worst-case needle, then fewest needles);
/// - an alternation must satisfy SOME branch, so every branch must contribute and
///   the result is the union - one branch without a safe needle kills the gate;
/// - a repetition with `min == 0` contributes nothing; `min >= 1` contributes its
///   inner's set once;
/// - classes, dots, and look-arounds contribute nothing (never a failure - a sibling
///   concat part can still anchor the gate).
fn hir_needles(hir: &regex_syntax::hir::Hir) -> Option<Vec<String>> {
    use regex_syntax::hir::HirKind;
    match hir.kind() {
        HirKind::Literal(lit) => {
            let s = std::str::from_utf8(&lit.0).ok()?;
            let run = s
                .split(|c: char| c.is_whitespace() || json_escapes_in_string(c))
                .max_by_key(|r| r.len())?;
            (run.len() >= MIN_NEEDLE_LEN).then(|| vec![run.to_string()])
        }
        HirKind::Concat(parts) => parts
            .iter()
            .filter_map(hir_needles)
            .max_by(|a, b| set_strength(a).cmp(&set_strength(b))),
        HirKind::Alternation(branches) => {
            let mut union: Vec<String> = Vec::new();
            for b in branches {
                for n in hir_needles(b)? {
                    if !union.contains(&n) {
                        union.push(n);
                    }
                }
            }
            (!union.is_empty()).then_some(union)
        }
        HirKind::Repetition(rep) if rep.min >= 1 => hir_needles(&rep.sub),
        HirKind::Capture(g) => hir_needles(&g.sub),
        _ => None,
    }
}

/// Ordering key for choosing a concat's strongest contribution: prefer the set whose
/// WORST needle is longest (that needle bounds the gate's selectivity), then fewer
/// needles.
fn set_strength(set: &[String]) -> (usize, std::cmp::Reverse<usize>) {
    (
        set.iter().map(String::len).min().unwrap_or(0),
        std::cmp::Reverse(set.len()),
    )
}

/// Build the SYNTHESIZED-text marker finders for [`Matcher::synth`] (one SIMD
/// `memmem` scan per needle, per line - see the field doc for why not Aho-Corasick).
///
/// Rationale: the literal prefilter proves absence only for matchable text that is a
/// VERBATIM substring of the record's raw line bytes. A small, closed set of render
/// paths synthesizes text from other sources; each is detectable by a raw marker its
/// carrier record ALWAYS contains:
/// - `<task-notification>` - `automation_label` fabricates the kind slug
///   (`subagent`/`background-command`/…), a `completed` status fallback, and `[…]`
///   scaffolding; the G1 inbox view normalizes the `<result>` body.
/// - `"answers"` + the two synthesized answer markers - the ANSWER carrier's
///   `auq_exchange` render fabricates the `[AskUserQuestion · N question(s)]` scaffold,
///   `Q1/A1` labels and option lists that appear verbatim nowhere in the raw line.
///   (The QUESTION-side `tool_use` needs no needle: its matchable text is
///   `render_tool_use` = the verbatim `name` + the re-serialized `input`, and the name
///   bytes sit in the raw line - a bare `AskUserQuestion` needle would disable the gate
///   for the ~29% of files whose injected context merely MENTIONS the tool.)
/// - `To tell you how to proceed` - the rejection reconstruction appends a
///   `[plan: <path>]` pointer whose path lives on a DIFFERENT record.
/// - `compact_boundary` (only when the `-t` selection can reach
///   `harness.compaction.boundary` - otherwise the boundary line is not even a scan
///   candidate, so its synthesized excerpt is unreachable) - `trigger=…`/`preTokens=…`
///   key=value text is fabricated from `compactMetadata`.
/// - `turn_duration` / `stop_hook_summary` / `file-history-` (v0.9.5, only under an
///   explicit selector reaching the leaf) - the promoted renders fabricate
///   `[turn duration: …]` / `[stop hooks: …]` / `<path>@vN` text from the fields.
/// - Under `--resolve-persisted`: `persistedOutputPath` / `Full output saved to:` -
///   the matched text is EXTERNAL file content, absent from the transcript bytes by
///   definition.
///
/// False positives only cost speed (the line/file falls back to the full parse +
/// regex pipeline); false negatives are what the set is built to make impossible.
pub(crate) fn synth_marker_finders(
    args: &SearchArgs,
) -> (Vec<memmem::Finder<'static>>, Vec<memmem::Finder<'static>>) {
    // VERIFIABLE (stage-2 re-renderable from the line alone; see `Matcher::synth_*`).
    let mut verifiable: Vec<&[u8]> = vec![
        b"<task-notification>",
        br#""answers""#,
        b"User has answered your questions",
        b"Your questions have been answered",
        // The agents-stopped kill notice renders a fabricated `[subagent stopped]` head.
        b"stopped by the user",
    ];
    if args
        .label_filter()
        .selected(Class::CompactionBoundary.path())
    {
        verifiable.push(b"compact_boundary");
    }
    // v0.9.5 promoted lines whose render FABRICATES text (key=value excerpts): the
    // marker is the line's own type/subtype value, active only when the explicit
    // selector admits the line (otherwise it is not a candidate at all). The queued
    // and away-summary leaves render VERBATIM content and need no marker.
    if args.reaches_gated(Class::MetaTurnDuration) {
        verifiable.push(b"turn_duration");
    }
    if args.reaches_gated(Class::MetaStopHooks) {
        verifiable.push(b"stop_hook_summary");
    }
    if args.reaches_gated(Class::MetaSnapshot) {
        verifiable.push(b"file-history-");
    }
    // CONSERVATIVE (needs cross-record / external data - force the full scan).
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
pub(crate) fn json_escapes_in_string(c: char) -> bool {
    c == '"' || (c as u32) < 0x20 || c == '\u{7f}'
}

/// Entry point for `csift search`.
/// Record-address selectors (`--line` / `--uuid`) parsed into membership sets - the "fetch
/// THESE records" filter that turns `search` into the in-permission message-getter. Active when
/// either set is non-empty; a record is addressed when its physical line OR uuid is in range.
pub(crate) struct AddressSet {
    pub(crate) lines: BTreeSet<usize>,
    pub(crate) uuids: BTreeSet<String>,
}
