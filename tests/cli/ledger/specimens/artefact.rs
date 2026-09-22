//! Rule 20's specimen: a receipt that decided nothing about Claude Code.
//!
//! The rule's whole content is the classifier, so the specimen exercises the classifier
//! directly as well as the rule that calls it - including the two ways a sentence naming a
//! real-sounding noun is still a text pass.

use super::good_claim;
use crate::ledger::artefact;
use serde_json::Value;

fn with_receipt(instrument: &str) -> Value {
    let mut claim = good_claim();
    claim["states"][0]["verified"]["instrument"] = Value::String(instrument.to_string());
    claim["states"][0].clone()
}

#[test]
fn ledger_gate_rule_20_refuses_a_receipt_that_only_re_read_this_repository() {
    let state = with_receipt("Re-read src/model/record.rs and tests/cli/search/taxonomy.rs, and the claim's own sentence.");
    let mut failures = Vec::new();
    artefact::open_receipt("TEST-001 states[0]", &state, &mut failures);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("names no artefact outside csift's own tree"),
        "rule 20 must say what is missing: {}",
        failures[0]
    );
    assert!(
        failures[0].contains("TEXT PASS"),
        "rule 20 must name the verdict it reached: {}",
        failures[0]
    );
}

#[test]
fn ledger_gate_rule_20_admits_a_receipt_that_touched_the_build_or_the_corpus() {
    for instrument in [
        "grep -abo on the shipped build at @164082185.",
        "A census over one projects root of every *.jsonl line carrying the key.",
        "csift search over @13d9645a with --count-by label.",
        "The registry row read from <claude-home>/sessions while the session ran.",
    ] {
        let state = with_receipt(instrument);
        let mut failures = Vec::new();
        artefact::open_receipt("TEST-001 states[0]", &state, &mut failures);
        assert!(failures.is_empty(), "{instrument}: {failures:?}");
    }
}

/// The five the shape used to walk past, each one an artefact spelled with a word in the
/// middle. They are the classifier's own regression rows.
#[test]
fn ledger_gate_rule_20_reads_an_artefact_named_with_a_word_between() {
    for instrument in [
        "Read the formatter's selector at its offsets in the shipped 2.1.258 build, then \
         check on a live 2.1.258 trial which sentence a ctrl+b press produces.",
        "Count the literal for the attachment construction over the whole Claude Code \
         2.1.258 file and read every occurrence.",
        "On a Windows 11 ARM64 host at the verified version: a print-mode session started \
         in the background and the sessions directory listed while it ran.",
        "The same Windows trial: a print-mode session under an msys bash, then the projects \
         directories of both candidate homes listed.",
    ] {
        assert!(
            artefact::names_an_artefact(instrument),
            "the artefact table must read this as a measurement: {instrument}"
        );
    }
}

/// Two sentences that NAME a real artefact and are still text passes: one declares itself
/// one in its opening clause, and one touched nothing but a fixture somebody here wrote.
#[test]
fn ledger_gate_rule_20_refuses_a_declared_text_pass_and_a_fixture_only_reading() {
    assert!(
        !artefact::names_an_artefact(
            "second text pass: behavior and depends rewritten to what the traced chain and \
             the corpus census support, re-read against every cited offset"
        ),
        "a check that declares itself a text pass is one whatever it goes on to mention"
    );
    assert!(
        !artefact::names_an_artefact(
            "Ran the command against a synthetic fixture transcript written for this shape."
        ),
        "a self-written fixture is never a specimen"
    );
    assert!(
        artefact::names_an_artefact(
            "Ran the command against a synthetic fixture transcript and then over one \
             projects root corpus."
        ),
        "a fixture beside a real artefact is still a measurement"
    );
}
