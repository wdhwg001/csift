//! Streaming + tail jsonl readers.
//!
//! Performance contract (NON-FUNCTIONAL, SPEC.md §7): must stay fast on 200MB+ files.
//! - `mmap` the file (immutable `memmap2::Mmap`, length fixed at open), scan line
//!   boundaries with `memchr` (SIMD newline search) — never a `BufReader` copy of
//!   every line.
//! - **Lazy parse**: callers prefilter on raw bytes; full `serde_json::from_slice`
//!   runs only on candidate lines.
//! - **Head read**: scan forward from offset 0, parsing candidate lines, stopping
//!   when the caller is satisfied (e.g. `list`'s first genuine-user message).
//! - **Tail read**: seek from EOF and walk lines BACKWARD (newest-first) so the
//!   last user/agent message is found after scanning only a small tail slice —
//!   never the whole file.
//! - Across files, `rayon` parallelizes (see `session`/`search`).
//!
//! Errors propagate (`anyhow`); no `unwrap`/`expect` on the hot path; no silent
//! truncation. A malformed line is skipped (counted by the caller), not fatal.
//!
//! ## Backward iteration: chunk-with-carry vs mmap
//!
//! SPEC §7b frames the tail read as a seek-from-EOF backward chunk scan with a
//! "carry" — the incomplete line straddling the LOW-offset edge of each chunk,
//! provisional until the next-lower chunk is read. On a memory-mapped file the
//! whole byte range is addressable, so we realize the same backward, newest-first
//! line order over the mmap slice via [`RevLines`], a chunked backward line
//! iterator. Keeping it chunk-based (rather than one `memrchr` sweep) makes the
//! carry boundary logic explicit and unit-testable with a tiny chunk size, and
//! bounds how much of a 225 MB file is touched to find the tail anchors.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use memchr::{memchr_iter, memrchr};
use memmap2::Mmap;

use crate::model::Record;

/// Default backward-scan chunk size (§7b). 64 KiB balances syscall/cache pressure;
/// [`RevLines`] grows it transparently when a chunk yields no newline (guards the
/// observed 400 KB single-line max).
const TAIL_CHUNK: usize = 64 * 1024;

/// Open + memory-map a file read-only. Length is captured at open; a concurrently
/// growing/shrinking live session is tolerated (we treat the length as fixed and
/// skip any torn trailing fragment). Returns `Ok(None)` for an empty file.
fn mmap_file(path: &Path) -> Result<Option<Mmap>> {
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
    // (§7a). The crate lints `unsafe_code = "warn"`; mmap is irreducibly unsafe and
    // SPEC-mandated, so allow it at exactly this call site.
    #[allow(unsafe_code)]
    let mmap =
        unsafe { Mmap::map(&file) }.with_context(|| format!("cannot mmap {}", path.display()))?;
    Ok(Some(mmap))
}

