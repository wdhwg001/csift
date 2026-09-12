//! The verbatim FOLD accounting: what a collapsed agent-message run discloses, and the
//! same-prefix re-send a later message supersedes.

use crate::harness::*;

/// A session whose ONE turn folds two agent messages - a 12-char opener and a 240-char
/// signal-free middle - while a 400-char last message wins the longest-privilege and is kept.
/// The 240-char member is over the preview threshold, the 12-char one is under it.
fn fold_preview_home() -> (Home, &'static str) {
    let sess = "3e3e3e3e-4f4f-4a4a-8b8b-5c5c5c5c5c5c";
    let middle = format!("MIDBODY {} MIDTAIL", "x".repeat(224));
    let last = format!("ANSWERHEAD {} ANSWERTAIL", "y".repeat(378));
    assert_eq!(middle.chars().count(), 240);
    assert_eq!(last.chars().count(), 400);
    let lines = [
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued."}}"#.to_string(),
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the fold"}}"#.to_string(),
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"brief opener"}]}}"#.to_string(),
        format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{middle}"}}]}}}}"#
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{last}"}}]}}}}"#
        ),
    ];
    let h = Home::new();
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
    (h, sess)
}

#[test]
fn turns_fold_marker_names_the_chars_and_previews_the_substantive_bodies() {
    // The fold used to say only HOW MANY messages it swallowed. It now names the summed chars
    // and shows the head of every collapsed message at or above 210 chars, so a reader can
    // tell a folded finding from a folded "let me look" without fetching the range.
    let (h, sess) = fold_preview_home();
    let text = h.run(&["verbatim", at(sess).as_str(), "--budget", "40000"]);
    assert!(text.success, "stderr: {}", text.stderr);
    // The opener (L3) and the middle (L4) fold together: 12 + 240 = 252 chars, 0 tool calls.
    assert!(
        text.stdout
            .contains("△ L3–L4  [2 agent messages collapsed, 252 chars, 0 tool calls]"),
        "the fold marker must name X, N chars and Y:\n{}",
        text.stdout
    );
    // ONE preview line, for the 240-char member only, indented under the marker and stating
    // its own remainder (240 - 60 = 180).
    let preview = format!("    L4  MIDBODY {}… (+180 chars)", "x".repeat(52));
    assert!(
        text.stdout.contains(&preview),
        "the 240-char folded body earns a preview line:\n{}",
        text.stdout
    );
    assert!(
        !text.stdout.contains("    L3  brief opener"),
        "a 12-char folded body is under the threshold and earns none:\n{}",
        text.stdout
    );
    // The kept longest message is still whole.
    assert!(text.stdout.contains("ANSWERHEAD") && text.stdout.contains("ANSWERTAIL"));

    // JSON twin: collapsed_chars + one collapsed_previews entry.
    let json = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let ph = json_rows(&json.stdout, "collapsed_agents")
        .into_iter()
        .next()
        .expect("a collapsed_agents row");
    assert_eq!(ph["agent_messages"].as_u64().unwrap(), 2);
    assert_eq!(ph["collapsed_chars"].as_u64().unwrap(), 252);
    let previews = ph["collapsed_previews"].as_array().expect("an array");
    assert_eq!(previews.len(), 1, "only the substantive member: {ph}");
    assert_eq!(previews[0]["line"].as_u64().unwrap(), 4);
    let ex = previews[0]["excerpt"].as_str().unwrap();
    assert!(
        ex.starts_with("MIDBODY ") && ex.ends_with("… (+180 chars)"),
        "{ex}"
    );
}

#[test]
fn turns_a_same_prefix_resend_renders_as_a_marker_and_keeps_its_json_row() {
    // One turn carries two assistant messages where the LATER repeats the earlier's whole body
    // and adds a tail. The text render prints the earlier as a one-line marker naming the
    // survivor's line and the suppressed char count; the JSON row keeps the full prose and
    // carries `superseded_by_line`, so nothing is lost to a machine reader.
    let sess = "5a5a5a5a-6b6b-4c4c-8d8d-7e7e7e7e7e7e";
    let first = "the committed answer runs long enough to clear the eighty-char fingerprint gate and then says its piece";
    let second = format!("{first} plus the addendum a blocked turn end asked for");
    let lines = [
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued."}}"#.to_string(),
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask once"}}"#.to_string(),
        format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{first}"}}]}}}}"#
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{second}"}}]}}}}"#
        ),
    ];
    let h = Home::new();
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));

    let text = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "40000",
        "--agent-msgs",
        "all",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    let marker = format!(
        "△ L3  [superseded by the same-prefix re-send at L4, {} chars]",
        first.chars().count()
    );
    assert!(
        text.stdout.contains(&marker),
        "the earlier message must render as the re-send marker:\n{}",
        text.stdout
    );
    // The survivor is printed in full, and the suppressed body appears ONCE (inside it).
    assert!(text
        .stdout
        .contains("plus the addendum a blocked turn end asked for"));
    assert_eq!(
        text.stdout.matches("and then says its piece").count(),
        1,
        "the prose is printed once, by the survivor:\n{}",
        text.stdout
    );

    let json = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "40000",
        "--agent-msgs",
        "all",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let rows = json_rows(&json.stdout, "turn");
    let earlier = rows
        .iter()
        .find(|o| o["line"].as_u64() == Some(3))
        .expect("the earlier unit's row");
    assert_eq!(earlier["superseded_by_line"].as_u64().unwrap(), 4);
    assert_eq!(
        earlier["text"].as_str().unwrap(),
        first,
        "JSON keeps it whole"
    );
    let survivor = rows
        .iter()
        .find(|o| o["line"].as_u64() == Some(4))
        .expect("the survivor's row");
    assert!(
        survivor["superseded_by_line"].is_null(),
        "an ordinary unit reports null: {survivor}"
    );
}

#[test]
fn turns_a_divergent_shared_prefix_is_not_folded() {
    // The measured majority case: two messages share far more than 80 chars, then diverge. The
    // later does NOT carry the earlier, so both bodies print - folding one would drop prose the
    // survivor never said.
    let sess = "6b6b6b6b-7c7c-4d4d-8e8e-8f8f8f8f8f8f";
    let head =
        "the committed answer runs long enough to clear the eighty-char fingerprint gate and then";
    let lines = [
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued."}}"#.to_string(),
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask once"}}"#.to_string(),
        format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{head} says ALPHAWORD"}}]}}}}"#
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{head} says BETAWORD"}}]}}}}"#
        ),
    ];
    let h = Home::new();
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
    let text = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "40000",
        "--agent-msgs",
        "all",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        !text.stdout.contains("superseded by"),
        "a divergent pair is not a re-send:\n{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("ALPHAWORD") && text.stdout.contains("BETAWORD"),
        "both bodies print:\n{}",
        text.stdout
    );
}
