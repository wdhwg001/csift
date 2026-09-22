//! Rule 19's specimen: an instrument whose embedded program stopped parsing.
//!
//! The damage this rule exists for changes no word: a whitespace pass squeezed every run,
//! the sentence still reads correctly, the counting rule is still there, and the python
//! loop body now sits at the loop's own column. So the specimen is the SAME instrument
//! twice, differing in nothing but the indent.

use super::{good_claim, only};
use crate::ledger::programs;
use serde_json::Value;

/// The instrument a claim really carries, at whatever indents it is handed.
///
/// TWO levels, because that is where the damage lives: python accepts a one-space indent
/// perfectly well, so a single loop survives a whitespace squeeze and says nothing. What
/// the squeeze destroys is NESTING - every level collapsed onto the same column, so the
/// inner body sits at the outer body's level and the loop has no block at all.
fn instrument(outer: &str, inner: &str) -> String {
    format!(
        "python3 - <<'EOF'\n\
         import json,glob\n\
         total=0\n\
         for f in glob.glob('*.jsonl'):\n\
         {outer}for line in open(f):\n\
         {inner}total+=1\n\
         print(total)\n\
         EOF\n\
         Counting rule: one tally per line."
    )
}

fn state_with(instrument: &str) -> Value {
    let mut claim = good_claim();
    claim["states"][0]["instrument"] = Value::String(instrument.to_string());
    claim["states"][0]["probe"]["command"] = Value::String(instrument.to_string());
    claim["states"][0].clone()
}

#[test]
fn ledger_gate_rule_19_catches_an_instrument_whose_program_no_longer_parses() {
    let state = state_with(&instrument(" ", " "));
    let mut tally = programs::Programs::default();
    let mut failures = Vec::new();
    programs::check_state("TEST-001 states[0]", &state, &mut tally, &mut failures);
    if tally.skipped.values().sum::<usize>() > 0 {
        // No python3 on this host: rule 19 counted the program and said so, which is the
        // documented skip, and the specimen has nothing to prove.
        return;
    }
    // Both fields carry the same broken program, so both are named.
    assert_eq!(failures.len(), 2, "expected two failures, got {failures:?}");
    assert!(
        failures[0].contains("the python program in `instrument`"),
        "rule 19 must name the language and the field: {}",
        failures[0]
    );
    assert!(
        failures[1].contains("the python program in `probe.command`"),
        "rule 19 must name the derived command too: {}",
        failures[1]
    );
    assert!(
        failures[0].contains("does not parse"),
        "rule 19 must say what went wrong: {}",
        failures[0]
    );
}

#[test]
fn ledger_gate_rule_19_passes_the_same_program_at_its_own_indent() {
    let state = state_with(&instrument("  ", "    "));
    let mut tally = programs::Programs::default();
    let mut failures = Vec::new();
    programs::check_state("TEST-001 states[0]", &state, &mut tally, &mut failures);
    assert!(
        failures.is_empty(),
        "the indented program parses: {failures:?}"
    );
    assert_eq!(
        tally.checked + tally.skipped.values().sum::<usize>(),
        2,
        "both fields carry one program each"
    );
}

/// A `<<` that opens nothing is prose, not a program: the rule must not reach for a
/// terminator that is not there, and must not read a heredoc with no interpreter in front
/// of it as one.
#[test]
fn ledger_gate_rule_19_ignores_a_shift_operator_and_a_data_heredoc() {
    let mut claim = good_claim();
    claim["states"][0]["instrument"] = Value::String(
        "The mask shifts the flag <<that bit and compares; cat <<'DATA'\nplain text\nDATA"
            .to_string(),
    );
    let state = claim["states"][0].clone();
    let mut tally = programs::Programs::default();
    let mut failures = Vec::new();
    programs::check_state("TEST-001 states[0]", &state, &mut tally, &mut failures);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(tally.checked, 0, "neither is a program this rule can ask");
    let _ = only;
}
