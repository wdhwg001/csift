//! EVERY advertised leaf is reachable from a transcript on disk, through the binary.
//!
//! Each family lane pinned the leaves it owns. What no per-family suite can answer is
//! whether the taxonomy as a WHOLE is inhabited: a leaf the selector space advertises and
//! nothing on disk can produce is a query that reports a definitive absence over records
//! that exist - the exact reading the empty-result diagnosis exists to rule out, and the
//! one csift must never cause itself.
//!
//! The census runs `--count-by label`, which counts a record under every leaf it carries
//! that survives the filter, so it answers the question directly and once: 73 keys, each
//! at least 1. It runs with an EMPTY pattern because four leaves render no body at all
//! (the two textless thinking shapes among them) and a pattern scan cannot reach those;
//! the empty pattern plus an exact selector is the documented way in.

use crate::harness::*;

/// The leaf count the taxonomy advertises. Typed out rather than derived: a test that
/// counted the keys it found would pass on any drift, which is the whole failure this
/// file exists to catch.
const LEAF_COUNT: usize = 73;

/// The 73 leaves, in `Class::ALL` order. Hand-kept, because an integration test cannot see
/// the crate's own enum and a list derived from the census would assert nothing about what
/// the taxonomy CLAIMS. It is held to the enum by
/// `the_leaf_list_here_is_the_selector_space_the_binary_prints` below, which reads the
/// selector space the binary prints from `Class::ALL` itself.
const EVERY_LEAF_PATH: [&str; LEAF_COUNT] = [
    "user.message",
    "user.answer",
    "user.rejection",
    "user.unsent",
    "user.rewound",
    "user.queued",
    "agent.message.plain",
    "agent.message.structured",
    "agent.thinking.summary",
    "agent.thinking.narration",
    "agent.thinking.omitted",
    "agent.thinking.redacted",
    "agent.tool.use",
    "agent.tool.result",
    "agent.communication.inbox",
    "agent.communication.sent",
    "agent.communication.signal",
    "agent.communication.channel",
    "harness.notification.workflow",
    "harness.notification.monitor",
    "harness.notification.subagent",
    "harness.notification.background-command",
    "harness.notification.task",
    "harness.compaction.summary",
    "harness.compaction.boundary",
    "harness.command.invocation",
    "harness.command.stdout",
    "harness.command.caveat",
    "harness.interrupt.user",
    "harness.interrupt.tool",
    "harness.schedule.wakeup",
    "harness.schedule.event",
    "harness.schedule.fire",
    "harness.schedule.tick",
    "harness.resume.prompt",
    "harness.resume.placeholder",
    "harness.hook.context",
    "harness.hook.outcome",
    "harness.hook.feedback",
    "harness.context.reminder",
    "harness.context.listing",
    "harness.context.prompt",
    "harness.context.file",
    "harness.context.mode",
    "harness.context.ide",
    "harness.context.task",
    "harness.context.image",
    "harness.context.skill",
    "harness.enforce.structured-output",
    "harness.enforce.brief",
    "harness.enforce.handback",
    "harness.enforce.retry",
    "harness.turn.duration",
    "harness.turn.stop-hooks",
    "harness.turn.recap",
    "harness.system.refusal",
    "harness.system.api-error",
    "harness.system.notice",
    "harness.system.kill",
    "harness.system.command",
    "harness.bookkeeping.last-prompt",
    "harness.bookkeeping.mode",
    "harness.bookkeeping.name",
    "harness.bookkeeping.atis",
    "harness.bookkeeping.bridge",
    "harness.bookkeeping.suppression",
    "harness.bookkeeping.pr-link",
    "harness.bookkeeping.artifact",
    "harness.bookkeeping.file-history",
    "harness.bookkeeping.cost",
    "harness.bookkeeping.continued",
    "harness.bookkeeping.fork",
    "harness.bookkeeping.queue",
];

/// The `--count-by label` census over the reachability home, as (leaf, records) pairs.
fn census(h: &Home, extra: &[&str]) -> Vec<(String, u64)> {
    let mut argv: Vec<&str> = vec![
        "search",
        "",
        &REACH_TARGET,
        "--count-by",
        "label",
        "--format",
        "json",
    ];
    argv.extend_from_slice(extra);
    let out = h.run(&argv);
    assert!(out.success, "census failed: {}", out.stderr);
    json_rows(&out.stdout, "census")
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap_or_default().to_string(),
                r["records"].as_u64().unwrap_or_default(),
            )
        })
        .collect()
}

/// EVERY leaf of the taxonomy is produced by at least one line of the fixture, and the
/// census reports exactly those keys and no others.
///
/// Both directions matter. A MISSING key is a leaf nothing on disk can reach - an
/// advertised selector over an empty set. An EXTRA key is a leaf this list has not caught
/// up with, which means the fixture stopped covering the space it claims to cover.
#[test]
fn every_leaf_of_the_taxonomy_is_reachable_from_one_fixture() {
    let h = reachability_home();
    let got = census(&h, &EVERY_LEAF);

    let missing: Vec<&str> = EVERY_LEAF_PATH
        .iter()
        .copied()
        .filter(|leaf| !got.iter().any(|(k, n)| k == leaf && *n >= 1))
        .collect();
    assert!(
        missing.is_empty(),
        "leaves no fixture line reaches: {missing:?}\nthe census gave {got:?}"
    );

    let extra: Vec<&str> = got
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !EVERY_LEAF_PATH.contains(k))
        .collect();
    assert!(
        extra.is_empty(),
        "leaves outside the pinned list: {extra:?}"
    );
    assert_eq!(
        got.len(),
        LEAF_COUNT,
        "the census reports {} keys, the taxonomy advertises {LEAF_COUNT}",
        got.len()
    );
}

