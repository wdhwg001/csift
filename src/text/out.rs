//! The ONE buffered stdout writer the row-dumping surfaces write through.
//!
//! ## Why a writer at all
//!
//! `println!` writes through a process-wide `LineWriter`: one `write(2)` per line, plus a
//! backward scan for the last newline on every call. A `search` that prints 493k text rows
//! pays 493k syscalls - measured on the section 7 landmark transcript as 0.45 s of the
//! 0.59 s of CPU the whole text render phase costs. One `BufWriter` over a held
//! `StdoutLock` turns that into one syscall per buffer.
//!
//! ## Why the lock is HELD, and what that obliges a caller to do
//!
//! [`Out`] owns a `StdoutLock` for its lifetime. Rust's stdout lock is REENTRANT, so a
//! stray `println!` on the same thread does not deadlock - it writes to the inner
//! `LineWriter` DIRECTLY and lands AHEAD of whatever this buffer still holds. So a surface
//! that builds an `Out` must route EVERY later stdout byte through it, and must
//! [`Out::flush`] before any stderr note whose order against stdout the contract states.
//! Output printed BEFORE the `Out` exists is already flushed (that is what a `LineWriter`
//! does at each newline), which is why the scope banners can stay `println!`.

use std::io::Write;

/// The stdout buffer, in bytes. Sized so a megabyte of rows is one `write(2)`.
const OUT_BUF_BYTES: usize = 1 << 20;

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

    /// Push everything buffered to the OS. Called at the end of a render and before any
    /// stderr note the contract orders against stdout - never silently, so a write error
    /// (a closed pipe, a full disk) reaches the caller's `Result` instead of being
    /// swallowed by the drop.
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.w.write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.w.write_all(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}
