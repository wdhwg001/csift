//! Rules 2, 3, 4, 6, 7, 9 and 11: the receipt, the code sites and the attribution.
//!
//! Each rule is a free function over the parsed JSON, so `ledger/specimens.rs` can run it
//! against a hand-built claim and read the failure it names. Nothing here touches the
//! README or the Claude Code build; those are `tally.rs` and `anchors.rs`.

use super::{norm, open_state, repo};
use serde_json::Value;
use std::collections::HashMap;

pub(crate) const VERDICTS: [&str; 4] = ["holds", "refined", "drifted", "unverifiable-here"];
pub(crate) const ATTRIBUTIONS: [&str; 6] = [
    "end-to-end",
    "producer-only",
    "specimen-only",
    "partial-producer",
    "by-elimination",
    "upstream",
];

/// Rule 1 BEFORE rule 2: a badge version no receipt was made at is ONE fact, not 739.
///
/// Bumping `verified_claude_code` ahead of the audit is the ordinary way to reach this gate,
/// and rule 2 then fires once per claim - 739 lines saying the same thing, with the line
/// that explains them (the badge moved) somewhere in the middle. So the driver asks this
/// first: when NO open state carries a receipt at the verified build, it reports the version
/// once, naming the build the receipts DO stand at, and rule 2's version arm stands down for
/// every claim. Every other arm of rule 2 still runs, because a verdict or an instrument is
/// wrong independently of which build the badge names.
pub(crate) fn badge_stands(claims: &[&Value], verified: &str) -> Option<String> {
    if claims.is_empty() {
        return None;
    }
    let mut at: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for c in claims {
        let Some(state) = open_state(c) else { continue };
        let v = state["verified"]["claude_code"]
            .as_str()
            .unwrap_or("<none>")
            .to_string();
        *at.entry(v).or_default() += 1;
    }
    if at.is_empty() || at.contains_key(verified) {
        return None;
    }
    let stand: Vec<String> = at.iter().map(|(v, n)| format!("{v} on {n}")).collect();
    Some(format!(
        "`verified_claude_code` is {verified}, and NO open state carries a receipt at it - \
         the receipts stand at {}. Rule 2 would name every claim; the badge moved ahead of \
         the audit, which is one fact. Run the audit at {verified} and rewrite each open \
         state's receipt, or put the badge back.",
        stand.join(", ")
    ))
}

/// Rules 2, 3 and 9: the OPEN state's receipt is made at the verified build, carries a
/// closed-set verdict that is not `drifted`, and its evidence is unique across the ledger.
pub(crate) fn receipts(
    id: &str,
    claim: &Value,
    verified: &str,
    report_version: bool,
    evidence: &mut HashMap<String, String>,
    failures: &mut Vec<String>,
) {
    let Some(state) = open_state(claim) else {
        return; // rule 13 already named it
    };
    let receipt = &state["verified"];
    if report_version {
        match receipt["claude_code"].as_str() {
            Some(v) if v == verified => {}
            other => failures.push(format!(
                "{id}: the open state's receipt is at {other:?}, not at Claude Code {verified}"
            )),
        }
    }
    let verdict = receipt["verdict"].as_str().unwrap_or("");
    if !VERDICTS.contains(&verdict) {
        failures.push(format!("{id}: verdict `{verdict}` is not in {VERDICTS:?}"));
    }
    if verdict == "drifted" {
        failures.push(format!(
            "{id}: the open state's verdict is `drifted` - a drift closes that state's range and opens the next one"
        ));
    }
    let instrument = receipt["instrument"].as_str().unwrap_or("").trim();
    let observed = receipt["observed"].as_str().unwrap_or("").trim();
    // Every receipt names its instrument: for a decided verdict the command that ran, for
    // `unverifiable-here` the instrument that WOULD decide it. An empty instrument is the
    // shape of a placeholder, and a placeholder never backs a badge.
    if instrument.is_empty() {
        failures.push(format!("{id}: `{verdict}` without an instrument"));
    }
    if observed.is_empty() {
        failures.push(format!(
            "{id}: the open state's receipt has an empty `observed`"
        ));
    }
    if verdict != "unverifiable-here" {
        let key = format!("{}|{}", norm(instrument), norm(observed));
        if let Some(other) = evidence.insert(key, id.to_string()) {
            failures.push(format!(
                "{id}: evidence (instrument, observed) identical to {other}'s - \
                 a receipt is written per claim, never replaced mechanically"
            ));
        }
    }
}

