//! The first-slug binding law: slug validity, the first slug-carrying record, and
//! the `plansDirectory` resolution the harness applies to it.

use super::*;

/// The FIRST record carrying a `slug` field (Claude Code's binding key), by a
/// sequential early-exit walk - the fallback runs only when no `plan_mode` exists, and
/// a slugged session's first carrier normally sits early after the mint point.
pub(crate) fn first_slug_record(bytes: &[u8]) -> Option<(usize, crate::model::Record)> {
    static SLUG: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"slug\""));
    let mut line_no = 0usize;
    for line in bytes.split(|&b| b == b'\n') {
        line_no += 1;
        if SLUG.find(line).is_none() {
            continue;
        }
        if let Ok(Some(rec)) = crate::parse::parse_line(line) {
            if rec.slug.is_some() {
                return Some((line_no, rec));
            }
        }
    }
    None
}

/// Claude Code's slug validity rule (lowercase alnum head, alnum/dash tail, <=120
/// chars) - a stray tolerated `slug` value that CC itself would reject never binds.
pub(crate) fn slug_is_valid(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(head) = chars.next() else {
        return false;
    };
    s.len() <= 120
        && (head.is_ascii_lowercase() || head.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The plans directory, by Claude Code's own rule (binary 2.1.258, re-verified for
/// v0.10.1): `plansDirectory` is read from the MERGED settings (`<project>/.claude/
/// settings.local.json` over `<project>/.claude/settings.json` over
/// `<claude-home>/settings.json`), resolved AGAINST THE PROJECT ROOT (the session's
/// cwd), and must stay CONTAINED in that root - a value that escapes it (`../x`, an
/// absolute path elsewhere) is refused and Claude Code falls back to
/// `<claude-home>/plans`. csift mirrors the lexical containment check (the harness
/// additionally walks symlinks of the nearest existing ancestor - unmodeled, a
/// symlinked plans dir that escapes only after resolution is the one divergence).
/// The transcript's FIRST recorded `cwd`: the harness's project root for the session -
/// the value the head records are stamped with before any Bash `cd` moves the
/// tracked shell cwd (section 3.11). The slug-carrying record's own cwd can already
/// have drifted into a subdirectory, and plansDirectory resolves against the root,
/// not the shell's position (v0.10.2). A bounded head walk; `None` when no head
/// record carries a cwd. A session resumed from another directory re-resolves against
/// that directory in the harness, which this instrument cannot see.
pub(crate) fn first_cwd(bytes: &[u8]) -> Option<String> {
    let mut pos = 0usize;
    let mut seen = 0usize;
    while pos < bytes.len() && seen < 256 {
        let end = memchr::memchr(b'\n', &bytes[pos..]).map_or(bytes.len(), |i| pos + i);
        let line = &bytes[pos..end];
        pos = end + 1;
        seen += 1;
        if let Ok(Some(rec)) = crate::parse::parse_line(line) {
            if let Some(c) = rec.cwd.as_deref().filter(|c| !c.is_empty()) {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// The three FILE scopes csift reads for the plan binding, in Claude Code's own order
/// (later wins). The flag scope and the policy tier both outrank them in the harness, but
/// the flag scope is per-invocation and unobservable, so binding on the policy tier alone
/// would make the answer depend on half of a cascade csift cannot see.
const PLAN_SCOPES: &[&str] = &[
    crate::path::settings::SCOPE_USER,
    crate::path::settings::SCOPE_PROJECT,
    crate::path::settings::SCOPE_LOCAL,
];

/// With no project root known (a record without `cwd`) a set value cannot be
/// resolved and the default applies.
pub(crate) fn plans_dir(project_root: Option<&Path>) -> PathBuf {
    let Ok(home) = crate::path::claude_home() else {
        return PathBuf::from("plans");
    };
    let default = home.join("plans");
    let Some(root) = project_root else {
        return default;
    };
    let merged = crate::path::settings::merged(&home, Some(root));
    let Some((d, _scope)) =
        crate::path::settings::string_value_in(&merged, "plansDirectory", PLAN_SCOPES)
    else {
        return default;
    };
    let joined = crate::path::lexical_normalize(&root.join(d));
    let root_n = crate::path::lexical_normalize(root);
    if joined.starts_with(&root_n) {
        joined
    } else {
        default
    }
}
