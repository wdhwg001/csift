//! Rule 21: a `bytes` probe names a literal the build carries.
//!
//! A `bytes` probe is the machine-readable twin of an instrument that reads the BUILD, and
//! its whole content is the `literal` a later pass re-locates. Rule 15 checks that the field
//! is non-empty, and nothing checked that it was ever there: four shipped counting ZERO in
//! every build named on their state - two cut at a length that runs past where the build
//! stops agreeing, and two carrying a ` ... ` elision inside the literal, which is not a
//! byte string at all. A probe like that cannot be run, which is the one thing a probe is
//! for.
//!
//! WHICH BUILD, and why it is a SET. A state names up to three: the build its receipt was
//! made at, the build its range OPENS at, and - on a closed state - the build it ended at. A
//! literal is a property of a build rather than of the ledger, and a rewrite between two of
//! them is exactly what a `bytes` probe is often there to date, so the rule asks whether ANY
//! build the state names carries it. BASH-038's is the shape that forces this: the target
//! regex it dates was rewritten at the build its range opens at, so the verified build the
//! badge names carries it zero times and carrying it zero times there is the FACT.
//!
//! A CLOSED STATE'S PROBE IS READ TOO, which is where rule 12 and this rule part company.
//! An anchor is a byte LOCATION and a later build moves it, so re-reading a closed state's
//! anchors would demand that a superseded fact still sit at its old bytes; a probe's literal
//! is a STRING, and whether the builds a closed range names carry it is decidable on this
//! host whenever it holds them. One shipped that could not be: a literal counting zero at
//! every build of its own range, first occurring at the build the NEXT state opens at, so it
//! dated that event rather than its own. The skip is the same as everywhere else - a build
//! this host does not hold is counted and moved past, never passed.
//!
//! A build this host does not hold is COUNTED and skipped, exactly as rules 12 and 19 skip
//! what they cannot read, and the rule prints its own tally beside theirs. The skip is on
//! the SET: a literal the builds here do not carry is a defect only when every build the
//! state names was read, because the one that is missing is exactly where the literal would
//! be if the rewrite the probe dates is the reason.
//!
//! RULE 22 lives here too, because it is the same subject read from the other side: a
//! probe's KIND and its SCOPE must name the SAME artefact. The kind is decided by the
//! artefact the command reads and the scope is derived from the instrument, and nothing
//! compared the two - so thirty-eight states shipped claiming `corpus` while their scope
//! named a build, and one claimed `bytes` while its scope named a projects root. Each half
//! was defensible on its own and the pair was not, which is exactly the shape a rule with
//! no cross-check leaves behind.

use super::anchors::BuildCache;
use serde_json::Value;

/// The counts rule 21 prints: literals found in a build the state names, and literals whose
/// builds are all missing from this host.
#[derive(Debug, Default)]
pub(crate) struct Probes {
    pub(crate) found: usize,
    pub(crate) uncached: usize,
}

/// The builds one state names, newest claim first: the receipt's, the range's floor, and a
/// closed range's ceiling.
fn named_builds(state: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in [
        &state["verified"]["claude_code"],
        &state["from"],
        &state["to"],
    ] {
        if let Some(v) = key.as_str() {
            if !out.iter().any(|x| x == v) {
                out.push(v.to_string());
            }
        }
    }
    out
}

pub(crate) fn check(claims: &[Value], failures: &mut Vec<String>) -> Probes {
    let mut cache = BuildCache::default();
    let mut tally = Probes::default();
    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        for state in super::states(c) {
            if state["probe"]["kind"].as_str() != Some("bytes") {
                continue;
            }
            let literal = state["probe"]["literal"].as_str().unwrap_or("");
            if literal.is_empty() {
                continue; // rule 15 already names an empty literal
            }
            let names = named_builds(state);
            let mut missing = 0usize;
            let mut found = false;
            for name in &names {
                let Some(bytes) = cache.get(name) else {
                    missing += 1;
                    continue;
                };
                if memchr::memmem::find(bytes, literal.as_bytes()).is_some() {
                    found = true;
                    break;
                }
            }
            if found {
                tally.found += 1;
                continue;
            }
            // A literal absent from the builds READ is only a defect when there is no build
            // left to look in: the set exists because a rewrite between two of them is what
            // a `bytes` probe often dates, so one uncached build is a reading this host
            // cannot take rather than a probe nobody can run.
            if missing > 0 {
                tally.uncached += 1;
                continue;
            }
            failures.push(format!(
                "{id}: the `bytes` probe's literal occurs 0 times in every build this state \
                 names ({}) - a probe that counts nothing cannot be run, so cut the literal \
                 to a run the build carries (an elision inside it is never a byte string)",
                names.join(", ")
            ));
        }
    }
    tally
}

