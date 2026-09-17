//! One session id under two project directories: the relation test, the winner rule, and
//! the report every surface discloses.
//!
//! A transcript's basename IS its session id, and Claude Code's own resume lookup keys on
//! `<uuid>.jsonl` PER project directory - it walks every project dir and returns nothing
//! when a second one also holds messages, so the harness treats the shape as real and as
//! ambiguous without resolving it. csift meets the shape on disk whenever a project is
//! renamed and its directory copied, and two harness doors mint the same shape as well:
//! the project-dir-name override, whose enumeration deliberately searches BOTH the
//! overridden and the natural directory, and a transcript relocation whose fallback mover
//! can strand its source.
//!
//! The merge is decided by the BYTES, never by a name, an mtime or a birth time. Two
//! copies that are identical, or one that is a strict byte PREFIX of the other (what
//! copying a file that is still being appended to produces), are the same session seen
//! twice and collapse onto the longest. Two copies that disagree BEFORE the shorter one
//! ends are not the same session seen twice, so csift merges neither: silently picking
//! one would destroy the records only the other carries, and a turn number minted from
//! one does not address the other.

use super::*;

/// How the copies of one session id relate, byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateRelation {
    /// Equal sizes and no differing byte: the same file seen twice.
    Identical,
    /// No differing byte up to the shorter copy's end, and the sizes differ: the shorter
    /// copy is a strict byte prefix of the longer one.
    Prefix,
    /// A differing byte before the shorter copy's end - two files claiming one session id
    /// with contradictory content. NEVER merged.
    Divergent,
    /// At least one copy could not be READ, so there are no bytes to compare it with.
    /// NEVER merged: a read failure is a GAP in the evidence, not evidence of sameness,
    /// and an unread copy that merged would be announced as dropped on the strength of a
    /// comparison that never happened.
    Unknown,
}

impl DuplicateRelation {
    /// The machine token a JSON consumer keys on.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            DuplicateRelation::Identical => "identical",
            DuplicateRelation::Prefix => "prefix",
            DuplicateRelation::Divergent => "divergent",
            DuplicateRelation::Unknown => "unknown",
        }
    }

    /// Whether this relation COLLAPSES the group onto one copy. The one rule: the
    /// resolver's drop set and the `list` row's fate line both ask here, so a relation
    /// that merges nothing can never be reported as having dropped something.
    #[must_use]
    pub fn merges(self) -> bool {
        matches!(
            self,
            DuplicateRelation::Identical | DuplicateRelation::Prefix
        )
    }
}

/// One copy of a duplicated session id.
#[derive(Debug, Clone)]
pub struct DuplicateMember {
    pub path: PathBuf,
    /// Physical jsonl lines (newlines, plus a torn final fragment) - the same domain
    /// `show --line` addresses, and the unit every disclosure of a dropped copy counts in.
    /// 0 for a copy csift could not read, whose `error` is the fact reported in its place;
    /// such a group merges nothing, so that 0 is never summed into a disclosure.
    pub lines: usize,
    /// Why this copy could not be read, when it could not. A member carrying one makes the
    /// whole group [`DuplicateRelation::Unknown`].
    pub error: Option<String>,
}

/// One session id resolved under more than one project directory.
#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    /// The shared basename - the session id itself.
    pub session_id: String,
    pub relation: DuplicateRelation,
    /// The copy kept in scope. For a group that merges NOTHING (divergent, or unknown
    /// because a copy could not be read) every member is kept and this names the longest
    /// one only, so the disclosure has a subject.
    pub kept: PathBuf,
    /// The copies the merge removed from the scope. EMPTY for every relation that does not
    /// merge.
    pub dropped: Vec<PathBuf>,
    /// Every copy, longest first, with its size and line count.
    pub members: Vec<DuplicateMember>,
}

impl DuplicateGroup {
    /// Physical jsonl lines carried by the copies the merge removed (0 for a divergent
    /// group, which drops nothing).
    #[must_use]
    pub fn dropped_lines(&self) -> usize {
        self.members
            .iter()
            .filter(|m| self.dropped.contains(&m.path))
            .map(|m| m.lines)
            .sum()
    }
}

