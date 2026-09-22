//! Rules 2, 3, 4, 6, 7, 9 and 11: the receipt, the code sites and the attribution.
//!
//! See the parent module for why each rule owes a specimen.

use super::*;

#[test]
fn ledger_gate_rule_2_names_a_receipt_at_the_wrong_build() {
    let mut c = good_claim();
    c["states"][0]["verified"]["claude_code"] = json!("2.1.200");
    let mut f = Vec::new();
    rules::receipts(
        "TEST-001",
        &c,
        "2.1.258",
        true,
        &mut Default::default(),
        &mut f,
    );
    assert!(only(&f)
        .contains("the open state's receipt is at Some(\"2.1.200\"), not at Claude Code 2.1.258"));

    let mut c = good_claim();
    c["states"][0]["verified"]["verdict"] = json!("probably");
    let mut f = Vec::new();
    rules::receipts(
        "TEST-001",
        &c,
        "2.1.258",
        true,
        &mut Default::default(),
        &mut f,
    );
    assert!(only(&f).contains("verdict `probably` is not in"));
}

#[test]
fn ledger_gate_rule_3_names_duplicated_evidence() {
    let a = good_claim();
    let mut b = good_claim();
    b["id"] = json!("TEST-002");
    let mut seen = Default::default();
    let mut f = Vec::new();
    rules::receipts("TEST-001", &a, "2.1.258", true, &mut seen, &mut f);
    rules::receipts("TEST-002", &b, "2.1.258", true, &mut seen, &mut f);
    assert!(only(&f).contains("evidence (instrument, observed) identical to TEST-001's"));

    // `unverifiable-here` is exempt from uniqueness but must still name its reason.
    let mut c = good_claim();
    c["states"][0]["verified"]["verdict"] = json!("unverifiable-here");
    c["states"][0]["verified"]["observed"] = json!("  ");
    let mut f = Vec::new();
    rules::receipts(
        "TEST-001",
        &c,
        "2.1.258",
        true,
        &mut Default::default(),
        &mut f,
    );
    assert!(only(&f).contains("empty `observed`"));
}

#[test]
fn ledger_gate_rule_9_names_an_open_state_that_drifted() {
    let mut c = good_claim();
    c["states"][0]["verified"]["verdict"] = json!("drifted");
    let mut f = Vec::new();
    rules::receipts(
        "TEST-001",
        &c,
        "2.1.258",
        true,
        &mut Default::default(),
        &mut f,
    );
    assert!(only(&f).contains("the open state's verdict is `drifted`"));
}

#[test]
fn ledger_gate_rule_6_names_a_claim_with_no_code_site() {
    let mut c = good_claim();
    c["code"] = json!([]);
    let mut f = Vec::new();
    rules::code_sites("TEST-001", &c, &mut Default::default(), &mut f);
    assert!(only(&f).contains("no code site (every claim cites at least one)"));
}

#[test]
fn ledger_gate_rule_4_names_a_missing_file_a_moved_snippet_and_a_wrong_range() {
    let mut c = good_claim();
    c["code"][0]["snippet"] = json!("fn this_function_does_not_exist_anywhere()");
    let mut f = Vec::new();
    rules::code_sites("TEST-001", &c, &mut Default::default(), &mut f);
    assert!(only(&f).contains("snippet not found verbatim in `Cargo.toml`"));

    let mut c = good_claim();
    c["code"][0]["path"] = json!("no/such/file.rs");
    let mut f = Vec::new();
    rules::code_sites("TEST-001", &c, &mut Default::default(), &mut f);
    assert!(only(&f).contains("code site `no/such/file.rs` does not exist"));

    let mut c = good_claim();
    c["code"][0]["lines"] = json!("40-41");
    let mut f = Vec::new();
    rules::code_sites("TEST-001", &c, &mut Default::default(), &mut f);
    assert!(only(&f).contains("snippet is not inside `Cargo.toml` lines 40-41"));
}

/// Rule 4's width bound: containment alone accepts a range padded to the whole file.

#[test]
fn ledger_gate_a_declared_range_may_not_be_padded() {
    let three = "one\ntwo\nthree";
    // Exact, and the two lines of slack, are accepted.
    assert_eq!(rules::range_too_wide(10, 12, three), None);
    assert_eq!(rules::range_too_wide(10, 14, three), None);
    // A third padding line is not.
    assert_eq!(rules::range_too_wide(10, 15, three), Some((6, 3)));
    // The shape the bound exists for: a range that clamps to the file and passes
    // containment while naming everything.
    assert_eq!(rules::range_too_wide(1, 99999, three), Some((99999, 3)));
    // A one-line snippet is the common case and keeps its own slack.
    assert_eq!(rules::range_too_wide(7, 7, "fn f() {}"), None);
    assert_eq!(rules::range_too_wide(7, 10, "fn f() {}"), Some((4, 1)));
}

