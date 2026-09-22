//! The ONE buffered stdout writer the row-dumping surfaces write through, and the
//! bounded ordered-parallel pipeline that fills it.
//!
//! ## Why a writer at all
//!
//! `println!` writes through a process-wide `LineWriter`: one `write(2)` per line, plus a
//! backward scan for the last newline on every call. `search 'the'` over the section 7
//! landmark transcript prints 297,976 text rows and so pays 297,976 syscalls - measured as
//! 0.45 s of the 0.59 s of CPU its whole text render phase costs. One `BufWriter` over a
//! held `StdoutLock` turns that into one syscall per buffer.
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
//! prints outside a buffer. A surface must also [`Out::flush`] before any stderr note whose
//! order against stdout the contract states. Output printed BEFORE the `Out` exists is
//! already flushed (that is what a `LineWriter` does at each newline), which is why the
//! scope banners can stay `println!`.
//!
//! ## Why rendering is parallel, and what bounds its memory
//!
//! A row is rendered from data the scan already produced, so rows are independent and the
//! only ordering requirement is that they REACH stdout in file order. [`write_ordered`]
//! renders fixed-size chunks on the rayon pool into per-chunk byte buffers and writes the
//! buffers in order. The look-ahead is bounded at [`ORDERED_CHUNK`] * [`ORDERED_BATCH`]
//! items: peak memory rises by the rendered bytes of that many CONSECUTIVE rows and by
//! nothing else, rather than by the whole stream - which for the landmark transcript's
//! 411 MB of JSON would otherwise be the whole 411 MB.
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
/// rendered before any of them is written, which is the whole memory bound this pipeline
/// adds.
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
    /// one leaves workers idle at each barrier.
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
