//! `list --lineage`: the sibling sessions a record of this transcript names as the one it
//! was MINTED in, as a SET with its count.

use crate::harness::*;

const ENC: &str = "-Users-dev-bridge-project";
const OWN: &str = "1a2b3c4d-5e6f-4708-9a1b-2c3d4e5f6071";
const OTHER_A: &str = "22222222-2222-4333-8444-555555555511";
const OTHER_B: &str = "33333333-3333-4333-8444-555555555522";

/// One record line: `sessionId` is always this transcript; `minted` names the snake-case
/// `session_id` the bridge carried, when there is one.
fn line(uuid: &str, parent: Option<&str>, secs: u32, minted: Option<&str>, text: &str) -> String {
    let parent = parent.map_or("null".to_string(), |p| format!("\"{p}\""));
    let snake = minted.map_or(String::new(), |m| format!(r#""session_id":"{m}","#));
    format!(
        r#"{{"type":"user","uuid":"{uuid}","parentUuid":{parent},{snake}"sessionId":"{OWN}","timestamp":"2026-06-07T05:00:{secs:02}.000Z","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// A transcript whose MIDDLE holds records hydrated from two other sessions - below the
/// head window and above the tail one, which is the shape the flag exists for.
fn bridged_home() -> Home {
    let mut lines = vec![line("u-1", None, 0, None, "zzbridge open")];
    for i in 2..=14u32 {
        lines.push(line(
            &format!("u-{i}"),
            Some(&format!("u-{}", i - 1)),
            i,
            None,
            "zzbridge local turn",
        ));
    }
    // The hydrated run: two sessions, both naming this transcript as the file they sit in.
    lines.push(line(
        "u-15",
        Some("u-14"),
        15,
        Some(OTHER_A),
        "zzbridge from A",
    ));
    lines.push(line(
        "u-16",
        Some("u-15"),
        16,
        Some(OTHER_A),
        "zzbridge from A again",
    ));
    lines.push(line(
        "u-17",
        Some("u-16"),
        17,
        Some(OTHER_B),
        "zzbridge from B",
    ));
    // A line whose snake id AGREES with the file: the pair points nowhere.
    lines.push(line(
        "u-18",
        Some("u-17"),
        18,
        Some(OWN),
        "zzbridge local again",
    ));
    for i in 19..=32u32 {
        lines.push(line(
            &format!("u-{i}"),
            Some(&format!("u-{}", i - 1)),
            i,
            None,
            "zzbridge local turn",
        ));
    }
    let h = Home::new();
    h.write(&format!("{ENC}/{OWN}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

/// The row for this transcript, as a parsed JSON object.
fn row(h: &Home, extra: &[&str]) -> serde_json::Value {
    let mut argv = vec!["list", ENC, "--format", "json"];
    argv.extend_from_slice(extra);
    let out = h.run(&argv);
    assert!(out.success, "{argv:?}: {}", out.stderr);
    let line = out
        .stdout
        .lines()
        .find(|l| l.contains("\"kind\":\"session\""))
        .unwrap_or_else(|| panic!("no session row:\n{}", out.stdout));
    serde_json::from_str(line).expect("one JSON object")
}

/// Both sessions are reported, sorted, with the count beside them - a set, because a
/// first-wins single value would drop the second in silence.
#[test]
fn the_whole_file_pass_reports_every_session_a_record_names() {
    let h = bridged_home();
    let r = row(&h, &["--lineage"]);
    assert_eq!(
        r["minted_elsewhere"],
        serde_json::json!([OTHER_A, OTHER_B]),
        "both hydrating sessions, sorted: {r}"
    );
    assert_eq!(r["minted_elsewhere_count"], 2);
    assert_eq!(r["lineage_scanned"], true);

    let text = h.run(&["list", ENC, "--lineage"]);
    assert!(text.success, "{}", text.stderr);
    assert!(
        text.stdout.contains(
            "  minted   a record here names 22222222, 33333333 as the session it was minted in"
        ),
        "the text row names both, by their first-8 tokens:\n{}",
        text.stdout
    );
    assert!(
        !text.stdout.contains("probably") && !text.stdout.contains("looks like"),
        "the link is STATED by the record, so the row hedges nothing:\n{}",
        text.stdout
    );
}

/// Without the flag the field is EMPTY and `lineage_scanned` says which empty it is -
/// the same honest empty the `session_kind` span already gives.
#[test]
fn without_the_flag_the_field_is_empty_and_says_which_empty() {
    let h = bridged_home();
    let r = row(&h, &[]);
    assert_eq!(r["minted_elsewhere"], serde_json::json!([]));
    assert_eq!(r["minted_elsewhere_count"], 0);
    assert_eq!(
        r["lineage_scanned"], false,
        "a consumer can tell 'not asked for' from 'asked and found none'"
    );

    let text = h.run(&["list", ENC]);
    assert!(text.success, "{}", text.stderr);
    assert!(
        !text.stdout.contains("  minted "),
        "no row without the flag:\n{}",
        text.stdout
    );
}

/// A snake id EQUAL to the transcript's own points nowhere, so it is no lineage link -
/// and a transcript where every record agrees reports an empty set even under the flag.
#[test]
fn an_agreeing_pair_is_not_a_lineage_link() {
    let lines = [
        line("u-1", None, 0, Some(OWN), "zzagree open"),
        line("u-2", Some("u-1"), 1, Some(OWN), "zzagree second"),
    ];
    let h = Home::new();
    h.write(&format!("{ENC}/{OWN}.jsonl"), &(lines.join("\n") + "\n"));
    let r = row(&h, &["--lineage"]);
    assert_eq!(r["minted_elsewhere"], serde_json::json!([]));
    assert_eq!(r["lineage_scanned"], true, "the pass DID run: {r}");
}
