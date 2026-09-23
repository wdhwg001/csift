//! The five-document contract, checked where it can be: `--help` against what the binary
//! actually emits.
//!
//! `--help` is the human manual and SKILL is the LLM's, at information parity, and most of
//! that parity is prose a person has to read. ONE half is mechanical and is the half that
//! rots first: the per-command JSON key enumerations. A key on the wire that no enumeration
//! names is a key a consumer cannot find, and a key the help promises and the wire never
//! carries sends them looking for something that is not there.
//!
//! The direction asserted here is the first one. The second cannot be asserted from one
//! fixture, because most keys are CONDITIONAL (present only on a record that carries them),
//! so an absent key is usually a fixture fact rather than a contract failure. What makes
//! the first direction worth a test on its own is that it fails exactly when a key reaches
//! the wire and its table entry is forgotten, which is how the `for` array got there.
//!
//! WHERE a key has to appear is the whole strength of the test. Asking whether the key's
//! characters occur anywhere on the page documents the nine short keys (`to`, `from`,
//! `line`, `uuid`, `mode`, `kind`, `text`, `body`, `for`) by accident, because each matches
//! ordinary English prose - under that reading the `to` row can be deleted from the key
//! table with the whole suite staying green. A key counts as NAMED here only if it is an
//! entry of the rendered KEY BLOCK - the `{a, b, c}` list the help builds from the table -
//! or is written in BACKTICKS somewhere on the page, which is how the prose names a key it
//! explains beside its feature. Both forms have a boundary on each side; running prose has
//! neither.
//!
//! The positive control for that rule: deleting the `to` row from the key table must fail
//! this file BY NAME (`emits keys show --help does not name: ["to"]`).
//!
//! The strictness earns its keep beyond formatting: what the WHERE rule catches is a key
//! ABSENT FROM THE HIT SCHEMA the help renders, not merely one unbackticked in the prose
//! beside it. A rule that asks only whether the characters occur cannot tell a key the
//! page DOCUMENTS from a key the page happens to spell.

use crate::harness::*;

