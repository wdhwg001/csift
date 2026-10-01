//! What csift WRITES into the search/show LABEL ZONE, checked against the skill
//! (`skills/csift/`).
//!
//! The sibling `manuals.rs` guards the other direction: what a repo DOC says about the
//! binary, by reading the doc. This file reads the SOURCE TREE, because the label zone's
//! tags are csift's own words - a reader meets them on screen and has nowhere but the
//! manual to look them up, and the skill's `references/labels.md` promises "every tag a unit
//! prints" (its title), so the set is closed.
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
/// Each entry is the WHOLE literal as it sits in the source and is matched for EQUALITY, not
/// as a prefix, because one entry must cover exactly the construct its reason names. A prefix
/// silently widens: `[{}` was one entry reading "`Marker::render` and the `for` edge builder",
/// and it was ALSO absorbing `render.rs`'s image suffix - a third construct, in a different
/// file, that is not a label-zone tag at all - so the sweep was holding a tag nobody had
/// looked at. A NEW dynamic form fails the test until it is added here WITH its own reason
/// and where the manual documents its values, and a CHANGED format string fails too, which is
/// correct: the reason was written against a shape, and a shape that moved needs it re-read.
const DYNAMIC_HEADS: &[(&str, &str)] = &[
    (
        "[{line_type}]",
        "a bookkeeping head with no fields - references/labels.md states the rule under its \
         leaf table and the `harness.bookkeeping` rows name every one of the eighteen line types",
    ),
    (
        "[{line_type} {}]",
        "the same head carrying its key=value run - same rule, same rows",
    ),
    (
        "[{op}]{nd}",
        "`user.queued` opens with the queue OPERATION word - the `user.queued` row of \
         references/labels.md names the observed values including the `queued` fallback",
    ),
    (
        "[{op} · {reason}]{nd}",
        "the same word carrying the `remove` reason - that row spells `[remove · <reason>]`",
    ),
    (
        "[{source}]",
        "the attachment body source, `[rendered]` or `[payload]` - references/labels.md, How a \
         unit's text is built",
    ),
    (
        "[{state}]{nd}",
        "the resume placeholder's pairing, `[paired]` or `[unpaired]` - its own row of \
         references/labels.md",
    ),
    (
        "[{}]",
        "a `Marker` with no detail, and the `for` edge builder's bare form - their heads are \
         `&'static str`s, swept separately below",
    ),
    (
        "[{} {d}]",
        "a `Marker` carrying its detail - same heads, same sweep",
    ),
    (
        "[{} {}: {}]",
        "NOT a label-zone tag: `render.rs`'s image suffix, which ends a `search` hit line \
         AFTER the excerpt and reads `[1 image: #265]` / `[3 images: ...]`. The COUNT leads, \
         so no head word exists; references/search.md's Text output table names it as the \
         bracket at the end of a hit line and gives the form",
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

/// The file with its inline `#[cfg(test)] mod ... { ... }` region and its comment lines removed:
/// a bracket in a doc comment is prose about the output, not output.
///
/// It cuts at an inline test MODULE or nowhere, and that precision is the whole function. An
/// earlier form cut at the first `#[cfg(test)]` of any kind, which silently truncated every
/// file carrying that attribute on an ITEM - a const inside a macro, one enum variant - and
/// took the sweep's eyes off every literal below it. A `#[cfg(test)]` on an item is CODE and
/// its literals are words csift prints; a trailing `#[cfg(test)] mod tests;` declaration has
/// no body here to strip at all.
fn code_only(src: &str) -> String {
    let body: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    match inline_test_module(&body) {
        Some(i) => body[..i].to_string(),
        None => body,
    }
}

/// The offset of an inline `#[cfg(test)] mod <name> { ... }`, or `None` for every other use of
/// the attribute. The discriminator is the BRACE: a module reached by `{` carries its tests
/// here, a `mod <name>;` reached by `;` carries them in another file.
fn inline_test_module(body: &str) -> Option<usize> {
    const ATTR: &str = "#[cfg(test)]";
    let mut from = 0;
    while let Some(rel) = body[from..].find(ATTR) {
        let at = from + rel;
        let rest = body[at + ATTR.len()..].trim_start();
        if let Some(tail) = rest.strip_prefix("mod ") {
            let opens = tail.find('{');
            let declares = tail.find(';');
            if matches!((opens, declares), (Some(o), d) if d.is_none_or(|d| o < d)) {
                return Some(at);
            }
        }
        from = at + ATTR.len();
    }
    None
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
        // and the struct's `head:` field both render as `[<head> ...]`.
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

/// Every bracket HEAD WORD the search/show label zone can print is named in the skill.
///
/// `references/labels.md` tells a reader it holds every tag a unit prints, so the label-zone
/// set is CLOSED. That is a promise, and this is what keeps it: a head word the manual does
/// not carry is a word a reader meets on screen with nowhere to look it up.
///
/// "The skill" is the FOLDER - `SKILL.md` plus every `references/*.md` joined - because a tag
/// is named in the reference that owns its leaf (the tag table in `references/labels.md`, a
/// family's head in its leaf row, a provenance tag in `references/provenance.md`), and a tag
/// named in any of them is one a reader following the routes can find.
///
/// HEAD WORDS rather than whole tags, because almost every tag interpolates - `[for: <kind>
/// L<n>]`, `[root L12 +3]`, `[api-error status=...]`. The head is the part a reader can grep
/// for and the part a rename moves, so matching `[<head>` in the skill survives every
/// placeholder while still failing on a renamed or deleted tag.
///
/// A green run means the sweep REACHED every head and found each one documented, and those
/// are two claims rather than one. The floor below checks that the walk is not empty; the
/// POSITIVE CONTROL checks that it did not SHRINK, by naming heads the sources are known to
/// build. Without it a narrowed extraction reads as a clean bill of health, which is exactly
/// what happened while `code_only` truncated at the first `#[cfg(test)]` of any kind.
///
/// What this test CANNOT catch, stated so nobody reads a green run as more than it is: a
/// FALLBACK ARM sharing its head with the arm beside it. `[checkpoint closes <instant>]` and
/// `[checkpoint closes unknown (...)]` are one head, and so are `[root L12 +3]` and the hopless
/// `[root L12]`. Documenting both arms is a duty the transparency law states and no head-word
/// sweep can enforce.
#[test]
fn skill_names_every_label_zone_head() {
    let skill = super::skill_folder_text();

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

    // THE POSITIVE CONTROL, and a floor alone is not one. A walk that silently SHRANK - an
    // earlier `code_only` truncated every file at its first `#[cfg(test)]`, item or module -
    // still cleared the floor on the files it did read, so the test went green while it had
    // stopped looking at the whole system family and at everything below a cfg(test) item.
    // Naming heads the sweep MUST reach turns that into a failure: each of these is built by
    // a different file, so losing one file loses one name.
    const MUST_REACH: &[&str] = &[
        "root",
        "checkpoint",
        "for:",
        "refusal",
        "api-error",
        "notice",
        "scheduled",
        "abandoned",
        "replay",
        "rewound",
        "summarize",
    ];
    for want in MUST_REACH {
        assert!(
            heads.iter().any(|(h, _)| h == want),
            "the sweep did not reach `[{want}`, which the sources do build - the EXTRACTION \
             shrank, so every assertion below is running on a short list. Found {} head(s) \
             over {} file(s): {:?}",
            heads.len(),
            sources.len(),
            {
                let mut seen: Vec<&str> = heads.iter().map(|(h, _)| h.as_str()).collect();
                seen.sort_unstable();
                seen.dedup();
                seen
            }
        );
    }

    for (head, file) in &heads {
        assert!(
            skill.contains(&format!("[{head}")),
            "the skill names no label-zone tag `[{head}` (built in {file}). Document it in the \
             row or paragraph that OWNS it - references/labels.md promises a reader every tag"
        );
    }
    for (tag, file) in &dynamic {
        assert!(
            DYNAMIC_HEADS.iter().any(|(lit, _)| tag == lit),
            "{file} builds `{tag}`, a tag whose head is a runtime value. It cannot be swept, \
             so add THAT EXACT literal to DYNAMIC_HEADS with its own reason and where the skill \
             documents its values. An existing entry that merely starts the same way does not \
             cover it - the match is equality, so one entry means one construct"
        );
    }
}
