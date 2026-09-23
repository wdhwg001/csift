//! The selector law of the 73-leaf taxonomy, end to end: what a bare role or category
//! selects, what the retired spellings do, and what a category with no visible face says.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "3a2b1c09-8d7e-4f65-a432-10fedcba9876";

/// A session carrying one human turn, one assistant reply, and one bookkeeping line -
/// enough for the visible-face law to have something on both sides of it.
fn selector_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzselector chart the reef"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzselector charted"}]}}"#, "\n",
            r#"{"type":"cost-state","sessionId":"3a2b1c09-8d7e-4f65-a432-10fedcba9876","totalCostUSD":1.5,"totalDuration":900,"startTime":"2026-06-07T05:00:00.000Z"}"#, "\n",
        ),
    );
    h
}

#[test]
fn every_retired_spelling_is_a_hard_error_naming_its_successors() {
    let h = selector_home();
    // Each retired spelling, and a fragment of the successor list its error must name.
    let table: &[(&str, &[&str])] = &[
        (
            "harness.meta",
            &["harness.hook.", "harness.turn.", "harness.bookkeeping."],
        ),
        (
            "harness.meta.hook",
            &[
                "harness.hook.context",
                "harness.hook.feedback",
                "harness.command.caveat",
            ],
        ),
        ("harness.meta.loop", &["harness.schedule.tick"]),
        (
            "harness.meta.attachment",
            &[
                "harness.context.",
                "harness.hook.outcome",
                "agent.message.structured",
            ],
        ),
        ("harness.meta.turn-duration", &["harness.turn.duration"]),
        ("harness.meta.away-summary", &["harness.turn.recap"]),
        ("harness.meta.stop-hooks", &["harness.turn.stop-hooks"]),
        (
            "harness.meta.snapshot",
            &["harness.bookkeeping.file-history"],
        ),
        (
            "harness.meta.system",
            &["harness.system.", "harness.schedule.event"],
        ),
        ("thinking", &["agent.thinking"]),
        ("tool", &["agent.tool"]),
        ("tool-response", &["agent.tool.result"]),
        ("harness.schedule.continuation", &["harness.resume.prompt"]),
    ];
    assert_eq!(table.len(), 13, "the retired table is not 13 spellings");
    for (retired, successors) in table {
        let out = h.run(&["search", "", &at(SESS), "-t", retired]);
        assert!(
            !out.success,
            "{retired} must be a hard error:\n{}",
            out.stdout
        );
        // Read the successors out of the RETIREMENT HINT, never out of the whole message:
        // the error also lists every valid selector, so a `stderr.contains` over the full
        // text is satisfied by that list and would pass with the retirement arm deleted.
        let hint = retirement_hint(&out.stderr, retired);
        for want in *successors {
            assert!(
                hint.contains(want),
                "{retired}'s retirement hint must name {want}, got: {hint}"
            );
        }
    }
    // The `.*` form of a retired prefix is the same retirement, not a different one.
    let glob = h.run(&["search", "", &at(SESS), "-t", "harness.meta.*"]);
    assert!(!glob.success);
    assert!(
        retirement_hint(&glob.stderr, "harness.meta").contains("harness.hook."),
        "the glob form names the same successors:\n{}",
        glob.stderr
    );
}

/// The parenthetical the error carries for a RETIRED spelling, isolated from the rest of
/// the message: `('<selector>' <why it is not a selector>, so this is: <successors>.)`.
/// Panics when it is absent, because a retired spelling that errors without naming where
/// its records went is the failure this pins.
fn retirement_hint(stderr: &str, retired: &str) -> String {
    let opener = format!("('{retired}' ");
    let at = stderr
        .find(&opener)
        .unwrap_or_else(|| panic!("no retirement hint for {retired} in: {stderr}"));
    let rest = &stderr[at + opener.len()..];
    let end = rest
        .find(".)")
        .unwrap_or_else(|| panic!("unterminated retirement hint for {retired}: {rest}"));
    rest[..end].to_string()
}

#[test]
fn the_two_renamed_leaves_stay_valid_as_categories() {
    // `agent.message` and `agent.thinking` are CATEGORIES rather than leaf paths, so they
    // never reach the retired table: they parse, and they select their face.
    let h = selector_home();
    for sel in ["agent.message", "agent.thinking"] {
        let out = h.run(&["search", "", &at(SESS), "-t", sel]);
        assert!(
            out.success,
            "{sel} must stay a valid selector: {}",
            out.stderr
        );
    }
    let cat = h.run(&["search", "zzselector", &at(SESS), "-t", "agent.message"]);
    assert!(
        cat.stdout.contains("agent.message.plain"),
        "the category reaches the renamed leaf:\n{}",
        cat.stdout
    );
}

