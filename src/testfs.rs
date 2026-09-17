//! Where a UNIT test's scratch files live, and why the name cannot repeat.
//!
//! A fixture tree under the system temp dir outlives a test binary that dies without
//! unwinding (a timeout kill, an abort), and MOST of the names in this crate were
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
//! TWO fixtures were already out of that shape and came here for the OTHER rule: the
//! channel and reconcile fixtures interpolated a per-CALL nanosecond reading beside the
//! pid, so their names did not repeat across runs - but they still claimed the tree with
//! `create_dir_all`, so a collision would have merged rather than failed.
//!
//! Three rules close it, the first two the e2e harness's (`tests/cli/harness/root.rs`):
//! the name carries a token unique to this process INSTANCE, so the collision does not
//! arise; the tree is created with `create_dir`, so if one ever did arise it is a loud
//! failure rather than a silent merge; and the guard removes its tree on drop, so a clean
//! run leaves nothing for a later one to adopt. The harness is a separate crate and cannot
//! be imported here, so the rule is mirrored rather than shared and the two tests below pin
//! this side of it.
//!
//! The SEQUENCE is the third field for a reason of its own, and it is about one run rather
//! than about two: two fixtures built in the same nanosecond on parallel test threads would
//! otherwise share a name, and the first one to finish would `remove_dir_all` the other's
//! tree mid-test. That was a measured ~8% flake on the default parallel runner before the
//! `subagent` fixture grew its counter, which is why the counter is process-wide and
//! atomic rather than per-tag.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// The MINT's clock reading, named so the mint says what it reads.
///
/// The token test deliberately does NOT call this: it reads the clock itself. A check that
/// went through this function would compare the mint against its own reading, so a mint
/// whose clock is broken would agree with itself and pass (measured: with this function
/// returning a constant, both tests passed).
fn clock_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// The per-process half of a scratch name: the pid, plus the instant this process first
/// asked. The pid alone repeats across runs; the pair does not.
fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| format!("{}-{:x}", std::process::id(), clock_nanos()))
}

/// The directory the `seq`-th scratch tree of this process claims under `tag`.
fn scratch_path(tag: &str, seq: u64) -> PathBuf {
    std::env::temp_dir().join(format!("csift-{tag}-{}-{seq}", run_token()))
}

/// Claim `root` for this test. Fails when the directory is already there.
fn claim(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir(root)
}

/// The refusal message, so the test that exercises it and the code that raises it spell
/// it once. It names the DIRECTORY: a reader who hits this needs the path to go look.
fn not_ours(root: &Path, e: &std::io::Error) -> String {
    format!("scratch tree {} is not this run's own: {e}", root.display())
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
    /// Claim a fresh tree tagged `tag`, at this process's next sequence number.
    pub(crate) fn dir(tag: &str) -> Scratch {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        Scratch::dir_at(tag, SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// Claim the tree `tag` and `seq` name. Panics, naming the directory, when one is
    /// already there: that can only be a tree leaked by another run, and starting a test on
    /// someone else's fixtures is a wrong answer rather than a slow one.
    ///
    /// The sequence is a parameter so the refusal is TESTABLE. `dir` draws it from a
    /// process-global counter, so the name of its next tree cannot be predicted from
    /// outside - another test on another thread may take it first - and a collision test
    /// written against `dir` would be a race rather than a pin.
    fn dir_at(tag: &str, seq: u64) -> Scratch {
        let root = scratch_path(tag, seq);
        claim(&root).unwrap_or_else(|e| panic!("{}", not_ours(&root, &e)));
        Scratch { root }
    }

    /// A fresh tree plus the path of one file inside it. A bare `temp_dir()/csift-x-<pid>.txt`
    /// is the same adoption hazard one level down, so a file fixture lives in a tree of its
    /// own. The file is not created here: a test that wants one writes it, and the ones that
    /// need a path with NO file behind it get exactly that.
    pub(crate) fn file(tag: &str, name: &str) -> (Scratch, PathBuf) {
        let dir = Scratch::dir(tag);
        let path = dir.join(name);
        (dir, path)
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
        // The claim runs through the guard's own constructor, so what is pinned is the arm
        // a test actually hits - the panic - rather than the `create_dir` one layer under it.
        let taken = scratch_path("guard", u64::MAX);
        let _ = std::fs::remove_dir_all(&taken);
        let held = Scratch::dir_at("guard", u64::MAX);
        assert!(
            held.is_dir(),
            "the first claim succeeds: {}",
            taken.display()
        );

        // The second claim panics; the test prints its message, which is expected output.
        let refused = std::panic::catch_unwind(|| Scratch::dir_at("guard", u64::MAX));
        let Err(payload) = refused else {
            panic!("a second claim on an existing tree must fail, not adopt it");
        };
        let said = payload
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            said.contains(&taken.display().to_string()),
            "the refusal names the directory so a reader can go look: {said:?}"
        );
        drop(held);
        assert!(!taken.exists(), "the guard took its tree with it");
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
        // The token must be the CLOCK, not merely some second field: a constant one would
        // repeat across runs exactly as the pid does, and that repeat is what this whole
        // module exists to prevent. So read it back as the hex reading it is and hold it
        // against the clock READ HERE - this expression, never the module's own
        // `clock_nanos()`. The test's clock must be independent of the mint's, or a broken
        // mint is checked against its own breakage and passes: measured, with
        // `clock_nanos()` returning a constant, both sides read that constant, the
        // difference was zero and this test went green.
        //
        // The window is the documented semantics, not a measurement: the token is minted at
        // this process's FIRST scratch call, so it lies within this process's own lifetime
        // of now, and a day is past any test binary's life while a constant literal misses
        // the current era by decades. Symmetric, because the system clock may step either
        // way in between.
        const TOKEN_WINDOW_NANOS: u128 = 24 * 60 * 60 * 1_000_000_000;
        let minted = u128::from_str_radix(token, 16)
            .unwrap_or_else(|e| panic!("the token is a hex clock reading: {token:?} ({e})"));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a system clock at or after the epoch")
            .as_nanos();
        assert!(
            now.abs_diff(minted) < TOKEN_WINDOW_NANOS,
            "the token is a reading of this clock taken during this run: minted {minted}, \
             now {now}, apart {} ns (window {TOKEN_WINDOW_NANOS})",
            now.abs_diff(minted)
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
