//! The words of a duplicate disclosure: the note for a merged, divergent or unreadable
//! group, where its copies sit, and the note for a `*.jsonl` target whose id has other
//! transcripts in scope. Pure strings - the stderr and stdout sinks stay beside the merge in
//! the parent, which is where every print of this module's text happens.

use super::*;

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
    let subject = g.subject();
    let places = places(g);
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
                "{subject} is on disk {places} and csift could not \
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
                "{subject} is on disk {places} whose contents DIFFER \
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
                "{subject} is on disk {places} ({how}); keeping {} \
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

/// Where a group's copies sit, in words a reader can check against the paths the note
/// prints. The ordinary shape is one copy per project directory. One agent id can also sit at
/// two paths inside ONE session's sidecar - a workflow run's `subagents/workflows/<run>/` and
/// the built-in `subagents/` beside it (PATH-017) - and calling that "two project directories"
/// sends a reader looking for a directory that is not there.
fn places(g: &DuplicateGroup) -> String {
    let n = g.members.len();
    let sidecars: std::collections::BTreeSet<Option<&Path>> =
        g.members.iter().map(|m| sidecar_of(&m.path)).collect();
    if let [Some(_)] = sidecars.iter().copied().collect::<Vec<_>>().as_slice() {
        let workflow = g.members.iter().any(|m| is_workflow_path(&m.path));
        let builtin = g.members.iter().any(|m| !is_workflow_path(&m.path));
        return if workflow && builtin {
            "under one session's sidecar at the built-in and the workflow locations".to_string()
        } else {
            format!("at {n} paths under one session's sidecar")
        };
    }
    let dirs: std::collections::BTreeSet<&Path> = g
        .members
        .iter()
        .filter_map(|m| project_dir_of(&m.path))
        .collect();
    match dirs.len() {
        d if d == n => format!("under {n} project directories"),
        d => format!(
            "at {n} paths under {d} {}",
            plural(d, "project directory", "project directories")
        ),
    }
}

/// The `<project>/<uuid>` directory whose `subagents/` holds a subagent transcript; `None` for
/// a top-level transcript, which belongs to no sidecar.
fn sidecar_of(p: &Path) -> Option<&Path> {
    p.ancestors()
        .find(|a| a.file_name().and_then(|n| n.to_str()) == Some("subagents"))?
        .parent()
}

/// A transcript under a workflow run's own directory rather than at the built-in location.
fn is_workflow_path(p: &Path) -> bool {
    p.components()
        .any(|c| c.as_os_str().to_str() == Some("workflows"))
}

/// The project directory a transcript belongs to: a top-level `<uuid>.jsonl` sits directly in
/// it, a subagent transcript under `<project>/<uuid>/subagents/`.
fn project_dir_of(p: &Path) -> Option<&Path> {
    match sidecar_of(p) {
        Some(sidecar) => sidecar.parent(),
        None => p.parent(),
    }
}

/// The note for a `*.jsonl` target whose agent id has other transcripts in scope: the path
/// reads only the file it names, and the reader learns what else is on disk.
#[must_use]
pub fn named_copy_note(named: &Path, others: &[PathBuf]) -> String {
    let lines = |p: &Path| {
        crate::parse::mmap_bytes(p)
            .ok()
            .and_then(|m| m.as_deref().map(line_count))
            .unwrap_or(0)
    };
    let listed: Vec<String> = others
        .iter()
        .map(|p| {
            let n = lines(p);
            format!("{n} {} at {}", plural(n, "line", "lines"), p.display())
        })
        .collect();
    let own = lines(named);
    format!(
        "subagent {} has {} other {} in scope ({}); a `*.jsonl` target reads only the file it \
         names ({own} {} at {}) - `@<id>` reads every one.",
        crate::subagent::session_id_from_path(named),
        others.len(),
        plural(others.len(), "transcript", "transcripts"),
        listed.join(", "),
        plural(own, "line", "lines"),
        named.display()
    )
}
