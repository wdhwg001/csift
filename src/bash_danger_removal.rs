//! The STRUCTURED removal checker `_9` (2.1.258 @162770807), reached for every
//! cleanly parsed command - which is the majority of real removals. It has five
//! ask arms, all written by the same factory `sF` @162770607 with the same
//! bypass-immune circuit breaker, and they differ only in the reason tail.
//!
//! `_9` touches the filesystem (`path.resolve` of the operand against the shell
//! cwd, a realpath of both, and the workspace set from the permission context), so
//! it cannot be ported 1:1 from a transcript. Three arms are statically decidable
//! and are ported here; the workspace-or-ancestor arm and the unenumerable-glob
//! arm are not, and csift answers `NeedsFs` for them rather than guessing.
//!
//! The path helpers are node's own, named by the import at @162762089:
//! `import{isAbsolute as hO,normalize as gto,resolve as Yon,sep as hto}from"path"`.
//!
//! Two deliberate reductions, both disclosed on the verdict:
//! - csift has no shell cwd here, so a RELATIVE operand's resolved form is
//!   unknown. Every test that keys on the operand text or on the presence of a
//!   trailing glob still runs; the tail interpolates the operand as written.
//! - the decomposer replaces an expansion with `__TRACKED_VAR__` and a command
//!   substitution with `__CMDSUB_OUTPUT__` before `_9` sees the operand, which is
//!   what makes the sentinel test `Is` fire. csift substitutes the same two
//!   sentinels, except for the twenty variable names the decomposer resolves from
//!   the real environment, which become a `NeedsFs` answer instead.

use crate::bash_danger_argv::{
    any_statement_is_cd, command_argv, normalise_verb, positional_operands, simple_commands,
    CMDSUB_SENTINEL, ENV_MARKER, VAR_SENTINEL,
};
use regex::Regex;
use std::sync::LazyLock;

/// `qYr` @162427278 - a drive root after slash normalization.
static DRIVE_ROOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]:/?$").expect("qYr"));
/// `VYr` @162427299 - a drive's top-level directory.
static DRIVE_TOP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]:/[^/]+$").expect("VYr"));
/// The trailing glob run `([\\/]\*+)+[\\/]*$` stripped by the `_9` fixpoint.
static TRAILING_GLOB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([\\/]\*+)+[\\/]*$").expect("trailing_glob"));
/// `rmdir`'s `-p` family, the one flag the unresolvable arm consults.
static RMDIR_PARENTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^--p|^-[a-z]*p").expect("rmdir_p"));
/// One operand's answer from the `_9` arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperandVerdict {
    /// An immune ask, carrying the harness's reason tail and the target it names.
    Ask { tail: &'static str, target: String },
    /// The remaining arms are the two that read the filesystem at the time.
    NeedsFs,
    /// Every arm csift can evaluate declined.
    Clear,
}

/// The reason tail shared by the cd-relative-glob, unresolvable-target and
/// unenumerable-glob arms @162770807 / @162771609 / @162772546.
pub(crate) const TAIL_UNRESOLVABLE: &str = "on statically-unresolvable target";
/// The critical-path arm's tail @162771910.
pub(crate) const TAIL_CRITICAL: &str = "on critical path";

/// Run the structured path over a whole command: decompose, keep every rm/rmdir,
/// and answer with the FIRST ask any operand produces. `NeedsFs` is remembered and
/// returned only when no operand asked, so an ask is never hidden behind it.
pub(crate) fn structured(command: &str) -> OperandVerdict {
    let cd_seen = any_statement_is_cd(command);
    let mut needs_fs = false;
    for stmt in simple_commands(command) {
        let argv = command_argv(&stmt);
        let Some(head) = argv.first() else { continue };
        let verb = normalise_verb(head);
        if verb != "rm" && verb != "rmdir" {
            continue;
        }
        let rest: Vec<String> = argv[1..].to_vec();
        let operands = positional_operands(&rest);
        for b in &operands {
            match removal_check(verb, b, &rest, cd_seen) {
                OperandVerdict::Ask { tail, target } => {
                    return OperandVerdict::Ask { tail, target }
                }
                OperandVerdict::NeedsFs => needs_fs = true,
                OperandVerdict::Clear => {}
            }
        }
    }
    if needs_fs {
        OperandVerdict::NeedsFs
    } else {
        OperandVerdict::Clear
    }
}

