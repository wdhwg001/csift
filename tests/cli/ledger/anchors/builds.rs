//! Where rule 12 looks for a build, and what a build NAME may be.
//!
//! Two roads, and they answer different questions. The VERIFIED build is the one the badge
//! names, so it is read from `$CSIFT_LEDGER_BINARY` or the native install layout. A hop's
//! OWN build - a Windows artifact, a version whose writer the current one replaced - is read
//! from `$CSIFT_LEDGER_BUILDS/<build>/claude`, else that same layout, and nothing else: a
//! verification cache belongs to the operator who keeps it, so the repository names an
//! environment variable rather than a path.
//!
//! The NAME is checked on any host, because it decides which artifact a hop is read against
//! and a name no build ever had would otherwise be counted `not cached` and pass.

use std::collections::HashMap;

pub(crate) fn build_path(verified: &str) -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("CSIFT_LEDGER_BINARY") {
        if !p.trim().is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    native_build(verified)
}

fn native_build(name: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".local/share/claude/versions")
            .join(name),
    )
}

/// Where a hop's OWN build is looked for, in order: the cache root `$CSIFT_LEDGER_BUILDS`
/// names, then the native install layout. A name is a directory holding one `claude`
/// executable, so a Windows artifact and an older version are addressed alike. No other
/// root is consulted: a verification cache is the operator's, and naming one here would
/// make a shipped test depend on a path that exists on one machine.
pub(crate) fn named_build_paths(name: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(root) = std::env::var("CSIFT_LEDGER_BUILDS") {
        if !root.trim().is_empty() {
            out.push(std::path::PathBuf::from(&root).join(name).join("claude"));
        }
    }
    out.extend(native_build(name));
    out
}

/// Reads each named build at most once. A build this host does not hold is remembered as
/// absent, so its anchors are counted rather than compared and nothing is read twice.
#[derive(Default)]
pub(crate) struct BuildCache {
    builds: HashMap<String, Option<Vec<u8>>>,
}

impl BuildCache {
    pub(crate) fn get(&mut self, name: &str) -> Option<&[u8]> {
        self.builds
            .entry(name.to_string())
            .or_insert_with(|| {
                named_build_paths(name)
                    .iter()
                    .find_map(|p| std::fs::read(p).ok())
            })
            .as_deref()
    }
}

/// A build NAME is a Claude Code version, optionally an artifact of it.
///
/// `9.9.9` is a version triple and is not a Claude Code version: every published one carries
/// a major of 1 or 2, which the prose law already reads. Without the shape a hop could name
/// any string, no host would hold it, the rule would count it `not cached`, and a rotted
/// anchor would pass by typing a build that never existed.
pub(crate) fn build_name_ok(name: &str) -> bool {
    let (version, artifact) = match name.split_once('-') {
        Some((v, rest)) => (v, Some(rest)),
        None => (name, None),
    };
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || !matches!(parts[0], "1" | "2") {
        return false;
    }
    if !parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    match artifact {
        None => true,
        // `<platform>-<arch>`, exactly two lowercase words.
        Some(rest) => {
            let words: Vec<&str> = rest.split('-').collect();
            words.len() == 2
                && words.iter().all(|w| {
                    !w.is_empty()
                        && w.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
        }
    }
}