pub(crate) fn line(p: &Probes) -> String {
    format!(
        "ledger rule 21: bytes probes whose literal the build carries: {} ({} skipped, no \
         named build on this host)",
        p.found, p.uncached
    )
}

/// The words a SCOPE uses when the artefact it names is a BUILD.
///
/// `build` alone is not on the list and cannot be: a corpus scope says `the builds the
/// records name` without reading one. What is here is a noun for the FILE - a binary, an
/// executable, a bundle - and the two phrases this file uses for a set of them.
pub(crate) const BUILD_WORDS: [&str; 6] = [
    "binary",
    "executable",
    "bundle",
    "build file",
    "build ladder",
    "build the state names",
];

/// The words a SCOPE uses when the artefact it names is a POPULATION ON DISK.
///
/// The harness's session REGISTRY is one of those populations and was reachable only as a
/// path (`registry directory`), so a scope calling it by its English name - `the harness
/// session registry under the claude home` - read as a build to this rule and a `bytes`
/// kind sat beside it unchallenged. A row set on disk is a corpus population whichever of
/// its two names the scope uses.
pub(crate) const CORPUS_WORDS: [&str; 8] = [
    "projects root",
    "*.jsonl",
    "corpus",
    "transcripts",
    "registry directory",
    "session registry",
    "registry row",
    "jsonl population",
];

fn names<'a>(scope: &str, words: &[&'a str]) -> Option<&'a str> {
    let low = scope.to_ascii_lowercase();
    words.iter().copied().find(|w| low.contains(w))
}

/// Rule 22: a probe's KIND and its SCOPE name the SAME artefact.
///
/// The kind is decided by the artefact the command reads and the scope is derived from the
/// instrument, and nothing else asks whether the two agree. They can disagree: thirty-eight
/// states carried `corpus` beside a scope reading `the shipped Claude Code binary the state
/// names`, and one carried `bytes` beside `the top-level *.jsonl of one projects root`. Each
/// half is defensible alone, which is how such a pair ships - a reader asking the file which
/// probes read a build gets one answer from the kind and another from the scope.
///
/// The test is on the two kinds whose artefact is FIXED. `csift`, `live` and `repo` name a
/// run, a gesture and this tree, and any of the three can legitimately mention either noun -
/// a csift run reads transcripts, a live trial is taken at a build - so widening the rule to
/// them would report vocabulary rather than a contradiction.
///
/// It stops at the SCOPE and does not reach the `command`, deliberately. A scope is a NOUN
/// PHRASE and a word list decides it; a command is a program, and classifying one by the
/// words in it has the same false-negative shape the kind-and-scope pair had - a census walk
/// that happens to say `binary` would read as a build read, and a build grep piped through
/// python would read as neither. What holds the command instead is the procedure law (a
/// command RUNS as written, naming the artefact its kind reads) and rule 15, which refuses a
/// probe whose kind arrives without the field that answers it.
///
/// It runs over EVERY state, open and closed alike, for the reason rule 21 does: a probe is a
/// STRING naming an artefact, and whether it names the right one is decidable whenever the
/// state is readable at all.
pub(crate) fn kind_scope(claims: &[Value], failures: &mut Vec<String>) {
    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        for (i, state) in super::states(c).iter().enumerate() {
            let probe = &state["probe"];
            let (Some(kind), Some(scope)) = (probe["kind"].as_str(), probe["scope"].as_str())
            else {
                continue; // rule 15 names a probe with no kind or no scope
            };
            match kind {
                "corpus" => {
                    if let Some(word) = names(scope, &BUILD_WORDS) {
                        failures.push(format!(
                            "{id} states[{i}]: `probe.kind` is corpus while `probe.scope` \
                             names a BUILD ({word:?}) - the kind follows the artefact the \
                             command reads, so either the kind is bytes or the scope names \
                             the population the census is taken over"
                        ));
                    }
                }
                "bytes" => {
                    if let Some(word) = names(scope, &CORPUS_WORDS) {
                        failures.push(format!(
                            "{id} states[{i}]: `probe.kind` is bytes while `probe.scope` \
                             names a CORPUS population ({word:?}) - a bytes probe re-locates \
                             a literal in a build, so either the kind is corpus or the scope \
                             names the build the literal is counted in"
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}