/// Strip a trailing `\r`/`\n` and report whether the remaining bytes are non-blank.
fn line_payload(line: &[u8]) -> Option<&[u8]> {
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

/// A line locator: byte offset + length within the mmap, plus 0-based line index.
#[derive(Debug, Clone, Copy)]
pub struct LineSpan {
    pub index: usize,
    pub start: usize,
    pub end: usize,
}

/// Full streaming scan: visit every line front-to-back, calling `visit` with the
/// line's span and raw bytes (excluding the trailing `\n`). The visitor decides
/// whether to fully parse (the lazy-parse boundary). A torn final fragment with no
/// trailing newline is still visited (it is a complete record in practice).
pub fn scan_lines<F>(path: &Path, mut visit: F) -> Result<()>
where
    F: FnMut(LineSpan, &[u8]) -> Result<()>,
{
    let Some(mmap) = mmap_file(path)? else {
        return Ok(());
    };
    let bytes: &[u8] = &mmap;
    let mut start = 0usize;
    let mut index = 0usize;
    for nl in memchr_iter(b'\n', bytes) {
        let line = &bytes[start..nl];
        visit(
            LineSpan {
                index,
                start,
                end: nl,
            },
            line,
        )?;
        index += 1;
        start = nl + 1;
    }
    // Trailing fragment with no final newline.
    if start < bytes.len() {
        let line = &bytes[start..];
        visit(
            LineSpan {
                index,
                start,
                end: bytes.len(),
            },
            line,
        )?;
    }
    Ok(())
}

/// Backward, newest-first line iterator over a byte slice, realized as a
/// chunk-with-carry scan from EOF (§7b). Each `next()` yields one line's bytes
/// **without** the trailing newline, from the end of the slice toward the start.
///
/// The "carry" is the bytes of the line that straddles the LOW-offset edge of the
/// current chunk: it is provisional until a lower chunk is consumed (or the start
/// of the slice is reached, at which point it is the first line). Empty lines are
/// yielded too — the caller filters blanks (matching [`line_payload`]).
pub struct RevLines<'a> {
    bytes: &'a [u8],
    /// Exclusive high boundary of the region not yet emitted as whole lines:
    /// everything in `bytes[..hi]` is still pending. Starts at `bytes.len()`.
    hi: usize,
    /// Bytes carried from the previous (higher) chunk — the provisional partial
    /// line at the low edge. Appended *after* freshly read lower bytes.
    carry: Vec<u8>,
    /// Lines split out of the current working buffer but not yet handed out, in
    /// newest-first order (so `pop()`/iteration is cheap).
    pending: std::collections::VecDeque<Vec<u8>>,
    chunk: usize,
    done: bool,
}

impl<'a> RevLines<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self::with_chunk(bytes, TAIL_CHUNK)
    }

    /// Construct with an explicit chunk size (used by tests to force the carry
    /// path across many boundaries). `chunk` is clamped to at least 1.
    #[must_use]
    pub fn with_chunk(bytes: &'a [u8], chunk: usize) -> Self {
        Self {
            bytes,
            hi: bytes.len(),
            carry: Vec::new(),
            pending: std::collections::VecDeque::new(),
            chunk: chunk.max(1),
            done: false,
        }
    }

    /// Pull and split one more lower chunk into `pending` (newest-first). Returns
    /// `false` when the start of the slice has been reached and nothing remains.
    fn fill(&mut self) -> bool {
        loop {
            if self.hi == 0 {
                // No more bytes to read. Flush the carry as the very first line.
                if self.carry.is_empty() {
                    return false;
                }
                let first = std::mem::take(&mut self.carry);
                self.pending.push_back(first);
                return true;
            }

            // Read a lower chunk, growing it until it contains a newline or we hit
            // the start (guards the 400 KB single-line max — a chunk with no
            // newline can't split a line, so widen the window).
            let mut take = self.chunk;
            let mut lo = self.hi.saturating_sub(take);
            while lo > 0 && memrchr(b'\n', &self.bytes[lo..self.hi]).is_none() {
                take = take.saturating_mul(2);
                lo = self.hi.saturating_sub(take);
            }

            // Working buffer = freshly read lower bytes ++ carry from above.
            let mut buf = Vec::with_capacity((self.hi - lo) + self.carry.len());
            buf.extend_from_slice(&self.bytes[lo..self.hi]);
            buf.extend_from_slice(&self.carry);
            self.carry.clear();
            self.hi = lo;
            let at_bof = lo == 0;

            // Split `buf` on '\n' into segments. With newlines at positions
            // n0<n1<…<n(k-1), the segments are:
            //   seg0 = buf[0..n0]                (the LOW-edge segment)
            //   segI = buf[n(I-1)+1 .. nI]       for 1..=k-1
            //   segK = buf[n(k-1)+1 .. ]         (the HIGH-edge tail; complete,
            //                                     since its newline was consumed
            //                                     by the chunk above as a carry)
            // seg0 is provisional → it is the new carry, UNLESS we are at BOF, in
            // which case it is the file's line 0 (complete). seg1..=segK are all
            // complete lines. They must be yielded newest-first (highest first).
            let nls: Vec<usize> = memchr_iter(b'\n', &buf).collect();

            if nls.is_empty() {
                // No newline anywhere: the whole buffer is one still-provisional
                // line — carry it down. At BOF it is the single, complete line 0.
                if at_bof {
                    if buf.is_empty() {
                        return false;
                    }
                    self.pending.push_back(buf);
                    return true;
                }
                self.carry = buf;
                continue; // keep reading lower chunks
            }

            // Build complete-line ranges seg1..=segK, then emit newest-first.
            // (start, end) byte ranges into `buf`.
            let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(nls.len());
            for w in 1..nls.len() {
                ranges.push((nls[w - 1] + 1, nls[w]));
            }
            // High-edge tail after the last newline (segK). Always a complete line.
            let tail_start = nls[nls.len() - 1] + 1;
            if tail_start <= buf.len() {
                ranges.push((tail_start, buf.len()));
            }

            // Emit newest-first: highest range first → push_back in reverse order
            // so pop_front hands them out newest→oldest.
            for &(s, e) in ranges.iter().rev() {
                self.pending.push_back(buf[s..e].to_vec());
            }

            // seg0 (low-edge): carry, or — at BOF — the oldest complete line.
            let seg0 = buf[..nls[0]].to_vec();
            if at_bof {
                self.pending.push_back(seg0); // oldest → yielded last
            } else {
                self.carry = seg0;
            }

            return true;
        }
    }
}