/// A cited file, read once: the whole text normalized for the verbatim check, and the
/// raw lines for the range check.
pub(crate) struct FileText {
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
pub(crate) fn range_too_wide(first: usize, last: usize, snippet: &str) -> Option<(usize, usize)> {
    let width = last - first + 1;
    let lines = snippet.split('\n').count();
    (width > lines + 2).then_some((width, lines))
}

/// Rules 6 and 4: a claim cites at least one code site; every site is real, its snippet
/// still exists verbatim, and it sits INSIDE the declared `lines`. The range is
/// documentation - a reader jumps to it - so a range that does not hold its own snippet
/// points the reader at unrelated code. `code[]` stays at CLAIM level under schema 2:
/// csift's tree is one tree at any time, so where a fact is relied on is not a property
/// of a Claude Code version range.
pub(crate) fn code_sites(
    id: &str,
    claim: &Value,
    cache: &mut HashMap<String, Option<FileText>>,
    failures: &mut Vec<String>,
) {
    let sites = claim["code"].as_array().cloned().unwrap_or_default();
    if sites.is_empty() {
        failures.push(format!(
            "{id}: no code site (every claim cites at least one)"
        ));
        return;
    }
    for site in sites {
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

/// Rule 7: attribution completeness is stated PER STATE, open legs are listed, the
/// attribution agrees with the two legs when they are recorded, and a state attributed by
/// elimination never holds. Rule 11: below end-to-end the legs are always recorded.
pub(crate) fn attribution(at: &str, state: &Value, failures: &mut Vec<String>) {
    let attribution = state["attribution"].as_str().unwrap_or("");
    if !ATTRIBUTIONS.contains(&attribution) {
        failures.push(format!(
            "{at}: `attribution` `{attribution}` is not in {ATTRIBUTIONS:?}"
        ));
    }
    // The legs, when recorded, DERIVE the attribution: complete+observed = end-to-end,
    // complete+none = producer-only, partial|none+observed = specimen-only,
    // partial+none = partial-producer, none+none = by-elimination.
    if let (Some(producer), Some(specimen)) =
        (state["producer_trace"].as_str(), state["specimen"].as_str())
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
                "{at}: legs producer_trace `{producer}` / specimen `{specimen}` are not from the closed sets"
            ));
        } else if derived != attribution {
            failures.push(format!(
                "{at}: attribution `{attribution}` does not follow from the legs (`{producer}` + `{specimen}` = `{derived}`)"
            ));
        }
    } else if attribution != "end-to-end" {
        // Rule 11: below end-to-end an attribution without its legs is an opinion.
        failures.push(format!(
            "{at}: attribution `{attribution}` with no `producer_trace`/`specimen` legs recorded"
        ));
    }
    // A TRACED leg carries the hops it claims. `complete` means three hops were read in the
    // shipped binary as verbatim excerpts AT BYTE OFFSETS, and `partial` means a template or
    // field was found - so a state declaring either and carrying no such hop is asserting a
    // reading the file does not hold, and the whole of rule 12 then has nothing to re-read.
    // Only an OPEN state is asked: a closed one records what was read at a build this pass no
    // longer compares against.
    if state["to"].is_null() {
        let hops = state["producer_chain"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        match state["producer_trace"].as_str() {
            Some("complete") if !hops.iter().any(|h| h["offset"].is_u64()) => {
                failures.push(format!(
                    "{at}: `producer_trace: complete` with no offset-carrying `producer_chain` \
                     hop - a complete trace is three hops read at byte offsets, and this state \
                     names {} hop(s), none of them located",
                    hops.len()
                ));
            }
            Some("partial") if hops.is_empty() => {
                failures.push(format!(
                    "{at}: `producer_trace: partial` with an empty `producer_chain` - `partial` \
                     asserts a template or a field was FOUND; with nothing read the leg is `none`"
                ));
            }
            _ => {}
        }
    }
    let legs_named = state["open_legs"].as_array().is_some_and(|l| {
        l.iter()
            .any(|x| x.as_str().is_some_and(|s| !s.trim().is_empty()))
    });
    // An open leg is a leg that is open. `end-to-end` has none by definition. `upstream`
    // closes the PRODUCER leg by construction (the producer is outside the shipped binary
    // and `upstream_reason` names the domain), so it needs no leg either - but it may
    // still carry one for a specimen or a sub-fact that a live capture would settle.
    if !matches!(attribution, "end-to-end" | "upstream") && !legs_named {
        failures.push(format!(
            "{at}: attribution `{attribution}` without a non-empty `open_legs` entry"
        ));
    }
    if attribution == "end-to-end" && legs_named {
        failures.push(format!(
            "{at}: end-to-end with a non-empty `open_legs` (close the leg, or move a non-gap note to `residue`)"
        ));
    }
    if attribution == "upstream"
        && state["upstream_reason"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
    {
        failures.push(format!(
            "{at}: attribution `upstream` without an `upstream_reason` naming the producer domain"
        ));
    }
    if attribution == "by-elimination" && state["verified"]["verdict"].as_str() == Some("holds") {
        failures.push(format!(
            "{at}: a by-elimination state carries a `holds` verdict (trace the producer or the specimen first)"
        ));
    }
}
