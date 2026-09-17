use super::*;
use crate::testfs::Scratch;
use std::io::Write;

/// Collect ALL backward lines (as Strings) for a byte slice at a given chunk
/// size, dropping blanks (matching the parse-path's blank filter).
fn rev_nonblank(bytes: &[u8], chunk: usize) -> Vec<String> {
    RevLines::with_chunk(bytes, chunk)
        .filter_map(|l| line_payload(&l).map(|p| String::from_utf8_lossy(p).into_owned()))
        .collect()
}

fn tmp_jsonl(lines: &[&str]) -> tempfile_path::TempJsonl {
    tempfile_path::TempJsonl::new(lines)
}

/// Minimal temp-file helper (no external dev-dep): writes lines to a file inside this
/// run's own scratch tree, which removes the file with it on drop.
mod tempfile_path {
    use super::{Scratch, Write};
    use std::path::{Path, PathBuf};

    #[derive(Debug)]
    pub struct TempJsonl {
        /// The guard whose Drop removes the file; held, never read.
        _dir: Scratch,
        path: PathBuf,
    }

    impl TempJsonl {
        pub fn new(lines: &[&str]) -> Self {
            let (dir, path) = Scratch::file("test", "t.jsonl");
            let mut f = std::fs::File::create(&path).expect("create temp");
            for l in lines {
                writeln!(f, "{l}").expect("write line");
            }
            f.flush().expect("flush");
            TempJsonl { _dir: dir, path }
        }

        pub fn empty() -> Self {
            let (dir, path) = Scratch::file("test", "t.jsonl");
            std::fs::File::create(&path).expect("create temp");
            TempJsonl { _dir: dir, path }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }
}

mod lineage;
mod lines;
mod parallel;
mod readers;
mod spine;
