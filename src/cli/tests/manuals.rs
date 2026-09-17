//! The REPO DOCS as a surface: what `SKILL.md` and its siblings must say about the
//! binary, checked against the binary.
//!
//! Split from `docs.rs`, which guards the RENDERED HELP. The two answer different
//! questions and fail for different reasons: a help guard breaks when a flag or a
//! table moves, and these break when a markdown file drifts from the code it
//! describes. They share only the word "docs".

#[allow(unused_imports)]
use super::*;

/// SKILL.md's surface stamp must match the crate version - forces the LLM-facing
/// skill to be (at least) OPENED on every release, so the "re-read this file on an
/// unexpected error" recovery path always lands on current truth.
#[test]
fn skill_stamp_matches_crate_version() {
    let skill = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"))
        .expect("SKILL.md at the repo root");
    let want = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert!(
        skill.contains(&want),
        "SKILL.md must carry the surface stamp `{want}` (found none) — update the \
             stamp (and the surface docs) with every version bump"
    );
}

/// The four repo docs must never resurrect a token this cleanup removed. SPEC's
/// `>`-quoted change LEDGER lines are exempt (they record the renames themselves).
#[test]
fn docs_do_not_resurrect_removed_tokens() {
    let deny: &[&str] = &[
        "--include-subagents",
        "agents --kind",
        "--by-file",
        "--line A-B",
        "category=",
    ];
    for doc in ["SKILL.md", "AGENTS.md", "SPEC.md", "README.md"] {
        let path = format!("{}/{doc}", env!("CARGO_MANIFEST_DIR"));
        let body = std::fs::read_to_string(&path).expect("repo doc");
        for (i, line) in body.lines().enumerate() {
            if line.starts_with('>') {
                continue; // SPEC change-ledger lines legitimately name old spellings
            }
            for tok in deny {
                assert!(
                    !line.contains(tok),
                    "{doc}:{} resurrects the removed token `{tok}`: {line}",
                    i + 1
                );
            }
        }
    }
}

/// SKILL.md section 10 is the ONE address a stale caller lands on, so every retired
/// selector has a ROW there naming the successors that selector's own error prints.
///
/// The error text and the manual are two copies of one mapping, written for two readers,
/// and nothing held them together: section 10 sent a reader from
/// `harness.schedule.continuation` to the four schedule leaves while the binary says
/// `harness.resume.prompt` (the record comes from the resume LOADER, never the
/// scheduler), listed seven `harness.meta` successors where the table has ten, and mapped
/// `tool` to `agent.tool.use` where the table says the `agent.tool` CATEGORY. Three of
/// five rows wrong, and every one of them sends a caller to a selector that does not
/// carry their records.
///
/// PER ROW, not per section, and that is the whole strength of it: successor paths repeat
/// across rows (`harness.schedule.tick` is its own row's answer AND one of the ten
/// `harness.meta` splits), so a section-wide containment check passes on a row that names
/// the WRONG successor as long as some other row happens to name the right one. Mutating
/// the `harness.meta.loop` row to `harness.schedule.WRONG` left a section-wide version of
/// this test green; the per-row form fails it.
///
/// The successor check is per TOKEN rather than on the whole string - the table joins its
/// successors with ` - ` and parenthesises a gloss, while the manual sets each in
/// backticks and may reword the gloss for a reader, so pinning the joined string would
/// pin the wording rather than the mapping.
#[test]
fn skill_section_10_names_every_retired_selector_and_its_successors() {
    let skill = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"))
        .expect("SKILL.md at the repo root");
    let start = skill
        .find("## 10. Retired spellings")
        .expect("SKILL.md carries the retirement section");
    let end = skill[start..]
        .find("\n## 11.")
        .map_or(skill.len(), |i| start + i);
    let section = &skill[start..end];

    // `| `<typed>` | <what happened to it> |` - the row is the unit, because a successor
    // path can appear in more than one row.
    let row_for = |selector: &str| -> Option<&str> {
        section.lines().find_map(|l| {
            let rest = l.strip_prefix("| `")?;
            let (typed, tail) = rest.split_once('`')?;
            (typed == selector).then_some(tail)
        })
    };

    for (selector, _era, successors) in crate::cli::selectors::RETIRED_SELECTORS {
        let row = row_for(selector)
            .unwrap_or_else(|| panic!("SKILL section 10 has no row for retired `{selector}`"));
        // A successor entry is `path` or `path (a gloss)`; the PATH is what a reader
        // retypes, so that is what has to be in THIS row.
        for entry in successors.split(" \u{b7} ") {
            let path = entry.split(" (").next().unwrap_or(entry).trim();
            assert!(
                row.contains(path),
                "SKILL section 10's `{selector}` row does not name its successor {path}\n  row: {row}"
            );
        }
    }
    // The section is the WHOLE map, so a reader meets no selector it cannot place: every
    // backticked `harness.meta...` path in a row head is one the table actually retires.
    for line in section.lines().filter(|l| l.starts_with("| `harness.meta")) {
        let typed = line
            .trim_start_matches("| `")
            .split('`')
            .next()
            .unwrap_or_default();
        assert!(
            crate::cli::selectors::RETIRED_SELECTORS
                .iter()
                .any(|(s, _, _)| *s == typed),
            "SKILL section 10 retires `{typed}`, which the table does not"
        );
    }
}
