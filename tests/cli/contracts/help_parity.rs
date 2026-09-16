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
//! the first direction worth a test on its own is that it fails exactly when a lane adds a
//! key and forgets the table, which is what six parallel lanes did to the `for` array.

use crate::harness::*;

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

    // The page NAMES the key: the record-row list carries most of them, and a few are
    // described where the feature they belong to is explained (the draft-distance keys
    // ride the paragraph about a superseded draft). Naming is the contract - a reader
    // searching the page for a key they met on the wire has to find it.
    let named = |key: &str| help.stdout.contains(key);

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
/// enumerates, and a family's own, which that family's region of the same page names where
/// it explains the fact. So the assertion is that the PAGE names the key - which is what a
/// reader who met one on the wire does with it.
#[test]
fn every_key_a_search_hit_emits_is_one_the_help_names() {
    let h = reachability_home();
    let help = h.run(&["search", "--help"]);
    assert!(help.success, "stderr {}", help.stderr);
    assert!(
        help.stdout.contains("hits:[{"),
        "search --help carries the hit schema"
    );
    // The schema paragraph carries the keys every hit has; a family's own keys are named
    // in that family's region of the same page. Either way the page NAMES the key, which
    // is the contract a reader relies on.
    let named = |key: &str| help.stdout.contains(key);

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
