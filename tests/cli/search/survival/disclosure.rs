//! What the axis says when there is nothing to say, and what it stops saying once a
//! second transcript joins the scope. Both directions matter: a disclosure that fires on
//! an ordinary transcript is noise on every scan in the world, and a line number carried
//! over from an unnamed file is worse than silence.

use super::*;

const DENC: &str = "-Users-dev-example-harbour";
const DSESS_A: &str = "11111111-2222-4333-8444-555555555555";
const DSESS_B: &str = "66666666-7777-4888-8999-aaaaaaaaaaaa";

/// A transcript the chain reaches end to end: one prompt, one reply, nothing recalled.
fn plain(uuid: &str) -> String {
    format!(
        concat!(
            r#"{{"type":"user","uuid":"p0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the {n} lagoon"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"q0","parentUuid":"p0","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","id":"m0","content":[{{"type":"text","text":"charting the {n} lagoon"}}]}}}}"#,
            "\n",
        ),
        n = uuid
    )
}

/// A transcript carrying one recalled draft (L3, resent as L4) and nothing else unusual.
fn with_a_draft(tag: &str) -> String {
    format!(
        concat!(
            r#"{{"type":"user","uuid":"p0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the {t} lagoon"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"q0","parentUuid":"p0","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","id":"m0","content":[{{"type":"text","text":"charting"}}]}}}}"#,
            "\n",
            r#"{{"type":"user","uuid":"p1","parentUuid":"q0","timestamp":"2026-06-07T05:01:00.000Z","message":{{"role":"user","content":"sound the {t} chanel"}}}}"#,
            "\n",
            r#"{{"type":"user","uuid":"p2","parentUuid":"q0","timestamp":"2026-06-07T05:02:00.000Z","message":{{"role":"user","content":"sound the {t} channel"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"q1","parentUuid":"p2","timestamp":"2026-06-07T05:02:05.000Z","message":{{"role":"assistant","id":"m1","content":[{{"type":"text","text":"sounding"}}]}}}}"#,
            "\n",
        ),
        t = tag
    )
}

#[test]
fn an_ordinary_transcript_discloses_nothing_about_the_axis() {
    // Every count is zero and the chain reached everything, so the scan prints exactly what
    // it printed before the axis existed. A disclosure that fires at zero would attach a
    // paragraph of chain prose to every search anyone ever runs.
    let h = Home::new();
    h.write(&format!("{DENC}/{DSESS_A}.jsonl"), &plain("northern"));
    let out = h.run(&["search", "lagoon", &at(DSESS_A)]);
    assert!(out.success, "stderr: {}", out.stderr);
    for absent in [
        "rewound turn(s) outside turn numbering",
        "record(s) off the surviving conversation",
        "replay copy line(s)",
        "superseded draft(s) outside turn numbering",
        "chain leaf:",
        "compaction boundary",
    ] {
        assert!(
            !out.stdout.contains(absent),
            "an ordinary transcript says nothing about `{absent}`:\n{}",
            out.stdout
        );
    }
}

#[test]
fn the_leaf_line_needs_something_to_disclose_beside_it() {
    // The leaf is only interesting next to a fact about the chain, and a replay copy is one
    // of the three that qualify - the whole point of naming the leaf is to say which branch
    // of a forked file the reading came from.
    let h = Home::new();
    let replayed = format!("{}{}", plain("northern"), plain("northern"));
    h.write(&format!("{DENC}/{DSESS_A}.jsonl"), &replayed);
    let out = h.run(&["search", "lagoon", &at(DSESS_A)]);
    assert!(
        out.stdout.contains("replay copy line(s)") && out.stdout.contains("chain leaf: tail"),
        "a replay copy is a fact worth naming the leaf for:\n{}",
        out.stdout
    );
    let s = summary(
        &h.run(&["search", "lagoon", &at(DSESS_A), "--format", "json"])
            .stdout,
    );
    assert_eq!(s["replay_copies"], 2);
    assert_eq!(s["abandoned_records"], 0);
    assert!(s["boundary_cut_line"].is_null());
}

#[test]
fn a_second_transcript_drops_the_single_transcript_facts() {
    // `leaf_source` and `boundary_cut_line` name ONE file. Once a second transcript folds
    // in, neither can be attributed, so both go - keeping the first one's answer would
    // report a leaf choice made in a file the reader was never told about.
    let h = Home::new();
    h.write(
        &format!("{DENC}/{DSESS_A}.jsonl"),
        &with_a_draft("northern"),
    );
    h.write(
        &format!("{DENC}/{DSESS_B}.jsonl"),
        &with_a_draft("southern"),
    );
    let out = h.run(&["search", "sound", DENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("record(s) off the surviving conversation"),
        "the counts still add up across the scope:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("chain leaf:"),
        "but the leaf belongs to no one file here:\n{}",
        out.stdout
    );
    let j = h.run(&["search", "sound", DENC, "--format", "json"]);
    let s = summary(&j.stdout);
    assert_eq!(s["superseded_drafts"], 2, "{}", j.stdout);
    assert_eq!(s["abandoned_records"], 2, "{}", j.stdout);
    assert!(s["leaf_source"].is_null(), "{}", j.stdout);
    assert!(s["boundary_cut_line"].is_null(), "{}", j.stdout);

    // The same scope narrowed back to one transcript names the leaf again.
    let one = h.run(&["search", "sound", &at(DSESS_A), "--format", "json"]);
    assert_eq!(
        summary(&one.stdout)["leaf_source"],
        "tail",
        "{}",
        one.stdout
    );
}
