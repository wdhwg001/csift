//! The two questions every channel command asks before it does anything: where is the body,
//! and which single lane is this about.
//!
//! Both are fail-loud by design. A body given twice is ambiguous, an empty body would render
//! an envelope with nothing in it, and a target naming zero or several lanes cannot be
//! narrowed by picking one - the wrong lane would be written to.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-targets";
const EMPTY_ENC: &str = "-Users-dev-Projects-nolanes";
const ONE: &str = "00000000-0000-4000-8000-0000000000a1";
const TWO: &str = "00000000-0000-4000-8000-0000000000a2";
const CWD: &str = "/Users/dev/Projects/targets";

fn transcript(sess: &str) -> String {
    format!(
        "{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"m1","timestamp":"2026-06-07T04:00:00.000Z","sessionId":"{sess}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"hello"}}}}"#
        )
    )
}

/// Two sessions in one project, plus an empty project directory.
fn home() -> Home {
    let h = Home::new();
    h.write(&format!("{ENC}/{ONE}.jsonl"), &transcript(ONE));
    h.write(&format!("{ENC}/{TWO}.jsonl"), &transcript(TWO));
    std::fs::create_dir_all(h.projects().join(EMPTY_ENC)).unwrap();
    h
}

/// Nothing was written to either session's channel root.
fn assert_no_channel(h: &Home) {
    for sess in [ONE, TWO] {
        assert!(
            !h.projects()
                .join(format!("{ENC}/{sess}/csift-channel"))
                .exists(),
            "a refused send wrote a channel root for {sess}"
        );
    }
}

#[test]
fn a_body_given_twice_is_refused_rather_than_one_of_them_being_picked() {
    let h = home();
    let file = h.root.join("body.txt");
    std::fs::write(&file, "from the file").unwrap();
    let out = h.run(&[
        "send",
        &at(ONE),
        "from the positional",
        "-f",
        file.to_str().unwrap(),
    ]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("positional OR `-f FILE`, not both"),
        "{}",
        out.stderr
    );
    assert_no_channel(&h);
}

#[test]
fn an_empty_stdin_body_is_refused_and_says_what_it_would_have_rendered() {
    // Nothing on stdin is the shape a mis-wired pipe produces, and an envelope around an
    // empty body would reach the receiver as a message with no content.
    let h = home();
    let out = h.run_with_stdin(&["send", &at(ONE)], "   \n");
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no message: pass it as a positional")
            && out.stderr.contains("an envelope with nothing in it"),
        "{}",
        out.stderr
    );
    assert_no_channel(&h);
}

#[test]
fn a_body_file_that_is_not_there_names_the_path_it_tried() {
    let h = home();
    let missing = h.root.join("absent.txt");
    let out = h.run(&["send", &at(ONE), "-f", missing.to_str().unwrap()]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("reading the message body from"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_send_target_naming_several_lanes_lists_them_and_writes_nothing() {
    let h = home();
    let out = h.run(&["send", &at(ENC), "ping"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("resolved to 2 transcripts")
            && out.stderr.contains("a send addresses exactly ONE lane")
            && out.stderr.contains(ONE)
            && out.stderr.contains(TWO),
        "{}",
        out.stderr
    );
    assert_no_channel(&h);
}

#[test]
fn a_send_target_naming_no_lane_is_an_error_not_a_silent_no_op() {
    let h = home();
    let out = h.run(&["send", &at(EMPTY_ENC), "ping"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr
            .contains("resolved to no transcript: a send addresses exactly one lane"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_msg_lane_naming_several_lanes_lists_them_by_transcript_id() {
    // The ledger is per lane, so a listing over two of them would merge two histories.
    let h = home();
    let out = h.run(&["msg", "--lane", &at(ENC)]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("names 2 lanes")
            && out.stderr.contains("operates on exactly one")
            && out.stderr.contains(ONE)
            && out.stderr.contains(TWO),
        "{}",
        out.stderr
    );
}

#[test]
fn a_msg_lane_naming_no_transcript_says_so() {
    let h = home();
    let out = h.run(&["msg", "--lane", &at(EMPTY_ENC)]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no transcript found for lane"),
        "{}",
        out.stderr
    );
}
