//! The REPO DOCS as a surface: what the skill (`skills/csift/`) and its siblings must say
//! about the binary, checked against the binary.
//!
//! Split from `docs.rs`, which guards the RENDERED HELP. The two answer different
//! questions and fail for different reasons: a help guard breaks when a flag or a
//! table moves, and these break when a markdown file drifts from the code it
//! describes. They share only the word "docs".

#[allow(unused_imports)]
use super::*;

/// The skill's surface stamp must match the crate version - forces the LLM-facing
/// skill to be (at least) OPENED on every release, so the "re-read this file on an
/// unexpected error" recovery path always lands on current truth.
///
/// The stamp is the body's `Surface: csift <version>` line, spelled the way
/// `csift --version` prints the version, because comparing the two is the reader's whole
/// use of it. It is held in `SKILL.md` itself rather than anywhere in the folder: the body
/// is the file a model always loads, so it is the only place the staleness check is sure
/// to be read.
#[test]
fn skill_stamp_matches_crate_version() {
    let skill = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/skills/csift/SKILL.md"
    ))
    .expect("skills/csift/SKILL.md");
    let want = format!("Surface: csift {}", env!("CARGO_PKG_VERSION"));
    assert!(
        skill.contains(&want),
        "skills/csift/SKILL.md must carry the surface stamp `{want}` (found none) - update \
             the stamp (and the surface docs) with every version bump"
    );
}

/// The repo docs - the skill folder, AGENTS, SPEC and README - must never resurrect a token
/// this cleanup removed. SPEC's `>`-quoted change LEDGER lines are exempt (they record the
/// renames themselves).
#[test]
fn docs_do_not_resurrect_removed_tokens() {
    let deny: &[&str] = &[
        "--include-subagents",
        "agents --kind",
        "--by-file",
        "--line A-B",
        "category=",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut docs: Vec<String> = vec!["skills/csift/SKILL.md".to_string()];
    let mut refs: Vec<String> = std::fs::read_dir(root.join("skills/csift/references"))
        .expect("skills/csift/references")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md"))
        .map(|n| format!("skills/csift/references/{n}"))
        .collect();
    refs.sort();
    assert!(
        !refs.is_empty(),
        "the skill's references/ read found no .md"
    );
    docs.extend(refs);
    docs.extend(["AGENTS.md", "SPEC.md", "README.md"].map(String::from));
    for doc in &docs {
        let body = std::fs::read_to_string(root.join(doc)).expect("repo doc");
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

/// The skill's `## Retired spellings` section (`references/labels.md`) is the ONE address a
/// stale caller lands on, so it names EVERY retired selector the binary refuses.
///
/// What the section states, and so what this holds it to: each retired spelling by name -
/// `harness.meta` once, with "and its leaves" standing for every `harness.meta.*` path - and
/// that the refusal names the successors, with the instruction to follow the error. The
/// SUCCESSOR MAP itself is not in the skill. It lived there as a second copy of
/// [`crate::cli::selectors::RETIRED_SELECTORS`], and a per-row test was the only thing
/// holding the two together: three of five rows once sent a reader to a selector that does
/// not carry their records. The rewritten skill keeps one copy, the error text the table
/// renders, so there is no second mapping left to drift and nothing per-row to pin here.
///
/// BOTH DIRECTIONS still hold, on the spelling: a retired selector the section omits is a
/// refusal a reader cannot place, and a `harness.meta...` path the section names that the
/// table does not retire is a spelling a reader will type and meet as an UNKNOWN selector
/// instead.
#[test]
fn skill_retired_spellings_name_every_retired_selector() {
    let labels = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/skills/csift/references/labels.md"
    ))
    .expect("skills/csift/references/labels.md");
    let start = labels
        .find("## Retired spellings")
        .expect("references/labels.md carries the retirement section");
    let end = labels[start + 1..]
        .find("\n## ")
        .map_or(labels.len(), |i| start + 1 + i);
    let section = &labels[start..end];

    assert!(
        section.contains("successors named"),
        "the retirement section no longer tells a reader the refusal names the successors - \
         the skill carries no successor map, so that sentence is the reader's only pointer"
    );
    let meta_family = section.contains("`harness.meta` and its leaves");
    for (selector, _reason, _successors) in crate::cli::selectors::RETIRED_SELECTORS {
        let named = section.contains(&format!("`{selector}`"))
            || (meta_family && selector.starts_with("harness.meta."));
        assert!(
            named,
            "the skill's retirement section does not name the retired selector `{selector}`"
        );
    }
    // Every backticked `harness.meta...` spelling the section sets is one the table retires.
    for typed in section
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|t| t.starts_with("harness.meta"))
    {
        assert!(
            crate::cli::selectors::RETIRED_SELECTORS
                .iter()
                .any(|(s, _, _)| *s == typed),
            "the skill's retirement section names `{typed}`, which the table does not retire"
        );
    }
}

