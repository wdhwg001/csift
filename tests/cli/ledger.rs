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
//! 4. Every cited code site exists, its snippet is found verbatim (whitespace
//!    normalized) in that file, and it is found INSIDE the declared `lines` range, which
//!    may not be wider than the snippet by more than two lines. So a refactor that moves
//!    the code fails here instead of silently orphaning the claim, and a range cannot be
//!    padded to the whole file to satisfy containment.
//! 5. The README also carries the mutation-score badge (a version-independent shape
//!    check; the score itself is the census's business).
//! 6. Every claim cites at least one code site: a claim with no site names a behavior
//!    csift does not demonstrably depend on, and the audit could never anchor it.
//! 7. Every claim states how completely its behavior is ATTRIBUTED (`attribution`, the
//!    closed set end-to-end | producer-only | specimen-only | by-elimination: whether
//!    the producing code was traced in the shipped binary and whether a specimen was
//!    observed) and, unless end-to-end, lists the `open_legs` an audit still has to
//!    close; an end-to-end claim carries NO open leg (a non-gap note goes to
//!    `residue`). A by-elimination claim can never carry a `holds` verdict: with
//!    neither leg traced, "nothing changed" is exactly the conclusion an audit is not
//!    entitled to.
//! 8. The README's ledger tally block equals the ledger's per-attribution counts and
//!    carries rule 12's anchor line, so the page states a number a reader re-derives.
//! 9. A claim's LATEST check is never `drifted`: a drift is a correctness task for the
//!    same release, so the fix (or the retirement) appends its own check after it, and
//!    a ledger whose last word on a claim is "drifted" is not releasable.
//! 10. No open leg is an unconsumed text instruction: a leg starting with `TEXT` is a
//!     claim-text correction a traced hop demanded, and a leg recording a rejected
//!     rewrite is the old, refuted text still shipping; both are consumed by rewriting
//!     the text, never released around.
//! 11. Every claim below end-to-end carries both leg fields (`producer_trace`,
//!     `specimen`), so rule 7's derivation always runs: an attribution with no legs
//!     recorded is an opinion, not a derivation.
//! 12. Every byte anchor still points at its own excerpt in the verified build: each
//!     `producer_chain` hop and `enumeration` entry carrying an `offset` must be
//!     byte-exact there, unless it is elided or marked (see `ledger/anchors.rs`). A MARK
//!     IS A CLAIM, NOT A PASS, so every mark is verified against the build too. A host
//!     that does not hold the build prints one line and passes. Rule 8 then requires the
//!     README to carry the same tally, so the page states a measured number.
//!
//! The attribution set gained `upstream` (v0.10.5): the producer lies OUTSIDE the
//! shipped binary by construction (the model or API side, the operating system, a
//! native runtime binding) while the client-side treatment is traced and a specimen is
//! observed; `producer_trace: upstream` + `specimen: observed` derives it.

mod anchors;

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
    let mut file_cache: HashMap<String, Option<FileText>> = HashMap::new();
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
        check_latest_verdict_and_leg_hygiene(id, c, &checks, &mut failures);
        check_code_sites(id, c, &mut file_cache, &mut failures);
    }

    // Rule 12: every byte anchor still carries its own excerpt in the verified build.
    let anchors = anchors::tally(claims, verified, &mut failures);

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
    check_readme_tally(claims, &anchors, verified, &readme, &mut failures);

    assert!(
        failures.is_empty(),
        "INTROSPECTION.json gate failed ({} problem(s)):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// A cited file, read once: the whole text normalized for the verbatim check, and the
/// raw lines for the range check.
struct FileText {
    whole: String,
    lines: Vec<String>,
}

fn file_text<'a>(
    cache: &'a mut HashMap<String, Option<FileText>>,
    path: &str,
) -> Option<&'a FileText> {
    cache
        .entry(path.to_string())
        .or_insert_with(|| {
            std::fs::read_to_string(repo().join(path))
                .ok()
                .map(|s| FileText {
                    whole: norm(&s),
                    lines: s.lines().map(str::to_string).collect(),
                })
        })
        .as_ref()
}

/// `N` or `A-B`, 1-based inclusive.
fn line_range(spec: &str) -> Option<(usize, usize)> {
    let (a, b) = spec.split_once('-').unwrap_or((spec, spec));
    let a: usize = a.trim().parse().ok()?;
    let b: usize = b.trim().parse().ok()?;
    (a > 0 && b >= a).then_some((a, b))
}

