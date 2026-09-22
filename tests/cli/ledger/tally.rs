//! Rules 1, 5 and 8: what the README must say, and why it is admitted only through here.
//!
//! The page claims two measured things - "verified against Claude Code X" and a table of
//! how completely each behavior is attributed - and both are numbers a reader is entitled
//! to re-derive. Rule 8's block is REGENERATED from the ledger, never typed, and every
//! figure in it is a count over the ledger's OPEN states: a closed state records what was
//! true of an older build, and the page speaks about the build the badge names.

use super::{anchors, open_state};
use regex::Regex;
use serde_json::Value;

/// Rules 1 and 5: the two badges. The Claude Code version is admitted only through the
/// ledger; the mutation score is a shape check, since the score itself is the census's
/// business.
pub(crate) fn badges(verified: &str, readme: &str, failures: &mut Vec<String>) {
    let badge = Regex::new(r"Claude%20Code-(\d+\.\d+\.\d+)-").unwrap();
    match badge.captures(readme) {
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
    let mutation = Regex::new(r"mutation%20score-\d+(\.\d+)?%25").unwrap();
    if !mutation.is_match(readme) {
        failures.push("README has no mutation-score badge".to_string());
    }
}

/// Rule 8, second half: the README states the measured anchor tally, so the page's claim
/// about byte offsets is a number a reader can re-derive. The ledger-derived counts are
/// checked everywhere; the byte-exact / elided split needs the build, so it is checked
/// only on a host that holds it.
fn anchor_line(a: &anchors::Anchors, verified: &str, block: &str, failures: &mut Vec<String>) {
    let re = Regex::new(
        r"anchors byte-exact at Claude Code (\d+\.\d+\.\d+): ([\d,]+) of ([\d,]+) \(elided ([\d,]+), absent ([\d,]+), prefix-only ([\d,]+)\), closed-state anchors ([\d,]+), not checked",
    )
    .unwrap();
    let Some(cap) = re.captures(block) else {
        failures.push(
            "README ledger tally carries no `anchors byte-exact at Claude Code ... closed-state anchors N, not checked` line"
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
        ("closed-state anchors", num(7), a.closed),
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

/// One attribution row's count: claims whose OPEN state carries that value.
pub(crate) fn attribution_count(claims: &[Value], key: &str) -> usize {
    claims
        .iter()
        .filter(|c| open_state(c).is_some_and(|s| s["attribution"].as_str() == Some(key)))
        .count()
}

/// Rule 8: the README's ledger tally (the table between the `ledger-tally` markers)
/// carries one row per attribution value plus a total, and every count equals the
/// ledger's - the table is regenerated from the ledger, never typed.
pub(crate) fn ledger_tally(
    claims: &[Value],
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
    let row_re = Regex::new(r"(?m)^\| ([a-z-]+) \| (\d+) \|").unwrap();
    let mut rows: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for cap in row_re.captures_iter(block) {
        rows.insert(cap[1].to_string(), cap[2].parse().unwrap_or(usize::MAX));
    }
    for key in super::rules::ATTRIBUTIONS {
        let expected = attribution_count(claims, key);
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
    anchor_line(anchors, verified, block, failures);
}
