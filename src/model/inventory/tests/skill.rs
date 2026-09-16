//! `SKILL.md`'s two transparency tables against the leaf space they describe.
//!
//! The tables are the LLM manual's answer to "what is this label and where did it come
//! from", one row per leaf in each, and there is no way to derive them - a row is prose a
//! person wrote. So the only guard against a table that has stopped covering the taxonomy
//! is a test that walks it.
//!
//! WHY IT EXISTS, concretely: while the six family branches were being picked, one pick
//! left a second copy of the file's first 196 lines INSIDE the identity table, and a row
//! two families had edited landed in two different copies of itself. Nothing failed. A set
//! comparison against `Class::ALL` catches every member of that class - a duplicated row,
//! a dropped row, a leaf renamed on one table and not the other - on the next edit rather
//! than at the next release.

use super::*;

/// The SKILL manual, read once.
fn skill() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"))
        .expect("SKILL.md at the repo root")
}

/// The leaf path in the FIRST cell of every row of the table that opens with `head`.
///
/// A markdown table row is `| cell | cell | ... |`, so the first cell is what sits between
/// the first two pipes; a leaf cell is a backticked path. The walk starts at the head line
/// and stops at the first line that is not a table row, which is what makes a table a
/// TABLE rather than every backticked path in the file.
fn leaf_column(md: &str, head: &str) -> Vec<String> {
    let start = md
        .find(head)
        .unwrap_or_else(|| panic!("SKILL.md carries no table headed {head:?}"));
    let mut out = Vec::new();
    for line in md[start..].lines().skip(1) {
        if !line.starts_with('|') {
            if out.is_empty() {
                continue; // the blank line and the `|---|` separator under the heading
            }
            break;
        }
        let cell = line.trim_start_matches('|').split('|').next().unwrap_or("");
        let cell = cell.trim();
        if let Some(path) = cell.strip_prefix('`').and_then(|c| c.strip_suffix('`')) {
            out.push(path.to_string());
        }
    }
    out
}

/// Each table lists EVERY leaf of `Class::ALL`, exactly once, and nothing else.
///
/// Three failures in one assertion, and each is a different way a reader is misled: a
/// MISSING row is a label csift prints and the manual does not explain, an EXTRA row is a
/// label the manual promises and no selector accepts, and a DUPLICATE is two answers to
/// one question with nothing saying which is current.
#[test]
fn both_skill_tables_list_every_leaf_exactly_once() {
    let md = skill();
    let want: Vec<&str> = Class::ALL.iter().map(|c| c.path()).collect();
    for head in [
        "| leaf | what | gate | LLM sees | human sees |",
        "| leaf | on disk | grab the raw bytes |",
    ] {
        let got = leaf_column(&md, head);
        let mut sorted = got.clone();
        sorted.sort();
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(
            sorted,
            deduped,
            "{head}: a leaf is listed twice - {:?}",
            sorted
                .iter()
                .enumerate()
                .filter(|(i, p)| *i > 0 && sorted[i - 1] == **p)
                .map(|(_, p)| p)
                .collect::<Vec<_>>()
        );
        let missing: Vec<&&str> = want
            .iter()
            .filter(|p| !got.iter().any(|g| g == *p))
            .collect();
        assert!(
            missing.is_empty(),
            "{head}: leaves with no row: {missing:?}"
        );
        let extra: Vec<&String> = got.iter().filter(|g| !want.contains(&g.as_str())).collect();
        assert!(extra.is_empty(), "{head}: rows for no leaf: {extra:?}");
        assert_eq!(
            got.len(),
            Class::ALL.len(),
            "{head} has {} rows for {} leaves",
            got.len(),
            Class::ALL.len()
        );
    }
}

/// The file's own HEAD occurs once, which is what names the duplication directly.
///
/// The identity table's duplicate copy carried the whole head of the file with it, front
/// matter included, so a reader who opened SKILL.md met its opening twice. The table
/// assertion above catches that too, by way of the leaves the copy repeated; this one says
/// WHERE, so a failure reads as "the head is in the file twice" rather than as a list of
/// duplicated rows.
///
/// The probe is the front matter itself - the opening `---`, its keys and its closing
/// `---` - rather than a count of `---` lines, because the manual uses that rule between
/// its major parts as well and a count would be a statement about section breaks.
#[test]
fn the_skill_manual_head_occurs_once() {
    let md = skill();
    assert!(md.starts_with("---\n"), "the file opens with front matter");
    let close = md[4..].find("\n---\n").expect("the front matter closes") + 4;
    let head = &md[..close + 5];
    assert_eq!(
        md.matches(head).count(),
        1,
        "SKILL.md carries its own head more than once - a pick duplicated the file's \
         opening into the body"
    );
}