/// A range is the snippet's OWN span, not a window around it: containment alone accepts
/// `1-99999`, which clamps to the file and passes while pointing a reader at everything.
/// Two lines of slack cover a signature that wraps or an attribute line above.
/// `Some((declared width, snippet lines))` when the range is padded.
fn range_too_wide(first: usize, last: usize, snippet: &str) -> Option<(usize, usize)> {
    let width = last - first + 1;
    let lines = snippet.split('\n').count();
    (width > lines + 2).then_some((width, lines))
}

/// Rule 4: every code site is real, its snippet still exists verbatim, and it sits
/// INSIDE the declared `lines`. The range is documentation - a reader jumps to it - so a
/// range that no longer holds its own snippet points the reader at unrelated code.
fn check_code_sites(
    id: &str,
    c: &serde_json::Value,
    cache: &mut HashMap<String, Option<FileText>>,
    failures: &mut Vec<String>,
) {
    for site in c["code"].as_array().cloned().unwrap_or_default() {
        let path = site["path"].as_str().unwrap_or("");
        let snippet = site["snippet"].as_str().unwrap_or("");
        if path.is_empty() || snippet.trim().is_empty() {
            failures.push(format!("{id}: a code site lacks a path or a snippet"));
            continue;
        }
        let Some(text) = file_text(cache, path) else {
            failures.push(format!("{id}: code site `{path}` does not exist"));
            continue;
        };
        let want = norm(snippet);
        if !text.whole.contains(&want) {
            failures.push(format!(
                "{id}: snippet not found verbatim in `{path}` (the code moved - fix the site)"
            ));
            continue;
        }
        let spec = site["lines"].as_str().unwrap_or("");
        let Some((first, last)) = line_range(spec) else {
            failures.push(format!(
                "{id}: code site `{path}` has `lines` `{spec}`, which is neither `N` nor `A-B`"
            ));
            continue;
        };
        let start = (first - 1).min(text.lines.len());
        let end = last.min(text.lines.len()).max(start);
        if !norm(&text.lines[start..end].join("\n")).contains(&want) {
            failures.push(format!(
                "{id}: snippet is not inside `{path}` lines {spec} \
                 (the range drifted - recompute it from the snippet)"
            ));
        }
        if let Some((width, lines)) = range_too_wide(first, last, snippet) {
            failures.push(format!(
                "{id}: `{path}` lines {spec} spans {width} line(s) for a {lines}-line snippet \
                 (a range is the snippet's own span, not a window around it)"
            ));
        }
    }
}

/// Rule 4's width bound: containment alone accepts a range padded to the whole file.
#[test]
fn ledger_gate_a_declared_range_may_not_be_padded() {
    let three = "one\ntwo\nthree";
    // Exact, and the two lines of slack, are accepted.
    assert_eq!(range_too_wide(10, 12, three), None);
    assert_eq!(range_too_wide(10, 14, three), None);
    // A third padding line is not.
    assert_eq!(range_too_wide(10, 15, three), Some((6, 3)));
    // The shape the bound exists for: a range that clamps to the file and passes
    // containment while naming everything.
    assert_eq!(range_too_wide(1, 99999, three), Some((99999, 3)));
    // A one-line snippet is the common case and keeps its own slack.
    assert_eq!(range_too_wide(7, 7, "fn f() {}"), None);
    assert_eq!(range_too_wide(7, 10, "fn f() {}"), Some((4, 1)));
}

/// Rule 8, second half: the README states the measured anchor tally, so the page's claim
/// about byte offsets is a number a reader can re-derive. The three ledger-derived counts
/// are checked everywhere; the byte-exact / elided split needs the build, so it is checked
/// only on a host that holds it.
fn check_anchor_line(
    a: &anchors::Anchors,
    verified: &str,
    block: &str,
    failures: &mut Vec<String>,
) {
    let re = regex::Regex::new(
        r"anchors byte-exact at Claude Code (\d+\.\d+\.\d+): ([\d,]+) of ([\d,]+) \(elided ([\d,]+), absent ([\d,]+), prefix-only ([\d,]+)\)",
    )
    .unwrap();
    let Some(cap) = re.captures(block) else {
        failures.push(
            "README ledger tally carries no `anchors byte-exact at Claude Code ...` line"
                .to_string(),
        );
        return;
    };
    if &cap[1] != verified {
        failures.push(format!(
            "README anchor line names Claude Code {} but the ledger is verified at {verified}",
            &cap[1]
        ));
    }
    let num = |i: usize| {
        cap[i]
            .replace(',', "")
            .parse::<usize>()
            .unwrap_or(usize::MAX)
    };
    let mut expected = vec![
        ("total anchors", num(3), a.total),
        ("absent", num(5), a.absent),
        ("prefix-only", num(6), a.prefix_only),
    ];
    if let Some((exact, elided)) = a.measured {
        expected.push(("byte-exact", num(2), exact));
        expected.push(("elided", num(4), elided));
    }
    for (label, said, is) in expected {
        if said != is {
            failures.push(format!(
                "README anchor line says {label} {said} but the ledger has {is}"
            ));
        }
    }
}

