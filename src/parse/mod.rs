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

mod lines;
mod parallel;
mod readers;

pub(crate) use lines::*;
pub(crate) use parallel::*;
pub(crate) use readers::*;

#[cfg(test)]
mod tests;