impl Iterator for RevLines<'_> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(line) = self.pending.pop_front() {
                return Some(line);
            }
            if self.done {
                return None;
            }
            if !self.fill() {
                self.done = true;
                return None;
            }
        }
    }
}

/// Iterate parsed records from the HEAD of a file, calling `keep` for each
/// successfully-parsed record. `keep` returns `false` to stop early (e.g. once the
/// first genuine-user is found). Malformed lines are skipped and counted; the skip
/// count is returned. Never parses past the caller's early-stop point.
pub fn head_records<F>(path: &Path, mut keep: F) -> Result<usize>
where
    F: FnMut(&Record) -> bool,
{
    let Some(mmap) = mmap_file(path)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    let mut skipped = 0usize;
    let mut start = 0usize;
    let mut stop = false;
    let mut handle = |line: &[u8], skipped: &mut usize, stop: &mut bool| match parse_line(line) {
        Ok(Some(rec)) => {
            if !keep(&rec) {
                *stop = true;
            }
        }
        Ok(None) => {}
        Err(_) => *skipped += 1,
    };
    for nl in memchr_iter(b'\n', bytes) {
        handle(&bytes[start..nl], &mut skipped, &mut stop);
        if stop {
            return Ok(skipped);
        }
        start = nl + 1;
    }
    if start < bytes.len() {
        handle(&bytes[start..], &mut skipped, &mut stop);
    }
    Ok(skipped)
}

/// Iterate parsed records from the TAIL of a file (newest-first) via the backward
/// chunk-with-carry scan, calling `keep` for each parsed record. `keep` returns
/// `false` to stop (e.g. once BOTH the last genuine-user and last agent message
/// are found). Malformed lines are skipped+counted; the skip count is returned.
/// Never parses the whole file when the anchors are near the end.
pub fn tail_records<F>(path: &Path, keep: F) -> Result<usize>
where
    F: FnMut(&Record) -> bool,
{
    tail_records_chunked(path, TAIL_CHUNK, keep)
}

