//! One test per gate rule, each on a HAND-BUILT specimen that violates exactly that rule.
//!
//! The driver test proves the shipped ledger passes; these prove each rule would CATCH a
//! ledger that did not, and that it says which rule by name. A rule with no specimen is a
//! rule nobody has seen fail, which is the shape a rule quietly stops enforcing in.

use super::{anchors, legs, prose, rules, states, tally};
use serde_json::{json, Value};

/// A claim that passes every rule, as the base every specimen deviates from in one way.
fn good_claim() -> Value {
    json!({
        "id": "TEST-001",
        "area": "record-model",
        "code": [{"path": "Cargo.toml", "lines": "1-1", "snippet": "[package]"}],
        "states": [{
            "from": "2.1.100",
            "from_basis": "first_seen",
            "to": null,
            "behavior": "The record carries a top-level key.",
            "depends": "csift reads that key.",
            "instrument": "Count the key over one projects root.",
            "probe": {"kind": "corpus", "command": "count", "scope": "one projects root", "rule": "one per record"},
            "producer_trace": "complete",
            // A `complete` trace owes at least one LOCATED hop (rule 7's twin), so the base
            // every specimen deviates from carries one.
            "producer_chain": [{
                "role": "writer",
                "name": "the record factory",
                "offset": 1,
                "excerpt": "[package]",
                "note": "the writer"
            }],
            "specimen": "observed",
            "attribution": "end-to-end",
            "verified": {
                "claude_code": "2.1.258",
                "verdict": "holds",
                "instrument": "the count",
                "observed": "7 of 7",
                "rule": "one per record",
                "note": ""
            }
        }]
    })
}

fn only(failures: &[String]) -> &str {
    assert_eq!(failures.len(), 1, "expected one failure, got {failures:?}");
    &failures[0]
}

mod artefact;
mod byte_anchors;
mod programs;
mod readme;
mod receipts;
mod shape;
mod text;