/// The three decidable arms of `_9` for ONE operand, in the harness's own order.
fn removal_check(verb: &str, b: &str, argv_rest: &[String], cd_seen: bool) -> OperandVerdict {
    if b.contains(ENV_MARKER) {
        return OperandVerdict::NeedsFs;
    }
    let absolute = is_absolute(b);
    // `U=hO(B)?B:Yon(r,B)`. Without the shell cwd the resolved head is unknown, so
    // it is carried symbolically; every test below reads only the tail of U.
    let u = if absolute {
        b.to_string()
    } else {
        format!("{CWD}/{b}")
    };
    let j = trailing_glob_fixpoint(&u);
    let w = j != u;
    let shown = b.to_string();
    // Arm 1 @162770807: cd anywhere in the command + a RELATIVE glob operand.
    if cd_seen && w && !absolute && u.ends_with("/*") {
        return OperandVerdict::Ask {
            tail: TAIL_UNRESOLVABLE,
            target: shown,
        };
    }
    // Arm 2 @162771609: the removal target cannot be statically resolved.
    let dotdot_after_segment = has_dotdot_after_segment(b);
    let sentinel = b.contains(CMDSUB_SENTINEL) || b.contains(VAR_SENTINEL);
    let unc = is_unc_or_drive(b);
    let rmdir_p =
        verb == "rmdir" && u.ends_with("/*") && argv_rest.iter().any(|f| RMDIR_PARENTS.is_match(f));
    let star_slash = !absolute && ends_with_star_slash(b) && u.ends_with("/*");
    let dotdot_glob = !absolute && has_dotdot_segment(b) && u.ends_with("/*");
    if w && (dotdot_after_segment
        || sentinel
        || b.starts_with('~')
        || unc
        || dotdot_glob
        || rmdir_p
        || star_slash)
    {
        return OperandVerdict::Ask {
            tail: TAIL_UNRESOLVABLE,
            target: shown,
        };
    }
    // Arms 3 and 4 sit behind one gate; only arm 3 is decidable here.
    let z = if absolute { j.clone() } else { relative_z(&j) };
    if !w || !z.contains(['*', '?', '[']) {
        // `Tp` expands a bare `~` / `~/` to the home directory, which `eDe` treats
        // as critical whatever its value is; a deeper `~/x` needs the environment.
        if b == "~" || b == "~/" || (!j.contains(CWD) && is_critical_path(&j)) {
            return OperandVerdict::Ask {
                tail: TAIL_CRITICAL,
                target: shown,
            };
        }
        // Arm 4 compares the target against the working-directory set and its
        // realpaths. Neither is in the transcript.
        return OperandVerdict::NeedsFs;
    }
    // Arm 5 counts the glob segments the pattern would have to enumerate, and the
    // count is taken against a realpath-resolved target.
    OperandVerdict::NeedsFs
}

/// The stand-in for the shell cwd csift does not hold. It carries no separator, so
/// every suffix test on the resolved form stays exact.
const CWD: &str = "\u{1}cwd";

/// `eDe` @162427324 - the critical-path test, minus the two homedir comparisons
/// (which read `os.homedir()`) and the macOS `/private` alias fold (which only
/// widens the homedir comparison).
fn is_critical_path(p: &str) -> bool {
    let n = collapse_slashes(p);
    if n == "*" || n.ends_with("/*") {
        return true;
    }
    let f = if n == "/" {
        n.clone()
    } else {
        n.trim_end_matches('/').to_string()
    };
    if f == "/" {
        return true;
    }
    if DRIVE_ROOT.is_match(&f) {
        return true;
    }
    if dirname(&f) == "/" {
        return true;
    }
    DRIVE_TOP.is_match(&f)
}

/// The `_9` trailing-glob fixpoint @162771008:
/// `for(let fe="";fe!==j;){fe=j;let me=j.replace(/([\\/]\*+)+[\\/]*$/,"")||"/";
///  if(me!==j)j=/[\\/]/.test(me)?gto(me):me}` with `gto` = `path.normalize`.
fn trailing_glob_fixpoint(u: &str) -> String {
    let mut j = u.to_string();
    loop {
        let fe = j.clone();
        let stripped = TRAILING_GLOB.replace(&j, "").into_owned();
        let me = if stripped.is_empty() {
            "/".to_string()
        } else {
            stripped
        };
        if me != j {
            j = if me.contains('/') || me.contains('\\') {
                normalize(&me)
            } else {
                me
            };
        }
        if fe == j {
            return j;
        }
    }
}

/// `z` - the target rebased off the shell cwd for a relative operand. Only its
/// glob characters are read, so the symbolic head drops out cleanly.
fn relative_z(j: &str) -> String {
    j.strip_prefix(CWD)
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_else(|| j.to_string())
}

/// `path.normalize` on posix: collapse separator runs, drop `.`, resolve `..`
/// against a preceding real segment, and keep the leading separator.
pub(crate) fn normalize(p: &str) -> String {
    let absolute = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(out.last(), Some(&last) if last != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

fn collapse_slashes(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    let mut prev_sep = false;
    for c in p.chars() {
        let sep = c == '/' || c == '\\';
        if sep {
            if !prev_sep {
                out.push('/');
            }
        } else {
            out.push(c);
        }
        prev_sep = sep;
    }
    out
}

fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => ".".to_string(),
    }
}

fn is_absolute(b: &str) -> bool {
    b.starts_with('/') || b.starts_with('\\') || is_drive_prefixed(b)
}

fn is_drive_prefixed(b: &str) -> bool {
    let mut cs = b.chars();
    matches!((cs.next(), cs.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic())
}

/// `Hn` @157079533 - a UNC shape - widened to the drive prefix the same test
/// reaches through `MW`.
fn is_unc_or_drive(b: &str) -> bool {
    let mut cs = b.chars();
    let two_seps = matches!((cs.next(), cs.next()), (Some('/' | '\\'), Some('/' | '\\')));
    two_seps || is_drive_prefixed(b)
}

/// `Hft` @162427948 - a `..` segment that follows a real segment.
fn has_dotdot_after_segment(b: &str) -> bool {
    let mut saw_real = false;
    for seg in b.split(['/', '\\']) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if saw_real {
                return true;
            }
        } else {
            saw_real = true;
        }
    }
    false
}

/// `/(^|[\\/])\.\.([\\/]|$)/` - a `..` segment anywhere.
fn has_dotdot_segment(b: &str) -> bool {
    b.split(['/', '\\']).any(|s| s == "..")
}

/// `/\*[\\/]+$/` - a glob segment immediately before the trailing separator run.
fn ends_with_star_slash(b: &str) -> bool {
    let t = b.trim_end_matches(['/', '\\']);
    t.len() < b.len() && t.ends_with('*')
}
