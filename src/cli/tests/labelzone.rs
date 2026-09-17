//! What csift WRITES into the search/show LABEL ZONE, checked against `SKILL.md`.
//!
//! The sibling `manuals.rs` guards the other direction: what a repo DOC says about the
//! binary, by reading the doc. This file reads the SOURCE TREE, because the label zone's
//! tags are csift's own words - a reader meets them on screen and has nowhere but the
//! manual to look them up, and `SKILL.md` section 7.4 promises the set is closed.
//!
//! The walk is over the label-zone render sources rather than over `src/` whole, because
//! only these files build a tag a `search` or `show` unit carries. A NEW file under any of
//! the listed directories is swept the moment it exists - which is why the sources are read
//! off the filesystem rather than through `include_str!`.

#[allow(unused_imports)]
use super::*;

use std::path::{Path, PathBuf};

/// The label-zone render sources. A plain file, or a directory swept for `.rs` recursively.
const SOURCES: &[&str] = &[
    "src/commands/search/render.rs",
    "src/commands/search/record_text.rs",
    "src/commands/search/record_text",
    "src/commands/search/hit_facts.rs",
    "src/commands/search/hit_facts",
    "src/commands/search/types/facts.rs",
    "src/model/provenance.rs",
    "src/model/exchange.rs",
    "src/model/automation.rs",
];

/// A tag whose HEAD is a runtime value, so no head word can be extracted statically.
///
/// Each entry is the literal as it sits in the source, and each carries the reason it
/// cannot be swept plus where the manual documents its values. A NEW dynamic form fails
/// the test until it is added here WITH that reason - which is the point: the allowlist is
/// the place a reader learns why a tag is not pinned, not a place to make one disappear.
const DYNAMIC_HEADS: &[(&str, &str)] = &[
    (
        "[{line_type}",
        "the bookkeeping head opens with the LINE's own `type` - SKILL 7.4 states the rule \
         and the 7.2 rows name every one of the eighteen types",
    ),
    (
        "[{op}",
        "`user.queued` opens with the queue OPERATION word - SKILL's `user.queued` 7.2 row \
         names the four observed values including the `queued` fallback",
    ),
    (
        "[{source}",
        "the attachment body source, `[rendered]` or `[payload]` - SKILL 7.2's intro and 7.4",
    ),
    (
        "[{state}",
        "the resume placeholder's pairing, `[paired]` or `[unpaired]` - its own 7.2 row",
    ),
    (
        "[{}",
        "`Marker::render` and the `for` edge builder, whose heads are `&'static str`s swept \
         separately below",
    ),
];

/// Every `.rs` under `dir`, recursively, skipping any `tests` directory.
fn rs_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            rs_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && path.file_name().is_some_and(|n| n != "tests.rs")
        {
            out.push(path);
        }
    }
}

/// The source files the sweep reads, in a stable order.
fn label_zone_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for rel in SOURCES {
        let path = root.join(rel);
        if path.is_dir() {
            rs_under(&path, &mut files);
        } else if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();
    files
}

/// The file with its inline `#[cfg(test)] mod tests { … }` region and its comment lines
/// removed: a bracket in a doc comment is prose about the output, not output.
fn code_only(src: &str) -> String {
    let body: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    match body.find("#[cfg(test)]") {
        Some(i) => body[..i].to_string(),
        None => body,
    }
}

/// Every string literal in `src`, as its raw inner text (escapes left as written).
fn string_literals(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = src.chars();
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
        out.push(lit);
    }
    out
}

/// The HEAD WORD of a tag: the run after `[` up to the first space, `{` or `]`.
fn head_word(after_bracket: &str) -> String {
    after_bracket
        .split([' ', '{', ']'])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// `(static head words, dynamic tag openings)` over one file's literals.
fn tags_in(src: &str) -> (Vec<String>, Vec<String>) {
    let (mut heads, mut dynamic) = (Vec::new(), Vec::new());
    for lit in string_literals(src) {
        // A `Marker`'s head is the literal itself, not a bracketed run: `Marker::plain`
        // and the struct's `head:` field both render as `[<head> …]`.
        if let Some(h) = marker_head(src, &lit) {
            heads.push(h);
        }
        for (i, ch) in lit.char_indices() {
            if ch != '[' {
                continue;
            }
            let rest = &lit[i + 1..];
            match rest.chars().next() {
                Some('{') => dynamic.push(format!("[{{{}", rest.trim_start_matches('{'))),
                Some(n) if n.is_alphanumeric() => heads.push(head_word(rest)),
                _ => {}
            }
        }
    }
    (heads, dynamic)
}

/// `Some(head)` when this literal is used as a `Marker` head in this file.
fn marker_head(src: &str, lit: &str) -> Option<String> {
    let used = src.contains(&format!("Marker::plain(\"{lit}\""))
        || src.contains(&format!("head: \"{lit}\""));
    (used && !lit.is_empty() && !lit.starts_with('[')).then(|| head_word(lit))
}

/// Every bracket HEAD WORD the search/show label zone can print is named in `SKILL.md`.
///
/// Section 7.4 tells a reader the label-zone set is CLOSED and that a tag the file does not
/// name means their copy is stale. That is a promise, and this is what keeps it: a head word
/// the manual does not carry is a word a reader meets on screen with nowhere to look it up.
///
/// HEAD WORDS rather than whole tags, because almost every tag interpolates - `[for: <kind>
/// L<n>]`, `[root L12 +3]`, `[api-error status=…]`. The head is the part a reader can grep
/// for and the part a rename moves, so matching `[<head>` in `SKILL.md` survives every
/// placeholder while still failing on a renamed or deleted tag.
///
/// What this test CANNOT catch, stated so nobody reads a green run as more than it is: a
/// FALLBACK ARM sharing its head with the arm beside it. `[checkpoint closes <instant>]` and
/// `[checkpoint closes unknown (…)]` are one head, and so are `[root L12 +3]` and the hopless
/// `[root L12]`. Documenting both arms is a duty the transparency law states and no head-word
/// sweep can enforce.
#[test]
fn skill_names_every_label_zone_head() {
    let skill = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"))
        .expect("SKILL.md at the repo root");

    let mut heads: Vec<(String, String)> = Vec::new();
    let mut dynamic: Vec<(String, String)> = Vec::new();
    let sources = label_zone_sources();
    for file in &sources {
        let name = file.display().to_string();
        let (h, d) = tags_in(&code_only(
            &std::fs::read_to_string(file).expect("a source file"),
        ));
        heads.extend(h.into_iter().map(|x| (x, name.clone())));
        dynamic.extend(d.into_iter().map(|x| (x, name.clone())));
    }

    // A silently empty walk would pass every assertion below, so the floor is its own check.
    assert!(
        sources.len() >= 15 && heads.len() >= 40,
        "the label-zone sweep read {} file(s) and found {} head(s) - the extraction broke, \
         not the manual",
        sources.len(),
        heads.len()
    );

    for (head, file) in &heads {
        assert!(
            skill.contains(&format!("[{head}")),
            "SKILL.md names no label-zone tag `[{head}` (built in {file}). Document it in the \
             row or paragraph that OWNS it - section 7.4 promises a reader the set is closed"
        );
    }
    for (tag, file) in &dynamic {
        assert!(
            DYNAMIC_HEADS.iter().any(|(pat, _)| tag.starts_with(pat)),
            "{file} builds `{tag}`, a tag whose head is a runtime value. It cannot be swept, \
             so add it to DYNAMIC_HEADS with the reason and where SKILL documents its values"
        );
    }
}
