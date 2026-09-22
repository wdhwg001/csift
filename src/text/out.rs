//! The ONE buffered stdout writer the row-dumping surfaces write through, and the
//! bounded ordered-parallel pipeline that fills it.
//!
//! ## Why a writer at all
//!
//! `println!` writes through a process-wide `LineWriter`: at least one `write(2)` per
//! call, plus a backward scan for the last newline on every call (a row carrying an
//! embedded newline pays more than one). `search 'the'` over the section 7 landmark
//! transcript prints 297,976 text rows, and that syscall floor was measured as 0.45 s of
//! the 0.59 s of CPU its whole text render phase costs. One `BufWriter` over a held
//! `StdoutLock` turns it into one syscall per buffer.
//!
//! ## Why the lock is HELD, and what that obliges a caller to do
//!
//! [`Out`] owns a `StdoutLock` for its lifetime, and what a stray `println!` does with it
//! depends on WHICH THREAD prints. Both answers are bad, and only one of them is loud.
//!
//! On the SAME thread the lock is REENTRANT, so the print does not block - it writes to the
//! inner `LineWriter` DIRECTLY and lands AHEAD of everything this buffer still holds. A
//! stray line after a [`write_ordered`] call comes out FIRST, with no error and exit 0.
//!
//! On a WORKER thread - inside the [`write_ordered`] render closure - reentrancy does not
//! apply, because the lock is held by ANOTHER thread. The worker parks waiting for stdout
//! while the main thread parks waiting for that worker's buffer, and the PROCESS HANGS with
//! nothing written. It hangs only at SCALE: rayon runs a small item set inline on the
//! calling thread, so a handful of rows finishes and a few hundred does not.
//!
//! So a surface that builds an `Out` routes EVERY later stdout byte through it, and the
//! render closure prints nothing at all. Both are guarded mechanically: every module that
//! builds or writes through an `Out` denies `clippy::print_stdout` and
//! `clippy::print_stderr`, and each legitimate site carries an `#[allow]` saying why it
//! prints outside a buffer. Output printed BEFORE the `Out` exists is already flushed
//! (that is what a `LineWriter` does at each newline), so it cannot land out of order -
//! but it is still a `println!`, which ABORTS on a write that cannot land where the buffer
//! REPORTS one, so a surface that builds an `Out` builds it FIRST and writes even its scope
//! and duplicate banners through it. Otherwise one query answers a closed pipe two ways,
//! chosen by whether its scope was wide enough to print a banner.
//!
//! The lint fires at the PRINT SITE, so a CALL that prints from ANOTHER module is outside
//! it, and the tree holds TWO such edges. The banner sinks named `emit_scope_banner` and
//! `emit_duplicate_banner` print from files that carry no deny, because both of those files
//! legitimately print - so a CALL to either from inside a buffered module compiles clean,
//! passes clippy, and puts a `println!` back in front of the buffer. Their callers are the
//! three surfaces that print every row with `println!` (`commands/session/render.rs` names
//! the scope sink; `commands/stats/render.rs` and `commands/recover/render.rs` name both);
//! a surface that holds an `Out` takes the writer form instead, and a tree sweep in
//! `tests/cli/contracts/write_errors.rs` refuses a module that carries the deny and names
//! either sink, asking PER SINK so that half a migration fails too. The SECOND edge is
//! `commands/image/run.rs`, which carries the deny and calls `extract` in
//! `commands/image/convert.rs`, a file that carries none and holds six prints. It is safe
//! by EXCLUSIVITY rather than by the lint: `run_image` dispatches `extract` and the two
//! listing renders in either/or branches and only the renders build an `Out`, so on the
//! `--out` path there is no buffer for a print to land ahead of. `convert.rs` says that at
//! its own module doc and names the condition that would end it. The sweep is deliberately
//! not widened to reach it - its sink set is the two banner names, and adding `extract`
//! would flag `run.rs` for a call that is correct.
//!
//! The FLUSH obligation is WIDER THAN THE LINT, and that is the half to read twice. A
//! surface [`Out::flush`]es before ANY CALL that can write to stderr, not only before an
//! `eprintln!` it can see: the lint reads one module, and a callee in another module is
//! invisible to it. `verbatim`'s `--out` block is the case that proved it - the refusal
//! note for an empty reconstruction is printed by `commands::recover::write_out_guarded`,
//! which no guarded module contains, and it came out AHEAD of the rows the buffer was
//! still holding.
//!
//! ## Why rendering is parallel, and what bounds its memory
//!
//! A row is rendered from data the scan already produced, so rows are independent and the
//! only ordering requirement is that they REACH stdout in file order. [`write_ordered`]
//! renders fixed-size chunks on the rayon pool into per-chunk byte buffers and writes the
//! buffers in order. The look-ahead is bounded at [`ORDERED_CHUNK`] * [`ORDERED_BATCH`]
//! items, so peak memory rises by the rendered bytes of that many CONSECUTIVE rows rather
//! than by the whole stream - which for the landmark transcript's 411 MB of JSON would
//! otherwise be the whole 411 MB. The bound is UP TO TWICE that, not exactly it: a chunk
//! renders into a `Vec` grown by `extend`, a `Vec` that has doubled sits at up to twice
//! the bytes it holds, and [`ORDERED_BATCH`] of them are live at once. Those buffers are
//! not the whole of what a wave holds: while it renders, each worker also holds ONE row's
//! intermediate structures, which for the JSON projector is the `serde_json::Value` tree it
//! builds before serializing it into its chunk's buffer - a term bounded by the pool rather
//! than by the look-ahead. On ONE measured fixture (SPEC 7k names it: 200 records whose
//! rendered rows are 4.2 MB each, so 128 of them is 512.2 MiB) the peak-RSS delta is a
//! RANGE, and one pair of numbers would state a property of one run rather than of the
//! pipeline: three runs of five interleaved pairs, at one-minute load averages of 7 to 27,
//! 8 to 10 and 7, read medians of 715.8, 844.2 and 723.7 MiB, with the widest single
//! observation at 929.3 and the narrowest at 522.3 - every one of them under the bound, and
//! none of them a ratio to carry anywhere else. SPEC 7k's second shape, 160 rows of 4.55
//! MiB, moved between 0 and +127 MiB: what a peak SHOWS depends on the fixture's own parse
//! footprint and on the pages the process already holds, which is why a reader sizing a run
//! works from the bound above rather than from an observed delta.
#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::io::Write;

