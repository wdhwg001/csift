//! One test per gate rule, each on a HAND-BUILT specimen that violates exactly that rule.
//!
//! The driver test proves the shipped ledger passes; these prove each rule would CATCH a
//! ledger that did not, and that it says which rule by name. A rule with no specimen is a
//! rule nobody has seen fail, which is the shape a rule quietly stops enforcing in.

use super::{anchors, prose, rules, tally};
use serde_json::{json, Value};

/// A claim that passes every rule, as the base every specimen deviates from in one way.
fn good_claim() -> Value {
    json!({
        "id": "TEST-001",
        "area": "record-model",
        "code": [{"path": "Cargo.toml", "lines": "1-1", "snippet": "[package]"}],
        "states": [{
            "from": "2.1.100",
            "to": null,
            "behavior": "The record carries a top-level key.",
            "depends": "csift reads that key.",
            "instrument": "Count the key over one projects root.",
            "probe": {"kind": "corpus", "command": "count", "scope": "one projects root", "rule": "one per record"},
            "producer_trace": "complete",
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

#[test]
fn ledger_gate_rule_13_names_a_broken_state_list() {
    // Three states.
    let mut c = good_claim();
    let s = c["states"][0].clone();
    c["states"] = json!([s.clone(), s.clone(), s]);
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("3 state(s) (a claim carries one or two)"));

    // No open state.
    let mut c = good_claim();
    c["states"][0]["to"] = json!("2.1.200");
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("0 open state(s)"));

    // Two open states.
    let mut c = good_claim();
    let s = c["states"][0].clone();
    c["states"] = json!([s.clone(), s]);
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("2 open state(s)"));

    // A missing `from`.
    let mut c = good_claim();
    c["states"][0]["from"] = Value::Null;
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("`from` is None, not a Claude Code version"));

    // A closed state whose `to` is not a version.
    let mut c = good_claim();
    let mut closed = c["states"][0].clone();
    closed["to"] = json!("later");
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
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
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("states are not ordered by `from`"));

    // An open state that opens ABOVE the verified build, with no reason.
    let mut c = good_claim();
    c["states"][0]["from"] = json!("2.1.261");
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(only(&f).contains("opens at 2.1.261, above the verified build 2.1.258"));

    // ... and with one, which is the shape a behavior first seen on a newer build has.
    let mut c = good_claim();
    c["states"][0]["from"] = json!("2.1.261");
    c["states"][0]["from_after_verified"] =
        json!("The rewrite this claim states is the 2.1.261 build's.");
    let mut f = Vec::new();
    rules::state_shape("TEST-001", &c, "2.1.258", &mut f);
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn ledger_gate_rule_2_names_a_receipt_at_the_wrong_build() {
    let mut c = good_claim();
    c["states"][0]["verified"]["claude_code"] = json!("2.1.200");
    let mut f = Vec::new();
    rules::receipts("TEST-001", &c, "2.1.258", &mut Default::default(), &mut f);
    assert!(only(&f)
        .contains("the open state's receipt is at Some(\"2.1.200\"), not at Claude Code 2.1.258"));

    let mut c = good_claim();
    c["states"][0]["verified"]["verdict"] = json!("probably");
    let mut f = Vec::new();
    rules::receipts("TEST-001", &c, "2.1.258", &mut Default::default(), &mut f);
    assert!(only(&f).contains("verdict `probably` is not in"));
}

#[test]
fn ledger_gate_rule_3_names_duplicated_evidence() {
    let a = good_claim();
    let mut b = good_claim();
    b["id"] = json!("TEST-002");
    let mut seen = Default::default();
    let mut f = Vec::new();
    rules::receipts("TEST-001", &a, "2.1.258", &mut seen, &mut f);
    rules::receipts("TEST-002", &b, "2.1.258", &mut seen, &mut f);
    assert!(only(&f).contains("evidence (instrument, observed) identical to TEST-001's"));

    // `unverifiable-here` is exempt from uniqueness but must still name its reason.
    let mut c = good_claim();
    c["states"][0]["verified"]["verdict"] = json!("unverifiable-here");
    c["states"][0]["verified"]["observed"] = json!("  ");
    let mut f = Vec::new();
    rules::receipts("TEST-001", &c, "2.1.258", &mut Default::default(), &mut f);
    assert!(only(&f).contains("empty `observed`"));
}

#[test]
fn ledger_gate_rule_9_names_an_open_state_that_drifted() {
    let mut c = good_claim();
    c["states"][0]["verified"]["verdict"] = json!("drifted");
    let mut f = Vec::new();
    rules::receipts("TEST-001", &c, "2.1.258", &mut Default::default(), &mut f);
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
fn ledger_gate_rule_4_names_a_moved_or_padded_code_site() {
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

#[test]
fn ledger_gate_rule_10_names_an_unconsumed_text_leg() {
    let mut c = good_claim();
    c["states"][0]["open_legs"] = json!(["TEXT: the second sentence overstates the count"]);
    let mut f = Vec::new();
    rules::leg_hygiene("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("an unconsumed text correction (`TEXT ...`)"));

    let mut c = good_claim();
    c["states"][0]["open_legs"] = json!(["Text rewrite rejected: the count is still wrong"]);
    let mut f = Vec::new();
    rules::leg_hygiene("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("records a rejected rewrite"));
}

#[test]
fn ledger_gate_rule_14_names_every_barred_prose_family() {
    // A calendar date, a month name, a csift version, and four audit-process words.
    let hits = prose::hits(
        "Measured 2026-09-04 in September with csift 0.12.3 by an independent verifier \
         whose tracer batch was kicked today",
    );
    let rules: Vec<&str> = hits.iter().map(|(r, _)| *r).collect();
    assert!(rules.contains(&"calendar date"), "{hits:?}");
    assert!(rules.contains(&"month name"), "{hits:?}");
    assert!(rules.contains(&"csift version"), "{hits:?}");
    assert_eq!(
        hits.iter()
            .filter(|(r, _)| *r == "audit-process word")
            .count(),
        4,
        "{hits:?}"
    );

    // csift's OWN surface spells the flag, the mode and the module with a `-`, a space
    // and a `/`; only the bare process word is barred.
    assert!(prose::hits("`csift plan --audit` reads src/commands/plan/audit.rs").is_empty());
    // A Claude Code version is never a csift version.
    assert!(prose::hits("present from Claude Code 2.1.208").is_empty());
    // Domain vocabulary a token ban would have destroyed.
    assert!(
        prose::hits("a frozen lane of the top-level session, whose tool use was rejected")
            .is_empty()
    );

    // The whole-state walk names the field and the rule.
    let mut c = good_claim();
    c["states"][0]["depends"] = json!("csift 0.12.3 reads that key.");
    let mut f = Vec::new();
    prose::check_state("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("states[0].depends: `0.12.3` is a csift version"));

    // An excerpt reproduces the build's own bytes and is exempt at every level.
    let mut c = good_claim();
    c["states"][0]["producer_chain"] = json!([{"role": "writer", "offset": 1, "excerpt": "kicked:!1,lastSeen:\"today\"", "note": "the state object"}]);
    let mut f = Vec::new();
    prose::check_state("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(f.is_empty(), "{f:?}");

    // The PLURALS and the agent noun: a list that stops at the singular has a hole in it.
    for token in [
        "adjudications",
        "auditor",
        "auditors",
        "verifiers",
        "tracers",
        "audits",
    ] {
        let hits = prose::hits(&format!("the {token} recorded it"));
        assert_eq!(hits.len(), 1, "{token}: {hits:?}");
        assert_eq!(hits[0].0, "audit-process word");
    }

    // A BACKTICKED span is exempt only where the backticks BALANCE: an unclosed one makes
    // left-to-right pairing exempt prose, which is how a date once hid behind it.
    assert!(prose::hits("the beta `server-side-fallback-2026-06-01` is interned").is_empty());
    let unbalanced = prose::hits("a cut quote `def || after: measured 2026-09-04 and got 42");
    assert_eq!(unbalanced.len(), 1, "{unbalanced:?}");
    assert_eq!(unbalanced[0].0, "calendar date");
}

/// The classifier over a synthetic build: one string carrying a literal `...`, so every
/// verdict and both ordering rules are exercised without the real binary.
#[test]
fn ledger_gate_rule_12_anchor_classifier_decides_every_verdict() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // Exact: the bytes at the offset are the excerpt.
    assert_eq!(
        anchors::classify(build, 6, "bravo", None),
        anchors::Anchor::Exact
    );
    // Byte-exactness outranks eliding: a `...` that really is in the build is exact.
    assert_eq!(
        anchors::classify(build, 20, "...rest", None),
        anchors::Anchor::Exact
    );
    // Elided: an ellipsis in the excerpt, in either spelling.
    assert_eq!(
        anchors::classify(build, 6, "bra...vo", None),
        anchors::Anchor::Elided
    );
    assert_eq!(
        anchors::classify(build, 6, "bra\u{2026}vo", None),
        anchors::Anchor::Elided
    );
    // Drifted: the head agrees for four bytes, then the build says something else.
    assert_eq!(
        anchors::classify(build, 6, "bravado", None),
        anchors::Anchor::Drifted { at: 4 }
    );
    // Drifted with the excerpt running off the end of the build.
    assert_eq!(
        anchors::classify(build, 34, "echoes", None),
        anchors::Anchor::Drifted { at: 4 }
    );
    // The two marks are counted and skipped.
    assert_eq!(
        anchors::classify(build, 6, "zulu", Some("absent")),
        anchors::Anchor::Absent
    );
    assert_eq!(
        anchors::classify(build, 6, "bravado", Some("prefix-only")),
        anchors::Anchor::PrefixOnly
    );
    // A mark never hides a hop that is byte-exact again.
    assert_eq!(
        anchors::classify(build, 6, "bravo", Some("absent")),
        anchors::Anchor::Exact
    );
    // An EMPTY excerpt matches at every offset, so it is refused before the byte test -
    // including at offset 0, the placeholder for "no byte location".
    assert_eq!(
        anchors::classify(build, 6, "", None),
        anchors::Anchor::Empty
    );
    assert_eq!(
        anchors::classify(build, 0, "", Some("absent")),
        anchors::Anchor::Empty
    );
}

/// A mark is a CLAIM about the build, so the rule tests what it asserts. Without this, an
/// anchor that rotted could be quieted by typing `absent` beside it.
#[test]
fn ledger_gate_rule_12_a_mark_is_a_claim_not_a_pass() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // The honest marks: the excerpt is nowhere in the build.
    assert_eq!(anchors::verify_mark(build, 6, "zulu", "absent"), None);
    assert_eq!(
        anchors::verify_mark(build, 6, "bravado", "prefix-only"),
        None
    );
    // Marking a hop whose excerpt IS in the build is refused, whatever the offset says.
    assert_eq!(
        anchors::verify_mark(build, 999, "bravo", "absent"),
        Some(
            "marked `absent` but the excerpt occurs in the build at offset 6 - \
             re-anchor the offset instead of marking the hop"
                .to_string()
        )
    );
    assert!(anchors::verify_mark(build, 999, "bravo", "prefix-only").is_some());
    // `prefix-only` also asserts a head match where the offset points.
    assert_eq!(
        anchors::verify_mark(build, 12, "bravado", "prefix-only"),
        Some(
            "marked `prefix-only` but no leading byte of the excerpt matches at offset 12 - \
             that mark asserts a head match, so the hop is `absent`"
                .to_string()
        )
    );
    // `absent` makes no claim about the offset, so a head mismatch there is fine.
    assert_eq!(anchors::verify_mark(build, 12, "zulu", "absent"), None);
    // An empty excerpt asserts nothing and can never be marked.
    assert!(anchors::verify_mark(build, 6, "", "absent").is_some());
}

/// Rule 12's open/closed split: a closed state's anchors are counted and never compared,
/// so a superseded fact is not asked to still sit at its old bytes.
#[test]
fn ledger_gate_rule_12_counts_closed_state_anchors_without_reading_them() {
    let mut c = good_claim();
    let mut closed = c["states"][0].clone();
    closed["from"] = json!("2.1.100");
    closed["to"] = json!("2.1.200");
    closed["producer_chain"] = json!([{"role": "writer", "offset": 99, "excerpt": "gone"}]);
    c["states"][0]["from"] = json!("2.1.201");
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
    assert_eq!(a.closed, 1, "the closed state's anchor is counted");
    assert_eq!(a.total, 0, "and is not among the anchors that are read");
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn ledger_gate_rule_8_names_a_tally_row_that_disagrees_with_the_ledger() {
    let claims = vec![good_claim()];
    let a = anchors::Anchors {
        total: 3,
        absent: 1,
        prefix_only: 0,
        closed: 2,
        measured: None,
    };
    let readme = "<!-- ledger-tally:begin -->\n\
        | end-to-end | 9 | 100.0% | x |\n\
        | producer-only | 0 | 0.0% | x |\n\
        | specimen-only | 0 | 0.0% | x |\n\
        | partial-producer | 0 | 0.0% | x |\n\
        | by-elimination | 0 | 0.0% | x |\n\
        | upstream | 0 | 0.0% | x |\n\
        | total | 1 | 100.0% | x |\n\n\
        `anchors byte-exact at Claude Code 2.1.258: 1 of 3 (elided 1, absent 1, prefix-only 0), closed-state anchors 2, not checked`\n\
        <!-- ledger-tally:end -->";
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", readme, &mut f);
    assert!(only(&f).contains("README tally row `end-to-end` says 9 but the ledger has 1"));

    // The anchor line is checked against the ledger's own counts too. The row that
    // disagreed above is corrected first, so the anchor line is the only failure left.
    let readme = readme
        .replace("| end-to-end | 9 |", "| end-to-end | 1 |")
        .replace("closed-state anchors 2", "closed-state anchors 7");
    let mut f = Vec::new();
    tally::ledger_tally(&claims, &a, "2.1.258", &readme, &mut f);
    assert!(
        only(&f).contains("README anchor line says closed-state anchors 7 but the ledger has 2")
    );
}

#[test]
fn ledger_gate_rule_1_and_5_name_a_badge_that_disagrees() {
    let mut f = Vec::new();
    tally::badges(
        "2.1.258",
        "![](https://img.shields.io/badge/verified%20against%20Claude%20Code-2.1.200-x) \
         ![](https://img.shields.io/badge/mutation%20score-90.1%25-x)",
        &mut f,
    );
    assert!(only(&f)
        .contains("README badge says Claude Code 2.1.200 but the ledger is verified at 2.1.258"));

    let mut f = Vec::new();
    tally::badges(
        "2.1.258",
        "![](https://img.shields.io/badge/verified%20against%20Claude%20Code-2.1.258-x)",
        &mut f,
    );
    assert!(only(&f).contains("README has no mutation-score badge"));
}
