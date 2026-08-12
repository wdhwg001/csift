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
use memchr::{memchr, memchr_iter, memrchr};
use memmap2::Mmap;
use rayon::prelude::*;

use crate::model::Record;

/// Default backward-scan chunk size (§7b). 64 KiB balances syscall/cache pressure;
/// [`RevLines`] grows it transparently when a chunk yields no newline (guards the
/// observed 400 KB single-line max).
const TAIL_CHUNK: usize = 64 * 1024;

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
fn line_role_value_matches(line: &[u8], accept_user: bool, accept_assistant: bool) -> bool {
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

/// Scan EVERY line of a jsonl byte slice IN PARALLEL, calling `visit(line_bytes, line_no)` with
/// each line's exact 1-based number (blank/noise lines are visited and counted too, 1:1 with
/// the file — identical numbering to a serial [`scan_lines_bytes`] pass). Each `Keep(T)` is
/// collected in file order; each `Skip` increments the returned malformed count. The slice is
/// split into newline-aligned chunks run on the `rayon` pool, so a single GIANT transcript
/// (SPEC §7's "200MB+" case) is no longer bottlenecked on one core the way the across-files
/// fan-out leaves it. Below `MIN_PARALLEL_BYTES` it runs as a single chunk (split/merge overhead
/// isn't worth it). The `(items, skipped)` result is byte-for-byte equivalent to the serial scan.
pub fn scan_lines_parallel<T, F>(bytes: &[u8], visit: F) -> (Vec<T>, usize)
where
    F: Fn(&[u8], usize) -> LineVerdict<T> + Sync,
    T: Send,
{
    const MIN_PARALLEL_BYTES: usize = 4 * 1024 * 1024;
    let target_chunks = if bytes.len() < MIN_PARALLEL_BYTES {
        1
    } else {
        // A few chunks per worker keeps load balanced when chunks parse at different rates.
        (rayon::current_num_threads() * 4).max(1)
    };
    scan_lines_parallel_chunked(bytes, &visit, target_chunks)
}

/// Convenience wrapper over [`scan_lines_parallel`] for the `(line_no, Record)` shape used by
/// recover/files: parse every PREFILTER-passing line, keeping `(line_no, Record)` in file order.
pub fn parse_candidates_parallel<F>(bytes: &[u8], prefilter: F) -> (Vec<(usize, Record)>, usize)
where
    F: Fn(&[u8]) -> bool + Sync,
{
    scan_lines_parallel(bytes, |line, line_no| {
        if !prefilter(line) {
            // R10: a NON-candidate line still gets the O(1) shape check, so obviously-
            // corrupt lines (free-text garbage, crash-truncation) are COUNTED, never
            // invisible — the byte prefilter must not exempt them from the malformed law.
            return non_candidate_verdict(line);
        }
        match parse_line(line) {
            Ok(Some(rec)) => LineVerdict::Keep((line_no, rec)),
            Ok(None) => LineVerdict::Ignore,
            Err(_) => LineVerdict::Skip,
        }
    })
}

/// The verdict for a line a byte-prefilter rejected: `Skip` (⇒ counted malformed) when the
/// line is OBVIOUSLY not a JSON object, else `Ignore` (a legit non-candidate record).
/// Convenience wrapper over [`line_shape_malformed`] for the scan closures.
#[must_use]
pub fn non_candidate_verdict<T>(line: &[u8]) -> LineVerdict<T> {
    if line_shape_malformed(line) {
        LineVerdict::Skip
    } else {
        LineVerdict::Ignore
    }
}

/// True when a line is OBVIOUSLY not a JSON object record: non-blank but not brace-framed
/// (`{…}`). This is the O(1) malformed-shape check every non-candidate prefilter path runs
/// so the "a skipped malformed line is COUNTED, never hidden" law (AGENTS §4) survives the
/// §7 byte prefilters — free-text garbage has no leading `{`, and a crash-truncated record
/// loses its trailing `}` (the two realistic corruption shapes; R10 found them silently
/// invisible to every `skipped_lines` counter). The documented residual boundary: a
/// brace-framed line whose INTERIOR is invalid JSON is only detected when it is a parse
/// CANDIDATE — validating every non-candidate line would repeal the §7 perf contract.
#[must_use]
pub fn line_shape_malformed(line: &[u8]) -> bool {
    let t = line.trim_ascii();
    !t.is_empty() && (t[0] != b'{' || t[t.len() - 1] != b'}')
}

/// Core of [`scan_lines_parallel`] with an explicit chunk target, so a test can force multi-chunk
/// splitting on a tiny input and assert identical output to a single serial pass.
fn scan_lines_parallel_chunked<T, F>(
    bytes: &[u8],
    visit: &F,
    target_chunks: usize,
) -> (Vec<T>, usize)
where
    F: Fn(&[u8], usize) -> LineVerdict<T> + Sync,
    T: Send,
{
    if bytes.is_empty() {
        return (Vec::new(), 0);
    }
    // ── Newline-aligned chunk boundaries. Each interior bound is the first byte AFTER a
    //    newline (a line start), so no line is ever split across a chunk. ──
    let target_chunks = target_chunks.max(1);
    let approx = (bytes.len() / target_chunks).max(1);
    let mut bounds = vec![0usize];
    let mut pos = approx;
    while pos < bytes.len() {
        match memchr(b'\n', &bytes[pos..]) {
            Some(off) => {
                let b = pos + off + 1;
                if b >= bytes.len() {
                    break;
                }
                bounds.push(b);
                pos = b + approx;
            }
            None => break,
        }
    }
    bounds.push(bytes.len());
    let chunks: Vec<(usize, usize)> = bounds.windows(2).map(|w| (w[0], w[1])).collect();

    // start_line[i] = 1 + (newlines strictly before chunk i's first byte) = the serial scan's
    // line number for that chunk's first visited line. Only the PREFIX sums are consumed
    // (chunk 0 always starts at line 1), so a single-chunk scan skips the counting pass
    // entirely, and a multi-chunk scan counts in PARALLEL and never counts the last chunk —
    // the serial whole-slice count here used to be a full extra pass over every byte before
    // any parallel work began (idle workers on the single-giant-file case).
    let mut start_line = Vec::with_capacity(chunks.len());
    start_line.push(1usize);
    if chunks.len() > 1 {
        let nl_per_chunk: Vec<usize> = chunks[..chunks.len() - 1]
            .par_iter()
            .map(|&(s, e)| memchr_iter(b'\n', &bytes[s..e]).count())
            .collect();
        let mut acc = 1usize;
        for &n in &nl_per_chunk {
            acc += n;
            start_line.push(acc);
        }
    }

    // ── Parallel scan; rayon's ordered collect keeps chunk order, and each chunk's items are
    //    already in ascending line order, so the flattened result is in exact file order. ──
    let per_chunk: Vec<(Vec<T>, usize)> = chunks
        .par_iter()
        .enumerate()
        .map(|(ci, &(s, e))| {
            let base = start_line[ci];
            let mut kept: Vec<T> = Vec::new();
            let mut skipped = 0usize;
            let mut j = 0usize;
            let _ = scan_lines_bytes(&bytes[s..e], |line| {
                let line_no = base + j;
                j += 1;
                match visit(line, line_no) {
                    LineVerdict::Keep(t) => kept.push(t),
                    LineVerdict::Skip => skipped += 1,
                    LineVerdict::Ignore => {}
                }
            });
            (kept, skipped)
        })
        .collect();

    let mut all: Vec<T> = Vec::new();
    let mut skipped_total = 0usize;
    for (kept, sk) in per_chunk {
        all.extend(kept);
        skipped_total += sk;
    }
    (all, skipped_total)
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
    /// Construct with an explicit chunk size (the default tail read uses
    /// [`TAIL_CHUNK`]; tests force a tiny chunk to exercise the carry path across
    /// many boundaries). `chunk` is clamped to at least 1.
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
/// first genuine-user is found). Malformed lines are skipped and counted. Never
/// parses past the caller's early-stop point.
///
/// Returns `(skipped, consumed_end)`: `consumed_end` is the byte offset just past
/// the last line this scan examined (a line boundary; `bytes.len()` when the scan
/// reached EOF). A caller pairing this with a tail scan over the SAME file MUST
/// pass `consumed_end` as the tail's `floor` so the two windows are disjoint and a
/// malformed line is counted exactly once (R12: the head+tail double-book).
pub fn head_records<F>(path: &Path, keep: F) -> Result<(usize, usize)>
where
    F: FnMut(&Record) -> bool,
{
    head_records_prefiltered(path, |_| true, keep)
}

/// [`head_records`] with a RAW-byte prefilter run BEFORE the parse: a line failing
/// `pre` is neither parsed nor counted (the same candidates-only malformed-count
/// discipline `search`/`turns`/`files` already use). Lets `list` skip the routinely
/// huge non-message lines (attachment / file-history-snapshot / queue-operation)
/// without paying `serde_json` for them.
pub fn head_records_prefiltered<P, F>(path: &Path, pre: P, mut keep: F) -> Result<(usize, usize)>
where
    P: Fn(&[u8]) -> bool,
    F: FnMut(&Record) -> bool,
{
    let Some(mmap) = mmap_file(path)? else {
        return Ok((0, 0));
    };
    let bytes: &[u8] = &mmap;
    let mut skipped = 0usize;
    let mut start = 0usize;
    let mut stop = false;
    let mut handle = |line: &[u8], skipped: &mut usize, stop: &mut bool| {
        if !pre(line) {
            // R10: obviously-corrupt non-candidates are still COUNTED (the malformed law).
            if line_shape_malformed(line) {
                *skipped += 1;
            }
            return;
        }
        match parse_line(line) {
            Ok(Some(rec)) => {
                if !keep(&rec) {
                    *stop = true;
                }
            }
            Ok(None) => {}
            Err(_) => *skipped += 1,
        }
    };
    for nl in memchr_iter(b'\n', bytes) {
        handle(&bytes[start..nl], &mut skipped, &mut stop);
        if stop {
            return Ok((skipped, nl + 1));
        }
        start = nl + 1;
    }
    if start < bytes.len() {
        handle(&bytes[start..], &mut skipped, &mut stop);
    }
    Ok((skipped, bytes.len()))
}

/// Iterate parsed records from the TAIL of a file (newest-first) via the backward
/// chunk-with-carry scan, calling `keep` for each parsed record. `keep` returns
/// `false` to stop (e.g. once BOTH the last genuine-user and last agent message
/// are found). Malformed lines are skipped+counted; the skip count is returned.
/// Never parses the whole file when the anchors are near the end.
///
/// `floor` = the paired head scan's `consumed_end` (0 when there is no head scan):
/// lines BELOW it are still walked for anchors when the tail region runs dry, but
/// are never re-counted — the head scan already booked them (R12).
pub fn tail_records<F>(path: &Path, floor: usize, keep: F) -> Result<usize>
where
    F: FnMut(&Record) -> bool,
{
    tail_records_chunked(path, TAIL_CHUNK, floor, keep)
}

/// [`tail_records`] with a RAW-byte prefilter (the backward mirror of
/// [`head_records_prefiltered`]): a line failing `pre` is neither parsed nor
/// counted, so a tail dominated by huge metadata lines costs only the newline scan.
pub fn tail_records_prefiltered<P, F>(
    path: &Path,
    pre: P,
    floor: usize,
    mut keep: F,
) -> Result<usize>
where
    P: Fn(&[u8]) -> bool,
    F: FnMut(&Record) -> bool,
{
    let Some(mmap) = mmap_file(path)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    let floor = floor.min(bytes.len());
    let mut skipped = 0usize;
    let mut stopped = false;
    // Phase 1 — the region the paired head scan did NOT examine (`bytes[floor..]`,
    // a line boundary): parsed, anchored, and COUNTED.
    for raw in RevLines::with_chunk(&bytes[floor..], TAIL_CHUNK) {
        if !pre(&raw) {
            // R10: obviously-corrupt non-candidates are still COUNTED (the malformed law).
            if line_shape_malformed(&raw) {
                skipped += 1;
            }
            continue;
        }
        match parse_line(&raw) {
            Ok(Some(rec)) => {
                if !keep(&rec) {
                    stopped = true;
                    break;
                }
            }
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    }
    // Phase 2 — anchors still missing: continue into the head-examined region
    // WITHOUT counting (the head scan already booked those lines; re-counting is the
    // pre-v0.6.8 double-book). Same lines, same newest-first order as the old
    // full-file walk, so anchor semantics are unchanged.
    if !stopped && floor > 0 {
        for raw in RevLines::with_chunk(&bytes[..floor], TAIL_CHUNK) {
            if !pre(&raw) {
                continue;
            }
            if let Ok(Some(rec)) = parse_line(&raw) {
                if !keep(&rec) {
                    break;
                }
            }
        }
    }
    Ok(skipped)
}

/// [`tail_records`] with an explicit chunk size (tests force the carry path).
pub fn tail_records_chunked<F>(
    path: &Path,
    chunk: usize,
    floor: usize,
    mut keep: F,
) -> Result<usize>
where
    F: FnMut(&Record) -> bool,
{
    let Some(mmap) = mmap_file(path)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    let floor = floor.min(bytes.len());
    let mut skipped = 0usize;
    let mut stopped = false;
    for raw in RevLines::with_chunk(&bytes[floor..], chunk) {
        match parse_line(&raw) {
            Ok(Some(rec)) => {
                if !keep(&rec) {
                    stopped = true;
                    break;
                }
            }
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    }
    if !stopped && floor > 0 {
        for raw in RevLines::with_chunk(&bytes[..floor], chunk) {
            if let Ok(Some(rec)) = parse_line(&raw) {
                if !keep(&rec) {
                    break;
                }
            }
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
        let skipped = tail_records_chunked(f.path(), 8, 0, |rec| {
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
        let skipped = tail_records_chunked(f.path(), 4, 0, |rec| {
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
        let skipped = tail_records_chunked(f.path(), 4, 0, |_rec| {
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
    fn validate_line_syntax_counts_corruption_like_parse_line() {
        // Blank lines are fine for both (never counted).
        assert!(validate_line_syntax(b"").is_ok());
        assert!(validate_line_syntax(b"   \r").is_ok());
        // Valid JSON passes both.
        let ok = br#"{"type":"user","message":{"role":"user","content":"x"}}"#;
        assert!(validate_line_syntax(ok).is_ok());
        assert!(parse_line(ok).unwrap().is_some());
        // Real-world corruption (a torn tail write) fails BOTH the same way — the
        // parity `search`'s whole-file gate relies on for its malformed count.
        for torn in [
            br#"{"type":"user","message":{"role":"user","content":"tor"#.as_slice(),
            b"{ garbage not json".as_slice(),
            br#"{"role":"user""#.as_slice(),
        ] {
            assert!(validate_line_syntax(torn).is_err(), "{torn:?}");
            assert!(parse_line(torn).is_err(), "{torn:?}");
        }
    }

    #[test]
    fn empty_file_is_safe() {
        let f = tempfile_path::TempJsonl::empty();
        let s = tail_records(f.path(), 0, |_| true).expect("tail empty");
        assert_eq!(s, 0);
        let (s, consumed) = head_records(f.path(), |_| true).expect("head empty");
        assert_eq!(s, 0);
        assert_eq!(consumed, 0);
    }

    // ── Branch-completeness ──

    #[test]
    fn mmap_open_error_surfaces_context() {
        // A path that does not exist → the `File::open` error context arm of mmap_file.
        let missing = std::env::temp_dir().join(format!(
            "csift-missing-{}-{}.jsonl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let err = mmap_bytes(&missing).unwrap_err();
        assert!(
            err.to_string().contains("cannot open")
                || err.to_string().contains(&missing.display().to_string()),
            "expected open-context error, got: {err:#}"
        );
        // head/tail over a missing file surface the same open error.
        assert!(head_records(&missing, |_| true).is_err());
        assert!(tail_records(&missing, 0, |_| true).is_err());
    }

    #[test]
    fn mmap_bytes_some_for_nonempty_none_for_empty() {
        let f = tmp_jsonl(&[r#"{"type":"user","message":{"role":"user","content":"x"}}"#]);
        assert!(mmap_bytes(f.path()).unwrap().is_some());
        let e = tempfile_path::TempJsonl::empty();
        assert!(mmap_bytes(e.path()).unwrap().is_none());
    }

    #[test]
    fn scan_lines_bytes_visits_every_line_including_torn_tail() {
        // A trailing fragment with NO newline must still be visited (the
        // `start < bytes.len()` true arm).
        let mut seen: Vec<String> = Vec::new();
        scan_lines_bytes(b"aa\nbb\ncc", |line| {
            seen.push(String::from_utf8_lossy(line).into_owned());
        })
        .unwrap();
        assert_eq!(seen, vec!["aa", "bb", "cc"]);
        // A slice ending exactly on a newline does NOT emit a trailing empty line
        // (the `start < bytes.len()` false arm).
        let mut seen2: Vec<String> = Vec::new();
        scan_lines_bytes(b"aa\nbb\n", |line| {
            seen2.push(String::from_utf8_lossy(line).into_owned());
        })
        .unwrap();
        assert_eq!(seen2, vec!["aa", "bb"]);
    }

    #[test]
    fn scan_lines_parallel_chunked_matches_serial_for_any_chunk_count() {
        // A mix of candidate lines, blank lines (counted in line numbering, never kept) and a
        // malformed candidate (parsed → skipped). The (kept line_no list, skip count) must be
        // IDENTICAL across every chunk split — that is the contract that lets recover/search/
        // files swap in the parallel scan with zero behaviour change.
        let mut raw = String::new();
        for i in 0..60 {
            if i % 7 == 0 {
                raw.push('\n'); // blank line: counts as a line, visitor ignores it
            }
            raw.push_str(&format!(
                r#"{{"type":"user","uuid":"u{i}","timestamp":"2026-06-07T05:00:0{}.000Z","message":{{"role":"user","content":"keep {i}"}}}}"#,
                i % 10
            ));
            raw.push('\n');
            if i % 11 == 5 {
                raw.push_str("{ broken json keep but unparseable\n"); // candidate → skip
            }
        }
        let bytes = raw.as_bytes();
        // Visitor keeps each parseable "keep" line's NUMBER; a malformed "keep" line → Skip.
        let visit = |line: &[u8], line_no: usize| -> LineVerdict<usize> {
            if !line.windows(4).any(|w| w == b"keep") {
                return LineVerdict::Ignore;
            }
            match parse_line(line) {
                Ok(Some(_)) => LineVerdict::Keep(line_no),
                Ok(None) => LineVerdict::Ignore,
                Err(_) => LineVerdict::Skip,
            }
        };

        let (serial, serial_skip) = scan_lines_parallel_chunked(bytes, &visit, 1);
        assert!(
            !serial.is_empty() && serial_skip > 0,
            "fixture exercises both arms"
        );
        // Line numbers strictly ascending (1:1 with the file, no duplicates/reordering).
        assert!(serial.windows(2).all(|w| w[0] < w[1]));

        for chunks in [2usize, 3, 5, 9, 17, 60, 500] {
            let (got, skip) = scan_lines_parallel_chunked(bytes, &visit, chunks);
            assert_eq!(got, serial, "line numbers diverge at chunks={chunks}");
            assert_eq!(skip, serial_skip, "skip count diverges at chunks={chunks}");
        }
    }

    #[test]
    fn scan_lines_bytes_empty_slice_visits_nothing() {
        let mut n = 0;
        scan_lines_bytes(b"", |_| n += 1).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn revlines_carry_flushed_at_bof_with_leading_partial() {
        // A file whose FIRST line straddles below the lowest chunk read so it lands in
        // the carry, then is flushed when `hi` reaches 0 (the `fill` hi==0 +
        // non-empty-carry flush arm). Force it with chunk=1 over content whose first
        // line is long and has no newline until late.
        let data = b"leadingline\nx\n";
        assert_eq!(rev_nonblank(data, 1), vec!["x", "leadingline"]);
    }

    #[test]
    fn head_records_skips_blank_and_malformed_then_continues() {
        // A blank line (Ok(None) arm), a malformed line (Err skip+count arm), then a
        // valid record reached at the torn-tail position (no trailing newline).
        let f = tmp_jsonl(&[
            "",                 // blank → Ok(None)
            r#"{ broken json"#, // malformed → counted
            r#"{"type":"user","message":{"role":"user","content":"real"}}"#,
        ]);
        let mut first: Option<String> = None;
        let (skipped, _) = head_records(f.path(), |rec| {
            if let Some(t) = rec.genuine_user_text() {
                first = Some(t);
                return false;
            }
            true
        })
        .expect("head read");
        assert_eq!(first.as_deref(), Some("real"));
        assert_eq!(skipped, 1, "the one malformed line is counted");
    }

    #[test]
    fn head_records_visits_torn_tail_fragment() {
        // The final record has NO trailing newline (the `start < bytes.len()` arm of
        // head_records). Scan everything (never early-stop) so the tail is reached.
        let mut content = String::new();
        content.push_str(r#"{"type":"user","message":{"role":"user","content":"a"}}"#);
        content.push('\n');
        content.push_str(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"tail-no-newline"}]}}"#);
        let p = std::env::temp_dir().join(format!(
            "csift-torn-{}-{}.jsonl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&p, content.as_bytes()).unwrap();
        let mut seen = 0usize;
        let mut last_agent: Option<String> = None;
        head_records(&p, |rec| {
            seen += 1;
            if let Some(t) = rec.agent_text() {
                last_agent = Some(t);
            }
            true // scan all → reach the torn tail
        })
        .expect("head read");
        std::fs::remove_file(&p).ok();
        assert_eq!(seen, 2);
        assert_eq!(last_agent.as_deref(), Some("tail-no-newline"));
    }

    #[test]
    fn head_records_early_stop_before_tail_fragment() {
        // Early-stop on the FIRST record (return false) so the `if stop { return }`
        // arm fires mid-loop and the torn-tail branch is NOT reached.
        let f = tmp_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"first"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"second"}}"#,
        ]);
        let mut count = 0;
        head_records(f.path(), |_rec| {
            count += 1;
            false // stop immediately
        })
        .expect("head read");
        assert_eq!(count, 1, "stopped after the first record");
    }

    #[test]
    fn role_marker_is_serialization_tolerant() {
        // R13: the compact wire form and every JSON-whitespace variant are the SAME
        // record — all must be candidates.
        for ok in [
            br#"{"message":{"role":"user","content":"x"}}"#.as_slice(),
            br#"{"message":{"role": "user","content":"x"}}"#.as_slice(),
            br#"{"message":{"role" : "user","content":"x"}}"#.as_slice(),
            b"{\"message\":{\"role\":\t\"assistant\",\"content\":[]}}".as_slice(),
            br#"{"message":{"role": "assistant"}}"#.as_slice(),
        ] {
            assert!(
                line_has_role_marker(ok),
                "{:?}",
                String::from_utf8_lossy(ok)
            );
        }
        // Non-markers: other role values, keyless mentions, and content-embedded
        // (escaped-quote) forms must NOT be admitted.
        for no in [
            br#"{"input":{"role":"admin"}}"#.as_slice(),
            br#"{"text":"the role of the user"}"#.as_slice(),
            br#"{"text":"quoted {\"role\": \"user\"} in prose"}"#.as_slice(),
            br#"{"role":}"#.as_slice(),
            br#"{"role"}"#.as_slice(),
        ] {
            assert!(
                !line_has_role_marker(no),
                "{:?}",
                String::from_utf8_lossy(no)
            );
        }
        // The user-only variant (files/recover) rejects assistant markers.
        assert!(line_has_user_role_marker(
            br#"{"message":{"role": "user"}}"#
        ));
        assert!(!line_has_user_role_marker(
            br#"{"message":{"role": "assistant"}}"#
        ));
        // A later valid marker after an earlier false hit is still found.
        assert!(line_has_role_marker(
            br#"{"a":{"role":"admin"},"message":{"role": "user"}}"#
        ));
    }

    #[test]
    fn head_tail_disjoint_windows_never_double_count() {
        // R12: a head scan + tail scan over the SAME file used to double-book every
        // malformed line both passes walked (an all-garbage file reported exactly 2×).
        // The head returns its consumed-end offset; passing it as the tail floor keeps
        // the windows disjoint, so each line is booked exactly once.
        let f = tmp_jsonl(&["{ garbage one", "{ garbage two", "{ garbage three"]);
        let (head_skipped, consumed) = head_records(f.path(), |_| true).expect("head");
        assert_eq!(head_skipped, 3);
        let tail_skipped = tail_records(f.path(), consumed, |_| true).expect("tail");
        assert_eq!(
            tail_skipped, 0,
            "the tail must not re-book head-counted lines"
        );
    }

    #[test]
    fn tail_floor_still_walks_below_for_anchors_without_counting() {
        // The ONLY genuine user sits inside the head window. The tail scan (floor past
        // it) must still find it as an anchor (phase 2 of the backward walk) while
        // counting nothing below the floor — anchor semantics are byte-identical to the
        // old full-file walk; only the double-booked count changed.
        let f = tmp_jsonl(&[
            r#"{ broken head line"#,
            r#"{"type":"user","message":{"role":"user","content":"only q"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
        ]);
        let mut first: Option<String> = None;
        let (head_skipped, consumed) = head_records(f.path(), |rec| {
            if let Some(t) = rec.genuine_user_text() {
                first = Some(t);
                return false;
            }
            true
        })
        .expect("head");
        assert_eq!(head_skipped, 1);
        assert_eq!(first.as_deref(), Some("only q"));
        let mut last_user: Option<String> = None;
        let mut last_agent: Option<String> = None;
        let tail_skipped = tail_records(f.path(), consumed, |rec| {
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
            last_user.is_none() || last_agent.is_none()
        })
        .expect("tail");
        assert_eq!(last_agent.as_deref(), Some("a"));
        assert_eq!(
            last_user.as_deref(),
            Some("only q"),
            "anchor below the floor is still found"
        );
        assert_eq!(
            tail_skipped, 0,
            "the broken head line is booked by the head scan only"
        );
    }

    #[test]
    fn revlines_all_blank_slice() {
        // A slice of only newlines → every line is blank, none survive the filter.
        assert!(rev_nonblank(b"\n\n\n", 2).is_empty());
    }

    #[test]
    fn revlines_next_after_exhaustion_returns_none() {
        // Calling `next()` again after the iterator is exhausted hits the `if
        // self.done { return None }` true arm (a normal `for` loop never re-polls).
        // Content has no trailing newline → exactly two raw lines yielded.
        let mut it = RevLines::with_chunk(b"a\nb", 64);
        let mut all = Vec::new();
        for l in it.by_ref() {
            all.push(l);
        }
        assert_eq!(all.len(), 2, "two lines, newest-first: {all:?}");
        assert_eq!(all[0], b"b");
        assert_eq!(all[1], b"a");
        assert!(
            it.next().is_none(),
            "post-exhaustion next is None via done flag"
        );
        assert!(it.next().is_none(), "still None");
    }

    #[test]
    fn revlines_empty_slice_is_immediately_none() {
        // An empty slice: hi==0 at construction, carry empty → fill returns false on
        // the first poll (the `self.carry.is_empty()` TRUE arm at hi==0).
        let mut it = RevLines::with_chunk(b"", 4);
        assert!(it.next().is_none());
    }

    #[test]
    fn revlines_trailing_newline_only_carry_empty_at_bof() {
        // Content that is a single newline: the high tail after the newline is empty,
        // seg0 (low edge) is also empty at BOF → buf-empty / empty-carry arms. No
        // non-blank lines survive.
        assert!(rev_nonblank(b"\n", 1).is_empty());
        assert!(rev_nonblank(b"\n", 64).is_empty());
    }

    #[test]
    fn revlines_carry_nonempty_flush_at_bof() {
        // A leading partial line with NO newline before it, reached only after the
        // carry has accumulated across chunks → the `self.carry.is_empty()` FALSE arm
        // at hi==0 (flush the carry as the first line). chunk=2 forces accumulation.
        let data = b"abcdefghij\nz\n";
        assert_eq!(rev_nonblank(data, 2), vec!["z", "abcdefghij"]);
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

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
