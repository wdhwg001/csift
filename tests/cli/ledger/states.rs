//! Rules 13, 15 and 16: the STATE list's shape, what one state must say, and the receipt a
//! CLOSED state carries.
//!
//! A claim is a fact, and a fact has a current reading and at most one superseded one. The
//! shape rules are the ones a reader of the file leans on without checking: that the LAST
//! state is the open one, that a range names a Claude Code version at both ends, and that
//! the floor says where it came from.

use super::{open_state, states, version_key, version_triple};
use serde_json::Value;

/// The four sources a range's floor can have, and they are not equally strong.
/// `first_seen` is a build the behaviour was OBSERVED at; `parsed` is the same, read out of
/// a value that also carried prose; `verified` is the earliest build a receipt ran at,
/// which BOUNDS the floor without measuring it; `located` is the build the claim was pinned
/// at, a default that says nothing about when the behaviour began. Without the word, the
/// 107 defaulted floors and the 110 measured ones read alike.
pub(crate) const FROM_BASES: [&str; 4] = ["first_seen", "parsed", "verified", "located"];

/// Rule 13: the shape of the state list. One or two states, exactly one open AND LAST, a
/// `from` and a `from_basis` on every one, a `to` on every closed one, ordered by `from`,
/// and the OPEN state's range opens at or below the verified build unless it says why not.
///
/// The open state is LAST because every other rule and every reader treats the list as a
/// timeline: `states[-1]` is the current reading, a closed state is what a build no longer
/// does. Ordering by `from` alone does not force it - a closed state can open at the same
/// build as the open one, and then "ordered" admits the pair in either order.
pub(crate) fn state_shape(id: &str, claim: &Value, verified: &str, failures: &mut Vec<String>) {
    let sts = states(claim);
    if sts.is_empty() || sts.len() > 2 {
        failures.push(format!(
            "{id}: {} state(s) (a claim carries one or two)",
            sts.len()
        ));
        return;
    }
    let open = sts.iter().filter(|s| s["to"].is_null()).count();
    if open != 1 {
        failures.push(format!(
            "{id}: {open} open state(s) (exactly one state is open)"
        ));
    }
    if open == 1 && !sts[sts.len() - 1]["to"].is_null() {
        failures.push(format!(
            "{id}: the open state is not the LAST one - the list is a timeline, so a closed \
             range never sits below the current reading"
        ));
    }
    let mut keys = Vec::new();
    for (i, s) in sts.iter().enumerate() {
        match s["from"].as_str() {
            Some(f) if version_triple(f) => keys.push(version_key(f).unwrap_or_default()),
            other => failures.push(format!(
                "{id} states[{i}]: `from` is {other:?}, not a Claude Code version"
            )),
        }
        match s["from_basis"].as_str() {
            Some(b) if FROM_BASES.contains(&b) => {}
            other => failures.push(format!(
                "{id} states[{i}]: `from_basis` is {other:?}, not in {FROM_BASES:?}"
            )),
        }
        if !s["to"].is_null() && !s["to"].as_str().is_some_and(version_triple) {
            failures.push(format!(
                "{id} states[{i}]: `to` is {:?}, neither null nor a Claude Code version",
                s["to"]
            ));
        }
    }
    if keys.windows(2).any(|w| w[0] > w[1]) {
        failures.push(format!("{id}: states are not ordered by `from`"));
    }
    // A state that opens ABOVE the badge's build is a behavior the verified build does not
    // have, while its receipt is made at that build. The shape is real, so it is allowed,
    // and it owes its reason.
    if let (Some(open_state), Some(vk)) = (open_state(claim), version_key(verified)) {
        if let Some(fk) = open_state["from"].as_str().and_then(version_key) {
            if fk > vk
                && open_state["from_after_verified"]
                    .as_str()
                    .is_none_or(|s| s.trim().is_empty())
            {
                failures.push(format!(
                    "{id}: the open state opens at {}, above the verified build {verified}, \
                     with no `from_after_verified` saying why",
                    open_state["from"].as_str().unwrap_or("")
                ));
            }
        }
    }
    if let Some(c) = claim["compat_of"].as_str() {
        if c.trim().is_empty() {
            failures.push(format!("{id}: `compat_of` is empty"));
        }
    }
}

/// Rule 15: every state names the behavior, what csift depends on, and the instrument that
/// re-verifies it; the machine-readable `probe` twin names a kind from the closed set and
/// carries the field that kind is answered by. A `probe` with a kind and nothing else is a
/// placeholder wearing a schema.
pub(crate) fn state_text(at: &str, state: &Value, failures: &mut Vec<String>) {
    for key in ["behavior", "depends", "instrument"] {
        if state[key].as_str().is_none_or(|s| s.trim().is_empty()) {
            failures.push(format!("{at}: `{key}` is empty"));
        }
    }
    // `repo` is the one kind that re-verifies what CSIFT does rather than what Claude Code
    // does - a re-read inside this worktree - so it can never be the measurement a receipt
    // rests on. It is its own word because folding those into `corpus` would make that word
    // mean "a filesystem read" and weaken every transcript row that uses it.
    const KINDS: [&str; 5] = ["bytes", "corpus", "csift", "live", "repo"];
    let kind = state["probe"]["kind"].as_str();
    match kind {
        Some(k) if KINDS.contains(&k) => {}
        other => failures.push(format!("{at}: `probe.kind` is {other:?}, not in {KINDS:?}")),
    }
    // A `bytes` probe re-locates a literal in a build; every other kind runs something.
    let answer = if kind == Some("bytes") {
        "literal"
    } else {
        "command"
    };
    if state["probe"][answer]
        .as_str()
        .is_none_or(|s| s.trim().is_empty())
    {
        failures.push(format!(
            "{at}: `probe.{answer}` is empty, so the probe names no way to run it"
        ));
    }
    for key in ["scope", "rule"] {
        if state["probe"][key]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        {
            failures.push(format!("{at}: `probe.{key}` is empty"));
        }
    }
}

/// Rule 16: a CLOSED state's receipt is shape-checked too. It is never re-made against a
/// later build - it records what was measured at the build the range ended on - but it is
/// still a receipt, and a closed state carrying an empty one, a verdict outside the set, or
/// a build that is not its own `to`, is a range nothing ever stood behind.
pub(crate) fn closed_receipt(at: &str, state: &Value, failures: &mut Vec<String>) {
    let Some(to) = state["to"].as_str() else {
        return; // the open state is rules 2, 3 and 9's business
    };
    let receipt = &state["verified"];
    if !receipt.is_object() {
        failures.push(format!("{at}: a closed state with no `verified` receipt"));
        return;
    }
    match receipt["claude_code"].as_str() {
        Some(v) if version_triple(v) => {
            if version_key(v) > version_key(to) {
                failures.push(format!(
                    "{at}: the closed state's receipt is at Claude Code {v}, above its own `to` {to}"
                ));
            }
        }
        other => failures.push(format!(
            "{at}: the closed state's receipt is at {other:?}, not a Claude Code version"
        )),
    }
    let verdict = receipt["verdict"].as_str().unwrap_or("");
    if !super::rules::VERDICTS.contains(&verdict) {
        failures.push(format!(
            "{at}: the closed state's verdict `{verdict}` is not in {:?}",
            super::rules::VERDICTS
        ));
    }
    for key in ["instrument", "observed"] {
        if receipt[key].as_str().is_none_or(|s| s.trim().is_empty()) {
            failures.push(format!(
                "{at}: the closed state's receipt has an empty `{key}`"
            ));
        }
    }
}
