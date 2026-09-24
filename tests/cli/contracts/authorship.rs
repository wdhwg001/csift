//! The AUTHORSHIP contract across the surfaces that count turns: a record the harness
//! wrote to accompany a turn is not the person's message, so it opens no turn and no
//! surface counts one for it.
//!
//! `isMeta` is the flag nearly every such record carries and `turnCompanion` is the
//! second, and the two are not one set: the harness stamps the companion flag on paths
//! that touch no meta flag at all. A fixture carrying the second alone is what separates
//! reading both from reading only the first.

use crate::harness::*;

const SESS: &str = "11111111-2222-4333-8444-555555555502";
const PROJ: &str = "-Users-dev-companion";

/// Two human turns with a COMPANION record inside the first: the harness continuing that
/// turn, carrying `turnCompanion` and no `isMeta`, which is the shape two of the shipped
/// build's stamping paths produce.
fn home() -> Home {
    let lines = [
        r#"{"type":"user","uuid":"h0","sessionId":"11111111-2222-4333-8444-555555555502","cwd":"/Users/dev/companion","version":"2.1.278","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzco the first ask"}}"#,
        r#"{"type":"assistant","uuid":"h1","parentUuid":"h0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzco working on it"}]}}"#,
        r#"{"type":"user","uuid":"h2","parentUuid":"h1","turnCompanion":true,"timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"zzco carry on from where you stopped"}}"#,
        r#"{"type":"assistant","uuid":"h3","parentUuid":"h2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzco carried on"}]}}"#,
        r#"{"type":"user","uuid":"h4","parentUuid":"h3","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"zzco the second ask"}}"#,
        r#"{"type":"assistant","uuid":"h5","parentUuid":"h4","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzco answered"}]}}"#,
    ];
    let h = Home::new();
    h.write(&format!("{PROJ}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

#[test]
fn a_turn_companion_opens_no_turn_on_either_counting_surface() {
    let h = home();
    // TWO turns, not three. The census keys on the turn numbering, so a third key here
    // would mean the companion had opened one.
    let c = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--no-subagents",
        "--count-by",
        "turn",
    ]);
    assert!(c.success, "stderr: {}", c.stderr);
    let keys = c
        .stdout
        .lines()
        .filter(|l| l.trim_start().starts_with(char::is_numeric))
        .count();
    assert_eq!(keys, 2, "two turns over the fixture: {}", c.stdout);
    // `stats` counts the LIVE turns of the chain and has to agree, because both read the
    // one boundary predicate.
    let s = h.run(&[
        "stats",
        at(SESS).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(s.success, "stderr: {}", s.stderr);
    let row = s
        .stdout
        .lines()
        .find(|l| l.contains("\"turns\""))
        .unwrap_or_else(|| panic!("no turns field: {}", s.stdout));
    let v: serde_json::Value = serde_json::from_str(row).expect("one JSON object per line");
    assert_eq!(v["turns"], serde_json::json!(2), "{row}");
}

#[test]
fn a_turn_companion_is_not_the_person_and_an_address_names_the_flag() {
    let h = home();
    // It is not `user.message`: the leaf and the turn boundary answer alike, so a reader
    // selecting the human's own messages gets the two asks and not the continuation.
    let u = h.run(&[
        "search",
        "zzco",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "user.message",
    ]);
    assert!(u.success, "stderr: {}", u.stderr);
    assert!(
        u.stdout.contains("zzco the first ask") && u.stdout.contains("zzco the second ask"),
        "the person's own messages are there: {}",
        u.stdout
    );
    assert!(
        !u.stdout.contains("carry on from where you stopped"),
        "the companion is not the person: {}",
        u.stdout
    );
    // It carries no leaf at all, so an ADDRESS is the only way to it - and the unit names
    // the flag that made it harness text rather than the operator's.
    let show = h.run(&["show", at(SESS).as_str(), "--line", "3"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout.contains("? unmodeled: user/turnCompanion"),
        "the address names the flag: {}",
        show.stdout
    );
}
