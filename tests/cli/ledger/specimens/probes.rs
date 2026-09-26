//! Rule 21's specimen: a `bytes` probe naming a literal no build carries. Rule 22's is here
//! too, because it reads the same field from the other side - whether the KIND and the SCOPE
//! name the same artefact.
//!
//! Rule 21 needs a build to decide anything, so every assertion for it is guarded on the
//! tally: on a host holding none of the named builds the probe is counted and skipped, which
//! is the rule working rather than the rule absent. Rule 22 needs no build at all - it reads
//! two strings of the ledger against each other - so its specimens are unconditional.

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

/// Rule 21 reads a CLOSED state too, which is where it parts company with rule 12. An
/// anchor is a byte LOCATION and a later build moves it; a probe's literal is a STRING, and
/// whether the builds a closed range names carry it is decidable wherever this host holds
/// them. One shipped that could not be run at all: a literal counting zero at every build of
/// its own range, first occurring at the build the NEXT state opens at.
#[test]
fn ledger_gate_rule_21_reads_a_closed_state_whose_builds_this_host_holds() {
    let mut c = with_probe(ABSENT, "2.1.100");
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("2.1.200");
    c["states"][0]["from"] = json!("2.1.201");
    c["states"][0]["probe"]["literal"] = json!(PRESENT);
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    let tally = probes::check(std::slice::from_ref(&c), &mut f);
    // The absence is named only where every build the closed state names was read; a host
    // holding none of them counts the probe and skips it, which is the rule working.
    if tally.uncached == 0 {
        assert_eq!(
            f.len(),
            1,
            "the closed state's absent literal is named: {f:?}"
        );
        assert!(f[0].contains("occurs 0 times in every build this state names"));
    }

    // ... and a closed state whose literal the build carries passes exactly as an open one
    // does, so the rule is reading rather than merely failing everything it newly reaches.
    let mut c = with_probe(PRESENT, "2.1.100");
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("2.1.200");
    c["states"][0]["from"] = json!("2.1.201");
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    probes::check(std::slice::from_ref(&c), &mut f);
    assert!(f.is_empty(), "{f:?}");
}

/// Rule 22 fails BY NAME on both directions, because the pair is what is wrong rather than
/// either half: a `corpus` kind beside a scope naming a build, and a `bytes` kind beside a
/// scope naming a projects root. One specimen per direction, each naming the word it read.
#[test]
fn ledger_gate_rule_22_names_a_kind_and_a_scope_that_disagree() {
    let mut c = good_claim();
    c["states"][0]["probe"] = json!({
        "kind": "corpus",
        "command": "count the records",
        "scope": "the shipped Claude Code binary the state names (2.1.258), read whole",
        "rule": "one per record",
    });
    let mut f = Vec::new();
    probes::kind_scope(std::slice::from_ref(&c), &mut f);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("TEST-001 states[0]"), "{}", f[0]);
    assert!(f[0].contains("corpus while"), "{}", f[0]);
    assert!(f[0].contains("\"binary\""), "{}", f[0]);

    let mut c = good_claim();
    c["states"][0]["probe"] = json!({
        "kind": "bytes",
        "literal": "stripForStorage",
        "scope": "the top-level `*.jsonl` of one projects root, subagent sidecars excluded",
        "rule": "raw byte occurrences of the literal",
    });
    let mut f = Vec::new();
    probes::kind_scope(std::slice::from_ref(&c), &mut f);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("bytes while"), "{}", f[0]);
    assert!(f[0].contains("\"projects root\""), "{}", f[0]);
}

/// The two kinds whose artefact is FIXED are the only two tested. A `csift` run reads
/// transcripts, a `live` trial is taken at a build and a `repo` probe reads this tree, so
/// any of the three may name either noun without contradicting itself - widening the rule
/// to them would report vocabulary rather than a defect. A matching pair passes too, which
/// is what says the rule is reading rather than refusing every scope it sees.
#[test]
fn ledger_gate_rule_22_spares_the_kinds_whose_artefact_is_not_fixed() {
    for (kind, field, scope) in [
        (
            "csift",
            "command",
            "the transcripts `<target>` resolves to under a projects root",
        ),
        (
            "live",
            "command",
            "one live session taken at the shipped Claude Code binary",
        ),
        (
            "repo",
            "command",
            "the csift worktree, whose transcripts are not read",
        ),
        (
            "corpus",
            "command",
            "every `*.jsonl` under one projects root",
        ),
        (
            "bytes",
            "literal",
            "the shipped Claude Code binary the state names, read whole",
        ),
    ] {
        let mut c = good_claim();
        c["states"][0]["probe"] = json!({
            "kind": kind, field: "x", "scope": scope, "rule": "one per observation",
        });
        let mut f = Vec::new();
        probes::kind_scope(std::slice::from_ref(&c), &mut f);
        assert!(f.is_empty(), "{kind}: {f:?}");
    }
}