/// The brace-balanced block that follows `marker`, `marker`'s own `{` included.
///
/// Balanced rather than "up to the next `}`" because entries nest: `for:[{kind,basis,...}]`
/// and `api_failure:{...}` both carry inner braces, and a naive cut would end the block in
/// the middle of the list and silently shorten the name set.
fn brace_block(hay: &str, marker: &str) -> String {
    let start = hay
        .find(marker)
        .unwrap_or_else(|| panic!("the help page carries {marker:?}"))
        + marker.len()
        - 1;
    let mut depth = 0usize;
    for (i, c) in hay[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return hay[start..=start + i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{marker:?} opens a block the page never closes");
}

/// Every key named by a BACKTICKED code span on the page.
///
/// The name is the span's LEADING IDENTIFIER, because the page writes a key with its shape
/// attached where it has one (`produced_by:{command,hook_name,payload_type}`,
/// `labels:[…]`). A backtick on each side is the boundary that makes this different from
/// asking whether the characters occur: `to` in a sentence is not `` `to` ``.
fn backticked_names(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = help;
    while let Some(open) = rest.find('`') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('`') else { break };
        let span = &rest[..close];
        rest = &rest[close + 1..];
        let name: String = span
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The TOP-LEVEL entry names of a rendered key block.
///
/// An entry is `name`, or `name` followed by its value set or shape - `labels:[…]`,
/// `pairing (paired | pending | orphan | null)` - so the name is the leading identifier
/// run. Splitting is depth-aware for the same reason the extraction is.
fn top_level_names(block: &str) -> Vec<String> {
    let inner = block
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .expect("a block is brace-delimited");
    let mut names = Vec::new();
    let mut depth = 0usize;
    let mut entry = String::new();
    for c in inner.chars() {
        match c {
            '{' | '[' | '(' => {
                depth += 1;
                entry.push(c);
            }
            '}' | ']' | ')' => {
                depth = depth.saturating_sub(1);
                entry.push(c);
            }
            ',' if depth == 0 => {
                names.push(std::mem::take(&mut entry));
            }
            _ => entry.push(c),
        }
    }
    names.push(entry);
    names
        .iter()
        .map(|e| {
            e.trim()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|n| !n.is_empty())
        .collect()
}

/// Every key `show --format json` puts on a RECORD row is a key `show --help` names.
///
/// The reachability home is the fixture on purpose: it carries one line per leaf of the
/// taxonomy, so the sweep reaches every family's keys in one run rather than whichever
/// ones a hand-built record happens to have.
#[test]
fn every_key_a_show_record_row_emits_is_one_the_help_names() {
    let h = reachability_home();
    let help = h.run(&["show", "--help"]);
    assert!(help.success, "stderr {}", help.stderr);

    // The rendered key block is `cli::show_keys::record_row_keys()`, built from the table
    // every family appends to - so a row deleted from the table leaves this list.
    let block = brace_block(&help.stdout, "Record rows carry {");
    let listed = top_level_names(&block);
    assert!(
        listed.len() > 50,
        "the key block parsed to {} names: {block}",
        listed.len()
    );
    // These three prove the PARSE found real entries, so an empty or truncated name list
    // cannot pass by silence. They are deliberately keys with a shape attached or a
    // distinctive spelling - never one of the nine short keys, which are the rows a
    // deletion would remove, and which must fail on the assertion below instead of here.
    for anchor in ["session_id", "labels", "compact_metadata"] {
        assert!(
            listed.iter().any(|n| n == anchor),
            "the block parse lost {anchor}, so this test would pass by silence"
        );
    }

    // NAMED = an entry of that block, or written in backticks in the prose (a few keys are
    // explained beside their feature rather than listed - the draft-distance keys ride the
    // paragraph about a superseded draft). Both have a boundary; a bare substring does not.
    let ticked = backticked_names(&help.stdout);
    let named = |key: &str| listed.iter().any(|n| n == key) || ticked.iter().any(|n| n == key);

    let out = h.run(&[
        "show",
        "@3a1b2c3d-4e5f-4061-8273-849506a7b8c9",
        "--line",
        "..",
        "--max-count",
        "0",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr {}", out.stderr);
    let rows = json_rows(&out.stdout, "record");
    assert!(rows.len() > 60, "the sweep read {} rows", rows.len());

    let mut undocumented: Vec<String> = Vec::new();
    for row in &rows {
        for key in row.as_object().expect("a record row is an object").keys() {
            // `kind` is the ENVELOPE's own discriminator, not a record key.
            if key == "kind" || named(key) {
                continue;
            }
            if !undocumented.contains(key) {
                undocumented.push(key.clone());
            }
        }
    }
    undocumented.sort();
    assert!(
        undocumented.is_empty(),
        "`show --format json` emits keys `show --help` does not name: {undocumented:?}"
    );
}

/// The same, for a `search` hit object against the whole of `search --help`.
///
/// A hit carries two kinds of key: the ones EVERY hit has, which the JSON schema paragraph
/// enumerates, and a family's own, which that family's region of the same page names in
/// backticks where it explains the fact. So the assertion is that the PAGE names the key -
/// which is what a reader who met one on the wire does with it.
#[test]
fn every_key_a_search_hit_emits_is_one_the_help_names() {
    let h = reachability_home();
    let help = h.run(&["search", "--help"]);
    assert!(help.success, "stderr {}", help.stderr);

    let block = brace_block(&help.stdout, "hits:[{");
    let listed = top_level_names(&block);
    assert!(
        listed.len() > 20,
        "the hit schema parsed to {} names: {block}",
        listed.len()
    );
    // Same role, same reason: a parse guard, never a stand-in for a short key's own row.
    for anchor in ["session_id", "excerpt", "refetch_uuid"] {
        assert!(
            listed.iter().any(|n| n == anchor),
            "the schema parse lost {anchor}, so this test would pass by silence"
        );
    }
    let ticked = backticked_names(&help.stdout);
    let named = |key: &str| listed.iter().any(|n| n == key) || ticked.iter().any(|n| n == key);

    let out = h.run(&[
        "search",
        "",
        "@3a1b2c3d-4e5f-4061-8273-849506a7b8c9",
        "-t",
        "user.*",
        "-t",
        "agent.*",
        "-t",
        "harness.*",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr {}", out.stderr);
    let mut undocumented: Vec<String> = Vec::new();
    let mut hits = 0usize;
    for ex in json_rows(&out.stdout, "exchange") {
        for hit in ex["hits"].as_array().into_iter().flatten() {
            hits += 1;
            for key in hit.as_object().expect("a hit is an object").keys() {
                if named(key) || undocumented.contains(key) {
                    continue;
                }
                undocumented.push(key.clone());
            }
        }
    }
    assert!(hits > 40, "the sweep read {hits} hits");
    undocumented.sort();
    assert!(
        undocumented.is_empty(),
        "`search --format json` emits hit keys the schema does not name: {undocumented:?}"
    );
}