#[test]
fn ledger_gate_rule_7_names_an_attribution_that_does_not_follow() {
    let mut c = good_claim();
    c["states"][0]["specimen"] = json!("none");
    let mut f = Vec::new();
    rules::attribution("TEST-001 states[0]", &c["states"][0], &mut f);
    // The legs now derive producer-only, and end-to-end also owes no open leg, so the
    // derivation failure is the only one.
    assert!(
        only(&f).contains("does not follow from the legs (`complete` + `none` = `producer-only`)")
    );

    // end-to-end with an open leg.
    let mut c = good_claim();
    c["states"][0]["open_legs"] = json!(["a live trial would close the specimen"]);
    let mut f = Vec::new();
    rules::attribution("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("end-to-end with a non-empty `open_legs`"));

    // Below end-to-end with no open leg.
    let mut c = good_claim();
    c["states"][0]["specimen"] = json!("none");
    c["states"][0]["attribution"] = json!("producer-only");
    let mut f = Vec::new();
    rules::attribution("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("attribution `producer-only` without a non-empty `open_legs` entry"));

    // by-elimination can never hold.
    let mut c = good_claim();
    c["states"][0]["producer_trace"] = json!("none");
    c["states"][0]["specimen"] = json!("none");
    c["states"][0]["attribution"] = json!("by-elimination");
    c["states"][0]["open_legs"] = json!(["trace the writer in the shipped build"]);
    let mut f = Vec::new();
    rules::attribution("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("a by-elimination state carries a `holds` verdict"));

    // upstream owes its reason.
    let mut c = good_claim();
    c["states"][0]["producer_trace"] = json!("upstream");
    c["states"][0]["attribution"] = json!("upstream");
    let mut f = Vec::new();
    rules::attribution("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("attribution `upstream` without an `upstream_reason`"));
}

#[test]
fn ledger_gate_rule_11_names_a_state_with_no_legs() {
    let mut c = good_claim();
    c["states"][0]["attribution"] = json!("specimen-only");
    c["states"][0]["producer_trace"] = Value::Null;
    c["states"][0]["specimen"] = Value::Null;
    c["states"][0]["open_legs"] = json!(["trace the writer in the shipped build"]);
    let mut f = Vec::new();
    rules::attribution("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f)
        .contains("attribution `specimen-only` with no `producer_trace`/`specimen` legs recorded"));
}

/// Rule 1 stands in FRONT of rule 2. Bumping `verified_claude_code` ahead of the audit is
/// the ordinary way to reach this gate, and rule 2 then fires once per claim - hundreds of
/// lines saying the same thing, with the one that explains them somewhere in the middle. So
/// the badge is asked first, it reports the version ONCE and names the build the receipts do
/// stand at, and rule 2's version arm stands down. Every other arm of rule 2 keeps running:
/// a verdict or an instrument is wrong whatever the badge says.
#[test]
fn ledger_gate_rule_1_reports_a_moved_badge_once_instead_of_per_claim() {
    let a = good_claim();
    let mut b = good_claim();
    b["id"] = json!("TEST-002");
    let claims = [&a, &b];

    // The badge stands where the receipts do: nothing to report, and rule 2 runs whole.
    assert_eq!(rules::badge_stands(&claims, "2.1.258"), None);

    // The badge moved ahead of the audit: ONE line, naming the build the receipts stand at.
    let moved = rules::badge_stands(&claims, "2.1.300").expect("the moved badge is reported");
    assert!(
        moved.contains("`verified_claude_code` is 2.1.300"),
        "{moved}"
    );
    assert!(
        moved.contains("the receipts stand at 2.1.258 on 2"),
        "{moved}"
    );

    // With it reported, rule 2's version arm says nothing - and its other arms still do.
    let mut c = good_claim();
    c["states"][0]["verified"]["claude_code"] = json!("2.1.258");
    c["states"][0]["verified"]["verdict"] = json!("probably");
    let mut f = Vec::new();
    rules::receipts(
        "TEST-001",
        &c,
        "2.1.300",
        false,
        &mut Default::default(),
        &mut f,
    );
    assert!(only(&f).contains("verdict `probably` is not in"));
}
