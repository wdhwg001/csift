//! Where one test's scratch `$HOME` lives, and why its name cannot repeat.
//!
//! A scratch home is a directory under the system temp dir, and a test binary that dies
//! without unwinding (a timeout kill, an abort) leaves every live one behind. The name used
//! to be `csift-it-<pid>-<counter>`: the counter is unique within a process, but a pid is
//! handed back out once the number space wraps, so a later binary could be handed a name a
//! leaked home already occupies. `create_dir_all` succeeds on a directory that is already
//! there, so that later test would have started on top of a FOREIGN fixture tree - measured
//! by seeding `csift-it-<pid>-0` for the pids about to be handed out and running one test
//! single-threaded: `list_scope_banner_reports_pre_cap_scope` read `sessions_in_scope: 4`
//! against the 3 its own fixture writes.
//!
//! Two rules close it. The name carries a token unique to this process INSTANCE, so the
//! collision does not arise; and the root is created with `create_dir`, so if one ever did
//! arise it is a loud failure rather than a silent merge.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The per-process half of a scratch home's name: the pid, plus the instant this process
/// first asked. The pid alone repeats across runs; the pair does not.
fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{}-{nanos:x}", std::process::id())
    })
}

/// The directory the `n`-th scratch home of this process claims.
pub(crate) fn scratch_root(n: u64) -> PathBuf {
    std::env::temp_dir().join(format!("csift-it-{}-{n}", run_token()))
}

/// Claim `root` for this test. Fails when the directory already exists: that can only be a
/// leaked home from another run, and starting a test on someone else's fixtures is a wrong
/// answer rather than a slow one.
pub(crate) fn create_fresh(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir(root)
}

#[test]
fn a_scratch_root_that_already_exists_is_refused_rather_than_adopted() {
    // u64::MAX is past any counter a run reaches, so this name belongs to no live home.
    let taken = scratch_root(u64::MAX);
    let _ = std::fs::remove_dir_all(&taken);
    create_fresh(&taken).expect("the first claim succeeds");
    assert!(
        create_fresh(&taken).is_err(),
        "a second claim on an existing root must fail, not adopt it: {}",
        taken.display()
    );
    std::fs::remove_dir_all(&taken).unwrap();
}

#[test]
fn scratch_roots_are_distinct_and_carry_the_run_token() {
    let a = scratch_root(0);
    let b = scratch_root(1);
    assert_ne!(a, b, "each home of a process gets its own directory");
    let name = a.file_name().unwrap().to_string_lossy().into_owned();
    let pid = std::process::id();
    assert!(
        name.starts_with(&format!("csift-it-{pid}-")),
        "the pid stays in the name so a leaked home is attributable: {name}"
    );
    assert!(
        name != format!("csift-it-{pid}-0"),
        "the pid alone is not the key - a repeated pid must not repeat the name: {name}"
    );
}
