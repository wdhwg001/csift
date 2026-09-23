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
        "| leaf | on disk |",
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

/// The shape cell of every leaf's row NAMES the on-disk shapes that reach it.
///
/// The leaf column is pinned above; the SHAPE column was not, and it is the half a reader
/// acts on - the manual promises a leaf can be cross-checked against the bytes, so a
/// shape the inventory carries and the table omits is a reader grepping for something the
/// table never told them exists.
///
/// The check is CONTAINMENT rather than equality, and that is a property of the cell
/// rather than a concession: a cell is prose, it factors a shared prefix across several
/// values (`attachment.type:"skill_listing" · "agent_listing_delta" · ...`) and it adds the
/// sentence a shape needs beside it. Equality would pin the wording and fail on every
/// rewrite. So the assertion is per INVENTORY ROW - one row, one shape, one leaf - and it
/// accepts the three spellings the table itself uses, measured over all of it rather than
/// guessed:
///
/// - the QUOTED value, which is how the four FIELD axes write a shape
///   (`type:"permission-mode"`, `attachment.type:"goal_status"`);
/// - `template:<value>`, which is how the TEMPLATE axis writes one, unquoted because a
///   template name is csift's own name for a text shape and not a value on the line;
/// - the whole `Entry::disk()` string, which is what a row whose mark on disk is not a
///   value needs (the fired scheduler prompt, whose mark is the PARENT LINK).
#[test]
fn every_shape_that_carries_a_leaf_is_named_in_that_leaf_row() {
    let md = skill();
    let cells = shape_cells(&md);
    assert!(
        cells.len() == Class::ALL.len(),
        "the shape table parsed to {} rows for {} leaves",
        cells.len(),
        Class::ALL.len()
    );

    let mut unnamed: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for r in rows() {
        let Some(leaf) = r.leaf else { continue };
        checked += 1;
        let cell = cells
            .iter()
            .find(|(l, _)| l == leaf.path())
            .map(|(_, c)| c.as_str())
            .unwrap_or_default();
        let named = cell.contains(&format!("\"{}\"", r.value()))
            || cell.contains(&format!("template:{}", r.value()))
            || cell.contains(&r.disk());
        if !named {
            unnamed.push(format!("{} <- {}", leaf.path(), r.disk()));
        }
    }
    assert!(
        checked > 90,
        "the sweep checked {checked} rows - the inventory walk found almost nothing"
    );
    assert!(
        unnamed.is_empty(),
        "shapes the inventory carries and SKILL's shape table does not name:\n  {}",
        unnamed.join("\n  ")
    );
}

/// `(leaf, shape cell)` for every row of the shape table.
fn shape_cells(md: &str) -> Vec<(String, String)> {
    let head = "| leaf | on disk |";
    let start = md
        .find(head)
        .unwrap_or_else(|| panic!("SKILL.md carries no table headed {head:?}"));
    let mut out = Vec::new();
    for line in md[start..].lines().skip(2) {
        if !line.starts_with('|') {
            break;
        }
        let mut cells = line.trim_start_matches('|').split('|');
        let leaf = cells.next().unwrap_or("").trim().trim_matches('`');
        let shape = cells.next().unwrap_or("").trim();
        if !leaf.is_empty() {
            out.push((leaf.to_string(), shape.to_string()));
        }
    }
    out
}