/// Collapse the resolved TOP-LEVEL files that share a session id, returning the kept set
/// plus one report per duplicated id. Files whose basename occurs once pass through
/// untouched, so an ordinary scope pays one map build and no file read.
///
/// The kept set preserves nothing about input order (the caller sorts), and a divergent
/// group contributes every one of its members.
pub fn dedup_top_level(files: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<DuplicateGroup>) {
    let mut by_stem: std::collections::BTreeMap<String, Vec<PathBuf>> =
        std::collections::BTreeMap::new();
    for f in files {
        let stem = f
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        by_stem.entry(stem).or_default().push(f);
    }
    let mut kept: Vec<PathBuf> = Vec::new();
    let mut groups: Vec<DuplicateGroup> = Vec::new();
    for (stem, paths) in by_stem {
        if paths.len() < 2 {
            kept.extend(paths);
            continue;
        }
        let group = classify_group(&stem, paths);
        kept.push(group.kept.clone());
        for m in &group.members {
            if m.path != group.kept && !group.dropped.contains(&m.path) {
                kept.push(m.path.clone());
            }
        }
        groups.push(group);
    }
    (kept, groups)
}

/// Build the report for ONE duplicated session id: order the copies (longest first), read
/// each one's bytes once, compare every other copy against the longest, and derive the
/// group's relation from the worst pairwise verdict - unless a copy could not be READ at
/// all, which answers UNKNOWN before any comparison and merges nothing.
fn classify_group(stem: &str, paths: Vec<PathBuf>) -> DuplicateGroup {
    let ordered = order_members(paths);
    // mmap, never a read-into-memory copy: a duplicated transcript runs to tens of
    // megabytes and both copies are compared byte for byte.
    //
    // A read FAILURE is kept as the error it returned rather than folded into an empty
    // map: an unreadable copy read as `&[]` is an empty one, which compares as a prefix
    // of every other copy and - being larger by metadata - wins the order, so the merge
    // would announce the READABLE copy as dropped and the command would then die on the
    // one it kept. An `Ok(None)` is different and stays: that is a genuinely EMPTY file,
    // read successfully, and 0 bytes is the honest answer for it.
    let maps: Vec<_> = ordered
        .iter()
        // The ROOT cause rather than the context chain: every sentence in that chain
        // names this same path, and the disclosure prints the path itself beside it.
        .map(|p| crate::parse::mmap_bytes(p).map_err(|e| e.root_cause().to_string()))
        .collect();
    let bytes: Vec<&[u8]> = maps
        .iter()
        .map(|m| m.as_ref().map_or(&[][..], |b| b.as_deref().unwrap_or(&[])))
        .collect();
    let members: Vec<DuplicateMember> = ordered
        .into_iter()
        .zip(maps.iter().zip(&bytes))
        .map(|(path, (map, data))| DuplicateMember {
            path,
            // An unreadable copy has no bytes to count, so this is the `&[]` zero its own
            // `error` accounts for rather than a measurement of the file.
            lines: line_count(data),
            error: map.as_ref().err().cloned(),
        })
        .collect();
    let relation = if members.iter().any(|m| m.error.is_some()) {
        DuplicateRelation::Unknown
    } else {
        classify_bytes(&bytes)
    };
    let kept = members[0].path.clone();
    let dropped = if relation.merges() {
        members[1..].iter().map(|m| m.path.clone()).collect()
    } else {
        Vec::new()
    };
    DuplicateGroup {
        session_id: stem.to_string(),
        relation,
        kept,
        dropped,
        members,
    }
}

