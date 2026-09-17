//! `--count-by for-kind`: the census over the provenance vocabulary.
//!
//! The census is what closing the vocabulary bought - a key space a reader can be handed -
//! and it is a TEXT surface that reads the JSON-only edges, which is the one combination a
//! format-keyed gate would silently empty. Its rows are also the shape every reader of that
//! surface splits on whitespace, so the key grammar is pinned here beside the counts.

use crate::harness::*;

use super::{ENGINE_KINDS, EVERY_EDGE, REACH_TARGET};

/// The `for-kind` census is a TEXT surface that READS the engine's edges, and it keeps
/// them.
///
/// The engine's two relations ride JSON alone, so they are built only where a reader
/// projects or censuses them - and this is the reader that is not JSON. A gate keyed on
/// the output format alone would empty two of these keys while every rendered byte stayed
/// identical, which is exactly the silent drop the honest-empties law exists to prevent.
#[test]
fn the_for_kind_census_carries_the_engine_edges_in_text_output() {
    let h = reachability_home();
    let mut argv: Vec<&str> = vec!["search", "", REACH_TARGET, "--count-by", "for-kind"];
    argv.extend_from_slice(&EVERY_LEAF);
    let out = h.run(&argv);
    assert!(out.success, "census failed: {}", out.stderr);
    for kind in ENGINE_KINDS {
        assert!(
            out.stdout
                .lines()
                .any(|l| l.split_whitespace().nth(1) == Some(kind)),
            "the engine's {kind} edge is missing from the TEXT census:\n{}",
            out.stdout
        );
    }
}

/// EVERY `for-kind` census key is ONE whitespace-free token, `hook_run` included.
///
/// The census renders `<count>  <key>` per line, which is what every other axis renders and
/// what every reader of that surface splits on whitespace - `the_for_kind_census_carries_
/// the_engine_edges_in_text_output` above is one such reader, and it takes field 1. A key
/// carrying a space parses as two fields there, so the reader silently reads the first word
/// of the relation and reports the rest as missing. `hook run` was the one such key in the
/// vocabulary; the rule is pinned over the WHOLE key space rather than over that one word,
/// because the next two-word relation would reopen it.
#[test]
fn every_for_kind_census_key_is_one_token() {
    let h = reachability_home();
    let mut argv: Vec<&str> = vec!["search", "", REACH_TARGET, "--count-by", "for-kind"];
    argv.extend_from_slice(&EVERY_LEAF);
    let out = h.run(&argv);
    assert!(out.success, "census failed: {}", out.stderr);
    let keys: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            f.next()?.parse::<u64>().ok()?;
            f.next()
        })
        .collect();
    assert!(
        keys.contains(&"hook_run"),
        "the respelled key is not in the census: {keys:?}\n{}",
        out.stdout
    );
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        assert_eq!(
            line.split_whitespace().count(),
            2,
            "a census row that does not split into count + one key: {line:?}"
        );
    }
}

/// `--count-by for-kind` censuses the PROVENANCE vocabulary: one key per relation the
/// scope carries, counted once per record.
///
/// The axis is what the closed vocabulary buys. Over an open set of relation words a
/// census would report whatever strings the family files happened to contain, with no way
/// to tell a relation from a typo; here the keys are a list a reader can be handed.
///
/// MULTI-KEY like `label`, because one unit carries several relations at once: an
/// attachment names its parent AND the call it belongs to, so a census that took the first
/// would report a vocabulary narrower than the one in use.
#[test]
fn count_by_for_kind_censuses_the_relations_the_scope_carries() {
    let h = reachability_home();
    let mut argv: Vec<&str> = vec![
        "search",
        "",
        REACH_TARGET,
        "--count-by",
        "for-kind",
        "--format",
        "json",
    ];
    argv.extend_from_slice(&EVERY_LEAF);
    let out = h.run(&argv);
    assert!(out.success, "census failed: {}", out.stderr);
    let rows = json_rows(&out.stdout, "census");
    let got: Vec<(&str, u64)> = rows
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap_or_default(),
                r["records"].as_u64().unwrap_or_default(),
            )
        })
        .collect();

    for (_, n) in &got {
        assert!(*n >= 1, "a census key with no records: {got:?}");
    }
    // Every relation word the family pin says the fixture renders IS a key here, and the
    // engine's two are keys as well: the census sees exactly what the hits carry.
    for (_, kind, _, _) in EVERY_EDGE {
        assert!(
            got.iter().any(|(k, _)| *k == kind),
            "{kind} is rendered on a hit but missing from the for-kind census: {got:?}"
        );
    }
    for kind in ENGINE_KINDS {
        assert!(
            got.iter().any(|(k, _)| *k == kind),
            "the engine's {kind} edge is missing from the census: {got:?}"
        );
    }
    let sum = json_summary(&out.stdout);
    assert_eq!(sum["axis"], "for-kind");
    assert_eq!(
        sum["distinct_keys"].as_u64().unwrap_or_default() as usize,
        got.len(),
        "the summary's key count is the row count"
    );
    // A record carrying no edge at all is EXCLUDED and counted, never silently dropped -
    // the chain head has no parent to point at, so the fixture has at least one.
    assert!(
        sum["excluded_records"].as_u64().unwrap_or_default() >= 1,
        "no record was excluded, so the domain rule is untested: {sum}"
    );
}