#[test]
fn a_category_with_no_visible_face_is_an_honest_empty() {
    let h = selector_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.bookkeeping"]);
    // ACCEPTED (exit 0) and empty: every leaf under the category is one the model never
    // receives, so the bare form selects none of them.
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "stdout keeps the ordinary empty result:\n{}",
        out.stdout
    );
    assert!(
        out.stderr
            .contains("-t harness.bookkeeping selects no LLM-visible leaf; use harness.bookkeeping.* or a leaf"),
        "the note names the law and the remedy:\n{}",
        out.stderr
    );
    // The remedy PARSES and runs, and carries no such note; what it then REACHES is the
    // candidate gate's question, pinned beside the gates.
    let glob = h.run(&["search", "", &at(SESS), "-t", "harness.bookkeeping.*"]);
    assert!(glob.success, "stderr: {}", glob.stderr);
    assert!(
        !glob.stderr.contains("selects no LLM-visible leaf"),
        "the glob form carries no such note:\n{}",
        glob.stderr
    );
    // JSON carries the same fact for a machine consumer.
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.bookkeeping",
        "--format",
        "json",
    ]);
    let summary: serde_json::Value = serde_json::from_str(js.stdout.lines().last().unwrap_or(""))
        .expect("the trailing summary line");
    assert_eq!(
        summary["empty_visible_face"],
        serde_json::json!(["harness.bookkeeping"]),
        "{}",
        js.stdout
    );
    // A category that DOES have a visible face carries an empty list instead.
    let ok = h.run(&[
        "search",
        "zzselector",
        &at(SESS),
        "-t",
        "agent.message",
        "--format",
        "json",
    ]);
    assert!(
        !ok.stdout.contains("empty_visible_face"),
        "a matching query carries no diagnosis at all:\n{}",
        ok.stdout
    );
}

#[test]
fn a_bare_role_and_a_bare_category_narrow_the_same_way() {
    let h = selector_home();
    // The human turn is visible under both the role and the category; the assistant
    // reply is not a user leaf under either.
    let role = h.run(&["search", "zzselector", &at(SESS), "-t", "user"]);
    assert!(role.stdout.contains("user.message"), "{}", role.stdout);
    assert!(
        !role.stdout.contains("agent.message.plain"),
        "{}",
        role.stdout
    );
    let cat = h.run(&["search", "zzselector", &at(SESS), "-t", "agent.message"]);
    assert!(
        cat.stdout.contains("agent.message.plain") && !cat.stdout.contains("user.message"),
        "{}",
        cat.stdout
    );
}

/// A PERSON can write any sentence a harness template opens with - quoting the reminder
/// they were just shown, pasting the nudge they are asking about, opening a message with
/// an image reference. Every template arm matches on TEXT, so `isMeta` (the authorship
/// flag the injecting call site stamps) is the only thing that separates the two
/// populations, and losing it relabels a person out of `user` while their message still
/// opens its turn.
#[test]
fn a_human_message_opening_with_a_template_stays_the_persons_own() {
    let h = Home::new();
    let sess = "6b5a4c3d-2e1f-4a09-8b7c-6d5e4f3a2b1c";
    let openers = [
        "[Image: 1024x768] what do you make of this?",
        "<system-reminder>quoting the one I was just shown</system-reminder> is that normal?",
        "Base directory for this skill: which one does it pick?",
        "Your tool call was malformed and could not be parsed. Please retry.",
        "The coordinator sent a message - did you see it?",
        "You ended the turn without calling SendUserMessage. why?",
        "[structured-output-enforce] what does this bracket mean?",
    ];
    let mut lines = String::new();
    for (i, text) in openers.iter().enumerate() {
        let user = serde_json::json!({
            "type": "user", "uuid": format!("u{i}"),
            "timestamp": format!("2026-06-07T05:0{i}:00.000Z"),
            "message": {"role": "user", "content": text},
        });
        let asst = serde_json::json!({
            "type": "assistant", "uuid": format!("a{i}"), "parentUuid": format!("u{i}"),
            "timestamp": format!("2026-06-07T05:0{i}:01.000Z"),
            "message": {"role": "assistant", "content": [{"type": "text", "text": "zzhuman noted"}]},
        });
        lines.push_str(&format!("{user}\n{asst}\n"));
    }
    h.write(&format!("{ENC}/{sess}.jsonl"), &lines);

    let out = h.run(&[
        "search",
        "",
        &at(sess),
        "-t",
        "user.message",
        "--count-by",
        "label",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("{}  user.message", openers.len())),
        "all {} human openers keep the person's leaf:\n{}",
        openers.len(),
        out.stdout
    );
    // And they are turns, which is the other half of the failure: a record cannot both
    // open a numbered turn and be classified as the harness's own text.
    let turns = h.run(&["search", "", &at(sess), "--count-by", "turn"]);
    assert!(turns.success, "stderr: {}", turns.stderr);
    assert_eq!(
        turns
            .stdout
            .lines()
            .filter(|l| l.trim().starts_with(char::is_numeric))
            .count(),
        openers.len(),
        "each opener numbers its own turn:\n{}",
        turns.stdout
    );
}

/// The same law over a record whose blocks are JOINED before the text arms see them: a
/// person's turn whose first block is an injected reminder they pasted and whose second
/// is their own prose is still their turn.
#[test]
fn a_joined_block_turn_opening_with_a_template_stays_the_persons_own() {
    let h = Home::new();
    let sess = "7c6b5a4d-3e2f-4b1a-9c8d-7e6f5a4b3c2e";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>the one I was shown</system-reminder>"},{"type":"text","text":"zzjoined and here is my actual question"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzjoined noted"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzjoined and here",
        &at(sess),
        "-t",
        "user.message",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.message"),
        "the person's prose keeps its leaf even when a pasted reminder leads the blocks:\n{}",
        out.stdout
    );
}
