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
        "csift search over @1a2b3c4d with --count-by label.",
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

/// The two artefacts the table was missing, and both are the commonest instruments in the
/// file: a csift RUN pointed at something reads transcripts off disk, and a count over the
/// WHOLE BUILD is the build arm under another spelling. Each moved a receipt, which is the
/// only way such a miss shows - the newest measurement at the verified build was passed
/// over for an older one.
#[test]
fn ledger_gate_rule_20_reads_a_csift_run_and_a_whole_build_count() {
    for instrument in [
        "csift, read-only: csift files <project> --by timeline --format json \
         --no-subagents over one project's top-level sessions, tallying the rows.",
        "Read the eleven observed subtypes against csift's own subtype table \
         (src/model/classify/system.rs), then re-ran the whole-build count of the \
         subtype literals.",
        "csift show @<session> --line 4 --raw, then the same record under --format json.",
        "csift verbatim <transcript>.jsonl --budget 40000 --format json.",
    ] {
        assert!(
            artefact::names_an_artefact(instrument),
            "the artefact table must read this as a measurement: {instrument}"
        );
    }
    // The TARGET is what makes a run a measurement: a subcommand named inside a sentence
    // about what csift does touched nothing.
    assert!(
        !artefact::names_an_artefact(
            "Re-read the sentence describing what csift recover reports, against src/."
        ),
        "a command word in a sentence is a noun, not a run"
    );
}

/// A build named by its PATH, and a byte offset written without the `@` sigil. The table
/// read words for both - `binary`, `the build`, `@158550335` - so a check that simply gave
/// the path it opened or wrote `at 158550335` named neither and read as prose.
#[test]
fn ledger_gate_rule_20_reads_a_build_by_its_path_and_an_offset_without_a_sigil() {
    for instrument in [
        "grep -c \"CLAUDE_CONFIG_DIR\" <builds cache>/2.1.258/claude, counting the hits.",
        "Read the drop predicate at 165953664 and the two callers below it.",
        "Counted the literal in ~/.local/share/claude/versions/2.1.258 and in its \
         predecessor.",
        "Re-read the writer at offset 158550335 and the gate above it.",
    ] {
        assert!(
            artefact::names_an_artefact(instrument),
            "the artefact table must read this as a measurement: {instrument}"
        );
    }
    // A count is not a location: the word in front is what makes the digits an offset.
    assert!(
        !artefact::names_an_artefact("The claim states 158550335 records, which src/ agrees with."),
        "a bare nine-digit number is a count until a locating word says otherwise"
    );
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
    // The veto stands down for EVERY artefact the outside table reads, not a narrower set:
    // an arm missing from the real-artefact table overrides an arm present in the other one,
    // which is the veto doing the opposite of its job.
    assert!(
        artefact::names_an_artefact(
            "Read the writer at 158550335 in the shipped build, twice, then ran the same \
             command in a fixture home to see what it prints with nothing there."
        ),
        "a build read by offset outranks a later mention of a fixture home"
    );
}
