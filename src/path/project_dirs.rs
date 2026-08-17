//! Project-dir resolution: encoded tokens, >200-char prefix scan, cwd probes.

use super::*;

/// Resolve a user-supplied target (actual cwd OR a pre-encoded dir, §2.3) to a
/// concrete `<encoded>` directory under `~/.claude/projects`. Errors (never an
/// empty silent result) when neither interpretation resolves to a directory.
pub fn resolve_target(target: &Path) -> Result<ProjectDir> {
    let root = projects_root()?;

    // §2.3 step 1: pre-encoded projects-dir token (bare or under the root).
    if let Some(token) = strip_projects_root_prefix(target, &root) {
        let dir = root.join(&token);
        if dir.is_dir() {
            // An EXPLICIT encoded-dir token: the user named the dir, so don't cwd-filter
            // (a collision is the user's chosen scope here).
            return Ok(ProjectDir {
                dir,
                target_cwd: None,
            });
        }
        // A leading-`-` token that doesn't exist as a dir: don't silently fall
        // through to path-encoding (it can't be a real absolute path anyway). A
        // WINDOWS-shaped token (`C--…`) CAN also be a real relative path, so that
        // shape falls through to the real-path interpretation below instead.
        if token.starts_with('-') {
            bail!(
                "no Claude Code project dir named {:?} under {}",
                token,
                root.display()
            );
        }
    }

    // §2.3 step 2: treat as a real filesystem path — absolutize + encode + look up.
    let abs = absolutize(target)?;
    let encoded = encode_cwd(&abs);
    let dir = root.join(&encoded);
    if dir.is_dir() {
        return Ok(ProjectDir {
            dir,
            target_cwd: Some(abs),
        });
    }

    // §2.3 step 3: LONG-PATH fallback. Claude Code caps the encoded dir name at
    // MAX_SANITIZED_LENGTH (200) chars and appends a hash suffix for anything longer —
    // `<first-200>-<hash>` — so the full encoding above does not exist on disk for a
    // deeply-nested project. The suffix is NOT reconstructible (the CLI uses Bun.hash,
    // the SDK djb2 — different digests for the same path), which is why CC's own
    // `findProjectDir` PREFIX-SCANS rather than recomputing it. We mirror that exactly.
    if encoded.len() > MAX_SANITIZED_LENGTH {
        let prefix = format!("{}-", &encoded[..MAX_SANITIZED_LENGTH]);
        if let Some(found) = find_dir_by_prefix(&root, &prefix, &abs)? {
            return Ok(ProjectDir {
                dir: found,
                target_cwd: Some(abs.clone()),
            });
        }
    }

    // §2.3 step 4: neither resolved — surface the attempted path, no empty result.
    bail!(
        "no Claude Code project dir for {} (looked for {})",
        abs.display(),
        dir.display()
    )
}

/// Claude Code's `MAX_SANITIZED_LENGTH`: a project's encoded dir-name is capped at 200
/// chars; a longer cwd is stored as `<first-200>-<hash>` (§2.1). Matches the cleanroom
/// `sanitizePath` + the shipping binary (`Siq`, verified 2026-06-16).
pub(crate) const MAX_SANITIZED_LENGTH: usize = 200;

/// Resolve a >200-char encoded path to its on-disk dir by prefix-scanning the projects
/// root for `<first-200>-<hash>` (the hash is not reconstructible — see [`resolve_target`]).
/// Among multiple matches (two paths identical for the first 200 encoded chars — vanishingly
/// rare), prefer the dir whose first session's recorded `cwd` equals the target; otherwise
/// fall back to the sole / first match. Returns `None` when nothing matches.
pub(crate) fn find_dir_by_prefix(root: &Path, prefix: &str, abs: &Path) -> Result<Option<PathBuf>> {
    let read = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let mut matches: Vec<PathBuf> = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(prefix) && entry.path().is_dir() {
            matches.push(entry.path());
        }
    }
    if matches.len() > 1 {
        let want = abs.to_string_lossy();
        if let Some(exact) = matches
            .iter()
            .find(|d| dir_first_cwd(d).as_deref() == Some(want.as_ref()))
        {
            return Ok(Some(exact.clone()));
        }
    }
    matches.sort();
    Ok(matches.into_iter().next())
}

/// Cheaply read the `cwd` of ONE session file from its first record — a BOUNDED head read
/// (≤64 KiB; the `cwd` field sits in the first record's first ~200 bytes), so a first line
/// that is huge (an image record on line 1) never forces a full-line load. No full JSON
/// parse — mirrors CC's `extractJsonStringField`.
pub(crate) fn read_first_cwd(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    let head = String::from_utf8_lossy(&buf);
    // Only the FIRST line's cwd (don't pick up a later record's if the head spans a newline).
    let first_line = head.split('\n').next().unwrap_or(&head);
    extract_json_string_field(first_line, "cwd")
}

/// The `cwd` string of a project dir's first session file (any one). Used to disambiguate a
/// 200-char-prefix collision among long-path candidates.
pub(crate) fn dir_first_cwd(dir: &Path) -> Option<String> {
    let read = std::fs::read_dir(dir).ok()?;
    for entry in read.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "jsonl") {
            if let Some(cwd) = read_first_cwd(&p) {
                return Some(cwd);
            }
        }
    }
    None
}

/// Whether a stored `cwd` string denotes the same directory as `want` (the canonical
/// target). Trailing-slash tolerant. Exact for the ASCII paths that dominate; a non-ASCII
/// NFC-vs-NFD mismatch is the one accepted edge (CC stores realpath+NFC, csift realpath only).
pub(crate) fn cwd_equivalent(stored: &str, want: &Path) -> bool {
    let norm = |s: &str| s.trim_end_matches('/').to_string();
    norm(stored) == norm(&want.to_string_lossy())
}

/// Extract a simple `"key":"value"` JSON string field from raw text without a full parse
/// (escape-aware; stops at the first unescaped `"`). Mirrors CC's `extractJsonStringField`.
pub(crate) fn extract_json_string_field(text: &str, key: &str) -> Option<String> {
    for pat in [format!("\"{key}\":\""), format!("\"{key}\": \"")] {
        let Some(idx) = text.find(&pat) else { continue };
        let bytes = text.as_bytes();
        let start = idx + pat.len();
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => return Some(text[start..i].replace("\\\\", "\\").replace("\\\"", "\"")),
                _ => i += 1,
            }
        }
    }
    None
}

/// Enumerate every project directory directly under `~/.claude/projects`.
/// Returns only entries that are directories (ignores stray files). Order is
/// sorted by basename for deterministic output.
pub fn all_project_dirs() -> Result<Vec<ProjectDir>> {
    let root = projects_root()?;
    let read = std::fs::read_dir(&root)
        .with_context(|| format!("cannot read projects root {}", root.display()))?;
    let mut dirs = Vec::new();
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", root.display()))?;
        let path = entry.path();
        // file_type() avoids an extra stat where the OS already knows; fall back
        // to is_dir() (follows symlinks) when the type is unavailable.
        let is_dir = match entry.file_type() {
            Ok(ft) if ft.is_symlink() => path.is_dir(),
            Ok(ft) => ft.is_dir(),
            Err(_) => path.is_dir(),
        };
        if is_dir {
            dirs.push(ProjectDir {
                dir: path,
                target_cwd: None,
            });
        }
    }
    dirs.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(dirs)
}
