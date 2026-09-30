//! mmap + per-line primitives: role matchers, lazy parse, syntax validation.

use super::*;

/// Default backward-scan chunk size (section 7b). 64 KiB balances syscall/cache pressure;
/// [`RevLines`] grows it transparently when a chunk yields no newline (guards the
/// observed 400 KB single-line max).
pub(crate) const TAIL_CHUNK: usize = 64 * 1024;

/// Plain positional read of `path[start..end)` into a `Vec` - the LIVE-FILE
/// reader. A memory map of a file another process truncates faults with SIGBUS the
/// moment a page past the new end is touched, and there is no `Result` to catch it;
/// Claude Code rewrites a transcript in place on a rewind tombstone (truncate + tail
/// rewrite) and, when its local GC is armed, on compaction. So the repeated readers
/// (`wait`'s per-poll increment, the `status` tail window) read through `pread`-style
/// I/O: a shrink returns fewer bytes, never a fault. The one-shot full scans keep the
/// mmap contract of SPEC section 7a. `end` is clamped to the current length; a `start`
/// past the end yields an empty buffer.
pub(crate) fn read_range(path: &Path, start: u64, end: u64) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let len = file
        .metadata()
        .with_context(|| format!("cannot stat {}", path.display()))?
        .len();
    let end = end.min(len);
    if start >= end {
        return Ok(Vec::new());
    }
    file.seek(SeekFrom::Start(start))
        .with_context(|| format!("cannot seek {}", path.display()))?;
    let mut buf = Vec::with_capacity(usize::try_from(end - start).unwrap_or(0));
    file.take(end - start)
        .read_to_end(&mut buf)
        .with_context(|| format!("cannot read {}", path.display()))?;
    Ok(buf)
}

/// The last `window` bytes of `path` (or the whole file when shorter), plus the file
/// offset the returned buffer starts at. Same plain-read contract as [`read_range`].
pub(crate) fn read_tail(path: &Path, window: u64) -> Result<(Vec<u8>, u64)> {
    let len = std::fs::metadata(path)
        .with_context(|| format!("cannot stat {}", path.display()))?
        .len();
    let start = len.saturating_sub(window);
    Ok((read_range(path, start, len)?, start))
}

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
    // SAFETY: read-only mmap of a file we just opened; we never write through the map
    // and treat its length as fixed-at-open. The one hazard the map cannot contain is a
    // concurrent TRUNCATION by another writer: touching a page past the new end faults
    // with SIGBUS, and no Result catches it. Claude Code does rewrite a transcript in
    // place (a rewind tombstone; an armed local GC on compaction), so the readers that
    // run repeatedly against a live file - `wait`'s poll and the `status` tail window -
    // use `read_range`/`read_tail` instead; the one-shot full scans keep the
    // map, whose sub-second life bounds the exposure (SPEC section 7a). The crate lints
    // `unsafe_code = "deny"`; mmap is irreducibly unsafe and SPEC-mandated, so this is
    // the single audited call site that explicitly allows it.
    #[allow(unsafe_code)]
    let mmap =
        unsafe { Mmap::map(&file) }.with_context(|| format!("cannot mmap {}", path.display()))?;
    Ok(Some(mmap))
}

/// Serialization-tolerant role-marker test - THE stage-1 candidate needle for
/// message records (the needle law). CC's own wire format is compact JSON, but a hand-authored
/// or reserialized line may carry whitespace around the colon (`"role": "user"`) -
/// valid JSON, the same record. An exact-byte needle (`"role":"user"`) silently
/// DROPS such a line one layer BEFORE any malformed counter could see it: not
/// skipped, not counted, simply invisible on every surface. One `memmem` pass for
/// the quoted key + an O(1) verify per hit; a keyless line costs ONE scan where a
/// two-spelling disjunct costs two, so section 7 holds (benchmarked on the real corpus).
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

/// `"role"` is `"user"` OR `"assistant"` (any JSON whitespace around the colon) -
/// the candidate test for `search`/`show`/`verbatim`/`list`/`stats` stage-1 filters.
pub fn line_has_role_marker(line: &[u8]) -> bool {
    line_role_value_matches(line, true, true)
}

