//! Rules 13, 15, 16 and 18: the SHAPE of a claim, a state and the file.
//!
//! See the parent module for why each rule owes a specimen.

use super::*;

#[test]
fn ledger_gate_rule_13_names_a_broken_state_list() {
    // Three states.
    let mut c = good_claim();
    let s = c["states"][0].clone();
    c["states"] = json!([s.clone(), s.clone(), s]);
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("3 state(s) (a claim carries one or two)"));

    // No open state.
    let mut c = good_claim();
    c["states"][0]["to"] = json!("2.1.200");
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("0 open state(s)"));

    // Two open states.
    let mut c = good_claim();
    let s = c["states"][0].clone();
    c["states"] = json!([s.clone(), s]);
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("2 open state(s)"));

    // A missing `from`.
    let mut c = good_claim();
    c["states"][0]["from"] = Value::Null;
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("`from` is None, not a Claude Code version"));

    // A closed state whose `to` is not a version.
    let mut c = good_claim();
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("later");
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("`to` is String(\"later\")"));

    // States out of order.
    let mut c = good_claim();
    let mut older = c["states"][0].clone();
    older["from"] = json!("2.1.200");
    older["to"] = json!("2.1.250");
    let mut newer = c["states"][0].clone();
    newer["from"] = json!("2.1.100");
    c["states"] = json!([older, newer]);
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("states are not ordered by `from`"));

    // An open state that opens ABOVE the verified build, with no reason.
    let mut c = good_claim();
    c["states"][0]["from"] = json!("2.1.261");
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("opens at 2.1.261, above the verified build 2.1.258"));

    // ... and with one, which is the shape a behavior first seen on a newer build has.
    let mut c = good_claim();
    c["states"][0]["from"] = json!("2.1.261");
    c["states"][0]["from_after_verified"] =
        json!("The rewrite this claim states is the 2.1.261 build's.");
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(f.is_empty(), "{f:?}");
}

/// Rule 13's second half: the open state is LAST. Ordering by `from` does not force it -
/// a closed state can open at the same build as the open one - and every reader treats
/// `states[-1]` as the current reading.
#[test]
fn ledger_gate_rule_13_names_an_open_state_that_is_not_last() {
    let mut c = good_claim();
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("2.1.200");
    c["states"] = json!([c["states"][0].clone(), closed]);
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("the open state is not the LAST one"));
}

/// Rule 13's third half: `from_basis` says WHERE the floor came from, and the four sources
/// are not equally strong. Without it a defaulted floor reads exactly like a measured one.
#[test]
fn ledger_gate_rule_13_names_a_floor_with_no_basis() {
    let mut c = good_claim();
    c["states"][0]["from_basis"] = json!("guessed");
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("`from_basis` is Some(\"guessed\")"));

    let mut c = good_claim();
    c["states"][0].as_object_mut().unwrap().remove("from_basis");
    let mut f = Vec::new();
    states::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("`from_basis` is None"));
}

/// Rule 15: a state says what the behavior is, what csift depends on and how to re-verify
/// it, and its `probe` carries the field its own kind is answered by.
#[test]
fn ledger_gate_rule_15_names_an_empty_state_field_or_a_hollow_probe() {
    let mut c = good_claim();
    c["states"][0]["depends"] = json!("   ");
    let mut f = Vec::new();
    states::state_text("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("`depends` is empty"));

    // A probe with a kind and nothing else is a placeholder wearing a schema.
    let mut c = good_claim();
    c["states"][0]["probe"] =
        json!({"kind": "corpus", "scope": "a root", "rule": "one per record"});
    let mut f = Vec::new();
    states::state_text("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("`probe.command` is empty"));

    // A `bytes` probe is answered by a literal rather than a command.
    let mut c = good_claim();
    c["states"][0]["probe"] = json!({"kind": "bytes", "scope": "the build", "rule": "re-location"});
    let mut f = Vec::new();
    states::state_text("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("`probe.literal` is empty"));
}

/// Rule 16: a closed state's receipt is never re-made against a later build, but a range
/// nothing ever stood behind is not a range.
#[test]
fn ledger_gate_rule_16_names_a_closed_state_with_no_receipt() {
    let c = good_claim();
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("2.1.200");
    closed["verified"] = json!({
        "claude_code": "2.1.200", "verdict": "holds", "instrument": "x", "observed": ""
    });
    let mut f = Vec::new();
    states::closed_receipt("TEST-001 states[0]", &closed, &mut f);
    assert!(only(&f).contains("the closed state's receipt has an empty `observed`"));

    // And its build never post-dates its own range.
    closed["verified"]["observed"] = json!("7 of 7");
    closed["verified"]["claude_code"] = json!("2.1.258");
    let mut f = Vec::new();
    states::closed_receipt("TEST-001 states[0]", &closed, &mut f);
    assert!(only(&f).contains("above its own `to` 2.1.200"));
}

/// Rule 18: the file's own shape. These were bare assertions in the driver, which is how a
/// check ends up with no specimen behind it.
#[test]
fn ledger_gate_rule_18_names_a_broken_file_shape() {
    let mut f = Vec::new();
    super::super::file_shape(
        &json!({"schema": 1, "verified_claude_code": "2.1.258", "claims": [good_claim()]}),
        &mut f,
    );
    assert!(only(&f).contains("`schema` is 1, not 2"));

    let mut f = Vec::new();
    let doc = json!({"schema": 2, "verified_claude_code": "2.1.258",
                     "claims": [good_claim(), good_claim()]});
    let kept = super::super::file_shape(&doc, &mut f);
    assert_eq!(kept.len(), 1, "the duplicate is dropped from the walk");
    assert!(only(&f).contains("claim id `TEST-001` occurs more than once"));

    let mut c = good_claim();
    c["area"] = json!("");
    let mut f = Vec::new();
    super::super::file_shape(
        &json!({"schema": 2, "verified_claude_code": "2.1.258", "claims": [c]}),
        &mut f,
    );
    assert!(only(&f).contains("`area` is empty"));
}
