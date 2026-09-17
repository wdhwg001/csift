//! Where a UNIT test's scratch files live, and why the name cannot repeat.
//!
//! A fixture tree under the system temp dir outlives a test binary that dies without
//! unwinding (a timeout kill, an abort), and the names in this crate were
//! `csift-<tag>-<pid>` or `csift-<tag>-<pid>-<counter>`: unique inside one process, but a
//! pid is handed back out once the number space wraps, so a later run could be handed a
//! name a leaked tree already occupies. `create_dir_all` returns Ok on a directory that is
//! already there, so that later test would have read the previous run's files as its own.
//! Both halves were measured: 218,987 leaked `csift-*` unit-fixture directories under one
//! temp dir (the largest groups 32k `csift-subdir` and 29k `csift-files-sub`), and one real
//! refusal - `commands::session::tests::cleared_from_origin_never_joins_a_file_to_itself`
//! failed in a gate run and passed in isolation, its `csift-clear-self-<pid>-<counter>`
//! directory having adopted a leaked sibling transcript.
//!
//! Two rules close it, the same two the e2e harness took (`tests/cli/harness/root.rs`):
//! the name carries a token unique to this process INSTANCE, so the collision does not
//! arise; and the tree is created with `create_dir`, so if one ever did arise it is a loud
//! failure rather than a silent merge. A third rule keeps the leak from starting: the guard
//! removes its tree on drop, so a clean run leaves nothing for a later one to adopt. The
//! harness is a separate crate and cannot be imported here, so the rule is mirrored rather
//! than shared and the two tests below pin this side of it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// The per-process half of a scratch name: the pid, plus the instant this process first
/// asked. The pid alone repeats across runs; the pair does not.
fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{}-{nanos:x}", std::process::id())
    })
}

/// The directory the `seq`-th scratch tree of this process claims under `tag`.
fn scratch_path(tag: &str, seq: u64) -> PathBuf {
    std::env::temp_dir().join(format!("csift-{tag}-{}-{seq}", run_token()))
}

/// Claim `root` for this test. Fails when the directory is already there.
fn claim(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir(root)
}

/// A scratch directory tree, removed on drop.
///
/// Hold it for the whole test body: dropping it deletes the tree, so a fixture helper
/// returns the guard (or returns it beside the path it built) rather than a bare `PathBuf`.
/// `Deref<Target = Path>` makes it usable wherever the directory path was.
#[derive(Debug)]
pub(crate) struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// Claim a fresh tree tagged `tag`. Panics, naming the directory, when one is already
    /// there: that can only be a tree leaked by another run, and starting a test on someone
    /// else's fixtures is a wrong answer rather than a slow one.
    pub(crate) fn dir(tag: &str) -> Scratch {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let root = scratch_path(tag, SEQ.fetch_add(1, Ordering::Relaxed));
        claim(&root).unwrap_or_else(|e| {
            panic!("scratch tree {} is not this run's own: {e}", root.display())
        });
        Scratch { root }
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_tree_that_already_exists_is_refused_rather_than_adopted() {
        // u64::MAX is past any sequence a run reaches, so this name belongs to no live tree.
        let taken = scratch_path("guard", u64::MAX);
        let _ = std::fs::remove_dir_all(&taken);
        claim(&taken).expect("the first claim succeeds");
        assert!(
            claim(&taken).is_err(),
            "a second claim on an existing tree must fail, not adopt it: {}",
            taken.display()
        );
        std::fs::remove_dir_all(&taken).unwrap();
    }

    #[test]
    fn scratch_trees_are_distinct_carry_the_run_token_and_vanish_on_drop() {
        let a = Scratch::dir("probe");
        let b = Scratch::dir("probe");
        assert_ne!(&*a, &*b, "each tree of a process gets its own directory");

        // `csift-probe-<pid>-<token>-<seq>`: split the sequence off the tail, and what is
        // left is the part that must be constant within a run and distinct between runs.
        let name = |s: &Scratch| s.file_name().unwrap().to_string_lossy().into_owned();
        let (na, nb) = (name(&a), name(&b));
        let (head_a, seq_a) = na.rsplit_once('-').expect("a sequence suffix");
        let (head_b, seq_b) = nb.rsplit_once('-').expect("a sequence suffix");
        assert_eq!(
            head_a, head_b,
            "the token is minted once per process, so only the sequence moves: {na} vs {nb}"
        );
        assert_ne!(seq_a, seq_b, "two trees of one tag must not share a name");
        let pid_head = format!("csift-probe-{}-", std::process::id());
        let token = head_a.strip_prefix(&pid_head).unwrap_or_else(|| {
            panic!("the tag and pid stay in the name so a leaked tree is attributable: {na}")
        });
        assert!(
            !token.is_empty(),
            "the pid alone is not the key - a repeated pid must not repeat the name: {na}"
        );

        let kept = a.to_path_buf();
        assert!(kept.is_dir(), "the tree exists while its guard does");
        drop(a);
        assert!(
            !kept.exists(),
            "the guard removes its tree, so a clean run leaves nothing to adopt"
        );
    }
}
