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
//! OPEN STATES ONLY, for rule 12's reason: a closed state's probe was runnable at the build
//! its range ended on, and a host that no longer holds that build cannot decide it.
//!
//! A build this host does not hold is COUNTED and skipped, exactly as rules 12 and 19 skip
//! what they cannot read, and the rule prints its own tally beside theirs. The skip is on
//! the SET: a literal the builds here do not carry is a defect only when every build the
//! state names was read, because the one that is missing is exactly where the literal would
//! be if the rewrite the probe dates is the reason.

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
            if !state["to"].is_null() {
                continue;
            }
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