/// Every `[`-led marker word csift READS OFF A CLAUDE CODE RECORD is quoted in the skill.
///
/// SCOPE, stated exactly, because the name reads wider than the test is: this guards the
/// HARNESS's own bracketed words - the two interrupt markers, the bracketed enforce and
/// retry sentinels, the notification frame and the two image references - and nothing csift
/// WRITES. Its whole domain is the string literals of `model::markers`, the one module where
/// csift writes down a word Claude Code put on the line. What csift prints into the label
/// zone is a different question with a different source tree, and `labelzone.rs` answers it.
///
/// The transparency law is why both exist: csift may never speak a language the jsonl does
/// not, so a reader who meets a bracketed word on screen can look it up and then go find it
/// in the raw line.
///
/// "The skill" is the FOLDER - `SKILL.md` plus every `references/*.md` - because the markers
/// are named where their leaves are documented (all eight in `references/shapes.md`, the
/// shape table's rows), and a reader following the routes reaches them there.
///
/// The set is WALKED out of that module rather than listed here, because a list here would
/// be a second copy drifting at the same rate as the prose it guards; a new harness marker
/// lands there first, and this test fails until the manual carries it too.
///
/// It reads the file's STRING LITERALS, skipping comment lines: a `[`-led word inside prose
/// is a doc comment describing the const below it, and matching those would pin the module's
/// own commentary rather than what the classifier tests. An ESCAPE is kept verbatim (the
/// backslash and the character after it) instead of decoded, so a marker carrying one is
/// compared in the spelling the source uses rather than in a shortened form the manual might
/// contain by accident.
#[test]
fn skill_names_every_bracketed_harness_marker() {
    let markers = include_str!("../../model/markers.rs");
    let skill = super::skill_folder_text();

    let mut found: Vec<String> = Vec::new();
    for line in markers.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c != '"' {
                continue;
            }
            let mut lit = String::new();
            loop {
                match chars.next() {
                    None | Some('"') => break,
                    Some('\\') => {
                        lit.push('\\');
                        if let Some(n) = chars.next() {
                            lit.push(n);
                        }
                    }
                    Some(ch) => lit.push(ch),
                }
            }
            if lit.starts_with('[') {
                found.push(lit);
            }
        }
    }

    // A silently empty walk would pass every assertion below, so the count is its own
    // check - and it is EXACT rather than a floor, because a `>=` equal to the current
    // number is a floor that can never bite: the module carries the two interrupt markers,
    // the three bracketed enforce and retry sentinels, the notification frame and the two
    // image references, and nothing else. Adding a bracketed harness marker to
    // `model::markers` means raising this number IN THE SAME EDIT that documents the marker
    // in the skill - the arithmetic is the reminder.
    assert_eq!(
        found.len(),
        8,
        "model/markers.rs carries {} bracketed literals, not 8: {found:?}. If you ADDED a \
         harness marker, document it in the skill and raise this count; if the extraction \
         broke, fix the walk",
        found.len()
    );
    for marker in &found {
        assert!(
            skill.contains(marker.as_str()),
            "the skill quotes no `{marker}` - csift matches that marker word on a record and \
             the manual never names it, so a reader meets it on screen with nowhere to look \
             it up"
        );
    }
}

/// The root help names EVERY `floor_cause` slug, and no token shaped like one that is not.
///
/// The eight slugs are a closed enum a consumer branches on, and they live in three
/// places: [`FloorCause`], the global `--cross-compactions` doc (rendered on all eighteen
/// help pages) and the skill's floor-cause table (`references/survival.md`). Nothing held
/// the three together: the help carried the KEY on seven pages and the value domain on
/// none, which is how a reader ends up guessing at a field the wire hands them.
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
    // clap closes a flag's block with its own `[default: ...]` line, so that is the block's
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
