//! Rule 21's specimen: a `bytes` probe naming a literal no build carries.
//!
//! The rule needs a build to decide anything, so every assertion here is guarded on the
//! tally: on a host holding none of the named builds the probe is counted and skipped, which
//! is the rule working rather than the rule absent.

use super::good_claim;
use crate::ledger::probes;
use serde_json::json;

/// The literal every Claude Code build of this line carries, and one no build does.
const PRESENT: &str = "CLAUDE_CODE_USE_POWERSHELL_TOOL";
const ABSENT: &str = "zzq-no-build-carries-these-bytes-zzq";

fn with_probe(literal: &str, from: &str) -> serde_json::Value {
    let mut c = good_claim();
    c["states"][0]["from"] = json!(from);
    c["states"][0]["probe"] = json!({
        "kind": "bytes",
        "literal": literal,
        "scope": "the one Claude Code build file the state's range names",
        "rule": "raw byte occurrences of the literal in that build",
    });
    c
}

#[test]
fn ledger_gate_rule_21_names_a_bytes_probe_no_build_can_run() {
    let c = with_probe(ABSENT, "2.1.258");
    let mut f = Vec::new();
    let tally = probes::check(std::slice::from_ref(&c), &mut f);
    if tally.uncached == 0 {
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("occurs 0 times in every build"), "{}", f[0]);
        assert!(f[0].contains("TEST-001"), "{}", f[0]);
    }

    let c = with_probe(PRESENT, "2.1.258");
    let mut f = Vec::new();
    let tally = probes::check(std::slice::from_ref(&c), &mut f);
    assert!(f.is_empty(), "{f:?}");
    if tally.uncached == 0 {
        assert_eq!(tally.found, 1);
    }
}

/// The build is a SET, and that is what lets a probe DATE a rewrite: a literal the verified
/// build carries zero times, present in the build the state's range opens at, is the fact
/// rather than a defect. And when one of the named builds is not on this host, the absence
/// is counted rather than named - the missing one is exactly where the literal would be.
#[test]
fn ledger_gate_rule_21_reads_every_build_the_state_names() {
    let c = with_probe(ABSENT, "2.1.999");
    let mut f = Vec::new();
    let tally = probes::check(std::slice::from_ref(&c), &mut f);
    assert!(
        f.is_empty(),
        "an unread build in the set is a reading this host cannot take: {f:?}"
    );
    assert_eq!(tally.uncached, 1);
}

/// Rule 21 asks the OPEN state only, for rule 12's reason: a closed state's probe was
/// runnable at the build its range ended on, which this host may no longer hold.
#[test]
fn ledger_gate_rule_21_leaves_a_closed_state_alone() {
    let mut c = with_probe(ABSENT, "2.1.100");
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("2.1.200");
    c["states"][0]["from"] = json!("2.1.201");
    c["states"][0]["probe"]["literal"] = json!(PRESENT);
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    probes::check(std::slice::from_ref(&c), &mut f);
    assert!(f.is_empty(), "{f:?}");
}