/// Rule 8: the README's ledger tally (the table between the `ledger-tally` markers)
/// carries one row per attribution value plus a total, and every count equals the
/// ledger's - the table is regenerated from the ledger, never typed.
fn check_readme_tally(
    claims: &[serde_json::Value],
    anchors: &anchors::Anchors,
    verified: &str,
    readme: &str,
    failures: &mut Vec<String>,
) {
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
        "upstream",
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
    check_anchor_line(anchors, verified, block, failures);
}

/// Rules 9 and 10: the last word on a claim is never `drifted`, and no open leg is an
/// unconsumed text instruction or a record of a rejected rewrite.
fn check_latest_verdict_and_leg_hygiene(
    id: &str,
    c: &serde_json::Value,
    checks: &[serde_json::Value],
    failures: &mut Vec<String>,
) {
    if let Some(last) = checks.last() {
        if last["verdict"].as_str() == Some("drifted") {
            failures.push(format!(
                "{id}: the latest check is `drifted` with no fix or retirement check after it (a drift is a same-release correctness task)"
            ));
        }
    }
    for leg in c["open_legs"].as_array().cloned().unwrap_or_default() {
        let s = leg.as_str().unwrap_or("").trim_start();
        if s.starts_with("TEXT") {
            failures.push(format!(
                "{id}: an open leg is an unconsumed text correction (`TEXT ...`): rewrite the claim text"
            ));
        } else if s.starts_with("Text rewrite rejected")
            || s.contains("was rejected on adversarial re-read")
        {
            failures.push(format!(
                "{id}: an open leg records a rejected rewrite, so the refuted text is still shipping: rewrite it to acceptance, split, or retire"
            ));
        }
    }
}

/// Rule 7: attribution completeness is stated, open legs are listed, the attribution
/// agrees with the two legs when they are recorded, and a claim attributed by
/// elimination never holds. Rule 11: below end-to-end the legs are always recorded.
fn check_attribution(id: &str, c: &serde_json::Value, failures: &mut Vec<String>) {
    const ATTRIBUTIONS: [&str; 6] = [
        "end-to-end",
        "producer-only",
        "specimen-only",
        "partial-producer",
        "by-elimination",
        "upstream",
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
            ("upstream", "observed") => "upstream",
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
    } else if attribution != "end-to-end" {
        // Rule 11: below end-to-end an attribution without its legs is an opinion.
        failures.push(format!(
            "{id}: attribution `{attribution}` with no `producer_trace`/`specimen` legs recorded"
        ));
    }
    let open_legs = c["open_legs"].as_array().cloned().unwrap_or_default();
    let legs_named = open_legs
        .iter()
        .any(|l| l.as_str().is_some_and(|s| !s.trim().is_empty()));
    // An open leg is a leg that is open. `end-to-end` has none by definition. `upstream`
    // closes the PRODUCER leg by construction (the producer is outside the shipped binary
    // and `upstream_reason` names the domain), so it needs no leg either - but it may
    // still carry one for a specimen or a sub-fact that a live capture would settle.
    if !matches!(attribution, "end-to-end" | "upstream") && !legs_named {
        failures.push(format!(
            "{id}: attribution `{attribution}` without a non-empty `open_legs` entry"
        ));
    }
    if attribution == "end-to-end" && legs_named {
        failures.push(format!(
            "{id}: end-to-end with a non-empty `open_legs` (close the leg, or move a non-gap note to `residue`)"
        ));
    }
    if attribution == "upstream"
        && c["upstream_reason"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
    {
        failures.push(format!(
            "{id}: attribution `upstream` without an `upstream_reason` naming the producer domain"
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
