//! The introspection-ledger gate: `INTROSPECTION.json` (every Claude Code behavior csift
//! depends on, one claim per entry) must be internally consistent and must back the
//! README's "verified against Claude Code" badge. Runs under `cargo test`, which the
//! pre-commit hook runs; the hook also names this test explicitly.
//!
//! Rules:
//! 1. `verified_claude_code` is a version triple, and the README badge carries the SAME
//!    version (the badge is admitted only through the ledger).
//! 2. Every claim has a unique id, an area, a behavior, a dependency statement, an
//!    instrument, and at least one check made AT `verified_claude_code` with a verdict
//!    from the closed set.
//! 3. A check's evidence is claim-specific: for `holds` / `refined` / `drifted` the
//!    (instrument, observed) pair is non-empty and unique across the ledger (a
//!    mechanical bulk replacement leaves identical pairs); `unverifiable-here` names its
//!    reason in `observed`.
//! 4. Every cited code site exists and its snippet is found verbatim (whitespace
//!    normalized) in that file, so a refactor that moves the code fails here instead of
//!    silently orphaning the claim.
//! 5. The README also carries the mutation-score badge (a version-independent shape
//!    check; the score itself is the census's business).
//! 6. Every claim cites at least one code site: a claim with no site names a behavior
//!    csift does not demonstrably depend on, and the audit could never anchor it.

use std::collections::{HashMap, HashSet};

fn repo() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn version_triple(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[test]
fn ledger_gate_ledger_is_consistent_and_backs_the_readme_badges() {
    let raw = std::fs::read_to_string(repo().join("INTROSPECTION.json"))
        .expect("INTROSPECTION.json must exist at the repo root");
    let doc: serde_json::Value = serde_json::from_str(&raw).expect("INTROSPECTION.json parses");
    assert_eq!(doc["schema"], 1, "ledger schema version");
    let verified = doc["verified_claude_code"]
        .as_str()
        .expect("verified_claude_code is a string");
    assert!(
        version_triple(verified),
        "verified_claude_code `{verified}` is a version triple"
    );

    let claims = doc["claims"].as_array().expect("claims is an array");
    assert!(!claims.is_empty(), "the ledger is never empty");
    let allowed = ["holds", "refined", "drifted", "unverifiable-here"];
    let mut ids: HashSet<&str> = HashSet::new();
    let mut evidence: HashMap<String, &str> = HashMap::new();
    let mut file_cache: HashMap<String, String> = HashMap::new();
    let mut failures: Vec<String> = Vec::new();

    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        if id.is_empty() || !ids.insert(id) {
            failures.push(format!("claim id missing or duplicated: `{id}`"));
            continue;
        }
        for key in ["area", "behavior", "depends", "instrument"] {
            if c[key].as_str().is_none_or(|s| s.trim().is_empty()) {
                failures.push(format!("{id}: `{key}` is empty"));
            }
        }
        // Rule 2: a check at the verified version, with a closed-set verdict.
        let checks = c["checks"].as_array().cloned().unwrap_or_default();
        let at_version: Vec<&serde_json::Value> = checks
            .iter()
            .filter(|k| k["claude_code"].as_str() == Some(verified))
            .collect();
        if at_version.is_empty() {
            failures.push(format!("{id}: no check at Claude Code {verified}"));
        }
        for k in &at_version {
            let verdict = k["verdict"].as_str().unwrap_or("");
            if !allowed.contains(&verdict) {
                failures.push(format!("{id}: verdict `{verdict}` is not in {allowed:?}"));
            }
            let instrument = k["instrument"].as_str().unwrap_or("").trim();
            let observed = k["observed"].as_str().unwrap_or("").trim();
            if observed.is_empty() {
                failures.push(format!(
                    "{id}: a check at {verified} has an empty `observed`"
                ));
            }
            // Every check names its instrument: for a decided verdict the command that
            // ran, for `unverifiable-here` the instrument that WOULD decide it. An empty
            // instrument is the shape of a placeholder, and a placeholder never backs a
            // badge.
            if instrument.is_empty() {
                failures.push(format!("{id}: `{verdict}` without an instrument"));
            }
            // Rule 3: claim-specific evidence for the decided verdicts.
            if verdict != "unverifiable-here" {
                let key = format!("{}|{}", norm(instrument), norm(observed));
                if let Some(other) = evidence.insert(key, id) {
                    failures.push(format!(
                        "{id}: evidence (instrument, observed) identical to {other}'s - \
                         a check is written per claim, never replaced mechanically"
                    ));
                }
            }
        }
        // Rule 6: at least one code site per claim.
        if c["code"].as_array().is_none_or(|a| a.is_empty()) {
            failures.push(format!(
                "{id}: no code site (every claim cites at least one)"
            ));
        }
        // Rule 4: every code site is real and its snippet still exists verbatim.
        for site in c["code"].as_array().cloned().unwrap_or_default() {
            let path = site["path"].as_str().unwrap_or("");
            let snippet = site["snippet"].as_str().unwrap_or("");
            if path.is_empty() || snippet.trim().is_empty() {
                failures.push(format!("{id}: a code site lacks a path or a snippet"));
                continue;
            }
            let body = file_cache.entry(path.to_string()).or_insert_with(|| {
                std::fs::read_to_string(repo().join(path))
                    .map(|s| norm(&s))
                    .unwrap_or_default()
            });
            if body.is_empty() {
                failures.push(format!("{id}: code site `{path}` does not exist"));
            } else if !body.contains(&norm(snippet)) {
                failures.push(format!(
                    "{id}: snippet not found verbatim in `{path}` (the code moved - fix the site)"
                ));
            }
        }
    }

    // Rule 1 + 5: the README badges.
    let readme = std::fs::read_to_string(repo().join("README.md")).expect("README.md");
    let badge_re = regex::Regex::new(r"Claude%20Code-(\d+\.\d+\.\d+)-").unwrap();
    match badge_re.captures(&readme) {
        Some(cap) => {
            if &cap[1] != verified {
                failures.push(format!(
                    "README badge says Claude Code {} but the ledger is verified at {verified}",
                    &cap[1]
                ));
            }
        }
        None => failures.push("README has no `verified against Claude Code` badge".to_string()),
    }
    let mutation_re = regex::Regex::new(r"mutation%20score-\d+(\.\d+)?%25").unwrap();
    if !mutation_re.is_match(&readme) {
        failures.push("README has no mutation-score badge".to_string());
    }

    assert!(
        failures.is_empty(),
        "INTROSPECTION.json gate failed ({} problem(s)):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
