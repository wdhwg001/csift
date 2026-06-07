//! Streaming + tail jsonl readers.
//!
//! Performance contract (NON-FUNCTIONAL, SPEC.md): must stay fast on 200MB+ files.
//! - `mmap` the file, scan line boundaries with `memchr` (SIMD newline search).
//! - **Lazy parse**: a cheap byte/regex prefilter rejects most lines; full
//!   `serde_json` runs only on candidate lines.
//! - **Head read**: parse only the first K records (for `list`'s first user msg).
//! - **Tail read**: SEEK from EOF and scan backward for the last user/agent msg —
//!   never parse the whole file.
//! - Across files, `rayon` parallelizes.
//!
//! Errors propagate (`anyhow`); no `unwrap`/`expect` on the hot path, no silent
//! truncation. A malformed line is skipped (counted), not fatal.

use std::path::Path;

use anyhow::Result;

use crate::model::Record;

/// Iterate parsed records from the head of a file, stopping after `limit` lines.
///
/// Returns successfully-parsed records; malformed lines are skipped. Intended for
/// `list`'s "first genuine-user message" without reading the whole file.
pub fn head_records(_path: &Path, _limit: usize) -> Result<Vec<Record>> {
    todo!("mmap + memchr line scan over first `limit` lines, lazy serde; Phase 2")
}

/// Read the file's tail by seeking backward from EOF, yielding up to `limit`
/// parsed records nearest the end (newest-first). Never parses the whole file.
pub fn tail_records(_path: &Path, _limit: usize) -> Result<Vec<Record>> {
    todo!("reverse line scan from EOF via mmap; Phase 2")
}

/// A line locator: byte offset + length within the mmap, plus 0-based line index.
#[derive(Debug, Clone, Copy)]
pub struct LineSpan {
    pub index: usize,
    pub start: usize,
    pub end: usize,
}

/// Full streaming scan: visit every line, calling `visit` with the line index and
/// raw bytes. The visitor decides whether to fully parse (lazy-parse boundary).
pub fn scan_lines<F>(_path: &Path, _visit: F) -> Result<()>
where
    F: FnMut(LineSpan, &[u8]) -> Result<()>,
{
    todo!("mmap + memchr::memchr_iter(b'\\n', ..) line splitting; Phase 2")
}

/// Parse one raw jsonl line into a [`Record`]. `Ok(None)` for an empty/blank line.
pub fn parse_line(line: &[u8]) -> Result<Option<Record>> {
    let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
    if trimmed.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let rec: Record = serde_json::from_slice(trimmed)?;
    Ok(Some(rec))
}
