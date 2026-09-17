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

/// The root help names EVERY `floor_cause` slug, and no token shaped like one that is not.
///
/// The eight slugs are a closed enum a consumer branches on, and they live in three
/// places: [`FloorCause`], the global `--cross-compactions` doc (rendered on all eighteen
/// help pages) and `SKILL.md` section 9.2. Nothing held the three together: the help
/// carried the KEY on seven pages and the value domain on none, which is how a reader
/// ends up guessing at a field the wire hands them.
///
/// BOTH DIRECTIONS, because they are two different failures. A slug the help OMITS is a
/// value a reader meets on the wire and cannot look up. A dashed token the help sets in
/// backticks that is NO slug is a value a reader will look for and never see - the shape
/// a rename leaves behind, since an old spelling reads exactly like a real one.
///
/// The extraction reads BACKTICKED tokens rather than sweeping for substrings: the page
/// is prose about boundaries and budgets, so `head` and `cycle` occur as ordinary English
/// all over it and a substring reading would call this green on a page naming none of
/// them as VALUES.
///
/// The reverse half is scoped to the `--cross-compactions` BLOCK rather than to the whole
/// page, and reads tokens of the SLUG GRAMMAR (lowercase words joined by dashes, which is
/// what every [`FloorCause::as_str`] is). Both narrowings are forced by the page rather
/// than chosen: a grammar-free sweep would demand that every flag and every `@`-target be
/// a floor cause, and a page-wide one would demand it of every other enum's values.
///
/// Inside that block TWO closed enums are documented, because the flag moves records
/// between them: a record above the floor stays `pre-cut`, which is a [`Survival`]. So
/// the reverse check admits both wire sets and nothing else - both are read from their
/// own `as_str`, never listed here, so the day `survival` gains a slug this test learns
/// it rather than failing on it.
#[test]
fn the_root_help_names_every_floor_cause() {
    use crate::model::FloorCause;
    use clap::CommandFactory;

    let help = crate::cli::Cli::command().render_long_help().to_string();
    let start = help
        .find("--cross-compactions <N|all>")
        .expect("the root help declares the crossing flag");
    // clap closes a flag's block with its own `[default: …]` line, so that is the block's
    // end and nothing from the next flag can leak in.
    let end = help[start..]
        .find("[default:")
        .map_or(help.len(), |i| start + i);
    let block = &help[start..end];
    let ticked: Vec<&str> = block
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    assert!(
        ticked.len() > 8,
        "the crossing block parsed to {} backticked tokens - the block extraction broke",
        ticked.len()
    );

    for cause in FloorCause::ALL {
        let slug = cause.as_str();
        assert!(
            ticked.contains(&slug),
            "`csift --help`'s --cross-compactions block names no floor_cause `{slug}` - a \
             value the wire hands a reader and the manual does not explain"
        );
    }

    let mut known: Vec<&str> = FloorCause::ALL.iter().map(|c| c.as_str()).collect();
    known.extend(
        [
            crate::model::Survival::Live,
            crate::model::Survival::PreCut,
            crate::model::Survival::Abandoned { root: 0 },
        ]
        .iter()
        .map(|s| s.as_str()),
    );
    let slug_shaped = |t: &str| {
        t.contains('-')
            && t.split('-')
                .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase()))
    };
    for token in &ticked {
        if !slug_shaped(token) {
            continue;
        }
        assert!(
            known.contains(token),
            "`csift --help`'s --cross-compactions block sets `{token}` in backticks, which \
             is no floor_cause and no survival value - a renamed slug reads exactly like a \
             real one"
        );
    }
}