/// The byte verdict over copies that were ALL read: compare every other copy against the
/// first (the longest) and take the WORST pairwise verdict - one divergent copy makes the
/// whole group unmergeable, and one short copy makes it a prefix group.
fn classify_bytes(bytes: &[&[u8]]) -> DuplicateRelation {
    let winner = bytes[0];
    let mut relation = DuplicateRelation::Identical;
    for other in &bytes[1..] {
        let n = other.len().min(winner.len());
        let verdict = if other[..n] != winner[..n] {
            DuplicateRelation::Divergent
        } else if other.len() == winner.len() {
            DuplicateRelation::Identical
        } else {
            DuplicateRelation::Prefix
        };
        relation = match (relation, verdict) {
            (DuplicateRelation::Divergent, _) | (_, DuplicateRelation::Divergent) => {
                DuplicateRelation::Divergent
            }
            (DuplicateRelation::Prefix, _) | (_, DuplicateRelation::Prefix) => {
                DuplicateRelation::Prefix
            }
            _ => DuplicateRelation::Identical,
        };
    }
    relation
}

/// Order the copies so the WINNER is first: longest wins, because a shorter copy that is a
/// prefix is a copy of a file still being appended to and choosing it would silently drop
/// the records only the longer one carries. An mtime never enters the rule - a bulk
/// preserving operation re-applies mtimes and synthesizes birth times, so neither instant
/// is evidence about which copy the harness wrote.
///
/// Equal sizes are broken by the copy whose DIRECTORY basename equals the encoding of the
/// first record that carries a `cwd` (the value the directory key was derived from at
/// write time), then by the lexicographically smallest path, so the answer is the same on
/// every run and every machine.
fn order_members(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    let mut keyed: Vec<(u64, bool, PathBuf)> = paths
        .into_iter()
        .map(|p| {
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            (size, false, p)
        })
        .collect();
    let by_size = |a: &(u64, bool, PathBuf), b: &(u64, bool, PathBuf)| {
        b.0.cmp(&a.0) // longest first
            .then(b.1.cmp(&a.1)) // then the copy whose dir name encodes the recorded cwd
            .then(a.2.cmp(&b.2)) // then deterministically by path
    };
    keyed.sort_by(by_size);
    // The cwd probe is a head READ, so it runs only when a size tie actually needs
    // breaking: a prefix group is decided by size alone and pays nothing for it.
    if keyed.windows(2).any(|w| w[0].0 == w[1].0) {
        for k in &mut keyed {
            k.1 = dir_matches_first_cwd(&k.2);
        }
        keyed.sort_by(by_size);
    }
    keyed.into_iter().map(|(_, _, p)| p).collect()
}

/// True when this copy's project-directory basename equals the encoding of the FIRST
/// record that carries a `cwd`.
///
/// Deliberately NOT [`read_first_cwd`], which reads physical line 1 only: line 1 of a real
/// transcript is routinely a session-state line with no `cwd` at all, and this tie-break
/// wants the session's own opening cwd. The two reads stay separate because widening the
/// line-1 one would change which files the cwd collision guard admits, which is a
/// different decision with its own consequences.
fn dir_matches_first_cwd(path: &Path) -> bool {
    let Some(cwd) = first_record_cwd(path) else {
        return false;
    };
    let Some(dir) = path
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
    else {
        return false;
    };
    encode_cwd(Path::new(&cwd)) == dir
}

/// How much of a transcript's head the first-`cwd` probe reads. A `cwd` sits in the first
/// records' first few hundred bytes; the window is generous enough to step over a huge
/// leading line (an image record) and bounded so the probe is never a file read.
const FIRST_CWD_WINDOW: u64 = 256 * 1024;

/// The `cwd` of the first record in the head window that carries one - the session's
/// opening cwd. `None` when the window holds no such record.
fn first_record_cwd(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(FIRST_CWD_WINDOW)
        .read_to_end(&mut buf)
        .ok()?;
    let head = String::from_utf8_lossy(&buf);
    // The last fragment may be cut by the window; a partial `"cwd":"...` there simply does
    // not parse, which is the honest answer for a record the probe could not see whole.
    head.split('\n')
        .find_map(|l| extract_json_string_field(l, "cwd"))
}

/// Physical jsonl lines: newline count plus a torn final fragment - the domain
/// `show --line` addresses.
fn line_count(bytes: &[u8]) -> usize {
    memchr::memchr_iter(b'\n', bytes).count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"))
}

