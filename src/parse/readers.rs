//! RevLines + head/tail record readers (prefiltered, floor-aware).

use super::*;

/// Backward, newest-first line iterator over a byte slice, realized as a
/// chunk-with-carry scan from EOF (§7b). Each `next()` yields one line's bytes
/// **without** the trailing newline, from the end of the slice toward the start.
///
/// The "carry" is the bytes of the line that straddles the LOW-offset edge of the
/// current chunk: it is provisional until a lower chunk is consumed (or the start
/// of the slice is reached, at which point it is the first line). Empty lines are
/// yielded too — the caller filters blanks (matching [`line_payload`]).
pub struct RevLines<'a> {
    pub(crate) bytes: &'a [u8],
    /// Exclusive high boundary of the region not yet emitted as whole lines:
    /// everything in `bytes[..hi]` is still pending. Starts at `bytes.len()`.
    pub(crate) hi: usize,
    /// Bytes carried from the previous (higher) chunk — the provisional partial
    /// line at the low edge. Appended *after* freshly read lower bytes.
    pub(crate) carry: Vec<u8>,
    /// Lines split out of the current working buffer but not yet handed out, in
    /// newest-first order (so `pop()`/iteration is cheap).
    pub(crate) pending: std::collections::VecDeque<Vec<u8>>,
    pub(crate) chunk: usize,
    pub(crate) done: bool,
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
    pub(crate) fn fill(&mut self) -> bool {
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