/// The list above IS the enum's, and this is what pins it from outside the crate.
///
/// An integration test cannot see `Class::ALL`, so the list above would otherwise be a
/// second copy that drifts the day a leaf is added. The binary prints the whole selector
/// space verbatim when a selector is rejected - `label_selectors()` joined, which is every
/// dot-segment PREFIX of every leaf path - and the LEAVES are exactly its maximal
/// elements: a selector no other selector extends. So the set is derived from the enum on
/// one side and typed out on the other, and a new leaf fails here until both move.
#[test]
fn the_leaf_list_here_is_the_selector_space_the_binary_prints() {
    let h = Home::new();
    let out = h.run(&["search", "zz", "-t", "nosuchselector"]);
    assert!(!out.success, "an unknown selector is a hard error");
    let (_, tail) = out
        .stderr
        .split_once("Valid: ")
        .expect("the error lists the valid selectors");
    // clap appends its own `For more information, try '--help'.` after a blank line.
    let list = tail.split("\n\n").next().unwrap_or(tail);
    let selectors: Vec<&str> = list
        .trim()
        .trim_end_matches('.')
        .split(", ")
        .map(str::trim)
        .collect();
    let mut leaves: Vec<&str> = selectors
        .iter()
        .copied()
        .filter(|s| {
            !selectors
                .iter()
                .any(|o| o.len() > s.len() && o.starts_with(s) && o.as_bytes()[s.len()] == b'.')
        })
        .collect();
    let mut want: Vec<&str> = EVERY_LEAF_PATH.to_vec();
    leaves.sort_unstable();
    want.sort_unstable();
    assert_eq!(leaves, want, "the pinned leaf list is not the binary's");
}

/// What the three BARE faces drop, exactly.
///
/// The 28 leaves `Class::llm_visible` calls invisible - the two scan-layer user leaves,
/// `user.queued` and the 13 bookkeeping leaves (non-transcript lines with no `message{}`),
/// the two textless thinking shapes, the nine message-less system records, and
/// `harness.hook.outcome` - PLUS one leaf whose absence is a different fact and the reason
/// this list is typed out rather than counted: `agent.message.structured` is VISIBLE as a
/// leaf (the model produced that text) while the harness converts its attachment to
/// nothing, so the RECORD-level delivery verdict is what removes it from a bare face. A
/// count alone would let those two reasons trade places unnoticed.
const DROPPED_BY_A_BARE_FACE: [&str; 29] = [
    "agent.message.structured",
    "agent.thinking.omitted",
    "agent.thinking.redacted",
    "harness.bookkeeping.artifact",
    "harness.bookkeeping.atis",
    "harness.bookkeeping.bridge",
    "harness.bookkeeping.continued",
    "harness.bookkeeping.cost",
    "harness.bookkeeping.file-history",
    "harness.bookkeeping.fork",
    "harness.bookkeeping.last-prompt",
    "harness.bookkeeping.mode",
    "harness.bookkeeping.name",
    "harness.bookkeeping.pr-link",
    "harness.bookkeeping.queue",
    "harness.bookkeeping.suppression",
    "harness.compaction.boundary",
    "harness.hook.outcome",
    "harness.schedule.event",
    "harness.system.api-error",
    "harness.system.kill",
    "harness.system.notice",
    "harness.system.refusal",
    "harness.turn.duration",
    "harness.turn.recap",
    "harness.turn.stop-hooks",
    "user.queued",
    "user.rewound",
    "user.unsent",
];

/// A BARE role selects only its LLM-VISIBLE face, and the fixture reaches every leaf of
/// all three faces. The complement is the same statement from the other side, and it is
/// the one this asserts exactly: what a bare face drops is what the visibility law and the
/// per-record delivery verdict say it drops, and nothing else.
///
/// Without this, the reachability sweep above would pass on a fixture that only ever
/// produced records under the explicit glob - and the coarse "show me the conversation"
/// form, which is what a reader reaches for first, would be untested.
#[test]
fn the_bare_faces_reach_every_visible_leaf_and_no_invisible_one() {
    let h = reachability_home();
    let all: Vec<String> = census(&h, &EVERY_LEAF)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let faces: Vec<String> = census(&h, &["-t", "user", "-t", "agent", "-t", "harness"])
        .into_iter()
        .map(|(k, _)| k)
        .collect();

    for leaf in &faces {
        assert!(
            all.contains(leaf),
            "{leaf} is on the visible face but not in the full census"
        );
    }
    let mut dropped: Vec<&str> = all
        .iter()
        .map(String::as_str)
        .filter(|k| !faces.iter().any(|f| f == k))
        .collect();
    dropped.sort_unstable();
    let mut want = DROPPED_BY_A_BARE_FACE;
    want.sort_unstable();
    assert_eq!(dropped, want, "what a bare face drops has moved");
}

/// The full-census target: the top-level transcript AND its subagent lane.
const REACH_TARGET: &str = "@3a1b2c3d-4e5f-4061-8273-849506a7b8c9";
