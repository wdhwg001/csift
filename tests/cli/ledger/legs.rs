//! Rules 10 and 17: an instruction that was never carried out, and a spelling that no
//! longer runs.
//!
//! Both are the same defect seen twice: the ledger telling a reader to do something the
//! ledger itself has not done. A `TEXT:` line is a correction a traced hop demanded and a
//! retired selector is a probe that hard-errors, and either one shipping means the file
//! says one thing and the tree does another.

use serde_json::Value;

/// Rule 10: no field carries an unconsumed text instruction or a record of a rejected
/// rewrite. Both mean a refuted text is still shipping, and both are consumed only by
/// rewriting the claim.
///
/// The scan covers `open_legs`, `residue` AND `verified.note`. It used to cover the legs
/// alone, which is the one place the instructions were NOT: fourteen of them sat in
/// `residue` and in a receipt's `note`, where the rule could not see them. An instruction
/// is an instruction wherever it is written down.
pub(crate) fn text_instructions(at: &str, state: &Value, failures: &mut Vec<String>) {
    let mut check = |path: &str, s: &str| {
        let t = s.trim_start();
        if t.starts_with("TEXT") {
            failures.push(format!(
                "{at}.{path}: an unconsumed text correction (`TEXT ...`): carry it out in the \
                 claim's own text, then remove the instruction"
            ));
        } else if t.starts_with("Text rewrite rejected")
            || t.contains("was rejected on adversarial re-read")
        {
            failures.push(format!(
                "{at}.{path}: records a rejected rewrite, so the refuted text is still \
                 shipping: rewrite it to acceptance, split, or retire"
            ));
        }
        // An instruction can also sit MID-FIELD, where a fold carried a sentence that
        // opened with one. The marker is the same and so is the remedy.
        if let Some(i) = mid_field_instruction(s) {
            failures.push(format!(
                "{at}.{path}: an unconsumed text correction at offset {i} (`{}`)",
                &s[i..(i + 60).min(s.len())]
            ));
        }
    };
    for key in ["open_legs", "residue"] {
        for (i, v) in state[key]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            if let Some(s) = v.as_str() {
                check(&format!("{key}[{i}]"), s);
            }
        }
    }
    if let Some(s) = state["verified"]["note"].as_str() {
        check("verified.note", s);
    }
}

/// `TEXT:` or `TEXT (<claim id>):` anywhere but the start, which the caller already read.
/// The marker is deliberately narrow: `TEXT` in ordinary prose is a word, so the colon and
/// the optional parenthesised id are what make it an instruction.
fn mid_field_instruction(s: &str) -> Option<usize> {
    if s.len() < 2 {
        return None;
    }
    let mut from = 1;
    while let Some(rel) = s[from..].find("TEXT") {
        let i = from + rel;
        let tail = &s[i + 4..];
        let tail = tail.strip_prefix(' ').unwrap_or(tail);
        let tail = match tail.strip_prefix('(') {
            Some(rest) => rest.split_once(')').map_or("", |(_, t)| t),
            None => tail,
        };
        if tail.starts_with(':') {
            return Some(i);
        }
        from = i + 4;
    }
    None
}

/// The selector spellings csift RETIRED. Each is a hard error in the shipped binary
/// (`retired_successors`, src/cli/selectors.rs), so a probe naming one cannot run at all.
/// The list is the table's own keys; `agent.message` and `agent.thinking` are deliberately
/// absent, because they became CATEGORIES rather than disappearing.
///
/// `tool` is the one spelling that is ALSO something else: `-t tool` is the pre-v0.5 flat
/// path and hard-errors, while `--count-by tool` is a valid census axis and always has
/// been. The list carries it and the `--count-by` position excludes it (`axis_spelling`),
/// so the rule reads the argument rather than the word - which is the whole of rule 17.
pub(crate) const RETIRED_SELECTORS: [&str; 13] = [
    "harness.meta.attachment",
    "harness.meta.away-summary",
    "harness.meta.hook",
    "harness.meta.loop",
    "harness.meta.snapshot",
    "harness.meta.stop-hooks",
    "harness.meta.system",
    "harness.meta.turn-duration",
    "harness.schedule.continuation",
    "harness.meta",
    "tool-response",
    "thinking",
    "tool",
];

/// A retired spelling that is a VALID `--count-by` axis under the same word. Only `tool` is
/// one: the census axes and the label paths are different vocabularies that happen to share
/// it, so the argument position decides and the word never does.
fn axis_spelling(spelling: &str) -> bool {
    spelling == "tool"
}

/// Rule 17: no field hands a RETIRED spelling to csift as an argument.
///
/// A probe naming `-t harness.meta.hook` does not merely read oddly - it exits non-zero
/// with "unknown label selector", so the state's own re-verification instruction cannot be
/// run. The test is on the ARGUMENT position (`-t`, `-T`, `--count-by label` and the
/// `select(.label==...)` shapes), never on the bare word: a residue line saying WHAT a leaf
/// used to be called is history, and history is exactly what a superseded reading is for.
pub(crate) fn retired_spellings(at: &str, state: &Value, failures: &mut Vec<String>) {
    let mut check = |path: &str, s: &str| {
        for spelling in RETIRED_SELECTORS {
            for (i, _) in s.match_indices(spelling) {
                // The longest spelling wins: `harness.meta.hook` also contains `harness.meta`.
                let after = s[i + spelling.len()..].chars().next();
                if after.is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '-') {
                    continue;
                }
                if argument_position(&s[..i], spelling) {
                    failures.push(format!(
                        "{at}.{path}: names the retired selector `{spelling}` as a csift \
                         argument - the shipped binary hard-errors on it, so this probe \
                         cannot run"
                    ));
                }
            }
        }
    };
    // THREE fields, and the scope IS the rule. `instrument`, `probe.command` and
    // `probe.rule` are what a reader RE-RUNS, so a retired spelling in one of them is an
    // instruction that cannot be carried out. Everywhere else - `behavior`, `depends`,
    // `residue`, a receipt's own record of what ran at the time - a retired spelling is
    // HISTORY, which is exactly what a superseded reading is for, and banning it there
    // would force the ledger to lie about what a leaf used to be called.
    if let Some(s) = state["instrument"].as_str() {
        check("instrument", s);
    }
    for key in ["command", "rule"] {
        if let Some(s) = state["probe"][key].as_str() {
            check(&format!("probe.{key}"), s);
        }
    }
}

/// Is the text immediately before this spelling a selector FLAG? Reading a short window is
/// enough: the flag and its value are adjacent by construction, and a sentence that merely
/// mentions the old name has ordinary words there.
fn argument_position(before: &str, spelling: &str) -> bool {
    let tail: String = before
        .chars()
        .rev()
        .take(24)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let tail = tail.trim_end_matches(['`', '"', '\'', ' ']);
    tail.ends_with("-t")
        || tail.ends_with("-T")
        || tail.ends_with("--label")
        || tail.ends_with("--label-not")
        || tail.ends_with(".label==")
        || tail.ends_with("label ==")
        || (tail.ends_with("--count-by") && !axis_spelling(spelling))
}
