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
//! 7. Every claim states how completely its behavior is ATTRIBUTED (`attribution`, the
//!    closed set end-to-end | producer-only | specimen-only | by-elimination: whether
//!    the producing code was traced in the shipped binary and whether a specimen was
//!    observed) and, unless end-to-end, lists the `open_legs` an audit still has to
//!    close. A by-elimination claim can never carry a `holds` verdict: with neither leg
//!    traced, "nothing changed" is exactly the conclusion an audit is not entitled to.

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
        check_attribution(id, c, &mut failures);
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
    check_readme_tally(claims, &readme, &mut failures);

    assert!(
        failures.is_empty(),
        "INTROSPECTION.json gate failed ({} problem(s)):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// Rule 8: the README's ledger tally (the table between the `ledger-tally` markers)
/// carries one row per attribution value plus a total, and every count equals the
/// ledger's - the table is regenerated from the ledger, never typed.
fn check_readme_tally(claims: &[serde_json::Value], readme: &str, failures: &mut Vec<String>) {
    let Some(start) = readme.find("<!-- ledger-tally:begin -->") else {
        failures.push("README has no ledger-tally table".to_string());
        return;
    };
    let block = &readme[start..];
    let block = &block[..block
        .find("<!-- ledger-tally:end -->")
        .unwrap_or(block.len())];
    let row_re = regex::Regex::new(r"(?m)^\| ([a-z-]+) \| (\d+) \|").unwrap();
    let mut rows: HashMap<String, usize> = HashMap::new();
    for cap in row_re.captures_iter(block) {
        rows.insert(cap[1].to_string(), cap[2].parse().unwrap_or(usize::MAX));
    }
    for key in [
        "end-to-end",
        "producer-only",
        "specimen-only",
        "partial-producer",
        "by-elimination",
    ] {
        let expected = claims
            .iter()
            .filter(|c| c["attribution"].as_str() == Some(key))
            .count();
        match rows.get(key) {
            Some(n) if *n == expected => {}
            Some(n) => failures.push(format!(
                "README tally row `{key}` says {n} but the ledger has {expected}"
            )),
            None => failures.push(format!("README tally has no `{key}` row")),
        }
    }
    match rows.get("total") {
        Some(n) if *n == claims.len() => {}
        _ => failures.push(format!(
            "README tally total does not equal the ledger's {} claims",
            claims.len()
        )),
    }
}

/// Rule 7: attribution completeness is stated, open legs are listed, the attribution
/// agrees with the two legs when they are recorded, and a claim attributed by
/// elimination never holds.
fn check_attribution(id: &str, c: &serde_json::Value, failures: &mut Vec<String>) {
    const ATTRIBUTIONS: [&str; 5] = [
        "end-to-end",
        "producer-only",
        "specimen-only",
        "partial-producer",
        "by-elimination",
    ];
    let attribution = c["attribution"].as_str().unwrap_or("");
    if !ATTRIBUTIONS.contains(&attribution) {
        failures.push(format!(
            "{id}: `attribution` `{attribution}` is not in {ATTRIBUTIONS:?}"
        ));
    }
    // The legs, when recorded, DERIVE the attribution: complete+observed = end-to-end,
    // complete+none = producer-only, partial|none+observed = specimen-only,
    // partial+none = partial-producer, none+none = by-elimination.
    if let (Some(producer), Some(specimen)) = (c["producer_trace"].as_str(), c["specimen"].as_str())
    {
        let derived = match (producer, specimen) {
            ("complete", "observed") => "end-to-end",
            ("complete", "none") => "producer-only",
            ("partial" | "none", "observed") => "specimen-only",
            ("partial", "none") => "partial-producer",
            ("none", "none") => "by-elimination",
            _ => "",
        };
        if derived.is_empty() {
            failures.push(format!(
                "{id}: legs producer_trace `{producer}` / specimen `{specimen}` are not from the closed sets"
            ));
        } else if derived != attribution {
            failures.push(format!(
                "{id}: attribution `{attribution}` does not follow from the legs (`{producer}` + `{specimen}` = `{derived}`)"
            ));
        }
    }
    let open_legs = c["open_legs"].as_array().cloned().unwrap_or_default();
    let legs_named = open_legs
        .iter()
        .any(|l| l.as_str().is_some_and(|s| !s.trim().is_empty()));
    if attribution != "end-to-end" && !legs_named {
        failures.push(format!(
            "{id}: attribution `{attribution}` without a non-empty `open_legs` entry"
        ));
    }
    if attribution == "by-elimination"
        && c["checks"]
            .as_array()
            .is_some_and(|ks| ks.iter().any(|k| k["verdict"].as_str() == Some("holds")))
    {
        failures.push(format!(
            "{id}: a by-elimination claim carries a `holds` verdict (trace the producer or the specimen first)"
        ));
    }
}