use rayon::prelude::*;

/// The stdout buffer, in bytes. Sized so a megabyte of rows is one `write(2)`.
const OUT_BUF_BYTES: usize = 1 << 20;

/// Rows rendered by ONE parallel task. Big enough to amortise the task over a row's own
/// render (the landmark's JSON rows average 78 KB each), small enough that the tail of a
/// wave does not leave workers idle.
const ORDERED_CHUNK: usize = 8;

/// Chunks in flight per wave - the LOOK-AHEAD. `ORDERED_CHUNK * ORDERED_BATCH` rows are
/// rendered before any of them is written, and each of those chunk buffers can sit at
/// twice the bytes it holds, which is the memory bound a reader sizing a run works from
/// (the module docs name the one term it leaves out).
const ORDERED_BATCH: usize = 16;

/// A buffered stdout, holding the lock for its lifetime. See the module docs: a surface
/// that builds one writes every later stdout byte through it.
#[derive(Debug)]
pub struct Out {
    w: std::io::BufWriter<std::io::StdoutLock<'static>>,
}

impl Out {
    /// Lock stdout and wrap it in the shared buffer.
    #[must_use]
    pub fn stdout() -> Out {
        Out {
            w: std::io::BufWriter::with_capacity(OUT_BUF_BYTES, std::io::stdout().lock()),
        }
    }
}

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.w.write(buf)
    }

    /// Push everything buffered to the OS. Called at the end of a render and before any
    /// stderr note the contract orders against stdout - never silently, so a write error
    /// (a closed pipe, a full disk) reaches the caller's `Result` instead of being
    /// swallowed by the drop.
    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

