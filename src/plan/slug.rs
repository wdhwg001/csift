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
    let mut candidates: Vec<PathBuf> = vec![home.join("settings.json")];
    candidates.push(root.join(".claude").join("settings.json"));
    candidates.push(root.join(".claude").join("settings.local.json"));
    let mut value: Option<String> = None;
    for p in candidates {
        let Ok(raw) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if let Some(d) = v.get("plansDirectory").and_then(serde_json::Value::as_str) {
            value = Some(d.to_string()); // later (more specific) scopes win
        }
    }
    let Some(d) = value else {
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