/// `"role"` is `"user"` only - the genuine-user/carrier hook `files`/`recover` use
/// (their assistant-side coverage rides tool-name needles, so admitting every
/// assistant text record here would repeal their section 7 prefilter).
pub fn line_has_user_role_marker(line: &[u8]) -> bool {
    line_role_value_matches(line, true, false)
}

/// `"role"` is `"assistant"` only.
pub fn line_has_assistant_role_marker(line: &[u8]) -> bool {
    line_role_value_matches(line, false, true)
}

/// An assistant line that may carry an `AskUserQuestion` tool_use. A surface whose
/// prefilter keeps file traffic alone (`files`, `recover`) admits it too, because a
/// marker-only answer opens a turn only when its `tool_use_id` names an AskUserQuestion
/// call in `model::CallIndex`, which the chain's builder fills from the FULL records a
/// surface parsed - a spine row carries no payload (claim REC-104). Without the call's line
/// such an answer opens a turn on `search`/`show`/`stats`/`verbatim` and on neither of
/// those two, and every later turn numbers one lower there. The tool name is a bare value
/// substring (the needle law), `&&`-gated on the assistant role; a line that merely QUOTES
/// the word costs one parse and adds nothing to the index, which reads only a `tool_use`
/// block of that name.
pub(crate) fn line_may_carry_ask_call(line: &[u8]) -> bool {
    static ASK: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"AskUserQuestion"));
    ASK.find(line).is_some() && line_has_assistant_role_marker(line)
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
/// A malformed (non-empty) line returns `Err` - the caller decides to skip+count.
///
/// ONE retry, and only for a line whose BYTES are not valid UTF-8. csift reads and renders
/// such a line lossily everywhere else - a rendered body is the source line decoded with
/// replacement characters - so a record whose only defect is a byte that does not decode is
/// a record every surface can show, and dropping it would be a silent loss of the text on
/// the same line. The decision is made HERE rather than left to serde: the EXPOSED KEYS
/// arrive through a `#[serde(flatten)]` map, which BUFFERS every unknown key instead of
/// skipping past it, so a byte that does not decode fails the whole record wherever on the
/// line it sits. The retry is gated on the payload actually being invalid UTF-8, so a line
/// that is merely bad JSON takes one parse, one error and one malformed count.
pub fn parse_line(line: &[u8]) -> Result<Option<Record>> {
    let Some(payload) = line_payload(line) else {
        return Ok(None);
    };
    match serde_json::from_slice::<Record>(payload) {
        Ok(rec) => Ok(Some(rec)),
        Err(e) if std::str::from_utf8(payload).is_err() => {
            let lossy = String::from_utf8_lossy(payload);
            match serde_json::from_str::<Record>(&lossy) {
                Ok(rec) => Ok(Some(rec)),
                // The lossy read failing means the line is bad JSON as well; report the
                // FIRST error, which is the one about the bytes actually on disk.
                Err(_) => Err(e.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Validate one raw jsonl line's JSON syntax WITHOUT building a [`Record`] (no
/// allocation, no field processing - `IgnoredAny` drives the full lexer and nothing
/// else). `Ok(())` for a blank line. Used by `search`'s whole-file gate to keep the
/// malformed-line count EXACT for a file it proved cannot match: real-world
/// corruption (a torn tail write, garbage bytes) fails here exactly as it fails
/// [`parse_line`]. The one divergence is deliberate and documented: a line that is
/// VALID JSON but violates the `Record` schema (e.g. a non-string/blocks
/// `message.content`) passes this check while `parse_line` would count it - a shape
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
/// records borrowed against it) and the visitor is infallible - `search` collects
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
    /// A produced item - collected in exact file order.
    Keep(T),
    /// A candidate line that failed to parse - counted in the returned malformed total.
    Skip,
    /// Not a candidate (or blank) - neither kept nor counted.
    Ignore,
}
