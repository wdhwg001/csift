//! Rules 2, 3, 4, 6, 7, 9, 10, 11 and 13: what one claim and one state must say.
//!
//! Each rule is a free function over the parsed JSON, so `ledger/specimens.rs` can run it
//! against a hand-built claim and read the failure it names. Nothing here touches the
//! README or the Claude Code build; those are `tally.rs` and `anchors.rs`.

use super::{norm, open_state, repo, states, version_key, version_triple};
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

/// Rule 13: the shape of the state list. One or two states, exactly one open, a `from` on
/// every one, a `to` on every closed one, ordered by `from`, and the OPEN state's range
/// opens at or below the verified build unless it says why not. Two states is the ceiling
/// because a claim is a fact and a fact has a current reading and at most one superseded
/// one; anything older is git's business or a backward-compatibility claim of its own.
pub(crate) fn state_shape(id: &str, claim: &Value, verified: &str, failures: &mut Vec<String>) {
    let sts = states(claim);
    if sts.is_empty() || sts.len() > 2 {
        failures.push(format!(
            "{id}: {} state(s) (a claim carries one or two)",
            sts.len()
        ));
        return;
    }
    let open = sts.iter().filter(|s| s["to"].is_null()).count();
    if open != 1 {
        failures.push(format!(
            "{id}: {open} open state(s) (exactly one state is open)"
        ));
    }
    let mut keys = Vec::new();
    for (i, s) in sts.iter().enumerate() {
        match s["from"].as_str() {
            Some(f) if version_triple(f) => keys.push(version_key(f).unwrap_or_default()),
            other => failures.push(format!(
                "{id} states[{i}]: `from` is {other:?}, not a Claude Code version"
            )),
        }
        if !s["to"].is_null() && !s["to"].as_str().is_some_and(version_triple) {
            failures.push(format!(
                "{id} states[{i}]: `to` is {:?}, neither null nor a Claude Code version",
                s["to"]
            ));
        }
    }
    if keys.windows(2).any(|w| w[0] > w[1]) {
        failures.push(format!("{id}: states are not ordered by `from`"));
    }
    // A state that opens ABOVE the badge's build is a behavior the verified build does not
    // have, while its receipt is made at that build. The shape is real - schema 1 forced
    // every check to carry the badge version - so it is allowed, and it owes its reason.
    if let (Some(open_state), Some(vk)) = (open_state(claim), version_key(verified)) {
        if let Some(fk) = open_state["from"].as_str().and_then(version_key) {
            if fk > vk
                && open_state["from_after_verified"]
                    .as_str()
                    .is_none_or(|s| s.trim().is_empty())
            {
                failures.push(format!(
                    "{id}: the open state opens at {}, above the verified build {verified}, \
                     with no `from_after_verified` saying why",
                    open_state["from"].as_str().unwrap_or("")
                ));
            }
        }
    }
    if let Some(c) = claim["compat_of"].as_str() {
        if c.trim().is_empty() {
            failures.push(format!("{id}: `compat_of` is empty"));
        }
    }
}

/// Every state names the behavior, what csift depends on, and the instrument that
/// re-verifies it; the machine-readable `probe` twin names a kind from the closed set.
pub(crate) fn state_text(at: &str, state: &Value, failures: &mut Vec<String>) {
    for key in ["behavior", "depends", "instrument"] {
        if state[key].as_str().is_none_or(|s| s.trim().is_empty()) {
            failures.push(format!("{at}: `{key}` is empty"));
        }
    }
    const KINDS: [&str; 4] = ["bytes", "corpus", "csift", "live"];
    match state["probe"]["kind"].as_str() {
        Some(k) if KINDS.contains(&k) => {}
        other => failures.push(format!("{at}: `probe.kind` is {other:?}, not in {KINDS:?}")),
    }
}

/// Rules 2, 3 and 9: the OPEN state's receipt is made at the verified build, carries a
/// closed-set verdict that is not `drifted`, and its evidence is unique across the ledger.
pub(crate) fn receipts(
    id: &str,
    claim: &Value,
    verified: &str,
    evidence: &mut HashMap<String, String>,
    failures: &mut Vec<String>,
) {
    let Some(state) = open_state(claim) else {
        return; // rule 13 already named it
    };
    let receipt = &state["verified"];
    match receipt["claude_code"].as_str() {
        Some(v) if v == verified => {}
        other => failures.push(format!(
            "{id}: the open state's receipt is at {other:?}, not at Claude Code {verified}"
        )),
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
/// documentation - a reader jumps to it - so a range that no longer holds its own snippet
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

/// Rule 10: no open leg is an unconsumed text instruction or a record of a rejected
/// rewrite. Both mean a refuted text is still shipping, and both are consumed only by
/// rewriting the claim.
pub(crate) fn leg_hygiene(at: &str, state: &Value, failures: &mut Vec<String>) {
    for leg in state["open_legs"].as_array().cloned().unwrap_or_default() {
        let s = leg.as_str().unwrap_or("").trim_start();
        if s.starts_with("TEXT") {
            failures.push(format!(
                "{at}: an open leg is an unconsumed text correction (`TEXT ...`): rewrite the claim text"
            ));
        } else if s.starts_with("Text rewrite rejected")
            || s.contains("was rejected on adversarial re-read")
        {
            failures.push(format!(
                "{at}: an open leg records a rejected rewrite, so the refuted text is still shipping: rewrite it to acceptance, split, or retire"
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
