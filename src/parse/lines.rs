//! mmap + per-line primitives: role matchers, lazy parse, syntax validation.

use super::*;

/// Default backward-scan chunk size (§7b). 64 KiB balances syscall/cache pressure;
/// [`RevLines`] grows it transparently when a chunk yields no newline (guards the
/// observed 400 KB single-line max).
pub(crate) const TAIL_CHUNK: usize = 64 * 1024;

/// Open + memory-map a file read-only and hand back the [`Mmap`] for the caller to
/// borrow a `&[u8]` from (used by `search`, which needs to retain records borrowed
/// against the live map for the whole scan). Returns `Ok(None)` for an empty file.
/// Same SAFETY contract as [`mmap_file`].
pub fn mmap_bytes(path: &Path) -> Result<Option<Mmap>> {
    mmap_file(path)
}

/// Open + memory-map a file read-only. Length is captured at open; a concurrently
/// growing/shrinking live session is tolerated (we treat the length as fixed and
/// skip any torn trailing fragment). Returns `Ok(None)` for an empty file.
pub(crate) fn mmap_file(path: &Path) -> Result<Option<Mmap>> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let len = file
        .metadata()
        .with_context(|| format!("cannot stat {}", path.display()))?
        .len();
    if len == 0 {
        return Ok(None);
    }
    // SAFETY: read-only mmap of a file we just opened. The documented hazard is a
    // concurrent truncation by another writer; we never write through the map and
    // treat its length as fixed-at-open, which is the SPEC's accepted contract
    // (§7a). The crate lints `unsafe_code = "deny"` (unsafe forbidden crate-wide);
    // mmap is irreducibly unsafe and SPEC-mandated, so this is the single audited
    // call site that explicitly allows it.
    #[allow(unsafe_code)]
    let mmap =
        unsafe { Mmap::map(&file) }.with_context(|| format!("cannot mmap {}", path.display()))?;
    Ok(Some(mmap))
}

/// Serialization-tolerant role-marker test — THE stage-1 candidate needle for
/// message records (R13). CC's own wire format is compact JSON, but a hand-authored
/// or reserialized line may carry whitespace around the colon (`"role": "user"`) —
/// valid JSON, the same record. The old exact-byte needles (`"role":"user"`)
/// silently DROPPED such lines one layer BEFORE any malformed counter could see
/// them: not skipped, not counted, simply invisible on every surface. One `memmem`
/// pass for the quoted key + an O(1) verify per hit; a keyless line costs ONE scan
/// where the old disjunct cost two, so §7 holds (benchmarked on the real corpus).
/// The quoted-key needle cannot match content text: inside a JSON string the quotes
/// are escaped (`\"role\"`), which breaks the needle bytes.
pub(crate) fn line_role_value_matches(
    line: &[u8],
    accept_user: bool,
    accept_assistant: bool,
) -> bool {
    const KEY: &[u8] = br#""role""#;
    // Built ONCE (memchr's construct-once pattern): this predicate runs per LINE across
    // every spanning command's stage-1 prefilter, and the stateless `memmem::find`
    // rebuilt its searcher on every call (a real-corpus profile showed `Searcher::new`
    // riding the hot path).
    static KEY_FINDER: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(br#""role""#));
    let mut at = 0usize;
    while let Some(rel) = KEY_FINDER.find(&line[at..]) {
        let mut j = at + rel + KEY.len();
        while line.get(j).is_some_and(|b| b.is_ascii_whitespace()) {
            j += 1;
        }
        if line.get(j) == Some(&b':') {
            j += 1;
            while line.get(j).is_some_and(|b| b.is_ascii_whitespace()) {
                j += 1;
            }
            let rest = &line[j.min(line.len())..];
            if (accept_user && rest.starts_with(br#""user""#))
                || (accept_assistant && rest.starts_with(br#""assistant""#))
            {
                return true;
            }
        }
        at += rel + KEY.len();
    }
    false
}

/// `"role"` is `"user"` OR `"assistant"` (any JSON whitespace around the colon) —
/// the candidate test for `search`/`show`/`verbatim`/`list`/`stats` stage-1 filters.
pub fn line_has_role_marker(line: &[u8]) -> bool {
    line_role_value_matches(line, true, true)
}

/// `"role"` is `"user"` only — the genuine-user/carrier hook `files`/`recover` use
/// (their assistant-side coverage rides tool-name needles, so admitting every
/// assistant text record here would repeal their §7 prefilter).
pub fn line_has_user_role_marker(line: &[u8]) -> bool {
    line_role_value_matches(line, true, false)
}

/// Strip a trailing `\r`/`\n` and report whether the remaining bytes are non-blank.
pub(crate) fn line_payload(line: &[u8]) -> Option<&[u8]> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        Some(line)
    }
}

/// Parse one raw jsonl line into a [`Record`]. `Ok(None)` for an empty/blank line.
/// A malformed (non-empty) line returns `Err` — the caller decides to skip+count.
pub fn parse_line(line: &[u8]) -> Result<Option<Record>> {
    let Some(payload) = line_payload(line) else {
        return Ok(None);
    };
    let rec: Record = serde_json::from_slice(payload)?;
    Ok(Some(rec))
}

/// Validate one raw jsonl line's JSON syntax WITHOUT building a [`Record`] (no
/// allocation, no field processing — `IgnoredAny` drives the full lexer and nothing
/// else). `Ok(())` for a blank line. Used by `search`'s whole-file gate to keep the
/// malformed-line count EXACT for a file it proved cannot match: real-world
/// corruption (a torn tail write, garbage bytes) fails here exactly as it fails
/// [`parse_line`]. The one divergence is deliberate and documented: a line that is
/// VALID JSON but violates the `Record` schema (e.g. a non-string/blocks
/// `message.content`) passes this check while `parse_line` would count it — a shape
/// never observed in real transcripts (the model is tolerant-by-construction:
/// every field is optional, unknown fields/blocks are ignored).
pub fn validate_line_syntax(line: &[u8]) -> Result<()> {
    let Some(payload) = line_payload(line) else {
        return Ok(());
    };
    serde_json::from_slice::<serde::de::IgnoredAny>(payload)?;
    Ok(())
}

/// Scan every line of an already-mapped byte slice front-to-back, calling `visit`
/// with each line's raw bytes (excluding the trailing `\n`). Unlike [`scan_lines`]
/// this takes the slice directly (the caller owns the [`Mmap`] so it can retain
/// records borrowed against it) and the visitor is infallible — `search` collects
/// its own errors/skips internally. A torn final fragment with no trailing newline
/// is still visited.
pub fn scan_lines_bytes<F>(bytes: &[u8], mut visit: F) -> Result<()>
where
    F: FnMut(&[u8]),
{
    let mut start = 0usize;
    for nl in memchr_iter(b'\n', bytes) {
        visit(&bytes[start..nl]);
        start = nl + 1;
    }
    if start < bytes.len() {
        visit(&bytes[start..]);
    }
    Ok(())
}

/// Per-line verdict from a [`scan_lines_parallel`] visitor.
pub enum LineVerdict<T> {
    /// A produced item — collected in exact file order.
    Keep(T),
    /// A candidate line that failed to parse — counted in the returned malformed total.
    Skip,
    /// Not a candidate (or blank) — neither kept nor counted.
    Ignore,
}
