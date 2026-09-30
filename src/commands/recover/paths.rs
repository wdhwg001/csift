//! How a record path is spelled, and when a `--file` target names it: the candidate a path
//! stands for, the one lexical spelling every path and target is compared in, and the
//! component-aligned suffix match.

/// The file a matched record path names, as a candidate: a full path as written; a relative
/// one joined to the record's absolute shell cwd (lexically, in the cwd's separator family);
/// the raw string where there is no such cwd, or where the path leads with `$` or `~`, whose
/// expansion the transcript does not record - each [`normalize_path`]ed, so two spellings of
/// one file are one candidate.
pub(crate) fn candidate(path: &str, cwd: Option<&str>) -> String {
    use crate::bash::mutations::{is_absolute_shell_path, join_shell_path};
    if is_full_path(path) || path.starts_with(['$', '~']) {
        return normalize_path(path).into_owned();
    }
    match cwd.filter(|c| is_absolute_shell_path(c)) {
        Some(c) => normalize_path(&join_shell_path(c, &[path])).into_owned(),
        None => normalize_path(path).into_owned(),
    }
}

/// One spelling of a path, LEXICALLY, for the target and every record path alike: `.`
/// segments are dropped and a run of separators collapses to its first character, so
/// `./a/lib.rs` and `a/lib.rs`, or `/p//q` and `/p/q`, compare equal. A LEADING pair (`//`,
/// `\\`) stays - POSIX leaves a leading `//` to the implementation and it is the Windows UNC
/// shape - while three or more leading separators are one. Nothing that needs the filesystem
/// is resolved: a `..` segment stays as written (a symlinked directory makes it name another
/// place), and so does a symlink's other spelling. A relative path joined to its record's cwd
/// has already been through [`join_shell_path`]'s lexical `..` rule, as a Bash operand is.
///
/// A path already in that form - the common case, a tool's own absolute path - comes back
/// borrowed: every record path is compared through it, so the copy is paid only for a path
/// the rule changes.
///
/// [`join_shell_path`]: crate::bash::mutations::join_shell_path
pub(crate) fn normalize_path(p: &str) -> std::borrow::Cow<'_, str> {
    if is_normal_path(p) {
        std::borrow::Cow::Borrowed(p)
    } else {
        std::borrow::Cow::Owned(renormalize_path(p))
    }
}

/// True when [`normalize_path`] leaves `p` as it is: at most two leading separators, no `.`
/// segment, and no run of separators after them (a single trailing one stays, as the rule
/// keeps it).
fn is_normal_path(p: &str) -> bool {
    let is_sep = |c: char| c == '/' || c == '\\';
    let body = p.trim_start_matches(is_sep);
    if p.len() - body.len() > 2 {
        return false;
    }
    let mut segs = body.split(is_sep).peekable();
    while let Some(seg) = segs.next() {
        if seg == "." || (seg.is_empty() && segs.peek().is_some()) {
            return false;
        }
    }
    true
}

/// [`normalize_path`]'s rule, building the new spelling.
fn renormalize_path(p: &str) -> String {
    let is_sep = |c: char| c == '/' || c == '\\';
    let body = p.trim_start_matches(is_sep);
    let lead = &p[..p.len() - body.len()];
    let mut out = match lead.len() {
        0 => String::new(),
        2 => lead.to_string(),
        _ => lead[..1].to_string(),
    };
    let mut rest = body;
    while !rest.is_empty() {
        let end = rest.find(is_sep).unwrap_or(rest.len());
        let (seg, after) = rest.split_at(end);
        let tail = after.trim_start_matches(is_sep);
        if seg != "." {
            out.push_str(seg);
            out.extend(after.chars().next());
        }
        rest = tail;
    }
    if out.is_empty() && !p.is_empty() {
        return ".".to_string();
    }
    out
}

/// The text after a path's last separator (`/` or `\`), the whole path when it has none.
pub(crate) fn last_component(p: &str) -> &str {
    p.rfind(['/', '\\']).map_or(p, |i| &p[i + 1..])
}

/// True when `target` names `path`: an exact raw-string match, or a basename-suffix
/// fallback so a user may pass a short path - component-aligned, so `b.rs` does not match
/// `/x/ab.rs`.
pub(crate) fn names_file(target: &str, path: &str) -> bool {
    if target == path {
        return true;
    }
    path.strip_suffix(target)
        .map(|prefix| prefix.is_empty() || prefix.ends_with(['/', '\\']))
        .unwrap_or(false)
}

/// True for a path that names a file on its own: rooted (`/...`, `\\...`) or
/// drive-qualified (`C:\\...`, `C:/...`).
pub(crate) fn is_full_path(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with(['/', '\\'])
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}
