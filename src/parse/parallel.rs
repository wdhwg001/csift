//! Parallel scans: rayon chunking, candidate parsing, malformed accounting.

use super::*;

/// Scan EVERY line of a jsonl byte slice IN PARALLEL, calling `visit(line_bytes, line_no)` with
/// each line's exact 1-based number (blank/noise lines are visited and counted too, 1:1 with
/// the file - identical numbering to a serial [`scan_lines_bytes`] pass). Each `Keep(T)` is
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
            // invisible - the byte prefilter must not exempt them from the malformed law.
            return non_candidate_verdict(line);
        }
        match parse_line(line) {
            Ok(Some(rec)) => LineVerdict::Keep((line_no, rec)),
            Ok(None) => LineVerdict::Ignore,
            Err(_) => LineVerdict::Skip,
        }
    })
}

/// The line-numbered records of one transcript, in file order.
pub type NumberedRecords = Vec<(usize, Record)>;

/// [`parse_candidates_parallel`] PLUS the conversation-chain SPINE.
///
/// A surface's byte prefilter keeps the lines that surface cares about, and a chain built
/// from those alone breaks at the first `attachment` the DAG threads through (see
/// [`spine_record`]). So the non-candidate arm lifts each dropped line's five structural
/// fields into a spine row instead of discarding it. Returns `(candidates, spine, skipped)`:
/// both vectors are in file order, keyed by their 1-based jsonl line, DISJOINT by
/// construction, and the malformed count still books every obviously-corrupt line exactly
/// once (a line that yields no spine row falls through to [`non_candidate_verdict`]).
///
/// The two kinds ride SEPARATE output streams from the scan down (`SplitVerdict`), never a
/// merged vector split afterwards. A spine row is a fraction of a record's width and three
/// lines in four of a real transcript are spine, so a shared vector sized every element to
/// the record and moved the whole thing twice - measured at ~120 MB per move on a 397 MB
/// transcript, which is the serial carry this shape removes.
pub fn parse_candidates_with_spine<F>(
    bytes: &[u8],
    prefilter: F,
) -> (NumberedRecords, Vec<SpineRow>, usize)
where
    F: Fn(&[u8]) -> bool + Sync,
{
    scan_lines_parallel_split(bytes, |line, line_no| {
        if !prefilter(line) {
            if let Some(row) = spine_record(line_no, line) {
                return SplitVerdict::Second(row);
            }
            return non_candidate_split(line);
        }
        match parse_line(line) {
            Ok(Some(rec)) => SplitVerdict::First((line_no, rec)),
            Ok(None) => SplitVerdict::Ignore,
            Err(_) => SplitVerdict::Skip,
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

/// [`non_candidate_verdict`] for a TWO-STREAM scan. Same rule, same single source
/// ([`line_shape_malformed`]) - only the verdict type differs, because a `SplitVerdict`
/// carries two item types and cannot be produced by a `From` on the one-stream form
/// without inventing an item.
#[must_use]
pub fn non_candidate_split<A, B>(line: &[u8]) -> SplitVerdict<A, B> {
    if line_shape_malformed(line) {
        SplitVerdict::Skip
    } else {
        SplitVerdict::Ignore
    }
}

/// True when a line is OBVIOUSLY not a JSON object record: non-blank but not brace-framed
/// (`{…}`). This is the O(1) malformed-shape check every non-candidate prefilter path runs
/// so the "a skipped malformed line is COUNTED, never hidden" law (AGENTS §4) survives the
/// §7 byte prefilters - free-text garbage has no leading `{`, and a crash-truncated record
/// loses its trailing `}` (the two realistic corruption shapes; R10 found them silently
/// invisible to every `skipped_lines` counter). The documented residual boundary: a
/// brace-framed line whose INTERIOR is invalid JSON is only detected when it is a parse
/// CANDIDATE - validating every non-candidate line would repeal the §7 perf contract.
#[must_use]
pub fn line_shape_malformed(line: &[u8]) -> bool {
    let t = line.trim_ascii();
    !t.is_empty() && (t[0] != b'{' || t[t.len() - 1] != b'}')
}

/// One line's verdict for a TWO-STREAM scan: the two kinds are collected into their own
/// vectors and never share one. [`LineVerdict`]'s `Skip`/`Ignore` arms carry over.
#[derive(Debug)]
pub enum SplitVerdict<A, B> {
    /// An item for the FIRST stream.
    First(A),
    /// An item for the SECOND stream.
    Second(B),
    /// A candidate line that failed to parse - counted in the returned malformed total.
    Skip,
    /// Not a candidate (or blank) - neither kept nor counted.
    Ignore,
}

/// [`scan_lines_parallel`] with TWO output streams: `visit` routes each line to one of
/// them, and each is collected in file order into its own vector. Identical chunking,
/// numbering and malformed accounting.
///
/// The point is WIDTH. A caller that keeps two row kinds of very different sizes (a full
/// record and a chain-structural spine row) would otherwise size every element of one
/// vector to the wider kind and move the whole thing on every hand-off. Keeping the streams
/// apart from the scan down is what lets the narrow row stay narrow.
pub fn scan_lines_parallel_split<A, B, F>(bytes: &[u8], visit: F) -> (Vec<A>, Vec<B>, usize)
where
    F: Fn(&[u8], usize) -> SplitVerdict<A, B> + Sync,
    A: Send,
    B: Send,
{
    const MIN_PARALLEL_BYTES: usize = 4 * 1024 * 1024;
    let target_chunks = if bytes.len() < MIN_PARALLEL_BYTES {
        1
    } else {
        (rayon::current_num_threads() * 4).max(1)
    };
    scan_lines_parallel_split_chunked(bytes, &visit, target_chunks)
}

/// Core of [`scan_lines_parallel`] with an explicit chunk target, so a test can force multi-chunk
/// splitting on a tiny input and assert identical output to a single serial pass.
pub(crate) fn scan_lines_parallel_chunked<T, F>(
    bytes: &[u8],
    visit: &F,
    target_chunks: usize,
) -> (Vec<T>, usize)
where
    F: Fn(&[u8], usize) -> LineVerdict<T> + Sync,
    T: Send,
{
    // The single-stream form IS the two-stream one with an uninhabited second stream, so
    // there is one chunking + line-numbering + malformed-accounting implementation, not two.
    // `Vec<Infallible>` never allocates and its merge is a no-op.
    let (kept, _empty, skipped): (Vec<T>, Vec<std::convert::Infallible>, usize) =
        scan_lines_parallel_split_chunked(
            bytes,
            &|line: &[u8], line_no: usize| match visit(line, line_no) {
                LineVerdict::Keep(t) => SplitVerdict::First(t),
                LineVerdict::Skip => SplitVerdict::Skip,
                LineVerdict::Ignore => SplitVerdict::Ignore,
            },
            target_chunks,
        );
    (kept, skipped)
}

/// Core of [`scan_lines_parallel_split`] with an explicit chunk target (see
/// [`scan_lines_parallel_chunked`]).
pub(crate) fn scan_lines_parallel_split_chunked<A, B, F>(
    bytes: &[u8],
    visit: &F,
    target_chunks: usize,
) -> (Vec<A>, Vec<B>, usize)
where
    F: Fn(&[u8], usize) -> SplitVerdict<A, B> + Sync,
    A: Send,
    B: Send,
{
    if bytes.is_empty() {
        return (Vec::new(), Vec::new(), 0);
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
    // entirely, and a multi-chunk scan counts in PARALLEL and never counts the last chunk -
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
    let per_chunk: Vec<(Vec<A>, Vec<B>, usize)> = chunks
        .par_iter()
        .enumerate()
        .map(|(ci, &(s, e))| {
            let base = start_line[ci];
            let mut first: Vec<A> = Vec::new();
            let mut second: Vec<B> = Vec::new();
            let mut skipped = 0usize;
            let mut j = 0usize;
            let _ = scan_lines_bytes(&bytes[s..e], |line| {
                let line_no = base + j;
                j += 1;
                match visit(line, line_no) {
                    SplitVerdict::First(t) => first.push(t),
                    SplitVerdict::Second(t) => second.push(t),
                    SplitVerdict::Skip => skipped += 1,
                    SplitVerdict::Ignore => {}
                }
            });
            (first, second, skipped)
        })
        .collect();

    let mut all_first: Vec<A> = Vec::new();
    let mut all_second: Vec<B> = Vec::new();
    let mut skipped_total = 0usize;
    for (first, second, sk) in per_chunk {
        all_first.extend(first);
        all_second.extend(second);
        skipped_total += sk;
    }
    (all_first, all_second, skipped_total)
}