/// Announce every duplicated session id on STDERR, unconditionally, at the moment the
/// resolver decides it - the same posture as the `@main` lane note, and for the same
/// reason: csift is making an inference the caller cannot see, so it says so rather than
/// quietly returning one file where the disk holds two. Stdout stays pure.
pub fn emit_duplicate_notes(groups: &[DuplicateGroup]) {
    for g in groups {
        eprintln!("csift: note: {}", duplicate_note(g));
    }
}

/// `one`/`many` by count - the disclosure names small numbers of copies and lines, so a
/// bare `{n} lines` reads as a machine's output on the one count a reader meets most.
fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

/// The one wording of a duplicate disclosure, shared by the stderr note and the text
/// footers so the two can never drift.
#[must_use]
pub fn duplicate_note(g: &DuplicateGroup) -> String {
    let n = g.members.len();
    let id = g.session_id.get(..8).unwrap_or(&g.session_id);
    match g.relation {
        DuplicateRelation::Unknown => {
            let unreadable: Vec<String> = g
                .members
                .iter()
                .filter_map(|m| {
                    m.error
                        .as_ref()
                        .map(|e| format!("{}: {e}", m.path.display()))
                })
                .collect();
            format!(
                "session {id} is on disk under {n} project directories and csift could not \
                 read {} of them ({}) - with no bytes to compare, csift merges nothing, so \
                 every copy stays in scope and every count below includes all of them. Fix \
                 that copy's permissions, or target a `*.jsonl` path to read one alone.",
                unreadable.len(),
                unreadable.join("; ")
            )
        }
        DuplicateRelation::Divergent => {
            let counts: Vec<String> = g
                .members
                .iter()
                .map(|m| {
                    format!(
                        "{} {} at {}",
                        m.lines,
                        plural(m.lines, "line", "lines"),
                        m.path.display()
                    )
                })
                .collect();
            format!(
                "session {id} is on disk under {n} project directories whose contents DIFFER \
                 ({}) - csift merges neither, so both stay in scope and every count below \
                 includes both. A turn number minted from one copy does not address the \
                 other; target a `*.jsonl` path to read one alone.",
                counts.join(" vs ")
            )
        }
        rel => {
            let dropped: Vec<String> = g.dropped.iter().map(|p| p.display().to_string()).collect();
            let how = if rel == DuplicateRelation::Identical {
                "byte-identical copies"
            } else {
                "one a strict byte PREFIX of the other - a copy taken while the session was \
                 still being written"
            };
            format!(
                "session {id} is on disk under {n} project directories ({how}); reading {} \
                 ({} {}) and dropping {} {} ({} {}) at {}. Target that path directly to read \
                 the dropped copy instead.",
                g.kept.display(),
                g.members[0].lines,
                plural(g.members[0].lines, "line", "lines"),
                g.dropped.len(),
                plural(g.dropped.len(), "copy", "copies"),
                g.dropped_lines(),
                plural(g.dropped_lines(), "line", "lines"),
                dropped.join(", ")
            )
        }
    }
}

/// The shared TEXT disclosure, one line per duplicated session id, printed beside the scope
/// banner rather than as a trailing footer: the merge is a fact about the resolved SCOPE,
/// like the span line it sits under, and the banner is the one place every spanning surface
/// already discloses that - always reached, even on an empty result. Silent when nothing
/// was duplicated, and never called from the `-c` / `-l` terminal modes, whose stdout is a
/// bare number or a pure id stream (the resolver's stderr note carries those).
pub fn emit_duplicate_banner(groups: &[DuplicateGroup]) {
    for g in groups {
        println!("copies {}", duplicate_note(g));
        println!();
    }
}

/// The shared JSON disclosure every spanning surface carries in its envelope header:
/// how many transcripts the merge removed from the scope, and how many physical jsonl
/// lines went with them. Both 0 when no session id was duplicated.
#[must_use]
pub fn duplicate_header_fields(groups: &[DuplicateGroup]) -> serde_json::Value {
    serde_json::json!({
        "duplicate_transcripts": groups.iter().map(|g| g.dropped.len()).sum::<usize>(),
        "duplicate_records_dropped": groups.iter().map(DuplicateGroup::dropped_lines).sum::<usize>(),
    })
}
