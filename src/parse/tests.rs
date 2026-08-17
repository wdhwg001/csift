use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Minimal temp-file helper (no external dev-dep): writes lines to a uniquely
/// named file under the OS temp dir and removes it on drop.
mod tempfile_path {
    use super::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

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
            let n = super::COUNTER.fetch_add(1, Ordering::Relaxed);
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

mod lines;
mod readers;