/// [`tail_records`] with an explicit chunk size (tests force the carry path).
pub fn tail_records_chunked<F>(path: &Path, chunk: usize, mut keep: F) -> Result<usize>
where
    F: FnMut(&Record) -> bool,
{
    let Some(mmap) = mmap_file(path)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    let mut skipped = 0usize;
    for raw in RevLines::with_chunk(bytes, chunk) {
        match parse_line(&raw) {
            Ok(Some(rec)) => {
                if !keep(&rec) {
                    break;
                }
            }
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    }
    Ok(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Collect ALL backward lines (as Strings) for a byte slice at a given chunk
    /// size, dropping blanks (matching the parse-path's blank filter).
    fn rev_nonblank(bytes: &[u8], chunk: usize) -> Vec<String> {
        RevLines::with_chunk(bytes, chunk)
            .filter_map(|l| line_payload(&l).map(|p| String::from_utf8_lossy(p).into_owned()))
            .collect()
    }

    #[test]
    fn revlines_simple_three_lines() {
        let data = b"alpha\nbeta\ngamma\n";
        // Newest-first.
        assert_eq!(
            rev_nonblank(data, 64 * 1024),
            vec!["gamma", "beta", "alpha"]
        );
    }

    #[test]
    fn revlines_no_trailing_newline() {
        let data = b"alpha\nbeta\ngamma"; // last line has no \n
        assert_eq!(
            rev_nonblank(data, 64 * 1024),
            vec!["gamma", "beta", "alpha"]
        );
    }

    #[test]
    fn revlines_carry_across_tiny_chunks() {
        // Chunk size 3 forces nearly every line to straddle a chunk boundary, so
        // the carry logic is exercised hard. Must STILL yield newest-first, intact.
        let data = b"one\ntwotwo\nthreethreethree\nfour\n";
        assert_eq!(
            rev_nonblank(data, 3),
            vec!["four", "threethreethree", "twotwo", "one"]
        );
    }

    #[test]
    fn revlines_chunk_size_one() {
        // Pathological chunk=1: every byte is its own read; carry must reassemble.
        let data = b"ab\ncd\nef\n";
        assert_eq!(rev_nonblank(data, 1), vec!["ef", "cd", "ab"]);
    }

    #[test]
    fn revlines_single_line_no_newline() {
        let data = b"only-one-line";
        assert_eq!(rev_nonblank(data, 4), vec!["only-one-line"]);
    }

    #[test]
    fn revlines_blank_lines_dropped_order_kept() {
        let data = b"first\n\n\nlast\n";
        assert_eq!(rev_nonblank(data, 2), vec!["last", "first"]);
    }

    #[test]
    fn revlines_chunk_boundary_at_newline() {
        // Chunk boundary lands exactly on the newline positions.
        let data = b"aaa\nbbb\nccc\n"; // each line+nl is 4 bytes
        assert_eq!(rev_nonblank(data, 4), vec!["ccc", "bbb", "aaa"]);
    }

    #[test]
    fn revlines_matches_forward_split_reversed_for_random_chunks() {
        // Property: for ANY chunk size, backward non-blank == forward non-blank
        // reversed. Build content with varied line lengths incl. a long line.
        let content = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            "a",
            "bb",
            "c".repeat(200), // long line spanning multiple small chunks
            "",              // a blank
            "dddd",
            "eeeee" // no trailing newline
        );
        let bytes = content.as_bytes();
        let forward: Vec<String> = content
            .split('\n')
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        let mut expected = forward.clone();
        expected.reverse();
        for chunk in [1usize, 2, 3, 7, 16, 64, 1000, 1 << 16] {
            assert_eq!(rev_nonblank(bytes, chunk), expected, "chunk={chunk}");
        }
    }

    fn tmp_jsonl(lines: &[&str]) -> tempfile_path::TempJsonl {
        tempfile_path::TempJsonl::new(lines)
    }

    #[test]
    fn tail_records_finds_last_user_and_agent() {
        let f = tmp_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"first q"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first a"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"r"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"last q"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"last a"}]}}"#,
        ]);
        let mut last_user: Option<String> = None;
        let mut last_agent: Option<String> = None;
        // Use a tiny chunk to force the carry path even on this small file.
        let skipped = tail_records_chunked(f.path(), 8, |rec| {
            if last_agent.is_none() {
                if let Some(t) = rec.agent_text() {
                    last_agent = Some(t);
                }
            }
            if last_user.is_none() {
                if let Some(t) = rec.genuine_user_text() {
                    last_user = Some(t);
                }
            }
            // Keep going until BOTH are filled.
            last_user.is_none() || last_agent.is_none()
        })
        .expect("tail read");
        assert_eq!(skipped, 0);
        assert_eq!(last_user.as_deref(), Some("last q"));
        assert_eq!(last_agent.as_deref(), Some("last a"));
    }

    #[test]
    fn tail_records_skips_malformed_lines_and_counts() {
        let f = tmp_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"ok"}}"#,
            r#"{ this is not valid json"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
        ]);
        let mut agent: Option<String> = None;
        let skipped = tail_records_chunked(f.path(), 4, |rec| {
            if let Some(t) = rec.agent_text() {
                agent = Some(t);
            }
            agent.is_none()
        })
        .expect("tail read");
        // The malformed middle line is between agent (end) and the first record,
        // so whether it is counted depends on how far we scan; here we stop at the
        // agent line (newest), so the malformed line is not reached → skipped==0.
        assert_eq!(agent.as_deref(), Some("a"));
        assert_eq!(skipped, 0);
    }

    #[test]
    fn tail_records_malformed_at_end_is_counted() {
        let f = tmp_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
            r#"{ broken tail line"#,
        ]);
        let mut seen = 0usize;
        let skipped = tail_records_chunked(f.path(), 4, |_rec| {
            seen += 1;
            true // scan everything
        })
        .expect("tail read");
        assert_eq!(skipped, 1, "the broken newest line must be skipped+counted");
        assert_eq!(seen, 1, "one valid record");
    }

    #[test]
    fn head_records_stops_at_first_genuine_user() {
        let f = tmp_jsonl(&[
            r#"{"type":"last-prompt","leafUuid":"x"}"#,
            r#"{"type":"attachment","timestamp":"2026-06-07T00:00:00.000Z"}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"user","message":{"role":"user","content":"the real first question"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"never reached"}]}}"#,
        ]);
        let mut first: Option<String> = None;
        let mut count = 0usize;
        head_records(f.path(), |rec| {
            count += 1;
            if let Some(t) = rec.genuine_user_text() {
                first = Some(t);
                return false; // stop
            }
            true
        })
        .expect("head read");
        assert_eq!(first.as_deref(), Some("the real first question"));
        // Must have stopped at the genuine user (line 4 of valid records: the
        // metadata + attachment + isMeta were visited but not matched).
        assert_eq!(count, 4, "stopped at the first genuine user, not later");
    }

    #[test]
    fn empty_file_is_safe() {
        let f = tempfile_path::TempJsonl::empty();
        let s = tail_records(f.path(), |_| true).expect("tail empty");
        assert_eq!(s, 0);
        let s = head_records(f.path(), |_| true).expect("head empty");
        assert_eq!(s, 0);
    }

    /// Minimal temp-file helper (no external dev-dep): writes lines to a uniquely
    /// named file under the OS temp dir and removes it on drop.
    mod tempfile_path {
        use super::Write;
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        #[derive(Debug)]
        pub struct TempJsonl {
            path: PathBuf,
        }

        impl TempJsonl {
            pub fn new(lines: &[&str]) -> Self {
                let t = Self::make_path();
                let mut f = std::fs::File::create(&t).expect("create temp");
                for l in lines {
                    writeln!(f, "{l}").expect("write line");
                }
                f.flush().expect("flush");
                TempJsonl { path: t }
            }

            pub fn empty() -> Self {
                let t = Self::make_path();
                std::fs::File::create(&t).expect("create temp");
                TempJsonl { path: t }
            }

            fn make_path() -> PathBuf {
                let n = COUNTER.fetch_add(1, Ordering::Relaxed);
                let pid = std::process::id();
                std::env::temp_dir().join(format!("csift-test-{pid}-{n}.jsonl"))
            }

            pub fn path(&self) -> &Path {
                &self.path
            }
        }

        impl Drop for TempJsonl {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