/// Render `items` in FILE ORDER through the rayon pool and write them through `out`.
///
/// `render(index, item, buf)` appends one item's bytes - newline included - to its chunk's
/// buffer; the index is the item's position in `items`, for the renderer that needs to know
/// where it sits (a note printed once, under the first row that earns it). The closure runs
/// on a worker thread, so it must be pure with respect to the run: every cross-row decision
/// is made by the caller BEFORE the pipeline starts.
///
/// Ordering is exact - the buffers are written in index order - so the byte stream is the
/// one a serial loop would have produced. Generic over the sink so a test can assert that
/// on a buffer rather than on the terminal.
pub fn write_ordered<W, T, F>(out: &mut W, items: &[T], render: F) -> anyhow::Result<()>
where
    W: Write,
    T: Sync,
    F: Fn(usize, &T, &mut Vec<u8>) -> anyhow::Result<()> + Sync,
{
    let wave = ORDERED_CHUNK * ORDERED_BATCH;
    let mut base = 0usize;
    for slice in items.chunks(wave) {
        let bufs: Vec<Vec<u8>> = slice
            .par_chunks(ORDERED_CHUNK)
            .enumerate()
            .map(|(ci, chunk)| {
                let mut buf: Vec<u8> = Vec::new();
                for (k, item) in chunk.iter().enumerate() {
                    render(base + ci * ORDERED_CHUNK + k, item, &mut buf)?;
                }
                Ok(buf)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        for b in &bufs {
            out.write_all(b)?;
        }
        base += slice.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pipeline's contract is ORDER: whatever the pool did, the bytes come out in the
    /// index order of `items`, and the index the renderer is handed is the item's own slot.
    /// Sized past three waves so the batching and the short final wave both run.
    #[test]
    fn write_ordered_emits_items_in_index_order() {
        let items: Vec<usize> = (0..(ORDERED_CHUNK * ORDERED_BATCH * 3 + 5)).collect();
        let mut sink: Vec<u8> = Vec::new();
        write_ordered(&mut sink, &items, |i, item, buf| {
            assert_eq!(i, *item, "the renderer is handed the item's own slot");
            buf.extend_from_slice(format!("{item}\n").as_bytes());
            Ok(())
        })
        .expect("a Vec sink cannot fail");
        let expected: String = items.iter().map(|i| format!("{i}\n")).collect();
        assert_eq!(String::from_utf8(sink).expect("ascii"), expected);
    }

    /// An error from one chunk aborts the whole pipeline rather than reporting success on
    /// a short stream: a partial render that returned `Ok` would be exactly the silent
    /// truncation the output contract forbids.
    #[test]
    fn write_ordered_propagates_a_render_error() {
        let items: Vec<usize> = (0..64).collect();
        let mut sink: Vec<u8> = Vec::new();
        let err = write_ordered(&mut sink, &items, |i, _item, _buf| {
            if i == 40 {
                anyhow::bail!("row 40 refused");
            }
            Ok(())
        });
        let err = err.expect_err("the failing chunk must reach the caller");
        assert!(format!("{err:#}").contains("row 40 refused"));
    }

    /// The LOOK-AHEAD is what bounds the memory this pipeline adds, so it is measured
    /// rather than asserted from the constants: the renderer counts the rows it has
    /// produced and the sink reads that count as each buffer arrives, so the largest gap
    /// between the two IS how many rendered rows were held unwritten at once. It must be
    /// 128 - a wider look-ahead is more peak memory than the contract states, a narrower
    /// one leaves workers idle at each barrier. This pins the ROW count; the BYTES bound
    /// is up to twice it, because a growing chunk buffer holds up to twice its content.
    #[test]
    fn write_ordered_holds_at_most_one_wave_of_rendered_rows() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counting<'a> {
            rendered: &'a AtomicUsize,
            written: usize,
            peak: usize,
        }
        impl Write for Counting<'_> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                // Read the gap BEFORE booking this buffer: what is held unwritten at the
                // moment a write begins is the peak. One byte per row, so the two counts
                // are in the same unit.
                let gap = self.rendered.load(Ordering::SeqCst) - self.written;
                self.peak = self.peak.max(gap);
                self.written += buf.len();
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let items: Vec<usize> = (0..1000).collect();
        let rendered = AtomicUsize::new(0);
        let mut sink = Counting {
            rendered: &rendered,
            written: 0,
            peak: 0,
        };
        write_ordered(&mut sink, &items, |_i, _item, buf| {
            buf.push(b'.');
            rendered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .expect("the counting sink never fails");
        assert_eq!(sink.written, items.len(), "every row reached the sink");
        assert_eq!(
            sink.peak, 128,
            "the look-ahead the memory bound is stated as"
        );
    }

    /// A wave is a barrier, not a boundary in the byte stream: an item count that is not a
    /// multiple of the chunk or the wave still emits every item exactly once, in order.
    #[test]
    fn write_ordered_emits_every_item_once_on_a_ragged_tail() {
        for n in [0usize, 1, 7, 8, 9, ORDERED_CHUNK * ORDERED_BATCH + 1] {
            let items: Vec<usize> = (0..n).collect();
            let mut sink: Vec<u8> = Vec::new();
            write_ordered(&mut sink, &items, |_i, item, buf| {
                buf.extend_from_slice(format!("{item};").as_bytes());
                Ok(())
            })
            .expect("a Vec sink cannot fail");
            let expected: String = items.iter().map(|i| format!("{i};")).collect();
            assert_eq!(String::from_utf8(sink).expect("ascii"), expected, "n={n}");
        }
    }
}
